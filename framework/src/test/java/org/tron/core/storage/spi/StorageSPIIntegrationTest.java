package org.tron.core.storage.spi;

import org.junit.Test;
import org.junit.Before;
import org.junit.After;
import org.junit.Assert;
import org.junit.Assume;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import java.util.Map;
import java.util.HashMap;
import java.util.List;
import java.util.Arrays;
import java.util.Random;

/**
 * Integration tests for StorageSPI with real gRPC server.
 * Requires rust-storage-service to be running on localhost:50051
 */
public class StorageSPIIntegrationTest {
    
    private static final String GRPC_HOST = System.getProperty("storage.grpc.host", "localhost");
    private static final int GRPC_PORT = Integer.parseInt(System.getProperty("storage.grpc.port", "50051"));
    private static final int TIMEOUT_SECONDS = 10;
    
    private GrpcStorageSPI storage;
    private String testDbName;
    
    @Before
    public void setUp() throws Exception {
        // Check if gRPC server is available
        storage = new GrpcStorageSPI(GRPC_HOST, GRPC_PORT);
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
        List<byte[]> keys = Arrays.asList(
            "key-0".getBytes(),
            "key-5".getBytes(),
            "key-9".getBytes(),
            "non-existent-key".getBytes()
        );
        
        Map<byte[], byte[]> batchResult = storage.batchGet(testDbName, keys).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        Assert.assertEquals("Should return 4 results", 4, batchResult.size());
        
        // Verify retrieved values
        Assert.assertArrayEquals("value-0".getBytes(), batchResult.get("key-0".getBytes()));
        Assert.assertArrayEquals("value-5".getBytes(), batchResult.get("key-5".getBytes()));
        Assert.assertArrayEquals("value-9".getBytes(), batchResult.get("key-9".getBytes()));
        Assert.assertNull("Non-existent key should return null", batchResult.get("non-existent-key".getBytes()));
    }
    
    @Test
    public void testDatabaseManagement() throws Exception {
        // Test database alive check
        Boolean isAlive = storage.isAlive(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        Assert.assertTrue("Database should be alive", isAlive);
        
        // Add some data
        storage.put(testDbName, "key1".getBytes(), "value1".getBytes()).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        storage.put(testDbName, "key2".getBytes(), "value2".getBytes()).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        
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
        String transactionId = storage.beginTransaction(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        Assert.assertNotNull("Transaction ID should not be null", transactionId);
        Assert.assertFalse("Transaction ID should not be empty", transactionId.isEmpty());
        
        // Commit transaction (simplified test)
        storage.commitTransaction(transactionId).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        
        // Test rollback with new transaction
        String rollbackTransactionId = storage.beginTransaction(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        storage.rollbackTransaction(rollbackTransactionId).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    }
    
    @Test
    public void testSnapshotOperations() throws Exception {
        // Add some data
        storage.put(testDbName, "snapshot-key".getBytes(), "snapshot-value".getBytes()).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        
        // Create snapshot
        String snapshotId = storage.createSnapshot(testDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        Assert.assertNotNull("Snapshot ID should not be null", snapshotId);
        Assert.assertFalse("Snapshot ID should not be empty", snapshotId.isEmpty());
        
        // Read from snapshot
        byte[] snapshotValue = storage.getFromSnapshot(snapshotId, "snapshot-key".getBytes()).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        Assert.assertArrayEquals("Snapshot value should match", "snapshot-value".getBytes(), snapshotValue);
        
        // Delete snapshot
        storage.deleteSnapshot(snapshotId).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    }
    
    @Test
    public void testIteratorOperations() throws Exception {
        // Add test data
        Map<byte[], byte[]> testData = new HashMap<>();
        for (int i = 0; i < 5; i++) {
            testData.put(("iter-key-" + String.format("%03d", i)).getBytes(), ("iter-value-" + i).getBytes());
        }
        storage.batchWrite(testDbName, testData).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        
        // Test getNext operation
        Map<byte[], byte[]> nextEntries = storage.getNext(testDbName, "iter-key-".getBytes(), 3).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        Assert.assertTrue("Should return at least 1 entry", nextEntries.size() >= 1);
        Assert.assertTrue("Should return at most 3 entries", nextEntries.size() <= 3);
        
        // Test getKeysNext operation
        List<byte[]> nextKeys = storage.getKeysNext(testDbName, "iter-key-".getBytes(), 3).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        Assert.assertTrue("Should return at least 1 key", nextKeys.size() >= 1);
        Assert.assertTrue("Should return at most 3 keys", nextKeys.size() <= 3);
        
        // Test getValuesNext operation
        List<byte[]> nextValues = storage.getValuesNext(testDbName, "iter-key-".getBytes(), 3).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        Assert.assertTrue("Should return at least 1 value", nextValues.size() >= 1);
        Assert.assertTrue("Should return at most 3 values", nextValues.size() <= 3);
        
        // Test prefixQuery operation
        Map<byte[], byte[]> prefixResults = storage.prefixQuery(testDbName, "iter-key-".getBytes()).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
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
        // Test operations on non-existent database
        String nonExistentDb = "non-existent-db-" + System.currentTimeMillis();
        
        try {
            storage.get(nonExistentDb, "key".getBytes()).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
            Assert.fail("Should throw exception for non-existent database");
        } catch (ExecutionException e) {
            Assert.assertTrue("Should wrap gRPC exception", e.getCause() instanceof RuntimeException);
        }
        
        // Test invalid snapshot operations
        try {
            storage.getFromSnapshot("invalid-snapshot-id", "key".getBytes()).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
            Assert.fail("Should throw exception for invalid snapshot");
        } catch (ExecutionException e) {
            Assert.assertTrue("Should wrap gRPC exception", e.getCause() instanceof RuntimeException);
        }
    }
} 