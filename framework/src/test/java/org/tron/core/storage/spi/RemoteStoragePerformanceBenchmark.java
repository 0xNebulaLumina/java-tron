package org.tron.core.storage.spi;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import java.util.concurrent.atomic.AtomicLong;
import org.junit.Assert;
import org.junit.Assume;
import org.junit.Test;

/**
 * Performance benchmark tests for gRPC StorageSPI implementation. Tests gRPC storage performance
 * against baseline metrics.
 */
public class RemoteStoragePerformanceBenchmark extends BasePerformanceBenchmark {

  private static final String REMOTE_HOST = System.getProperty("storage.remote.host", "localhost");
  private static final int REMOTE_PORT =
      Integer.parseInt(System.getProperty("storage.remote.port", "50011"));

  @Override
  protected StorageSPI createStorageImplementation() throws Exception {
    return new RemoteStorageSPI(REMOTE_HOST, REMOTE_PORT);
  }

  @Override
  protected void initializeStorage(StorageConfig config) throws Exception {
    try {
      // Test server connectivity
      CompletableFuture<HealthStatus> healthFuture = storage.healthCheck();
      HealthStatus health = healthFuture.get(5, TimeUnit.SECONDS);
      Assume.assumeTrue("gRPC server not available", health != null);

      storage.initDB(benchmarkDbName, config).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    } catch (TimeoutException | ExecutionException e) {
      Assume.assumeNoException("gRPC server not responding", e);
    }
  }

  @Override
  protected void cleanupStorage() throws Exception {
    storage.resetDB(benchmarkDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    // gRPC storage cleanup is handled by the connection
  }

  @Override
  protected String getImplementationName() {
    return "Grpc";
  }

  @Override
  protected double getExpectedPutLatencyMs() {
    return 50.0; // 50ms threshold for gRPC PUT operations
  }

  @Override
  protected double getExpectedGetLatencyMs() {
    return 20.0; // 20ms threshold for gRPC GET operations
  }

  // Test methods inherited from BasePerformanceBenchmark need @Test annotations
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

  // These benchmark methods are inherited from BasePerformanceBenchmark:
  // - benchmarkSingleOperationLatency()
  // - benchmarkBatchOperationThroughput()
  // - generatePerformanceReport()

  @Test
  public void benchmarkConcurrentOperations() throws Exception {
    System.out.println("\n=== Concurrent Operations Benchmark ===");

    int totalOperations = 10000;
    int operationsPerThread = totalOperations / CONCURRENT_THREADS;

    // Prepare test data
    Map<byte[], byte[]> testData = new HashMap<>();
    for (int i = 0; i < totalOperations; i++) {
      testData.put(("concurrent-key-" + i).getBytes(), generateRandomValue(128));
    }

    // Pre-populate database
    storage.batchWrite(benchmarkDbName, testData).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Concurrent read test
    AtomicLong totalReadTime = new AtomicLong(0);
    AtomicLong successfulReads = new AtomicLong(0);

    List<CompletableFuture<Void>> readFutures = new ArrayList<>();
    long readTestStart = System.nanoTime();

    for (int t = 0; t < CONCURRENT_THREADS; t++) {
      final int threadId = t;
      CompletableFuture<Void> future =
          CompletableFuture.runAsync(
              () -> {
                try {
                  for (int i = 0; i < operationsPerThread; i++) {
                    byte[] key =
                        ("concurrent-key-" + (threadId * operationsPerThread + i)).getBytes();
                    long opStart = System.nanoTime();
                    byte[] result =
                        storage.get(benchmarkDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
                    long opEnd = System.nanoTime();

                    if (result != null) {
                      totalReadTime.addAndGet(opEnd - opStart);
                      successfulReads.incrementAndGet();
                    }
                  }
                } catch (Exception e) {
                  System.err.println("Thread " + threadId + " error: " + e.getMessage());
                }
              });
      readFutures.add(future);
    }

    // Wait for all read operations to complete
    CompletableFuture.allOf(readFutures.toArray(new CompletableFuture[0]))
        .get(TIMEOUT_SECONDS * 2, TimeUnit.SECONDS);
    long readTestEnd = System.nanoTime();

    double totalReadDurationMs = (readTestEnd - readTestStart) / 1_000_000.0;
    double readThroughput = successfulReads.get() / (totalReadDurationMs / 1000.0);
    double avgReadLatencyMs = (totalReadTime.get() / successfulReads.get()) / 1_000_000.0;

    System.out.printf(
        "Concurrent reads: %d ops in %.2f ms\n", successfulReads.get(), totalReadDurationMs);
    System.out.printf("Read throughput: %.0f ops/sec\n", readThroughput);
    System.out.printf("Average read latency: %.2f ms\n", avgReadLatencyMs);

    // Performance assertions
    Assert.assertTrue(
        "Should complete most reads successfully", successfulReads.get() > totalOperations * 0.95);
    Assert.assertTrue("Concurrent throughput should be reasonable", readThroughput > 1000);
  }

  @Test
  public void benchmarkDataSizeImpact() throws Exception {
    System.out.println("\n=== Data Size Impact Benchmark ===");

    int[] valueSizes = {64, 256, 1024, 4096, 16384}; // bytes
    int operationsPerSize = 500;

    for (int valueSize : valueSizes) {
      byte[] value = generateRandomValue(valueSize);

      // Benchmark PUT operations for this value size
      long putTimeSum = 0;
      for (int i = 0; i < operationsPerSize; i++) {
        byte[] key = ("size-test-" + valueSize + "-" + i).getBytes();
        long startTime = System.nanoTime();
        storage.put(benchmarkDbName, key, value).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        long endTime = System.nanoTime();
        putTimeSum += (endTime - startTime);
      }

      double avgPutLatencyMs = (putTimeSum / operationsPerSize) / 1_000_000.0;
      double putThroughputMBps =
          (valueSize * operationsPerSize) / (putTimeSum / 1_000_000_000.0) / (1024 * 1024);

      // Benchmark GET operations for this value size
      long getTimeSum = 0;
      for (int i = 0; i < operationsPerSize; i++) {
        byte[] key = ("size-test-" + valueSize + "-" + i).getBytes();
        long startTime = System.nanoTime();
        byte[] result = storage.get(benchmarkDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        long endTime = System.nanoTime();
        getTimeSum += (endTime - startTime);
        Assert.assertNotNull("GET should return value", result);
        Assert.assertEquals("Value size should match", valueSize, result.length);
      }

      double avgGetLatencyMs = (getTimeSum / operationsPerSize) / 1_000_000.0;
      double getThroughputMBps =
          (valueSize * operationsPerSize) / (getTimeSum / 1_000_000_000.0) / (1024 * 1024);

      System.out.printf("Value size %d bytes:\n", valueSize);
      System.out.printf("  PUT: %.2f ms avg, %.2f MB/s\n", avgPutLatencyMs, putThroughputMBps);
      System.out.printf("  GET: %.2f ms avg, %.2f MB/s\n", avgGetLatencyMs, getThroughputMBps);
    }
  }

  @Test
  public void benchmarkIteratorPerformance() throws Exception {
    System.out.println("\n=== Iterator Performance Benchmark ===");

    // Prepare sorted test data
    int dataSize = 5000;
    Map<byte[], byte[]> sortedData = new HashMap<>();
    for (int i = 0; i < dataSize; i++) {
      sortedData.put(String.format("iter-key-%06d", i).getBytes(), generateRandomValue(128));
    }

    // Batch write test data
    storage.batchWrite(benchmarkDbName, sortedData).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Benchmark different iterator operations
    int[] limits = {10, 50, 100, 500, 1000};

    for (int limit : limits) {
      // Test getNext operation
      long getNextStart = System.nanoTime();
      Map<byte[], byte[]> nextResults =
          storage
              .getNext(benchmarkDbName, "iter-key-".getBytes(), limit)
              .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      long getNextEnd = System.nanoTime();

      double getNextDurationMs = (getNextEnd - getNextStart) / 1_000_000.0;
      double getNextThroughput = nextResults.size() / (getNextDurationMs / 1000.0);

      System.out.printf(
          "getNext limit %d: %d results in %.2f ms, %.0f ops/sec\n",
          limit, nextResults.size(), getNextDurationMs, getNextThroughput);

      // Test getKeysNext operation
      long getKeysStart = System.nanoTime();
      List<byte[]> keyResults =
          storage
              .getKeysNext(benchmarkDbName, "iter-key-".getBytes(), limit)
              .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      long getKeysEnd = System.nanoTime();

      double getKeysDurationMs = (getKeysEnd - getKeysStart) / 1_000_000.0;
      double getKeysThroughput = keyResults.size() / (getKeysDurationMs / 1000.0);

      System.out.printf(
          "getKeysNext limit %d: %d results in %.2f ms, %.0f ops/sec\n",
          limit, keyResults.size(), getKeysDurationMs, getKeysThroughput);
    }

    // Test prefix query performance
    String[] prefixes = {"iter-key-0000", "iter-key-001", "iter-key-01", "iter-key-1"};

    for (String prefix : prefixes) {
      long prefixStart = System.nanoTime();
      Map<byte[], byte[]> prefixResults =
          storage
              .prefixQuery(benchmarkDbName, prefix.getBytes())
              .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      long prefixEnd = System.nanoTime();

      double prefixDurationMs = (prefixEnd - prefixStart) / 1_000_000.0;
      double prefixThroughput = prefixResults.size() / (prefixDurationMs / 1000.0);

      System.out.printf(
          "Prefix '%s': %d results in %.2f ms, %.0f ops/sec\n",
          prefix, prefixResults.size(), prefixDurationMs, prefixThroughput);
    }
  }

  @Test
  public void benchmarkMemoryAndResourceUsage() throws Exception {
    System.out.println("\n=== Memory and Resource Usage Benchmark ===");

    Runtime runtime = Runtime.getRuntime();

    // Baseline memory usage
    System.gc();
    long baselineMemory = runtime.totalMemory() - runtime.freeMemory();

    // Load test data and measure memory impact
    int dataSize = 50000;
    Map<byte[], byte[]> testData = new HashMap<>();

    for (int i = 0; i < dataSize; i++) {
      testData.put(("memory-test-" + i).getBytes(), generateRandomValue(512));
    }

    long dataLoadStart = System.nanoTime();
    storage.batchWrite(benchmarkDbName, testData).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    long dataLoadEnd = System.nanoTime();

    System.gc();
    long afterLoadMemory = runtime.totalMemory() - runtime.freeMemory();

    double loadDurationMs = (dataLoadEnd - dataLoadStart) / 1_000_000.0;
    long memoryIncrease = afterLoadMemory - baselineMemory;

    System.out.printf("Loaded %d records in %.2f ms\n", dataSize, loadDurationMs);
    System.out.printf("Memory usage increase: %.2f MB\n", memoryIncrease / (1024.0 * 1024.0));

    // Test database statistics
    StorageStats stats = storage.getStats(benchmarkDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    System.out.printf(
        "Database stats - Keys: %d, Size: %d bytes\n", stats.getTotalKeys(), stats.getTotalSize());

    // Performance assertions
    Assert.assertTrue("Load time should be reasonable", loadDurationMs < 30000); // 30 seconds
    Assert.assertTrue(
        "Memory increase should be reasonable", memoryIncrease < 500 * 1024 * 1024); // 500MB
  }

  // Helper methods are inherited from BasePerformanceBenchmark
}
