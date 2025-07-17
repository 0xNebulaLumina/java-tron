package org.tron.core.execution.spi;

import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.mockito.Mock;
import org.mockito.MockitoAnnotations;
import org.mockito.Mockito;
import org.tron.core.db.TransactionContext;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.protos.Protocol;

/**
 * Test class for ShadowExecutionSPI.
 */
public class ShadowExecutionSPITest {

  @Mock
  private ExecutionSPI embeddedExecution;
  
  @Mock
  private ExecutionSPI remoteExecution;
  
  @Mock
  private TransactionContext context;
  
  @Mock
  private Protocol.Transaction.Contract.Builder contractBuilder;
  
  private ShadowExecutionSPI shadowExecution;

  @Before
  public void setUp() {
    MockitoAnnotations.openMocks(this);
    
    // Mock transaction context
    Protocol.Transaction.Builder txBuilder = Protocol.Transaction.newBuilder();
    txBuilder.getRawDataBuilder().setTimestamp(System.currentTimeMillis());
    Protocol.Transaction tx = txBuilder.build();
    TransactionCapsule txCapsule = new TransactionCapsule(tx);

    Mockito.when(context.getTrxCap()).thenReturn(txCapsule);
    
    // Create shadow execution with mocked implementations
    shadowExecution = new ShadowExecutionSPI(embeddedExecution, remoteExecution);
  }

  @After
  public void tearDown() {
    if (shadowExecution != null) {
      shadowExecution.cleanup();
    }
  }

  @Test
  public void testShadowExecutionCreation() {
    Assert.assertNotNull(shadowExecution);
    String stats = shadowExecution.getMismatchStats();
    Assert.assertTrue(stats.contains("0 total"));
  }

  @Test
  public void testExecuteTransactionSuccess() throws Exception {
    // Create matching execution results
    ExecutionSPI.ExecutionResult embeddedResult = createSuccessResult();
    ExecutionSPI.ExecutionResult remoteResult = createSuccessResult();
    
    // Mock both executions to return successful results
    Mockito.when(embeddedExecution.executeTransaction(context))
        .thenReturn(CompletableFuture.completedFuture(embeddedResult));
    Mockito.when(remoteExecution.executeTransaction(context))
        .thenReturn(CompletableFuture.completedFuture(remoteResult));
    
    // Execute transaction
    CompletableFuture<ExecutionSPI.ExecutionResult> future = shadowExecution.executeTransaction(context);
    ExecutionSPI.ExecutionResult result = future.get();

    // Should return embedded result
    Assert.assertNotNull(result);
    Assert.assertTrue(result.isSuccess());
    Assert.assertEquals(1000, result.getEnergyUsed());
    
    // Verify both executions were called
    Mockito.verify(embeddedExecution).executeTransaction(context);
    Mockito.verify(remoteExecution).executeTransaction(context);
  }

  @Test
  public void testExecuteTransactionMismatch() throws Exception {
    // Create mismatched execution results
    ExecutionSPI.ExecutionResult embeddedResult = createSuccessResult();
    ExecutionSPI.ExecutionResult remoteResult = createFailureResult();
    
    // Mock executions to return different results
    Mockito.when(embeddedExecution.executeTransaction(context))
        .thenReturn(CompletableFuture.completedFuture(embeddedResult));
    Mockito.when(remoteExecution.executeTransaction(context))
        .thenReturn(CompletableFuture.completedFuture(remoteResult));
    
    // Execute transaction
    CompletableFuture<ExecutionSPI.ExecutionResult> future = shadowExecution.executeTransaction(context);
    ExecutionSPI.ExecutionResult result = future.get();
    
    // Should still return embedded result
    Assert.assertNotNull(result);
    Assert.assertTrue(result.isSuccess());
    
    // Check mismatch stats
    String stats = shadowExecution.getMismatchStats();
    Assert.assertTrue(stats.contains("1 total"));
    Assert.assertTrue(stats.contains("1 mismatches"));
  }

  @Test
  public void testCallContractSuccess() throws Exception {
    // Create matching call results
    ExecutionSPI.ExecutionResult embeddedResult = createSuccessResult();
    ExecutionSPI.ExecutionResult remoteResult = createSuccessResult();
    
    // Mock both calls to return successful results
    Mockito.when(embeddedExecution.callContract(context))
        .thenReturn(CompletableFuture.completedFuture(embeddedResult));
    Mockito.when(remoteExecution.callContract(context))
        .thenReturn(CompletableFuture.completedFuture(remoteResult));
    
    // Call contract
    CompletableFuture<ExecutionSPI.ExecutionResult> future = shadowExecution.callContract(context);
    ExecutionSPI.ExecutionResult result = future.get();
    
    // Should return embedded result
    Assert.assertNotNull(result);
    Assert.assertTrue(result.isSuccess());
    
    // Verify both calls were made
    Mockito.verify(embeddedExecution).callContract(context);
    Mockito.verify(remoteExecution).callContract(context);
  }

  @Test
  public void testEstimateEnergySuccess() throws Exception {
    // Mock both estimations to return same value
    Mockito.when(embeddedExecution.estimateEnergy(context))
        .thenReturn(CompletableFuture.completedFuture(1000L));
    Mockito.when(remoteExecution.estimateEnergy(context))
        .thenReturn(CompletableFuture.completedFuture(1000L));
    
    // Estimate energy
    CompletableFuture<Long> future = shadowExecution.estimateEnergy(context);
    Long result = future.get();
    
    // Should return embedded result
    Assert.assertEquals(Long.valueOf(1000L), result);
    
    // Verify both estimations were called
    Mockito.verify(embeddedExecution).estimateEnergy(context);
    Mockito.verify(remoteExecution).estimateEnergy(context);
  }

  @Test
  public void testEstimateEnergyMismatch() throws Exception {
    // Mock estimations to return different values
    Mockito.when(embeddedExecution.estimateEnergy(context))
        .thenReturn(CompletableFuture.completedFuture(1000L));
    Mockito.when(remoteExecution.estimateEnergy(context))
        .thenReturn(CompletableFuture.completedFuture(1500L));
    
    // Estimate energy
    CompletableFuture<Long> future = shadowExecution.estimateEnergy(context);
    Long result = future.get();
    
    // Should still return embedded result
    Assert.assertEquals(Long.valueOf(1000L), result);
  }

  @Test
  public void testHealthCheck() throws Exception {
    // Mock health checks
    ExecutionSPI.HealthStatus embeddedHealth = new ExecutionSPI.HealthStatus(true, "Embedded OK");
    ExecutionSPI.HealthStatus remoteHealth = new ExecutionSPI.HealthStatus(true, "Remote OK");
    
    Mockito.when(embeddedExecution.healthCheck())
        .thenReturn(CompletableFuture.completedFuture(embeddedHealth));
    Mockito.when(remoteExecution.healthCheck())
        .thenReturn(CompletableFuture.completedFuture(remoteHealth));
    
    // Check health
    CompletableFuture<ExecutionSPI.HealthStatus> future = shadowExecution.healthCheck();
    ExecutionSPI.HealthStatus result = future.get();
    
    // Should indicate both are healthy
    Assert.assertNotNull(result);
    Assert.assertTrue(result.isHealthy());
    Assert.assertTrue(result.getMessage().contains("embedded=true"));
    Assert.assertTrue(result.getMessage().contains("remote=true"));
  }

  @Test
  public void testHealthCheckPartialFailure() throws Exception {
    // Mock health checks with one failure
    ExecutionSPI.HealthStatus embeddedHealth = new ExecutionSPI.HealthStatus(true, "Embedded OK");
    ExecutionSPI.HealthStatus remoteHealth = new ExecutionSPI.HealthStatus(false, "Remote Failed");
    
    Mockito.when(embeddedExecution.healthCheck())
        .thenReturn(CompletableFuture.completedFuture(embeddedHealth));
    Mockito.when(remoteExecution.healthCheck())
        .thenReturn(CompletableFuture.completedFuture(remoteHealth));
    
    // Check health
    CompletableFuture<ExecutionSPI.HealthStatus> future = shadowExecution.healthCheck();
    ExecutionSPI.HealthStatus result = future.get();
    
    // Should indicate overall unhealthy
    Assert.assertNotNull(result);
    Assert.assertFalse(result.isHealthy());
    Assert.assertTrue(result.getMessage().contains("embedded=true"));
    Assert.assertTrue(result.getMessage().contains("remote=false"));
  }

  @Test
  public void testReadOperationsDelegation() throws Exception {
    byte[] address = new byte[20];
    byte[] key = new byte[32];
    String snapshotId = "test_snapshot";
    
    // Mock read operations to return from embedded execution only
    Mockito.when(embeddedExecution.getCode(address, snapshotId))
        .thenReturn(CompletableFuture.completedFuture(new byte[]{1, 2, 3}));
    Mockito.when(embeddedExecution.getStorageAt(address, key, snapshotId))
        .thenReturn(CompletableFuture.completedFuture(new byte[]{4, 5, 6}));
    Mockito.when(embeddedExecution.getNonce(address, snapshotId))
        .thenReturn(CompletableFuture.completedFuture(42L));
    Mockito.when(embeddedExecution.getBalance(address, snapshotId))
        .thenReturn(CompletableFuture.completedFuture(new byte[]{7, 8, 9}));
    
    // Test read operations
    Assert.assertArrayEquals(new byte[]{1, 2, 3}, shadowExecution.getCode(address, snapshotId).get());
    Assert.assertArrayEquals(new byte[]{4, 5, 6}, shadowExecution.getStorageAt(address, key, snapshotId).get());
    Assert.assertEquals(Long.valueOf(42L), shadowExecution.getNonce(address, snapshotId).get());
    Assert.assertArrayEquals(new byte[]{7, 8, 9}, shadowExecution.getBalance(address, snapshotId).get());
    
    // Verify only embedded execution was called (not remote)
    Mockito.verify(embeddedExecution).getCode(address, snapshotId);
    Mockito.verify(embeddedExecution).getStorageAt(address, key, snapshotId);
    Mockito.verify(embeddedExecution).getNonce(address, snapshotId);
    Mockito.verify(embeddedExecution).getBalance(address, snapshotId);
    
    Mockito.verifyNoInteractions(remoteExecution);
  }

  @Test
  public void testMetricsCallback() {
    List<String> metricNames = new ArrayList<>();
    List<Double> metricValues = new ArrayList<>();
    
    // Register metrics callback
    shadowExecution.registerMetricsCallback((name, value) -> {
      metricNames.add(name);
      metricValues.add(value);
    });
    
    // Verify callback was registered with both underlying implementations
    Mockito.verify(embeddedExecution).registerMetricsCallback(Mockito.any());
    Mockito.verify(remoteExecution).registerMetricsCallback(Mockito.any());
  }

  // Helper methods

  private ExecutionSPI.ExecutionResult createSuccessResult() {
    return new ExecutionSPI.ExecutionResult(
        true, // success
        new byte[]{0x42}, // returnData
        1000, // energyUsed
        100, // energyRefunded
        new ArrayList<>(), // stateChanges
        new ArrayList<>(), // logs
        null, // errorMessage
        50 // bandwidthUsed
    );
  }

  private ExecutionSPI.ExecutionResult createFailureResult() {
    return new ExecutionSPI.ExecutionResult(
        false, // success
        new byte[0], // returnData
        500, // energyUsed
        0, // energyRefunded
        new ArrayList<>(), // stateChanges
        new ArrayList<>(), // logs
        "Execution failed", // errorMessage
        25 // bandwidthUsed
    );
  }
}
