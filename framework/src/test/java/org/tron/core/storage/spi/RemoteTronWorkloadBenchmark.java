package org.tron.core.storage.spi;

import java.util.concurrent.TimeUnit;
import org.junit.Before;

/**
 * Tron workload benchmark for remote gRPC storage implementation.
 * Tests realistic blockchain operations against the Rust gRPC storage service.
 */
public class RemoteTronWorkloadBenchmark extends TronWorkloadBenchmark {

  private String remoteHost;
  private int remotePort;

  @Before
  public void setUpRemote() throws Exception {
    remoteHost = System.getProperty("storage.remote.host", "localhost");
    remotePort = Integer.parseInt(System.getProperty("storage.remote.port", "50011"));
    System.out.println("Remote storage configuration:");
    System.out.println("  Host: " + remoteHost);
    System.out.println("  Port: " + remotePort);
  }

  @Override
  protected StorageSPI createStorageImplementation() throws Exception {
    return new RemoteStorageSPI(remoteHost, remotePort);
  }

  @Override
  protected String getImplementationName() {
    return "RemoteStorage";
  }

  @Override
  protected double getExpectedPutLatencyMs() {
    return 10.0; // Remote storage expected latency
  }

  @Override
  protected double getExpectedGetLatencyMs() {
    return 5.0; // Remote storage expected latency
  }

  @Override
  protected void initializeStorage(StorageConfig config) throws Exception {
    // Initialize storage implementation first
    StorageConfig remoteConfig = new StorageConfig("ROCKSDB");
    remoteConfig.setMaxOpenFiles(2000);
    remoteConfig.setBlockCacheSize(64 * 1024 * 1024); // 64MB
    remoteConfig.addEngineOption("write_buffer_size", "128MB");
    remoteConfig.addEngineOption("max_write_buffer_number", "4");
    remoteConfig.addEngineOption("compression_type", "snappy");
    remoteConfig.addEngineOption("bloom_filter_bits_per_key", "10");

    // Initialize the benchmark database
    storage.initDB(benchmarkDbName, remoteConfig).get(30, TimeUnit.SECONDS);
    
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
        storage.resetDB(benchmarkDbName).get(10, TimeUnit.SECONDS);
      } catch (Exception e) {
        // Ignore cleanup errors
      }
    }
  }
} 