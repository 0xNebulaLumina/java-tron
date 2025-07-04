package org.tron.core.storage.spi;

import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import org.junit.Assert;
import org.junit.Test;

/** Basic test for StorageSPI implementation. */
public class StorageSPITest {

  @Test
  public void testGrpcStorageSPIBasicOperations() throws ExecutionException, InterruptedException {
    // Create a gRPC storage client (will fail to connect but should not crash)
    GrpcStorageSPI storage = new GrpcStorageSPI("localhost", 50011);

    try {
      // Test basic operations (these are placeholder implementations)
      String dbName = "test-db";
      byte[] key = "test-key".getBytes();
      byte[] value = "test-value".getBytes();

      // Test configuration
      StorageConfig config = new StorageConfig("ROCKSDB");
      config.setMaxOpenFiles(1000);
      config.addEngineOption("write_buffer_size", "64MB");

      Assert.assertEquals("ROCKSDB", config.getEngine());
      Assert.assertEquals(1000, config.getMaxOpenFiles());
      Assert.assertTrue(config.getEngineOptions().containsKey("write_buffer_size"));

      // Test basic async operations (these will complete immediately with placeholder values)
      CompletableFuture<Void> initFuture = storage.initDB(dbName, config);
      initFuture.get(); // Should complete without error

      CompletableFuture<Boolean> aliveFuture = storage.isAlive(dbName);
      Boolean isAlive = aliveFuture.get();
      Assert.assertNotNull(isAlive);

      CompletableFuture<HealthStatus> healthFuture = storage.healthCheck();
      HealthStatus health = healthFuture.get();
      Assert.assertNotNull(health);

      // Test stats
      StorageStats stats = new StorageStats();
      stats.setTotalKeys(100);
      stats.setTotalSize(1024);
      stats.addEngineStat("test-stat", "test-value");

      Assert.assertEquals(100, stats.getTotalKeys());
      Assert.assertEquals(1024, stats.getTotalSize());
      Assert.assertTrue(stats.getEngineStats().containsKey("test-stat"));

    } finally {
      storage.close();
    }
  }

  @Test
  public void testStorageConfigBuilder() {
    StorageConfig config = new StorageConfig();
    config.setEngine("ROCKSDB");
    config.setEnableStatistics(true);
    config.setBlockCacheSize(16 * 1024 * 1024); // 16MB
    config.addEngineOption("compression_type", "snappy");
    config.addEngineOption("max_write_buffer_number", "4");

    Assert.assertEquals("ROCKSDB", config.getEngine());
    Assert.assertTrue(config.isEnableStatistics());
    Assert.assertEquals(16 * 1024 * 1024, config.getBlockCacheSize());
    Assert.assertEquals("snappy", config.getEngineOptions().get("compression_type"));
    Assert.assertEquals("4", config.getEngineOptions().get("max_write_buffer_number"));
  }

  @Test
  public void testHealthStatusEnum() {
    Assert.assertEquals(3, HealthStatus.values().length);
    Assert.assertNotNull(HealthStatus.HEALTHY);
    Assert.assertNotNull(HealthStatus.DEGRADED);
    Assert.assertNotNull(HealthStatus.UNHEALTHY);
  }

  @Test
  public void testHealthCheck() throws ExecutionException, InterruptedException {
    // Simple health check test for script validation
    String host = System.getProperty("storage.grpc.host", "localhost");
    int port = Integer.parseInt(System.getProperty("storage.grpc.port", "50011"));

    GrpcStorageSPI storage = new GrpcStorageSPI(host, port);

    try {
      // Try to connect and perform health check
      CompletableFuture<HealthStatus> healthFuture = storage.healthCheck();
      HealthStatus health = healthFuture.get();
      Assert.assertNotNull(health);
      System.out.println("Health check successful: " + health);
    } finally {
      storage.close();
    }
  }
}
