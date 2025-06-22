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
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Random;
import java.util.concurrent.atomic.AtomicLong;
import java.io.File;
import java.io.FileWriter;
import java.io.IOException;
import java.text.SimpleDateFormat;
import java.util.Date;

/**
 * Performance benchmark tests for StorageSPI implementations.
 * Compares gRPC storage performance against baseline metrics.
 */
public class StoragePerformanceBenchmark {
    
    private static final String GRPC_HOST = System.getProperty("storage.grpc.host", "localhost");
    private static final int GRPC_PORT = Integer.parseInt(System.getProperty("storage.grpc.port", "50051"));
    private static final int TIMEOUT_SECONDS = 30;
    
    // Benchmark parameters
    private static final int SMALL_DATASET_SIZE = 1000;
    private static final int MEDIUM_DATASET_SIZE = 10000;
    private static final int LARGE_DATASET_SIZE = 100000;
    private static final int BATCH_SIZE = 100;
    private static final int CONCURRENT_THREADS = 10;
    
    private GrpcStorageSPI storage;
    private String benchmarkDbName;
    private Random random = new Random(42); // Fixed seed for reproducible results
    private FileWriter metricsWriter;
    private FileWriter csvWriter;
    private String testTimestamp;
    private String reportsDir;
    
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
            metricsWriter.write("    \"grpcHost\": \"" + GRPC_HOST + "\",\n");
            metricsWriter.write("    \"grpcPort\": " + GRPC_PORT + ",\n");
            metricsWriter.write("    \"javaVersion\": \"" + System.getProperty("java.version") + "\",\n");
            metricsWriter.write("    \"availableProcessors\": " + Runtime.getRuntime().availableProcessors() + ",\n");
            metricsWriter.write("    \"maxMemoryMB\": " + (Runtime.getRuntime().maxMemory() / (1024 * 1024)) + "\n");
            metricsWriter.write("  },\n");
            metricsWriter.write("  \"benchmarks\": {\n");
            
            // Write CSV header
            csvWriter.write("TestName,Metric,Value,Unit,Timestamp\n");
            
        } catch (IOException e) {
            System.err.println("Failed to initialize metrics files: " + e.getMessage());
        }
        
        // Check if gRPC server is available
        storage = new GrpcStorageSPI(GRPC_HOST, GRPC_PORT);
        benchmarkDbName = "benchmark-db-" + System.currentTimeMillis();
        
        try {
            // Test server connectivity
            CompletableFuture<HealthStatus> healthFuture = storage.healthCheck();
            HealthStatus health = healthFuture.get(5, TimeUnit.SECONDS);
            Assume.assumeTrue("gRPC server not available", health != null);
            
            // Initialize benchmark database with optimized settings
            StorageConfig config = new StorageConfig("ROCKSDB");
            config.setMaxOpenFiles(2000);
            config.setBlockCacheSize(64 * 1024 * 1024); // 64MB
            config.addEngineOption("write_buffer_size", "128MB");
            config.addEngineOption("max_write_buffer_number", "4");
            config.addEngineOption("compression_type", "snappy");
            config.addEngineOption("bloom_filter_bits_per_key", "10");
            
            storage.initDB(benchmarkDbName, config).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
            
        } catch (TimeoutException | ExecutionException e) {
            Assume.assumeNoException("gRPC server not responding", e);
        }
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
                storage.resetDB(benchmarkDbName).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
                storage.close();
            } catch (Exception e) {
                // Ignore cleanup errors
            }
        }
    }
    
    @Test
    public void benchmarkSingleOperationLatency() throws Exception {
        String testName = "SingleOperationLatency";
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
        String jsonMetrics = String.format(
            "{\n" +
            "      \"put\": {\n" +
            "        \"avgLatencyMs\": %.6f,\n" +
            "        \"minLatencyMs\": %.6f,\n" +
            "        \"maxLatencyMs\": %.6f,\n" +
            "        \"throughputOpsPerSec\": %.2f,\n" +
            "        \"iterations\": %d\n" +
            "      },\n" +
            "      \"get\": {\n" +
            "        \"avgLatencyMs\": %.6f,\n" +
            "        \"minLatencyMs\": %.6f,\n" +
            "        \"maxLatencyMs\": %.6f,\n" +
            "        \"throughputOpsPerSec\": %.2f,\n" +
            "        \"iterations\": %d\n" +
            "      }\n" +
            "    }",
            avgPutLatencyMs, minPutLatencyMs, maxPutLatencyMs, putThroughput, putIterations,
            avgGetLatencyMs, minGetLatencyMs, maxGetLatencyMs, getThroughput, getIterations
        );
        writeJsonMetric(testName, jsonMetrics, false);
        
        // Print summary
        Map<String, Double> summary = new HashMap<>();
        summary.put("PUT avg latency (ms)", avgPutLatencyMs);
        summary.put("PUT throughput (ops/sec)", putThroughput);
        summary.put("GET avg latency (ms)", avgGetLatencyMs);
        summary.put("GET throughput (ops/sec)", getThroughput);
        printTestSummary(testName, summary);
        
        // Performance assertions (adjust thresholds based on environment)
        Assert.assertTrue("PUT latency should be reasonable", avgPutLatencyMs < 50.0);
        Assert.assertTrue("GET latency should be reasonable", avgGetLatencyMs < 20.0);
    }
    
    @Test
    public void benchmarkBatchOperationThroughput() throws Exception {
        String testName = "BatchOperationThroughput";
        printTestHeader(testName);
        
        // Test different batch sizes
        int[] batchSizes = {10, 50, 100, 500, 1000};
        
        for (int batchSize : batchSizes) {
            System.out.println("Testing batch size: " + batchSize);
            
            Map<byte[], byte[]> batchData = new HashMap<>();
            for (int i = 0; i < batchSize; i++) {
                batchData.put(
                    ("batch-key-" + batchSize + "-" + i).getBytes(),
                    generateRandomValue(256)
                );
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
            Map<byte[], byte[]> results = storage.batchGet(benchmarkDbName, keys).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
            long batchGetEnd = System.nanoTime();
            
            double batchGetDurationMs = (batchGetEnd - batchGetStart) / 1_000_000.0;
            double batchGetThroughput = batchSize / (batchGetDurationMs / 1000.0);
            double batchGetMbPerSec = (batchSize * 256) / (batchGetDurationMs / 1000.0) / (1024 * 1024);
            
            // Write metrics
            writeMetric(testName, "batch_write_" + batchSize + "_latency", durationMs, "ms");
            writeMetric(testName, "batch_write_" + batchSize + "_throughput", throughput, "ops/sec");
            writeMetric(testName, "batch_write_" + batchSize + "_bandwidth", mbPerSec, "MB/sec");
            writeMetric(testName, "batch_get_" + batchSize + "_latency", batchGetDurationMs, "ms");
            writeMetric(testName, "batch_get_" + batchSize + "_throughput", batchGetThroughput, "ops/sec");
            writeMetric(testName, "batch_get_" + batchSize + "_bandwidth", batchGetMbPerSec, "MB/sec");
            
            System.out.printf("  Batch WRITE size %d: %.2f ms, %.0f ops/sec, %.2f MB/sec\n", 
                batchSize, durationMs, throughput, mbPerSec);
            System.out.printf("  Batch GET size %d: %.2f ms, %.0f ops/sec, %.2f MB/sec\n", 
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
    
    @Test
    public void benchmarkConcurrentOperations() throws Exception {
        System.out.println("\n=== Concurrent Operations Benchmark ===");
        
        int totalOperations = 10000;
        int operationsPerThread = totalOperations / CONCURRENT_THREADS;
        
        // Prepare test data
        Map<byte[], byte[]> testData = new HashMap<>();
        for (int i = 0; i < totalOperations; i++) {
            testData.put(
                ("concurrent-key-" + i).getBytes(),
                generateRandomValue(128)
            );
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
            CompletableFuture<Void> future = CompletableFuture.runAsync(() -> {
                try {
                    for (int i = 0; i < operationsPerThread; i++) {
                        byte[] key = ("concurrent-key-" + (threadId * operationsPerThread + i)).getBytes();
                        long opStart = System.nanoTime();
                        byte[] result = storage.get(benchmarkDbName, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
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
        CompletableFuture.allOf(readFutures.toArray(new CompletableFuture[0])).get(TIMEOUT_SECONDS * 2, TimeUnit.SECONDS);
        long readTestEnd = System.nanoTime();
        
        double totalReadDurationMs = (readTestEnd - readTestStart) / 1_000_000.0;
        double readThroughput = successfulReads.get() / (totalReadDurationMs / 1000.0);
        double avgReadLatencyMs = (totalReadTime.get() / successfulReads.get()) / 1_000_000.0;
        
        System.out.printf("Concurrent reads: %d ops in %.2f ms\n", successfulReads.get(), totalReadDurationMs);
        System.out.printf("Read throughput: %.0f ops/sec\n", readThroughput);
        System.out.printf("Average read latency: %.2f ms\n", avgReadLatencyMs);
        
        // Performance assertions
        Assert.assertTrue("Should complete most reads successfully", successfulReads.get() > totalOperations * 0.95);
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
            double putThroughputMBps = (valueSize * operationsPerSize) / (putTimeSum / 1_000_000_000.0) / (1024 * 1024);
            
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
            double getThroughputMBps = (valueSize * operationsPerSize) / (getTimeSum / 1_000_000_000.0) / (1024 * 1024);
            
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
            sortedData.put(
                String.format("iter-key-%06d", i).getBytes(),
                generateRandomValue(128)
            );
        }
        
        // Batch write test data
        storage.batchWrite(benchmarkDbName, sortedData).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        
        // Benchmark different iterator operations
        int[] limits = {10, 50, 100, 500, 1000};
        
        for (int limit : limits) {
            // Test getNext operation
            long getNextStart = System.nanoTime();
            Map<byte[], byte[]> nextResults = storage.getNext(benchmarkDbName, "iter-key-".getBytes(), limit).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
            long getNextEnd = System.nanoTime();
            
            double getNextDurationMs = (getNextEnd - getNextStart) / 1_000_000.0;
            double getNextThroughput = nextResults.size() / (getNextDurationMs / 1000.0);
            
            System.out.printf("getNext limit %d: %d results in %.2f ms, %.0f ops/sec\n", 
                limit, nextResults.size(), getNextDurationMs, getNextThroughput);
            
            // Test getKeysNext operation
            long getKeysStart = System.nanoTime();
            List<byte[]> keyResults = storage.getKeysNext(benchmarkDbName, "iter-key-".getBytes(), limit).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
            long getKeysEnd = System.nanoTime();
            
            double getKeysDurationMs = (getKeysEnd - getKeysStart) / 1_000_000.0;
            double getKeysThroughput = keyResults.size() / (getKeysDurationMs / 1000.0);
            
            System.out.printf("getKeysNext limit %d: %d results in %.2f ms, %.0f ops/sec\n", 
                limit, keyResults.size(), getKeysDurationMs, getKeysThroughput);
        }
        
        // Test prefix query performance
        String[] prefixes = {"iter-key-0000", "iter-key-001", "iter-key-01", "iter-key-1"};
        
        for (String prefix : prefixes) {
            long prefixStart = System.nanoTime();
            Map<byte[], byte[]> prefixResults = storage.prefixQuery(benchmarkDbName, prefix.getBytes()).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
            long prefixEnd = System.nanoTime();
            
            double prefixDurationMs = (prefixEnd - prefixStart) / 1_000_000.0;
            double prefixThroughput = prefixResults.size() / (prefixDurationMs / 1000.0);
            
            System.out.printf("Prefix '%s': %d results in %.2f ms, %.0f ops/sec\n", 
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
            testData.put(
                ("memory-test-" + i).getBytes(),
                generateRandomValue(512)
            );
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
        System.out.printf("Database stats - Keys: %d, Size: %d bytes\n", stats.getTotalKeys(), stats.getTotalSize());
        
        // Performance assertions
        Assert.assertTrue("Load time should be reasonable", loadDurationMs < 30000); // 30 seconds
        Assert.assertTrue("Memory increase should be reasonable", memoryIncrease < 500 * 1024 * 1024); // 500MB
    }
    
    @Test
    public void generatePerformanceReport() throws Exception {
        String testName = "PerformanceReport";
        printTestHeader(testName);
        
        // System information
        Runtime runtime = Runtime.getRuntime();
        long maxMemoryMB = runtime.maxMemory() / (1024 * 1024);
        long totalMemoryMB = runtime.totalMemory() / (1024 * 1024);
        long freeMemoryMB = runtime.freeMemory() / (1024 * 1024);
        long usedMemoryMB = totalMemoryMB - freeMemoryMB;
        int processors = runtime.availableProcessors();
        
        System.out.println("Test Environment:");
        System.out.printf("  gRPC Server: %s:%d\n", GRPC_HOST, GRPC_PORT);
        System.out.printf("  Java Version: %s\n", System.getProperty("java.version"));
        System.out.printf("  Available Processors: %d\n", processors);
        System.out.printf("  Max Memory: %d MB\n", maxMemoryMB);
        System.out.printf("  Used Memory: %d MB\n", usedMemoryMB);
        System.out.printf("  OS: %s %s\n", System.getProperty("os.name"), System.getProperty("os.version"));
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
        String envJson = String.format(
            "{\n" +
            "      \"environment\": {\n" +
            "        \"grpcHost\": \"%s\",\n" +
            "        \"grpcPort\": %d,\n" +
            "        \"javaVersion\": \"%s\",\n" +
            "        \"osName\": \"%s\",\n" +
            "        \"osVersion\": \"%s\",\n" +
            "        \"osArch\": \"%s\",\n" +
            "        \"availableProcessors\": %d,\n" +
            "        \"maxMemoryMB\": %d,\n" +
            "        \"usedMemoryMB\": %d\n" +
            "      },\n" +
            "      \"connectivity\": {\n" +
            "        \"healthStatus\": \"%s\",\n" +
            "        \"activeDatabases\": %d,\n" +
            "        \"basicPutLatencyMs\": %.6f,\n" +
            "        \"basicGetLatencyMs\": %.6f\n" +
            "      }\n" +
            "    }",
            GRPC_HOST, GRPC_PORT,
            System.getProperty("java.version"),
            System.getProperty("os.name"),
            System.getProperty("os.version"),
            System.getProperty("os.arch"),
            processors, maxMemoryMB, usedMemoryMB,
            health, databases.size(),
            putLatencyMs, getLatencyMs
        );
        writeJsonMetric(testName, envJson, true); // This is the last test
        
        System.out.println("\nRecommendations:");
        System.out.println("  - Run all benchmark tests individually for detailed metrics");
        System.out.println("  - Compare results with embedded storage baseline");
        System.out.println("  - Monitor resource usage during peak loads");
        System.out.println("  - Test with production-like data patterns");
        System.out.println("  - Check reports/" + testTimestamp + "/ for detailed metrics files");
    }
    
    private byte[] generateRandomValue(int size) {
        byte[] value = new byte[size];
        random.nextBytes(value);
        return value;
    }
    
    private void writeMetric(String testName, String metricName, double value, String unit) {
        try {
            // Write to CSV
            if (csvWriter != null) {
                csvWriter.write(String.format("%s,%s,%.6f,%s,%s\n", 
                    testName, metricName, value, unit, System.currentTimeMillis()));
                csvWriter.flush();
            }
            
            // Write to console with clear markers
            System.out.println(String.format("METRIC: %s.%s = %.6f %s", testName, metricName, value, unit));
            
        } catch (IOException e) {
            System.err.println("Failed to write metric: " + e.getMessage());
        }
    }
    
    private void writeJsonMetric(String testName, String metricJson, boolean isLast) {
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
    
    private void printTestHeader(String testName) {
        StringBuilder border = new StringBuilder();
        for (int i = 0; i < 60; i++) {
            border.append("=");
        }
        System.out.println("\n" + border.toString());
        System.out.println("BENCHMARK: " + testName);
        System.out.println(border.toString());
    }
    
    private void printTestSummary(String testName, Map<String, Double> metrics) {
        System.out.println("\nSUMMARY: " + testName);
        for (Map.Entry<String, Double> entry : metrics.entrySet()) {
            System.out.println("  " + entry.getKey() + ": " + entry.getValue());
        }
        System.out.println();
    }
} 