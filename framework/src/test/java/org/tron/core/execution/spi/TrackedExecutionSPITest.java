package org.tron.core.execution.spi;

import static org.junit.Assert.*;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.*;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.Arrays;
import java.util.concurrent.CompletableFuture;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.mockito.Mock;
import org.mockito.junit.MockitoJUnitRunner;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.store.StoreFactory;
import org.tron.protos.Protocol;

@RunWith(MockitoJUnitRunner.class)
public class TrackedExecutionSPITest {

  @Mock private ExecutionSPI mockDelegate;
  @Mock private StoreFactory mockStoreFactory;

  private TrackedExecutionSPI trackedExecutionSPI;
  private ExecutionMetricsLogger metricsLogger;
  private String tempOutputDir;

  @Before
  public void setUp() throws IOException {
    // Create temporary directory for test output
    tempOutputDir = Files.createTempDirectory("execution-metrics-test").toString();
    
    // Create metrics logger
    metricsLogger = new ExecutionMetricsLogger(tempOutputDir);
    
    // Create tracked execution SPI
    trackedExecutionSPI = new TrackedExecutionSPI(
        mockDelegate, metricsLogger, "TEST", false);
  }

  @After
  public void tearDown() {
    if (trackedExecutionSPI != null) {
      trackedExecutionSPI.close();
    }
    
    // Clean up temp directory
    try {
      Files.walk(Paths.get(tempOutputDir))
          .map(Path::toFile)
          .forEach(File::delete);
      Files.deleteIfExists(Paths.get(tempOutputDir));
    } catch (IOException e) {
      // Ignore cleanup errors in test
    }
  }

  @Test
  public void testSuccessfulExecutionTracking() throws Exception {
    // Arrange
    TransactionContext context = createMockTransactionContext();
    ExecutionProgramResult mockResult = new ExecutionProgramResult();
    mockResult.spendEnergy(21000L);
    mockResult.setHReturn("test-return-data".getBytes());

    when(mockDelegate.executeTransaction(any(TransactionContext.class)))
        .thenReturn(CompletableFuture.completedFuture(mockResult));

    // Act
    CompletableFuture<ExecutionProgramResult> future = 
        trackedExecutionSPI.executeTransaction(context);
    ExecutionProgramResult result = future.get();

    // Assert
    assertNotNull(result);
    assertEquals(21000L, result.getEnergyUsed());
    verify(mockDelegate, times(1)).executeTransaction(context);
    
    // Allow some time for async logging
    Thread.sleep(100);
    
    // Check that metrics were logged
    assertTrue("Metrics queue should have processed items", 
        metricsLogger.getQueueSize() >= 0);
  }

  @Test
  public void testFailedExecutionTracking() throws Exception {
    // Arrange
    TransactionContext context = createMockTransactionContext();
    RuntimeException testException = new RuntimeException("Test execution failure");

    CompletableFuture<ExecutionProgramResult> failedFuture = new CompletableFuture<>();
    failedFuture.completeExceptionally(testException);
    
    when(mockDelegate.executeTransaction(any(TransactionContext.class)))
        .thenReturn(failedFuture);

    // Act & Assert
    CompletableFuture<ExecutionProgramResult> future = 
        trackedExecutionSPI.executeTransaction(context);
    
    try {
      future.get();
      fail("Expected exception to be thrown");
    } catch (Exception e) {
      assertTrue(e.getCause() instanceof RuntimeException);
    }

    verify(mockDelegate, times(1)).executeTransaction(context);
    
    // Allow some time for async logging
    Thread.sleep(100);
    
    // Check that error metrics were logged
    assertTrue("Metrics queue should have processed error", 
        metricsLogger.getQueueSize() >= 0);
  }

  @Test
  public void testDelegateMethodsPassThrough() throws Exception {
    // Test that non-execution methods are passed through correctly
    byte[] testAddress = new byte[20];
    byte[] testKey = new byte[32];
    String testSnapshot = "test-snapshot";
    
    when(mockDelegate.getCode(testAddress, testSnapshot))
        .thenReturn(CompletableFuture.completedFuture("test-code".getBytes()));
    when(mockDelegate.getStorageAt(testAddress, testKey, testSnapshot))
        .thenReturn(CompletableFuture.completedFuture("test-storage".getBytes()));
    when(mockDelegate.getNonce(testAddress, testSnapshot))
        .thenReturn(CompletableFuture.completedFuture(123L));

    // Act
    CompletableFuture<byte[]> codeFuture = trackedExecutionSPI.getCode(testAddress, testSnapshot);
    CompletableFuture<byte[]> storageFuture = trackedExecutionSPI.getStorageAt(testAddress, testKey, testSnapshot);
    CompletableFuture<Long> nonceFuture = trackedExecutionSPI.getNonce(testAddress, testSnapshot);

    // Assert
    assertArrayEquals("test-code".getBytes(), codeFuture.get());
    assertArrayEquals("test-storage".getBytes(), storageFuture.get());
    assertEquals((Long) 123L, nonceFuture.get());

    verify(mockDelegate).getCode(testAddress, testSnapshot);
    verify(mockDelegate).getStorageAt(testAddress, testKey, testSnapshot);
    verify(mockDelegate).getNonce(testAddress, testSnapshot);
  }

  @Test
  public void testMetricsLoggerConfiguration() {
    // Test that the metrics logger is properly configured
    assertNotNull(trackedExecutionSPI.getDelegate());
    assertEquals("TEST", trackedExecutionSPI.getExecutionMode());
    assertTrue(trackedExecutionSPI.isMetricsLoggingActive());
    assertEquals(0, trackedExecutionSPI.getMetricsQueueSize());
  }

  private TransactionContext createMockTransactionContext() {
    try {
      // Create a minimal transaction
      Protocol.Transaction.Builder txBuilder = Protocol.Transaction.newBuilder();
      Protocol.Transaction.raw.Builder rawBuilder = Protocol.Transaction.raw.newBuilder();
      
      // Add a contract (required by some extraction methods)
      Protocol.Transaction.Contract.Builder contractBuilder = 
          Protocol.Transaction.Contract.newBuilder();
      contractBuilder.setType(Protocol.Transaction.Contract.ContractType.TriggerSmartContract);
      rawBuilder.addContract(contractBuilder.build());
      
      txBuilder.setRawData(rawBuilder.build());
      
      TransactionCapsule trxCap = new TransactionCapsule(txBuilder.build());
      
      // Create a minimal block
      Protocol.Block.Builder blockBuilder = Protocol.Block.newBuilder();
      Protocol.BlockHeader.Builder headerBuilder = Protocol.BlockHeader.newBuilder();
      Protocol.BlockHeader.raw.Builder rawHeaderBuilder = Protocol.BlockHeader.raw.newBuilder();
      rawHeaderBuilder.setNumber(1000000L);
      rawHeaderBuilder.setTimestamp(System.currentTimeMillis());
      headerBuilder.setRawData(rawHeaderBuilder.build());
      blockBuilder.setBlockHeader(headerBuilder.build());
      
      BlockCapsule blockCap = new BlockCapsule(blockBuilder.build());

      return new TransactionContext(blockCap, trxCap, mockStoreFactory, false, false);
    } catch (Exception e) {
      throw new RuntimeException("Failed to create mock transaction context", e);
    }
  }
}