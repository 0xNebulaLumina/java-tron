package org.tron.core.storage.spi;

import java.io.File;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import org.junit.After;
import org.junit.Assert;
import org.junit.Assume;
import org.junit.Before;
import org.junit.Test;

/**
 * Integration tests for dual storage mode functionality. Tests both embedded and remote storage
 * implementations through the factory.
 */
public class DualStorageModeIntegrationTest {

  private static final int TIMEOUT_SECONDS = 10;
  private String testDbName;
  private String originalSystemProperty;

  @Before
  public void setUp() {
    testDbName = "dual-mode-test-" + System.currentTimeMillis();
    originalSystemProperty = System.getProperty("storage.mode");
  }

  @After
  public void tearDown() {
    // Restore original system property
    if (originalSystemProperty != null) {
      System.setProperty("storage.mode", originalSystemProperty);
    } else {
      System.clearProperty("storage.mode");
    }

    // Clear test properties
    System.clearProperty("storage.embedded.basePath");
    System.clearProperty("storage.grpc.host");
    System.clearProperty("storage.grpc.port");
  }

  @Test
  public void testEmbeddedStorageMode() throws Exception {
    // Configure for embedded mode
    System.setProperty("storage.mode", "embedded");
    System.setProperty("storage.embedded.basePath", "test-data/dual-mode-embedded");

    StorageSPI storage = StorageSpiFactory.createStorage();
    Assert.assertTrue("Should create EmbeddedStorageSPI", storage instanceof EmbeddedStorageSPI);

    try {
      // Test basic functionality
      testBasicStorageOperations(storage);
      testBatchOperations(storage);
      testDatabaseManagement(storage);

    } finally {
      if (storage instanceof EmbeddedStorageSPI) {
        ((EmbeddedStorageSPI) storage).close();
      }

      // Clean up test data
      cleanupTestData("test-data/dual-mode-embedded");
    }
  }

  @Test
  public void testRemoteStorageMode() throws Exception {
    // Configure for remote mode
    System.setProperty("storage.mode", "remote");
    System.setProperty("storage.grpc.host", "localhost");
    System.setProperty("storage.grpc.port", "50051");

    StorageSPI storage = StorageSpiFactory.createStorage();
    Assert.assertTrue("Should create GrpcStorageSPI", storage instanceof GrpcStorageSPI);

    try {
      // Check if gRPC server is available
      CompletableFuture<HealthStatus> healthFuture = storage.healthCheck();
      HealthStatus health = healthFuture.get(5, TimeUnit.SECONDS);
      Assume.assumeTrue("gRPC server not available for testing", health != null);

      // Test basic functionality
      testBasicStorageOperations(storage);
      testBatchOperations(storage);
      testDatabaseManagement(storage);

    } catch (TimeoutException | ExecutionException e) {
      Assume.assumeNoException("gRPC server not responding", e);
    } finally {
      if (storage instanceof GrpcStorageSPI) {
        ((GrpcStorageSPI) storage).close();
      }
    }
  }

  @Test
  public void testFactoryConfigurationInfo() {
    // Test embedded mode info
    System.setProperty("storage.mode", "embedded");
    System.setProperty("storage.embedded.basePath", "test-path");

    String info = StorageSpiFactory.getConfigurationInfo();
    Assert.assertTrue("Should contain mode info", info.contains("Mode: embedded"));
    Assert.assertTrue("Should contain base path", info.contains("Base Path: test-path"));

    // Test remote mode info
    System.setProperty("storage.mode", "remote");
    System.setProperty("storage.grpc.host", "test-host");
    System.setProperty("storage.grpc.port", "9999");

    info = StorageSpiFactory.getConfigurationInfo();
    Assert.assertTrue("Should contain mode info", info.contains("Mode: remote"));
    Assert.assertTrue("Should contain host info", info.contains("gRPC Host: test-host"));
    Assert.assertTrue("Should contain port info", info.contains("gRPC Port: 9999"));
  }

  @Test
  public void testModeSwitching() throws Exception {
    // Test switching between modes
    System.setProperty("storage.mode", "embedded");
    System.setProperty("storage.embedded.basePath", "test-data/mode-switch");

    StorageSPI embeddedStorage = StorageSpiFactory.createStorage();
    Assert.assertTrue(
        "Should create EmbeddedStorageSPI", embeddedStorage instanceof EmbeddedStorageSPI);

    // Clean up embedded storage
    if (embeddedStorage instanceof EmbeddedStorageSPI) {
      ((EmbeddedStorageSPI) embeddedStorage).close();
    }

    // Switch to remote mode
    System.setProperty("storage.mode", "remote");
    StorageSPI remoteStorage = StorageSpiFactory.createStorage();
    Assert.assertTrue("Should create GrpcStorageSPI", remoteStorage instanceof GrpcStorageSPI);

    // Clean up remote storage
    if (remoteStorage instanceof GrpcStorageSPI) {
      ((GrpcStorageSPI) remoteStorage).close();
    }

    // Clean up test data
    cleanupTestData("test-data/mode-switch");
  }

  private void testBasicStorageOperations(StorageSPI storage) throws Exception {
    // Initialize database
    StorageConfig config = new StorageConfig("ROCKSDB");
    config.setMaxOpenFiles(1000);
    storage.initDB(testDbName, config).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Test basic operations
    byte[] key = "test-key".getBytes();
    byte[] value = "test-value".getBytes();

    // Put operation
    storage.put(testDbName, key, value).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Get operation
    byte[] retrievedValue = storage.get(testDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertArrayEquals("Retrieved value should match", value, retrievedValue);

    // Has operation
    Boolean exists = storage.has(testDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertTrue("Key should exist", exists);

    // Delete operation
    storage.delete(testDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Verify deletion
    byte[] deletedValue = storage.get(testDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertNull("Value should be null after deletion", deletedValue);
  }

  private void testBatchOperations(StorageSPI storage) throws Exception {
    // Test batch write
    Map<byte[], byte[]> batchData = new HashMap<>();
    batchData.put("batch-key-1".getBytes(), "batch-value-1".getBytes());
    batchData.put("batch-key-2".getBytes(), "batch-value-2".getBytes());
    batchData.put("batch-key-3".getBytes(), "batch-value-3".getBytes());

    storage.batchWrite(testDbName, batchData).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Test batch get
    List<byte[]> keys =
        Arrays.asList(
            "batch-key-1".getBytes(),
            "batch-key-2".getBytes(),
            "batch-key-3".getBytes(),
            "non-existent-key".getBytes());

    Map<byte[], byte[]> results =
        storage.batchGet(testDbName, keys).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertEquals("Should return 4 results", 4, results.size());

    // Verify batch results
    Assert.assertArrayEquals("batch-value-1".getBytes(), results.get("batch-key-1".getBytes()));
    Assert.assertArrayEquals("batch-value-2".getBytes(), results.get("batch-key-2".getBytes()));
    Assert.assertArrayEquals("batch-value-3".getBytes(), results.get("batch-key-3".getBytes()));
    Assert.assertNull(
        "Non-existent key should return null", results.get("non-existent-key".getBytes()));
  }

  private void testDatabaseManagement(StorageSPI storage) throws Exception {
    // Test database status
    Boolean isAlive = storage.isAlive(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertTrue("Database should be alive", isAlive);

    // Test size operations
    Long size = storage.size(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertTrue("Size should be non-negative", size >= 0);

    // Test health check
    HealthStatus health = storage.healthCheck().get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertNotNull("Health status should not be null", health);

    // Test list databases
    List<String> databases = storage.listDatabases().get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertTrue("Should contain test database", databases.contains(testDbName));

    // Test stats
    StorageStats stats = storage.getStats(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertNotNull("Stats should not be null", stats);
    Assert.assertTrue("Total keys should be non-negative", stats.getTotalKeys() >= 0);
  }

  private void cleanupTestData(String path) {
    try {
      File testDir = new File(path);
      if (testDir.exists()) {
        deleteDirectory(testDir);
      }
    } catch (Exception e) {
      // Ignore cleanup errors
    }
  }

  private void deleteDirectory(File dir) {
    if (dir.isDirectory()) {
      File[] files = dir.listFiles();
      if (files != null) {
        for (File file : files) {
          deleteDirectory(file);
        }
      }
    }
    dir.delete();
  }
}
