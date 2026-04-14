package org.tron.core.storage.spi;

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
 * Integration tests for StorageSPI with real gRPC server. Requires rust-storage-service to be
 * running on localhost:50011
 */
public class StorageSPIIntegrationTest {

  private static final int TIMEOUT_SECONDS = 10;

  private RemoteStorageSPI storage;
  private String testDbName;

  @Before
  public void setUp() throws Exception {
    // Read configuration properties at runtime
    String remoteHost = System.getProperty("storage.remote.host", "localhost");
    String remotePortStr = System.getProperty("storage.remote.port", "50011");

    int remotePort;
    try {
      remotePort = Integer.parseInt(remotePortStr);
    } catch (NumberFormatException e) {
      System.err.println("Invalid port value: " + remotePortStr + ", using default: 50011");
      remotePort = 50011;
    }

    System.out.println("Remote storage configuration:");
    System.out.println("  Host: " + remoteHost);
    System.out.println("  Port: " + remotePort);

    // Check if gRPC server is available
    storage = new RemoteStorageSPI(remoteHost, remotePort);
    testDbName = "test-db-" + System.currentTimeMillis();

    try {
      // Test server connectivity
      CompletableFuture<HealthStatus> healthFuture = storage.healthCheck();
      HealthStatus health = healthFuture.get(5, TimeUnit.SECONDS);
      Assume.assumeTrue("gRPC server not available", health != null);

      // Initialize test database
      StorageConfig config = new StorageConfig("ROCKSDB");
      config.setMaxOpenFiles(1000);
      config.setBlockCacheSize(16 * 1024 * 1024); // 16MB
      config.addEngineOption("write_buffer_size", "64MB");
      config.addEngineOption("compression_type", "snappy");

      storage.initDB(testDbName, config).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    } catch (TimeoutException | ExecutionException e) {
      Assume.assumeNoException("gRPC server not responding", e);
    }
  }

  @After
  public void tearDown() throws Exception {
    if (storage != null) {
      try {
        // Clean up test database
        storage.resetDB(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        storage.close();
      } catch (Exception e) {
        // Ignore cleanup errors
      }
    }
  }

  @Test
  public void testBasicOperations() throws Exception {
    byte[] key = "test-key".getBytes();
    byte[] value = "test-value".getBytes();

    // Test put operation
    storage.put(testDbName, key, value).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Test get operation
    byte[] retrievedValue = storage.get(testDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertArrayEquals("Retrieved value should match stored value", value, retrievedValue);

    // Test has operation
    Boolean hasKey = storage.has(testDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertTrue("Key should exist", hasKey);

    // Test delete operation
    storage.delete(testDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Verify deletion
    byte[] deletedValue = storage.get(testDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertNull("Value should be null after deletion", deletedValue);

    Boolean hasDeletedKey = storage.has(testDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertFalse("Key should not exist after deletion", hasDeletedKey);
  }

  @Test
  public void testBatchOperations() throws Exception {
    Map<byte[], byte[]> batchData = new HashMap<>();
    for (int i = 0; i < 10; i++) {
      batchData.put(("key-" + i).getBytes(), ("value-" + i).getBytes());
    }

    // Test batch write
    storage.batchWrite(testDbName, batchData).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Test batch get
    List<byte[]> keys =
        Arrays.asList(
            "key-0".getBytes(),
            "key-5".getBytes(),
            "key-9".getBytes(),
            "non-existent-key".getBytes());

    Map<byte[], byte[]> batchResult =
        storage.batchGet(testDbName, keys).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertEquals("Should return 4 results", 4, batchResult.size());

    // Verify retrieved values using iteration (to avoid byte[] key matching issues)
    boolean foundKey0 = false;
    boolean foundKey5 = false;
    boolean foundKey9 = false;
    boolean foundNonExistent = false;

    for (Map.Entry<byte[], byte[]> entry : batchResult.entrySet()) {
      String keyStr = new String(entry.getKey());
      byte[] value = entry.getValue();

      switch (keyStr) {
        case "key-0":
          Assert.assertArrayEquals("value-0".getBytes(), value);
          foundKey0 = true;
          break;
        case "key-5":
          Assert.assertArrayEquals("value-5".getBytes(), value);
          foundKey5 = true;
          break;
        case "key-9":
          Assert.assertArrayEquals("value-9".getBytes(), value);
          foundKey9 = true;
          break;
        case "non-existent-key":
          Assert.assertNull("Non-existent key should return null", value);
          foundNonExistent = true;
          break;
        default:
          // No action needed for unexpected keys
          break;
      }
    }

    Assert.assertTrue("Should find key-0", foundKey0);
    Assert.assertTrue("Should find key-5", foundKey5);
    Assert.assertTrue("Should find key-9", foundKey9);
    Assert.assertTrue("Should find non-existent-key", foundNonExistent);
  }

  @Test
  public void testDatabaseManagement() throws Exception {
    // Test database alive check
    Boolean isAlive = storage.isAlive(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertTrue("Database should be alive", isAlive);

    // Add some data
    storage
        .put(testDbName, "key1".getBytes(), "value1".getBytes())
        .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    storage
        .put(testDbName, "key2".getBytes(), "value2".getBytes())
        .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Test size operation
    Long size = storage.size(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertTrue("Size should be greater than 0", size > 0);

    // Test isEmpty operation
    Boolean isEmpty = storage.isEmpty(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertFalse("Database should not be empty", isEmpty);

    // Test stats operation
    StorageStats stats = storage.getStats(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertNotNull("Stats should not be null", stats);
    Assert.assertTrue("Stats should show keys > 0", stats.getTotalKeys() > 0);
  }

  @Test
  public void testTransactionOperations() throws Exception {
    // Begin transaction
    String transactionId =
        storage.beginTransaction(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertNotNull("Transaction ID should not be null", transactionId);
    Assert.assertFalse("Transaction ID should not be empty", transactionId.isEmpty());

    // Commit transaction (simplified test)
    storage.commitTransaction(transactionId).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Test rollback with new transaction
    String rollbackTransactionId =
        storage.beginTransaction(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    storage.rollbackTransaction(rollbackTransactionId).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
  }

  /**
   * Storage snapshot APIs are explicitly UNSUPPORTED in close_loop Phase 1.
   *
   * <p>This test was previously a happy-path test against the fake-success
   * placeholder behavior (which silently read from the live DB). After the
   * Phase 1 hardening described in {@code planning/close_loop.snapshot.md},
   * every snapshot method on the Rust storage engine and every snapshot
   * method on the Java {@link RemoteStorageSPI} client must surface an
   * explicit error rather than fake success. This test asserts that
   * contract: each snapshot call must complete exceptionally, and the
   * cause chain must contain {@link UnsupportedOperationException}, so a
   * transport hiccup or unrelated runtime error cannot accidentally
   * satisfy the assertion.
   */
  @Test
  public void testSnapshotOperationsAreUnsupported() throws Exception {
    // Add some data first so the test would otherwise have something to read.
    storage
        .put(testDbName, "snapshot-key".getBytes(), "snapshot-value".getBytes())
        .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // createSnapshot must fail — no fake snapshot id may be returned.
    try {
      storage.createSnapshot(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      Assert.fail(
          "createSnapshot must throw in Phase 1 — see planning/close_loop.snapshot.md");
    } catch (ExecutionException expected) {
      assertCauseChainContainsUnsupported("createSnapshot", expected);
    }

    // getFromSnapshot must also fail. Use any opaque id — the implementation
    // is expected to reject the call before it even consults the id.
    try {
      storage
          .getFromSnapshot("placeholder-snapshot-id", "snapshot-key".getBytes())
          .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      Assert.fail(
          "getFromSnapshot must throw in Phase 1 — see planning/close_loop.snapshot.md");
    } catch (ExecutionException expected) {
      assertCauseChainContainsUnsupported("getFromSnapshot", expected);
    }

    // deleteSnapshot must also fail explicitly rather than completing silently.
    try {
      storage
          .deleteSnapshot("placeholder-snapshot-id")
          .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      Assert.fail(
          "deleteSnapshot must throw in Phase 1 — see planning/close_loop.snapshot.md");
    } catch (ExecutionException expected) {
      assertCauseChainContainsUnsupported("deleteSnapshot", expected);
    }
  }

  /**
   * Walk the cause chain of {@code thrown} and assert that at least one
   * link is an {@link UnsupportedOperationException}. This protects the
   * Phase 1 snapshot contract from being satisfied by an unrelated
   * transport or runtime failure.
   */
  private static void assertCauseChainContainsUnsupported(
      String methodName, Throwable thrown) {
    Throwable cause = thrown;
    while (cause != null) {
      if (cause instanceof UnsupportedOperationException) {
        return;
      }
      cause = cause.getCause();
    }
    Assert.fail(
        methodName
            + " must surface UnsupportedOperationException in its cause chain "
            + "(see planning/close_loop.snapshot.md); actual: "
            + thrown);
  }

  @Test
  public void testIteratorOperations() throws Exception {
    // Add test data
    Map<byte[], byte[]> testData = new HashMap<>();
    for (int i = 0; i < 5; i++) {
      testData.put(
          ("iter-key-" + String.format("%03d", i)).getBytes(), ("iter-value-" + i).getBytes());
    }
    storage.batchWrite(testDbName, testData).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Test getNext operation
    Map<byte[], byte[]> nextEntries =
        storage
            .getNext(testDbName, "iter-key-".getBytes(), 3)
            .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertTrue("Should return at least 1 entry", nextEntries.size() >= 1);
    Assert.assertTrue("Should return at most 3 entries", nextEntries.size() <= 3);

    // Test getKeysNext operation
    List<byte[]> nextKeys =
        storage
            .getKeysNext(testDbName, "iter-key-".getBytes(), 3)
            .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertTrue("Should return at least 1 key", nextKeys.size() >= 1);
    Assert.assertTrue("Should return at most 3 keys", nextKeys.size() <= 3);

    // Test getValuesNext operation
    List<byte[]> nextValues =
        storage
            .getValuesNext(testDbName, "iter-key-".getBytes(), 3)
            .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertTrue("Should return at least 1 value", nextValues.size() >= 1);
    Assert.assertTrue("Should return at most 3 values", nextValues.size() <= 3);

    // Test prefixQuery operation
    Map<byte[], byte[]> prefixResults =
        storage
            .prefixQuery(testDbName, "iter-key-".getBytes())
            .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertTrue("Prefix query should return results", prefixResults.size() >= 1);
  }

  @Test
  public void testHealthAndMetadata() throws Exception {
    // Test health check
    HealthStatus health = storage.healthCheck().get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertNotNull("Health status should not be null", health);

    // Test list databases
    List<String> databases = storage.listDatabases().get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    Assert.assertNotNull("Database list should not be null", databases);
    Assert.assertTrue("Should contain our test database", databases.contains(testDbName));
  }

  @Test
  public void testErrorHandling() throws Exception {
    // // Test operations on non-existent database
    // String nonExistentDb = "non-existent-db-" + System.currentTimeMillis();

    // try {
    //   storage.get(nonExistentDb, "key".getBytes()).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    //   Assert.fail("Should throw exception for non-existent database");
    // } catch (ExecutionException e) {
    //   Assert.assertTrue("Should wrap gRPC exception", e.getCause() instanceof RuntimeException);
    // }

    // Test invalid snapshot operations
    try {
      storage
          .getFromSnapshot("invalid-snapshot-id", "key".getBytes())
          .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      Assert.fail("Should throw exception for invalid snapshot");
    } catch (ExecutionException e) {
      Assert.assertTrue("Should wrap gRPC exception", e.getCause() instanceof RuntimeException);
    }
  }
}
