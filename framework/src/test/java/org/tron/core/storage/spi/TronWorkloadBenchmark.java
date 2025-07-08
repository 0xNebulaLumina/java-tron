package org.tron.core.storage.spi;

import java.nio.ByteBuffer;
import java.security.SecureRandom;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;
import org.junit.Assert;
import org.junit.Test;

/**
 * Production-grade load testing for java-tron storage patterns.
 * Simulates realistic blockchain workloads including:
 * - Block processing (sequential writes with transaction batches)
 * - Account state updates (frequent small updates)
 * - Transaction queries (hash-based lookups)
 * - Smart contract storage (key-value patterns)
 * - Blockchain explorer queries (range scans, historical data)
 * - Fast sync operations (bulk data loading)
 */
public abstract class TronWorkloadBenchmark extends BasePerformanceBenchmark {

  // Tron-specific constants
  private static final int TRON_BLOCK_SIZE = 2000; // ~2000 transactions per block
  private static final int TRON_BLOCK_INTERVAL_MS = 3000; // 3 second block time
  private static final int TRON_ACCOUNT_SIZE = 200; // Average account data size
  private static final int TRON_TRANSACTION_SIZE = 300; // Average transaction size
  private static final int TRON_CONTRACT_STATE_SIZE = 64; // Contract storage slot size
  private static final int TRON_MAINNET_TPS = 2000; // Target TPS for mainnet
  private static final int TRON_SYNC_BATCH_SIZE = 10000; // Fast sync batch size
  
  // Test databases (simulating different stores in java-tron)
  private static final String BLOCK_DB = "block-store";
  private static final String TRANSACTION_DB = "transaction-store";
  private static final String ACCOUNT_DB = "account-store";
  private static final String CONTRACT_DB = "contract-store";
  private static final String DYNAMIC_DB = "dynamic-properties-store";
  
  private SecureRandom secureRandom = new SecureRandom();
  private AtomicLong blockNumber = new AtomicLong(0);
  private AtomicLong transactionId = new AtomicLong(0);
  private AtomicInteger accountId = new AtomicInteger(0);

  protected void initializeTronDatabases(StorageConfig config) throws Exception {
    // Initialize all database stores
    storage.initDB(BLOCK_DB, config).get(30, TimeUnit.SECONDS);
    storage.initDB(TRANSACTION_DB, config).get(30, TimeUnit.SECONDS);
    storage.initDB(ACCOUNT_DB, config).get(30, TimeUnit.SECONDS);
    storage.initDB(CONTRACT_DB, config).get(30, TimeUnit.SECONDS);
    storage.initDB(DYNAMIC_DB, config).get(30, TimeUnit.SECONDS);
  }

  protected void cleanupTronDatabases() throws Exception {
    // Clean up all test databases
    String[] databases = {BLOCK_DB, TRANSACTION_DB, ACCOUNT_DB, CONTRACT_DB, DYNAMIC_DB};
    for (String db : databases) {
      try {
        storage.resetDB(db).get(10, TimeUnit.SECONDS);
      } catch (Exception e) {
        // Ignore cleanup errors
      }
    }
  }

  /**
   * Test 1: Block Processing Workload
   * Simulates the sequential processing of blocks with transaction batches.
   * This is the core write workload of a blockchain node.
   */
  @Test
  public void benchmarkBlockProcessingWorkload() throws Exception {
    String testName = getImplementationName() + "BlockProcessingWorkload";
    printTestHeader(testName);

    int totalBlocks = 100;
    int transactionsPerBlock = TRON_BLOCK_SIZE;
    long totalTransactions = (long) totalBlocks * transactionsPerBlock;
    
    System.out.println("Simulating block processing workload:");
    System.out.printf("  - %d blocks\n", totalBlocks);
    System.out.printf("  - %d transactions per block\n", transactionsPerBlock);
    System.out.printf("  - %d total transactions\n", totalTransactions);

    long startTime = System.currentTimeMillis();
    
    for (int blockIdx = 0; blockIdx < totalBlocks; blockIdx++) {
      long blockStart = System.nanoTime();
      
      // Generate block data
      TronBlock block = generateBlock(blockIdx, transactionsPerBlock);
      
      // 1. Store block header and metadata
      byte[] blockKey = longToBytes(block.number);
      byte[] blockData = serializeBlock(block);
      storage.put(BLOCK_DB, blockKey, blockData).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      
      // 2. Batch store all transactions in the block
      Map<byte[], byte[]> transactionBatch = new HashMap<>();
      for (TronTransaction tx : block.transactions) {
        transactionBatch.put(tx.hash, serializeTransaction(tx));
      }
      storage.batchWrite(TRANSACTION_DB, transactionBatch).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      
      // 3. Update account states (simulate transaction effects)
      Map<byte[], byte[]> accountUpdates = new HashMap<>();
      for (TronTransaction tx : block.transactions) {
        // Simulate sender account update
        byte[] senderKey = tx.fromAddress;
        TronAccount senderAccount = new TronAccount(senderKey, 
            1000000 - tx.amount, // Deduct amount
            System.currentTimeMillis());
        accountUpdates.put(senderKey, serializeAccount(senderAccount));
        
        // Simulate receiver account update
        byte[] receiverKey = tx.toAddress;
        TronAccount receiverAccount = new TronAccount(receiverKey,
            1000000 + tx.amount, // Add amount
            System.currentTimeMillis());
        accountUpdates.put(receiverKey, serializeAccount(receiverAccount));
      }
      storage.batchWrite(ACCOUNT_DB, accountUpdates).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      
      // 4. Update dynamic properties (blockchain state)
      Map<byte[], byte[]> dynamicUpdates = new HashMap<>();
      dynamicUpdates.put("latest_block_number".getBytes(), longToBytes(block.number));
      dynamicUpdates.put("latest_block_hash".getBytes(), block.hash);
      dynamicUpdates.put("latest_block_timestamp".getBytes(), longToBytes(block.timestamp));
      storage.batchWrite(DYNAMIC_DB, dynamicUpdates).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      
      long blockEnd = System.nanoTime();
      double blockLatencyMs = (blockEnd - blockStart) / 1_000_000.0;
      
      if (blockIdx % 10 == 0) {
        System.out.printf("  Block %d processed in %.2f ms\n", blockIdx, blockLatencyMs);
      }
    }
    
    long endTime = System.currentTimeMillis();
    double totalDurationSec = (endTime - startTime) / 1000.0;
    double blockThroughput = totalBlocks / totalDurationSec;
    double transactionThroughput = totalTransactions / totalDurationSec;
    
    // Write metrics
    writeMetric(testName, "total_blocks", totalBlocks, "count");
    writeMetric(testName, "total_transactions", totalTransactions, "count");
    writeMetric(testName, "total_duration", totalDurationSec, "seconds");
    writeMetric(testName, "block_throughput", blockThroughput, "blocks/sec");
    writeMetric(testName, "transaction_throughput", transactionThroughput, "tx/sec");
    writeMetric(testName, "avg_block_latency", (totalDurationSec * 1000) / totalBlocks, "ms");
    
    System.out.printf("\nBlock Processing Results:\n");
    System.out.printf("  Total Duration: %.2f seconds\n", totalDurationSec);
    System.out.printf("  Block Throughput: %.2f blocks/sec\n", blockThroughput);
    System.out.printf("  Transaction Throughput: %.0f tx/sec\n", transactionThroughput);
    System.out.printf("  Average Block Latency: %.2f ms\n", (totalDurationSec * 1000) / totalBlocks);
    
    // Performance assertion
    Assert.assertTrue("Transaction throughput should meet mainnet requirements", 
        transactionThroughput >= TRON_MAINNET_TPS * 0.5); // 50% of mainnet TPS
  }

  /**
   * Test 2: Account Query Workload
   * Simulates frequent account balance and state queries.
   * This represents the most common read operations.
   */
  @Test
  public void benchmarkAccountQueryWorkload() throws Exception {
    String testName = getImplementationName() + "AccountQueryWorkload";
    printTestHeader(testName);

    int totalAccounts = 10000;
    int queryOperations = 50000;
    
    System.out.println("Setting up account query workload:");
    System.out.printf("  - %d accounts\n", totalAccounts);
    System.out.printf("  - %d query operations\n", queryOperations);

    // Setup: Create accounts
    System.out.println("Creating test accounts...");
    Map<byte[], byte[]> accountBatch = new HashMap<>();
    List<byte[]> accountKeys = new ArrayList<>();
    
    for (int i = 0; i < totalAccounts; i++) {
      byte[] accountKey = ("account-" + i).getBytes();
      TronAccount account = new TronAccount(accountKey, 
          secureRandom.nextInt(1000000), // Random balance
          System.currentTimeMillis());
      accountBatch.put(accountKey, serializeAccount(account));
      accountKeys.add(accountKey);
    }
    
    long setupStart = System.nanoTime();
    storage.batchWrite(ACCOUNT_DB, accountBatch).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    long setupEnd = System.nanoTime();
    double setupTimeMs = (setupEnd - setupStart) / 1_000_000.0;
    
    System.out.printf("Account setup completed in %.2f ms\n", setupTimeMs);

    // Benchmark: Random account queries
    System.out.println("Running account queries...");
    long queryStart = System.nanoTime();
    long queryLatencySum = 0;
    long queryLatencyMin = Long.MAX_VALUE;
    long queryLatencyMax = 0;
    
    for (int i = 0; i < queryOperations; i++) {
      byte[] randomKey = accountKeys.get(secureRandom.nextInt(accountKeys.size()));
      
      long singleQueryStart = System.nanoTime();
      byte[] result = storage.get(ACCOUNT_DB, randomKey).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      long singleQueryEnd = System.nanoTime();
      
      long latency = singleQueryEnd - singleQueryStart;
      queryLatencySum += latency;
      queryLatencyMin = Math.min(queryLatencyMin, latency);
      queryLatencyMax = Math.max(queryLatencyMax, latency);
      
      Assert.assertNotNull("Account should exist", result);
      
      if (i % 10000 == 0) {
        System.out.printf("  Completed %d queries\n", i);
      }
    }
    
    long queryEnd = System.nanoTime();
    double totalQueryTimeMs = (queryEnd - queryStart) / 1_000_000.0;
    double avgQueryLatencyMs = (queryLatencySum / queryOperations) / 1_000_000.0;
    double minQueryLatencyMs = queryLatencyMin / 1_000_000.0;
    double maxQueryLatencyMs = queryLatencyMax / 1_000_000.0;
    double queryThroughput = queryOperations / (totalQueryTimeMs / 1000.0);
    
    // Write metrics
    writeMetric(testName, "setup_time", setupTimeMs, "ms");
    writeMetric(testName, "total_queries", queryOperations, "count");
    writeMetric(testName, "avg_query_latency", avgQueryLatencyMs, "ms");
    writeMetric(testName, "min_query_latency", minQueryLatencyMs, "ms");
    writeMetric(testName, "max_query_latency", maxQueryLatencyMs, "ms");
    writeMetric(testName, "query_throughput", queryThroughput, "queries/sec");
    
    System.out.printf("\nAccount Query Results:\n");
    System.out.printf("  Setup Time: %.2f ms\n", setupTimeMs);
    System.out.printf("  Average Query Latency: %.3f ms\n", avgQueryLatencyMs);
    System.out.printf("  Min Query Latency: %.3f ms\n", minQueryLatencyMs);
    System.out.printf("  Max Query Latency: %.3f ms\n", maxQueryLatencyMs);
    System.out.printf("  Query Throughput: %.0f queries/sec\n", queryThroughput);
    
    // Performance assertion
    Assert.assertTrue("Query latency should be under 50ms for 95th percentile", 
        avgQueryLatencyMs < 50.0);
  }

  /**
   * Test 3: Transaction History Workload
   * Simulates blockchain explorer queries for transaction history.
   * Tests range queries and historical data access patterns.
   */
  @Test
  public void benchmarkTransactionHistoryWorkload() throws Exception {
    String testName = getImplementationName() + "TransactionHistoryWorkload";
    printTestHeader(testName);

    int totalTransactions = 100000;
    int historyQueries = 1000;
    
    System.out.println("Setting up transaction history workload:");
    System.out.printf("  - %d transactions\n", totalTransactions);
    System.out.printf("  - %d history queries\n", historyQueries);

    // Setup: Create transaction history
    System.out.println("Creating transaction history...");
    Map<byte[], byte[]> transactionBatch = new HashMap<>();
    List<byte[]> transactionHashes = new ArrayList<>();
    
    for (int i = 0; i < totalTransactions; i++) {
      TronTransaction tx = generateTransaction(i);
      transactionBatch.put(tx.hash, serializeTransaction(tx));
      transactionHashes.add(tx.hash);
      
      // Batch write every 10000 transactions
      if (i % 10000 == 9999) {
        storage.batchWrite(TRANSACTION_DB, transactionBatch).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        transactionBatch.clear();
        System.out.printf("  Stored %d transactions\n", i + 1);
      }
    }
    
    // Store remaining transactions
    if (!transactionBatch.isEmpty()) {
      storage.batchWrite(TRANSACTION_DB, transactionBatch).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
    }

    // Benchmark: Random transaction lookups
    System.out.println("Running transaction history queries...");
    long queryStart = System.nanoTime();
    long queryLatencySum = 0;
    int successfulQueries = 0;
    
    for (int i = 0; i < historyQueries; i++) {
      byte[] randomHash = transactionHashes.get(secureRandom.nextInt(transactionHashes.size()));
      
      long singleQueryStart = System.nanoTime();
      byte[] result = storage.get(TRANSACTION_DB, randomHash).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      long singleQueryEnd = System.nanoTime();
      
      if (result != null) {
        successfulQueries++;
        queryLatencySum += (singleQueryEnd - singleQueryStart);
      }
      
      if (i % 100 == 0) {
        System.out.printf("  Completed %d history queries\n", i);
      }
    }
    
    long queryEnd = System.nanoTime();
    double totalQueryTimeMs = (queryEnd - queryStart) / 1_000_000.0;
    double avgQueryLatencyMs = (queryLatencySum / successfulQueries) / 1_000_000.0;
    double queryThroughput = successfulQueries / (totalQueryTimeMs / 1000.0);
    double successRate = (double) successfulQueries / historyQueries * 100;
    
    // Write metrics
    writeMetric(testName, "total_transactions", totalTransactions, "count");
    writeMetric(testName, "history_queries", historyQueries, "count");
    writeMetric(testName, "successful_queries", successfulQueries, "count");
    writeMetric(testName, "success_rate", successRate, "percent");
    writeMetric(testName, "avg_query_latency", avgQueryLatencyMs, "ms");
    writeMetric(testName, "query_throughput", queryThroughput, "queries/sec");
    
    System.out.printf("\nTransaction History Results:\n");
    System.out.printf("  Successful Queries: %d/%d (%.1f%%)\n", successfulQueries, historyQueries, successRate);
    System.out.printf("  Average Query Latency: %.3f ms\n", avgQueryLatencyMs);
    System.out.printf("  Query Throughput: %.0f queries/sec\n", queryThroughput);
    
    // Performance assertion
    Assert.assertTrue("Success rate should be 100%", successRate == 100.0);
    Assert.assertTrue("Query latency should be reasonable", avgQueryLatencyMs < 100.0);
  }

  /**
   * Test 4: Smart Contract State Workload
   * Simulates smart contract storage operations.
   * Tests frequent small updates and state queries.
   */
  @Test
  public void benchmarkSmartContractStateWorkload() throws Exception {
    String testName = getImplementationName() + "SmartContractStateWorkload";
    printTestHeader(testName);

    int totalContracts = 1000;
    int stateOperationsPerContract = 100;
    int totalOperations = totalContracts * stateOperationsPerContract;
    
    System.out.println("Setting up smart contract state workload:");
    System.out.printf("  - %d contracts\n", totalContracts);
    System.out.printf("  - %d state operations per contract\n", stateOperationsPerContract);
    System.out.printf("  - %d total operations\n", totalOperations);

    // Benchmark: Contract state operations
    System.out.println("Running contract state operations...");
    long operationStart = System.nanoTime();
    long writeLatencySum = 0;
    long readLatencySum = 0;
    int writeCount = 0;
    int readCount = 0;
    
    for (int contractIdx = 0; contractIdx < totalContracts; contractIdx++) {
      String contractAddress = "contract-" + contractIdx;
      
      // Simulate contract state operations
      for (int opIdx = 0; opIdx < stateOperationsPerContract; opIdx++) {
        String stateKey = contractAddress + "-state-" + opIdx;
        byte[] key = stateKey.getBytes();
        byte[] value = generateRandomValue(TRON_CONTRACT_STATE_SIZE);
        
        // Write operation
        long writeStart = System.nanoTime();
        storage.put(CONTRACT_DB, key, value).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        long writeEnd = System.nanoTime();
        writeLatencySum += (writeEnd - writeStart);
        writeCount++;
        
        // Read operation (simulate contract call)
        long readStart = System.nanoTime();
        byte[] result = storage.get(CONTRACT_DB, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        long readEnd = System.nanoTime();
        readLatencySum += (readEnd - readStart);
        readCount++;
        
        Assert.assertNotNull("Contract state should exist", result);
        Assert.assertArrayEquals("Contract state should match", value, result);
      }
      
      if (contractIdx % 100 == 0) {
        System.out.printf("  Processed %d contracts\n", contractIdx);
      }
    }
    
    long operationEnd = System.nanoTime();
    double totalOperationTimeMs = (operationEnd - operationStart) / 1_000_000.0;
    double avgWriteLatencyMs = (writeLatencySum / writeCount) / 1_000_000.0;
    double avgReadLatencyMs = (readLatencySum / readCount) / 1_000_000.0;
    double operationThroughput = totalOperations / (totalOperationTimeMs / 1000.0);
    
    // Write metrics
    writeMetric(testName, "total_contracts", totalContracts, "count");
    writeMetric(testName, "total_operations", totalOperations, "count");
    writeMetric(testName, "avg_write_latency", avgWriteLatencyMs, "ms");
    writeMetric(testName, "avg_read_latency", avgReadLatencyMs, "ms");
    writeMetric(testName, "operation_throughput", operationThroughput, "ops/sec");
    
    System.out.printf("\nSmart Contract State Results:\n");
    System.out.printf("  Total Operations: %d\n", totalOperations);
    System.out.printf("  Average Write Latency: %.3f ms\n", avgWriteLatencyMs);
    System.out.printf("  Average Read Latency: %.3f ms\n", avgReadLatencyMs);
    System.out.printf("  Operation Throughput: %.0f ops/sec\n", operationThroughput);
    
    // Performance assertion
    Assert.assertTrue("Write latency should be reasonable", avgWriteLatencyMs < 10.0);
    Assert.assertTrue("Read latency should be reasonable", avgReadLatencyMs < 5.0);
  }

  /**
   * Test 5: Fast Sync Workload
   * Simulates fast synchronization operations.
   * Tests bulk data loading and high-throughput writes.
   */
  @Test
  public void benchmarkFastSyncWorkload() throws Exception {
    String testName = getImplementationName() + "FastSyncWorkload";
    printTestHeader(testName);

    int totalBatches = 100;
    int batchSize = TRON_SYNC_BATCH_SIZE;
    long totalOperations = (long) totalBatches * batchSize;
    
    System.out.println("Setting up fast sync workload:");
    System.out.printf("  - %d batches\n", totalBatches);
    System.out.printf("  - %d operations per batch\n", batchSize);
    System.out.printf("  - %d total operations\n", totalOperations);

    // Benchmark: Fast sync operations
    System.out.println("Running fast sync operations...");
    long syncStart = System.nanoTime();
    long batchLatencySum = 0;
    
    for (int batchIdx = 0; batchIdx < totalBatches; batchIdx++) {
      Map<byte[], byte[]> syncBatch = new HashMap<>();
      
      // Generate batch data
      for (int i = 0; i < batchSize; i++) {
        String key = "sync-" + batchIdx + "-" + i;
        byte[] value = generateRandomValue(TRON_TRANSACTION_SIZE);
        syncBatch.put(key.getBytes(), value);
      }
      
      // Batch write
      long batchStart = System.nanoTime();
      storage.batchWrite(TRANSACTION_DB, syncBatch).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
      long batchEnd = System.nanoTime();
      batchLatencySum += (batchEnd - batchStart);
      
      if (batchIdx % 10 == 0) {
        double batchLatencyMs = (batchEnd - batchStart) / 1_000_000.0;
        System.out.printf("  Batch %d completed in %.2f ms\n", batchIdx, batchLatencyMs);
      }
    }
    
    long syncEnd = System.nanoTime();
    double totalSyncTimeMs = (syncEnd - syncStart) / 1_000_000.0;
    double avgBatchLatencyMs = (batchLatencySum / totalBatches) / 1_000_000.0;
    double syncThroughput = totalOperations / (totalSyncTimeMs / 1000.0);
    double dataThroughputMBps = (totalOperations * TRON_TRANSACTION_SIZE) / (totalSyncTimeMs / 1000.0) / (1024 * 1024);
    
    // Write metrics
    writeMetric(testName, "total_batches", totalBatches, "count");
    writeMetric(testName, "batch_size", batchSize, "count");
    writeMetric(testName, "total_operations", totalOperations, "count");
    writeMetric(testName, "total_sync_time", totalSyncTimeMs, "ms");
    writeMetric(testName, "avg_batch_latency", avgBatchLatencyMs, "ms");
    writeMetric(testName, "sync_throughput", syncThroughput, "ops/sec");
    writeMetric(testName, "data_throughput", dataThroughputMBps, "MB/sec");
    
    System.out.printf("\nFast Sync Results:\n");
    System.out.printf("  Total Sync Time: %.2f ms\n", totalSyncTimeMs);
    System.out.printf("  Average Batch Latency: %.2f ms\n", avgBatchLatencyMs);
    System.out.printf("  Sync Throughput: %.0f ops/sec\n", syncThroughput);
    System.out.printf("  Data Throughput: %.2f MB/sec\n", dataThroughputMBps);
    
    // Performance assertion
    Assert.assertTrue("Sync throughput should support fast sync", dataThroughputMBps >= 10.0);
  }

  /**
   * Test 6: Mixed Workload Stress Test
   * Simulates concurrent operations mixing reads and writes.
   * Tests realistic production load with multiple operation types.
   */
  @Test
  public void benchmarkMixedWorkloadStressTest() throws Exception {
    String testName = getImplementationName() + "MixedWorkloadStressTest";
    printTestHeader(testName);

    int durationSeconds = 60;
    int concurrentThreads = 10;
    
    System.out.println("Setting up mixed workload stress test:");
    System.out.printf("  - %d seconds duration\n", durationSeconds);
    System.out.printf("  - %d concurrent threads\n", concurrentThreads);

    // Setup: Pre-populate data
    System.out.println("Pre-populating data...");
    Map<byte[], byte[]> initialData = new HashMap<>();
    for (int i = 0; i < 10000; i++) {
      initialData.put(("stress-" + i).getBytes(), generateRandomValue(256));
    }
    storage.batchWrite(ACCOUNT_DB, initialData).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);

    // Benchmark: Mixed workload
    System.out.println("Running mixed workload stress test...");
    ExecutorService executor = Executors.newFixedThreadPool(concurrentThreads);
    CountDownLatch latch = new CountDownLatch(concurrentThreads);
    AtomicLong totalOperations = new AtomicLong(0);
    AtomicLong totalLatency = new AtomicLong(0);
    
    long stressStart = System.nanoTime();
    
    for (int threadIdx = 0; threadIdx < concurrentThreads; threadIdx++) {
      final int threadId = threadIdx;
      executor.submit(() -> {
        try {
          long threadStart = System.nanoTime();
          long threadOperations = 0;
          long threadLatency = 0;
          
          while ((System.nanoTime() - threadStart) < durationSeconds * 1_000_000_000L) {
            // Mix of operations: 70% reads, 20% writes, 10% batch operations
            int operation = secureRandom.nextInt(100);
            
            long opStart = System.nanoTime();
            
            if (operation < 70) {
              // Read operation
              byte[] key = ("stress-" + secureRandom.nextInt(10000)).getBytes();
              storage.get(ACCOUNT_DB, key).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
            } else if (operation < 90) {
              // Write operation
              byte[] key = ("stress-new-" + threadId + "-" + threadOperations).getBytes();
              byte[] value = generateRandomValue(256);
              storage.put(ACCOUNT_DB, key, value).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
            } else {
              // Batch operation
              Map<byte[], byte[]> batch = new HashMap<>();
              for (int i = 0; i < 10; i++) {
                byte[] key = ("stress-batch-" + threadId + "-" + threadOperations + "-" + i).getBytes();
                batch.put(key, generateRandomValue(256));
              }
              storage.batchWrite(ACCOUNT_DB, batch).get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
            }
            
            long opEnd = System.nanoTime();
            threadLatency += (opEnd - opStart);
            threadOperations++;
          }
          
          totalOperations.addAndGet(threadOperations);
          totalLatency.addAndGet(threadLatency);
          
        } catch (Exception e) {
          e.printStackTrace();
        } finally {
          latch.countDown();
        }
      });
    }
    
    latch.await();
    executor.shutdown();
    
    long stressEnd = System.nanoTime();
    double totalStressTimeMs = (stressEnd - stressStart) / 1_000_000.0;
    double avgLatencyMs = (totalLatency.get() / totalOperations.get()) / 1_000_000.0;
    double stressThroughput = totalOperations.get() / (totalStressTimeMs / 1000.0);
    
    // Write metrics
    writeMetric(testName, "duration", durationSeconds, "seconds");
    writeMetric(testName, "concurrent_threads", concurrentThreads, "count");
    writeMetric(testName, "total_operations", totalOperations.get(), "count");
    writeMetric(testName, "avg_latency", avgLatencyMs, "ms");
    writeMetric(testName, "stress_throughput", stressThroughput, "ops/sec");
    
    System.out.printf("\nMixed Workload Stress Results:\n");
    System.out.printf("  Total Operations: %d\n", totalOperations.get());
    System.out.printf("  Average Latency: %.3f ms\n", avgLatencyMs);
    System.out.printf("  Stress Throughput: %.0f ops/sec\n", stressThroughput);
    System.out.printf("  Operations per Thread: %.0f\n", (double) totalOperations.get() / concurrentThreads);
    
    // Performance assertion
    Assert.assertTrue("Stress throughput should handle concurrent load", stressThroughput >= 1000.0);
  }

  // Helper classes for Tron data structures
  private static class TronBlock {
    long number;
    byte[] hash;
    byte[] parentHash;
    long timestamp;
    List<TronTransaction> transactions;
    
    TronBlock(long number, byte[] hash, byte[] parentHash, long timestamp, List<TronTransaction> transactions) {
      this.number = number;
      this.hash = hash;
      this.parentHash = parentHash;
      this.timestamp = timestamp;
      this.transactions = transactions;
    }
  }
  
  private static class TronTransaction {
    byte[] hash;
    byte[] fromAddress;
    byte[] toAddress;
    long amount;
    long timestamp;
    
    TronTransaction(byte[] hash, byte[] fromAddress, byte[] toAddress, long amount, long timestamp) {
      this.hash = hash;
      this.fromAddress = fromAddress;
      this.toAddress = toAddress;
      this.amount = amount;
      this.timestamp = timestamp;
    }
  }
  
  private static class TronAccount {
    byte[] address;
    long balance;
    long lastModified;
    
    TronAccount(byte[] address, long balance, long lastModified) {
      this.address = address;
      this.balance = balance;
      this.lastModified = lastModified;
    }
  }

  // Helper methods for data generation
  private TronBlock generateBlock(long blockNumber, int transactionCount) {
    byte[] blockHash = generateHash();
    byte[] parentHash = blockNumber > 0 ? generateHash() : new byte[32];
    long timestamp = System.currentTimeMillis();
    
    List<TronTransaction> transactions = new ArrayList<>();
    for (int i = 0; i < transactionCount; i++) {
      transactions.add(generateTransaction(transactionId.incrementAndGet()));
    }
    
    return new TronBlock(blockNumber, blockHash, parentHash, timestamp, transactions);
  }
  
  private TronTransaction generateTransaction(long txId) {
    byte[] hash = generateHash();
    byte[] fromAddress = generateAddress();
    byte[] toAddress = generateAddress();
    long amount = secureRandom.nextInt(1000000);
    long timestamp = System.currentTimeMillis();
    
    return new TronTransaction(hash, fromAddress, toAddress, amount, timestamp);
  }
  
  private byte[] generateHash() {
    byte[] hash = new byte[32];
    secureRandom.nextBytes(hash);
    return hash;
  }
  
  private byte[] generateAddress() {
    byte[] address = new byte[21]; // Tron address length
    secureRandom.nextBytes(address);
    return address;
  }
  
  private byte[] longToBytes(long value) {
    return ByteBuffer.allocate(8).putLong(value).array();
  }
  
  // Serialization methods (simplified for testing)
  private byte[] serializeBlock(TronBlock block) {
    // Simplified serialization
    return ByteBuffer.allocate(8 + 32 + 32 + 8 + 4)
        .putLong(block.number)
        .put(block.hash)
        .put(block.parentHash)
        .putLong(block.timestamp)
        .putInt(block.transactions.size())
        .array();
  }
  
  private byte[] serializeTransaction(TronTransaction tx) {
    // Simplified serialization
    return ByteBuffer.allocate(32 + 21 + 21 + 8 + 8)
        .put(tx.hash)
        .put(tx.fromAddress)
        .put(tx.toAddress)
        .putLong(tx.amount)
        .putLong(tx.timestamp)
        .array();
  }
  
  private byte[] serializeAccount(TronAccount account) {
    // Simplified serialization
    return ByteBuffer.allocate(21 + 8 + 8)
        .put(account.address)
        .putLong(account.balance)
        .putLong(account.lastModified)
        .array();
  }
} 