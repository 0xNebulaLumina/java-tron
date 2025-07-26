package org.tron.core.storage.spi;

/**
 * Tron workload benchmark for embedded RocksDB storage implementation. Tests realistic blockchain
 * operations against the embedded RocksDB storage.
 */
public class EmbeddedTronWorkloadBenchmark extends TronWorkloadBenchmark {

  @Override
  protected StorageSPI createStorageImplementation() throws Exception {
    String basePath = "data/embedded-tron-workload-" + System.currentTimeMillis();
    return new EmbeddedStorageSPI(basePath);
  }

  @Override
  protected String getImplementationName() {
    return "EmbeddedStorage";
  }

  @Override
  protected double getExpectedPutLatencyMs() {
    return 1.0; // Embedded storage expected latency
  }

  @Override
  protected double getExpectedGetLatencyMs() {
    return 0.5; // Embedded storage expected latency
  }

  @Override
  protected void initializeStorage(StorageConfig config) throws Exception {
    // Initialize storage implementation first
    StorageConfig embeddedConfig = new StorageConfig("ROCKSDB");
    embeddedConfig.setMaxOpenFiles(2000);
    embeddedConfig.setBlockCacheSize(64 * 1024 * 1024); // 64MB
    embeddedConfig.addEngineOption("write_buffer_size", "128MB");
    embeddedConfig.addEngineOption("max_write_buffer_number", "4");
    embeddedConfig.addEngineOption("compression_type", "snappy");
    embeddedConfig.addEngineOption("bloom_filter_bits_per_key", "10");

    // Initialize the benchmark database
    storage.initDB(benchmarkDbName, embeddedConfig).get(30, java.util.concurrent.TimeUnit.SECONDS);

    // Initialize Tron-specific databases
    initializeTronDatabases(config);
  }

  @Override
  protected void cleanupStorage() throws Exception {
    // Clean up Tron databases
    cleanupTronDatabases();

    // Clean up benchmark database
    if (storage != null) {
      try {
        storage.resetDB(benchmarkDbName).get(10, java.util.concurrent.TimeUnit.SECONDS);
      } catch (Exception e) {
        // Ignore cleanup errors
      }
    }
  }
}
