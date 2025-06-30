package org.tron.core.storage.spi;

import java.io.File;
import java.io.FileWriter;
import java.io.IOException;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Date;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.concurrent.TimeUnit;
import org.junit.After;
import org.junit.Assert;
import org.junit.Before;

/**
 * Abstract base class for performance benchmarks. Contains common benchmark logic shared between
 * different storage implementations.
 */
public abstract class BasePerformanceBenchmark {

  protected static final int TIMEOUT_SECONDS = 30;

  // Benchmark parameters
  protected static final int SMALL_DATASET_SIZE = 1000;
  protected static final int MEDIUM_DATASET_SIZE = 10000;
  protected static final int LARGE_DATASET_SIZE = 100000;
  protected static final int BATCH_SIZE = 100;
  protected static final int CONCURRENT_THREADS = 10;

  protected StorageSPI storage;
  protected String benchmarkDbName;
  protected Random random = new Random(42); // Fixed seed for reproducible results
  protected FileWriter metricsWriter;
  protected FileWriter csvWriter;
  protected String testTimestamp;
  protected String reportsDir;

  @Before
  public void setUp() throws Exception {
    // Initialize metrics output
    testTimestamp = new SimpleDateFormat("yyyyMMdd-HHmmss").format(new Date());
    reportsDir = "reports/" + testTimestamp;
    new File(reportsDir).mkdirs();

    // Initialize metrics files
    try {
      metricsWriter = new FileWriter(reportsDir + "/performance-metrics.json");
      csvWriter = new FileWriter(reportsDir + "/performance-metrics.csv");

      // Write JSON header
      metricsWriter.write("{\n");
      metricsWriter.write("  \"testRun\": {\n");
      metricsWriter.write("    \"timestamp\": \"" + testTimestamp + "\",\n");
      metricsWriter.write("    \"implementation\": \"" + getImplementationName() + "\",\n");
      metricsWriter.write("    \"javaVersion\": \"" + System.getProperty("java.version") + "\",\n");
      metricsWriter.write(
          "    \"availableProcessors\": " + Runtime.getRuntime().availableProcessors() + ",\n");
      metricsWriter.write(
          "    \"maxMemoryMB\": " + (Runtime.getRuntime().maxMemory() / (1024 * 1024)) + "\n");
      metricsWriter.write("  },\n");
      metricsWriter.write("  \"benchmarks\": {\n");

      // Write CSV header
      csvWriter.write("TestName,Metric,Value,Unit,Timestamp\n");

    } catch (IOException e) {
      System.err.println("Failed to initialize metrics files: " + e.getMessage());
    }

    // Initialize storage implementation
    storage = createStorageImplementation();
    benchmarkDbName = "benchmark-db-" + System.currentTimeMillis();

    // Initialize benchmark database with optimized settings
    StorageConfig config = new StorageConfig("ROCKSDB");
    config.setMaxOpenFiles(2000);
    config.setBlockCacheSize(64 * 1024 * 1024); // 64MB
    config.addEngineOption("write_buffer_size", "128MB");
    config.addEngineOption("max_write_buffer_number", "4");
    config.addEngineOption("compression_type", "snappy");
    config.addEngineOption("bloom_filter_bits_per_key", "10");

    initializeStorage(config);
  }

  @After
  public void tearDown() throws Exception {
    // Close metrics files
    try {
      if (metricsWriter != null) {
        metricsWriter.write("  }\n");
        metricsWriter.write("}\n");
        metricsWriter.close();
      }
      if (csvWriter != null) {
        csvWriter.close();
      }
    } catch (IOException e) {
      System.err.println("Failed to close metrics files: " + e.getMessage());
    }

    if (storage != null) {
      try {
        // Clean up benchmark database
        cleanupStorage();
      } catch (Exception e) {
        // Ignore cleanup errors
      }
    }
  }

  // Abstract methods to be implemented by subclasses
  protected abstract StorageSPI createStorageImplementation() throws Exception;

  protected abstract void initializeStorage(StorageConfig config) throws Exception;

  protected abstract void cleanupStorage() throws Exception;

  protected abstract String getImplementationName();

  protected abstract double getExpectedPutLatencyMs();

  protected abstract double getExpectedGetLatencyMs();

  // Common benchmark methods
  public void benchmarkSingleOperationLatency() throws Exception {
    String testName = getImplementationName() + "SingleOperationLatency";
    printTestHeader(testName);

    byte[] key = "latency-test-key".getBytes();
    byte[] value = generateRandomValue(256); // 256 bytes

    // Warm up
    System.out.println("Warming up with 100 operations...");
    for (int i = 0; i < 100; i++) {
      storage.put(benchmarkDbName, ("warmup-" + i).getBytes(), value).get();
    }

    // Benchmark PUT operations
    System.out.println("Benchmarking PUT operations...");
    long putLatencySum = 0;
    long putLatencyMin = Long.MAX_VALUE;
    long putLatencyMax = 0;
    int putIterations = 1000;

    for (int i = 0; i < putIterations; i++) {
      byte[] testKey = ("put-test-" + i).getBytes();
      long startTime = System.nanoTime();
      storage.put(benchmarkDbName, testKey, value).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      long endTime = System.nanoTime();
      long latency = endTime - startTime;
      putLatencySum += latency;
      putLatencyMin = Math.min(putLatencyMin, latency);
      putLatencyMax = Math.max(putLatencyMax, latency);
    }

    double avgPutLatencyMs = (putLatencySum / putIterations) / 1_000_000.0;
    double minPutLatencyMs = putLatencyMin / 1_000_000.0;
    double maxPutLatencyMs = putLatencyMax / 1_000_000.0;
    double putThroughput = putIterations / (putLatencySum / 1_000_000_000.0);

    // Benchmark GET operations
    System.out.println("Benchmarking GET operations...");
    long getLatencySum = 0;
    long getLatencyMin = Long.MAX_VALUE;
    long getLatencyMax = 0;
    int getIterations = 1000;

    for (int i = 0; i < getIterations; i++) {
      byte[] testKey = ("put-test-" + i).getBytes();
      long startTime = System.nanoTime();
      byte[] result = storage.get(benchmarkDbName, testKey).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      long endTime = System.nanoTime();
      long latency = endTime - startTime;
      getLatencySum += latency;
      getLatencyMin = Math.min(getLatencyMin, latency);
      getLatencyMax = Math.max(getLatencyMax, latency);
      Assert.assertNotNull("GET should return value", result);
    }

    double avgGetLatencyMs = (getLatencySum / getIterations) / 1_000_000.0;
    double minGetLatencyMs = getLatencyMin / 1_000_000.0;
    double maxGetLatencyMs = getLatencyMax / 1_000_000.0;
    double getThroughput = getIterations / (getLatencySum / 1_000_000_000.0);

    // Write metrics
    writeMetric(testName, "put_avg_latency", avgPutLatencyMs, "ms");
    writeMetric(testName, "put_min_latency", minPutLatencyMs, "ms");
    writeMetric(testName, "put_max_latency", maxPutLatencyMs, "ms");
    writeMetric(testName, "put_throughput", putThroughput, "ops/sec");
    writeMetric(testName, "get_avg_latency", avgGetLatencyMs, "ms");
    writeMetric(testName, "get_min_latency", minGetLatencyMs, "ms");
    writeMetric(testName, "get_max_latency", maxGetLatencyMs, "ms");
    writeMetric(testName, "get_throughput", getThroughput, "ops/sec");

    // Write JSON metrics
    String jsonMetrics =
        String.format(
            "{\n"
                + "      \"put\": {\n"
                + "        \"avgLatencyMs\": %.6f,\n"
                + "        \"minLatencyMs\": %.6f,\n"
                + "        \"maxLatencyMs\": %.6f,\n"
                + "        \"throughputOpsPerSec\": %.2f,\n"
                + "        \"iterations\": %d\n"
                + "      },\n"
                + "      \"get\": {\n"
                + "        \"avgLatencyMs\": %.6f,\n"
                + "        \"minLatencyMs\": %.6f,\n"
                + "        \"maxLatencyMs\": %.6f,\n"
                + "        \"throughputOpsPerSec\": %.2f,\n"
                + "        \"iterations\": %d\n"
                + "      }\n"
                + "    }",
            avgPutLatencyMs,
            minPutLatencyMs,
            maxPutLatencyMs,
            putThroughput,
            putIterations,
            avgGetLatencyMs,
            minGetLatencyMs,
            maxGetLatencyMs,
            getThroughput,
            getIterations);
    writeJsonMetric(testName, jsonMetrics, false);

    // Print summary
    Map<String, Double> summary = new HashMap<>();
    summary.put("PUT avg latency (ms)", avgPutLatencyMs);
    summary.put("PUT throughput (ops/sec)", putThroughput);
    summary.put("GET avg latency (ms)", avgGetLatencyMs);
    summary.put("GET throughput (ops/sec)", getThroughput);
    printTestSummary(testName, summary);

    // Performance assertions (adjust thresholds based on implementation)
    double putThreshold = getExpectedPutLatencyMs();
    double getThreshold = getExpectedGetLatencyMs();
    Assert.assertTrue("PUT latency should be reasonable", avgPutLatencyMs < putThreshold);
    Assert.assertTrue("GET latency should be reasonable", avgGetLatencyMs < getThreshold);
  }

  public void benchmarkBatchOperationThroughput() throws Exception {
    String testName = getImplementationName() + "BatchOperationThroughput";
    printTestHeader(testName);

    // Test different batch sizes
    int[] batchSizes = {10, 50, 100, 500, 1000};

    for (int batchSize : batchSizes) {
      System.out.println("Testing batch size: " + batchSize);

      Map<byte[], byte[]> batchData = new HashMap<>();
      for (int i = 0; i < batchSize; i++) {
        batchData.put(("batch-key-" + batchSize + "-" + i).getBytes(), generateRandomValue(256));
      }

      // Batch write benchmark
      long startTime = System.nanoTime();
      storage.batchWrite(benchmarkDbName, batchData).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      long endTime = System.nanoTime();

      double durationMs = (endTime - startTime) / 1_000_000.0;
      double throughput = batchSize / (durationMs / 1000.0); // ops/sec
      double mbPerSec = (batchSize * 256) / (durationMs / 1000.0) / (1024 * 1024); // MB/sec

      // Verify batch write success with batch get
      List<byte[]> keys = new ArrayList<>();
      batchData.keySet().forEach(keys::add);

      long batchGetStart = System.nanoTime();
      Map<byte[], byte[]> results =
          storage.batchGet(benchmarkDbName, keys).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      long batchGetEnd = System.nanoTime();

      double batchGetDurationMs = (batchGetEnd - batchGetStart) / 1_000_000.0;
      double batchGetThroughput = batchSize / (batchGetDurationMs / 1000.0);
      double batchGetMbPerSec = (batchSize * 256) / (batchGetDurationMs / 1000.0) / (1024 * 1024);

      // Write metrics
      writeMetric(testName, "batch_write_" + batchSize + "_latency", durationMs, "ms");
      writeMetric(testName, "batch_write_" + batchSize + "_throughput", throughput, "ops/sec");
      writeMetric(testName, "batch_write_" + batchSize + "_bandwidth", mbPerSec, "MB/sec");
      writeMetric(testName, "batch_get_" + batchSize + "_latency", batchGetDurationMs, "ms");
      writeMetric(
          testName, "batch_get_" + batchSize + "_throughput", batchGetThroughput, "ops/sec");
      writeMetric(testName, "batch_get_" + batchSize + "_bandwidth", batchGetMbPerSec, "MB/sec");

      System.out.printf(
          "  Batch WRITE size %d: %.2f ms, %.0f ops/sec, %.2f MB/sec\n",
          batchSize, durationMs, throughput, mbPerSec);
      System.out.printf(
          "  Batch GET size %d: %.2f ms, %.0f ops/sec, %.2f MB/sec\n",
          batchSize, batchGetDurationMs, batchGetThroughput, batchGetMbPerSec);

      Assert.assertEquals("All keys should be retrieved", batchSize, results.size());
    }

    // Write JSON summary for batch operations
    StringBuilder batchJsonBuilder = new StringBuilder();
    batchJsonBuilder.append("{\n");
    for (int i = 0; i < batchSizes.length; i++) {
      int batchSize = batchSizes[i];
      batchJsonBuilder.append(String.format("      \"batchSize%d\": {\n", batchSize));
      batchJsonBuilder.append(String.format("        \"size\": %d,\n", batchSize));
      batchJsonBuilder.append("        \"valueSize\": 256\n");
      batchJsonBuilder.append("      }");
      if (i < batchSizes.length - 1) {
        batchJsonBuilder.append(",");
      }
      batchJsonBuilder.append("\n");
    }
    batchJsonBuilder.append("    }");
    writeJsonMetric(testName, batchJsonBuilder.toString(), false);

    System.out.println("\nBatch operation throughput test completed");
  }

  public void generatePerformanceReport() throws Exception {
    String testName = getImplementationName() + "PerformanceReport";
    printTestHeader(testName);

    // System information
    Runtime runtime = Runtime.getRuntime();
    long maxMemoryMB = runtime.maxMemory() / (1024 * 1024);
    long totalMemoryMB = runtime.totalMemory() / (1024 * 1024);
    long freeMemoryMB = runtime.freeMemory() / (1024 * 1024);
    long usedMemoryMB = totalMemoryMB - freeMemoryMB;
    int processors = runtime.availableProcessors();

    System.out.println("Test Environment:");
    System.out.printf("  Implementation: %s\n", getImplementationName());
    System.out.printf("  Java Version: %s\n", System.getProperty("java.version"));
    System.out.printf("  Available Processors: %d\n", processors);
    System.out.printf("  Max Memory: %d MB\n", maxMemoryMB);
    System.out.printf("  Used Memory: %d MB\n", usedMemoryMB);
    System.out.printf(
        "  OS: %s %s\n", System.getProperty("os.name"), System.getProperty("os.version"));
    System.out.printf("  Architecture: %s\n", System.getProperty("os.arch"));

    // Test basic connectivity and health
    HealthStatus health = storage.healthCheck().get(5, TimeUnit.SECONDS);
    System.out.printf("  Storage Health: %s\n", health);

    List<String> databases = storage.listDatabases().get(5, TimeUnit.SECONDS);
    System.out.printf("  Active Databases: %d\n", databases.size());

    // Write system metrics
    writeMetric(testName, "max_memory", maxMemoryMB, "MB");
    writeMetric(testName, "used_memory", usedMemoryMB, "MB");
    writeMetric(testName, "available_processors", processors, "count");
    writeMetric(testName, "active_databases", databases.size(), "count");

    // Basic performance test
    System.out.println("\nRunning basic performance test...");
    byte[] testKey = "perf-report-test".getBytes();
    byte[] testValue = generateRandomValue(1024);

    // Test single PUT/GET latency
    long putStart = System.nanoTime();
    storage.put(benchmarkDbName, testKey, testValue).get(5, TimeUnit.SECONDS);
    long putEnd = System.nanoTime();
    double putLatencyMs = (putEnd - putStart) / 1_000_000.0;

    long getStart = System.nanoTime();
    byte[] retrievedValue = storage.get(benchmarkDbName, testKey).get(5, TimeUnit.SECONDS);
    long getEnd = System.nanoTime();
    double getLatencyMs = (getEnd - getStart) / 1_000_000.0;

    Assert.assertNotNull("Value should be retrieved", retrievedValue);
    Assert.assertEquals("Retrieved value should match", testValue.length, retrievedValue.length);

    writeMetric(testName, "basic_put_latency", putLatencyMs, "ms");
    writeMetric(testName, "basic_get_latency", getLatencyMs, "ms");

    System.out.printf("  Basic PUT latency: %.2f ms\n", putLatencyMs);
    System.out.printf("  Basic GET latency: %.2f ms\n", getLatencyMs);

    // Write environment JSON
    String envJson =
        String.format(
            "{\n"
                + "      \"environment\": {\n"
                + "        \"implementation\": \"%s\",\n"
                + "        \"javaVersion\": \"%s\",\n"
                + "        \"osName\": \"%s\",\n"
                + "        \"osVersion\": \"%s\",\n"
                + "        \"osArch\": \"%s\",\n"
                + "        \"availableProcessors\": %d,\n"
                + "        \"maxMemoryMB\": %d,\n"
                + "        \"usedMemoryMB\": %d\n"
                + "      },\n"
                + "      \"connectivity\": {\n"
                + "        \"healthStatus\": \"%s\",\n"
                + "        \"activeDatabases\": %d,\n"
                + "        \"basicPutLatencyMs\": %.6f,\n"
                + "        \"basicGetLatencyMs\": %.6f\n"
                + "      }\n"
                + "    }",
            getImplementationName(),
            System.getProperty("java.version"),
            System.getProperty("os.name"),
            System.getProperty("os.version"),
            System.getProperty("os.arch"),
            processors,
            maxMemoryMB,
            usedMemoryMB,
            health,
            databases.size(),
            putLatencyMs,
            getLatencyMs);
    writeJsonMetric(testName, envJson, true); // This is the last test

    System.out.println("\nRecommendations:");
    System.out.println("  - Compare results with other storage implementations");
    System.out.println("  - Monitor resource usage during peak loads");
    System.out.println("  - Test with production-like data patterns");
    System.out.println("  - Check reports/" + testTimestamp + "/ for detailed metrics files");
  }

  // Helper methods
  protected byte[] generateRandomValue(int size) {
    byte[] value = new byte[size];
    random.nextBytes(value);
    return value;
  }

  protected void writeMetric(String testName, String metricName, double value, String unit) {
    try {
      // Write to CSV
      if (csvWriter != null) {
        csvWriter.write(
            String.format(
                "%s,%s,%.6f,%s,%s\n",
                testName, metricName, value, unit, System.currentTimeMillis()));
        csvWriter.flush();
      }

      // Write to console with clear markers
      System.out.println(
          String.format("METRIC: %s.%s = %.6f %s", testName, metricName, value, unit));

    } catch (IOException e) {
      System.err.println("Failed to write metric: " + e.getMessage());
    }
  }

  protected void writeJsonMetric(String testName, String metricJson, boolean isLast) {
    try {
      if (metricsWriter != null) {
        metricsWriter.write("    \"" + testName + "\": " + metricJson);
        if (!isLast) {
          metricsWriter.write(",");
        }
        metricsWriter.write("\n");
        metricsWriter.flush();
      }
    } catch (IOException e) {
      System.err.println("Failed to write JSON metric: " + e.getMessage());
    }
  }

  protected void printTestHeader(String testName) {
    StringBuilder border = new StringBuilder();
    for (int i = 0; i < 60; i++) {
      border.append("=");
    }
    System.out.println("\n" + border.toString());
    System.out.println("BENCHMARK: " + testName);
    System.out.println(border.toString());
  }

  protected void printTestSummary(String testName, Map<String, Double> metrics) {
    System.out.println("\nSUMMARY: " + testName);
    for (Map.Entry<String, Double> entry : metrics.entrySet()) {
      System.out.println("  " + entry.getKey() + ": " + entry.getValue());
    }
    System.out.println();
  }
}
