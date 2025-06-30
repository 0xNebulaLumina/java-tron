package org.tron.core.storage.spi;

import java.io.File;
import org.junit.Test;

/**
 * Performance benchmark tests for embedded RocksDB StorageSPI implementation. Tests embedded
 * storage performance to compare against gRPC implementation.
 */
public class EmbeddedStoragePerformanceBenchmark extends BasePerformanceBenchmark {

  private EmbeddedStorageSPI embeddedStorage;
  private String dataDir;

  @Override
  protected StorageSPI createStorageImplementation() throws Exception {
    dataDir = "data/rocksdb-embedded/" + testTimestamp;
    new File(dataDir).mkdirs();
    embeddedStorage = new EmbeddedStorageSPI(dataDir);
    return embeddedStorage;
  }

  @Override
  protected void initializeStorage(StorageConfig config) throws Exception {
    storage
        .initDB(benchmarkDbName, config)
        .get(TIMEOUT_SECONDS, java.util.concurrent.TimeUnit.SECONDS);
  }

  @Override
  protected void cleanupStorage() throws Exception {
    storage.resetDB(benchmarkDbName).get(TIMEOUT_SECONDS, java.util.concurrent.TimeUnit.SECONDS);
    if (embeddedStorage != null) {
      embeddedStorage.close();
    }

    // Clean up data directory
    if (dataDir != null) {
      deleteDirectory(new File(dataDir));
    }
  }

  @Override
  protected String getImplementationName() {
    return "Embedded";
  }

  @Override
  protected double getExpectedPutLatencyMs() {
    return 5.0; // 5ms threshold for embedded PUT operations
  }

  @Override
  protected double getExpectedGetLatencyMs() {
    return 2.0; // 2ms threshold for embedded GET operations
  }

  // Test methods are inherited from BasePerformanceBenchmark
  @Test
  public void benchmarkSingleOperationLatency() throws Exception {
    super.benchmarkSingleOperationLatency();
  }

  @Test
  public void benchmarkBatchOperationThroughput() throws Exception {
    super.benchmarkBatchOperationThroughput();
  }

  @Test
  public void generatePerformanceReport() throws Exception {
    super.generatePerformanceReport();
  }

  // Additional embedded-specific benchmarks can be added here

  /** Recursively delete a directory and all its contents. */
  private void deleteDirectory(File directory) {
    if (directory.exists()) {
      File[] files = directory.listFiles();
      if (files != null) {
        for (File file : files) {
          if (file.isDirectory()) {
            deleteDirectory(file);
          } else {
            file.delete();
          }
        }
      }
      directory.delete();
    }
  }
}
