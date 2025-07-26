package org.tron.core.execution.spi;

import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;

/**
 * Historical Replay Tool for Shadow Execution Verification.
 *
 * <p>This tool replays historical mainnet blocks through shadow execution to verify equivalence
 * between Java and Rust execution engines on real-world data.
 */
public class HistoricalReplayTool {

  private static final Logger logger = LoggerFactory.getLogger(HistoricalReplayTool.class);

  // Configuration
  private final int defaultBlockCount;
  private final int maxConcurrentBlocks;
  private final boolean enableDetailedLogging;
  private final boolean failOnFirstMismatch;

  // Execution components
  private final ExecutionSPI shadowExecutionSPI;
  private final ExecutorService executorService;

  // Metrics
  private final AtomicLong totalBlocks = new AtomicLong(0);
  private final AtomicLong totalTransactions = new AtomicLong(0);
  private final AtomicLong successfulTransactions = new AtomicLong(0);
  private final AtomicLong failedTransactions = new AtomicLong(0);
  private final AtomicLong mismatches = new AtomicLong(0);
  private final AtomicInteger currentlyProcessing = new AtomicInteger(0);

  // Results
  private final List<ReplayResult> results = new ArrayList<>();
  private final List<MismatchReport> mismatchReports = new ArrayList<>();

  public HistoricalReplayTool() {
    this(
        getConfigInt("replay.block_count", 100000),
        getConfigInt("replay.max_concurrent", 10),
        getConfigBoolean("replay.detailed_logging", false),
        getConfigBoolean("replay.fail_on_mismatch", false));
  }

  public HistoricalReplayTool(
      int defaultBlockCount,
      int maxConcurrentBlocks,
      boolean enableDetailedLogging,
      boolean failOnFirstMismatch) {
    this.defaultBlockCount = defaultBlockCount;
    this.maxConcurrentBlocks = maxConcurrentBlocks;
    this.enableDetailedLogging = enableDetailedLogging;
    this.failOnFirstMismatch = failOnFirstMismatch;

    // Initialize shadow execution SPI
    this.shadowExecutionSPI = ExecutionSpiFactory.createExecution();

    // Initialize thread pool for concurrent processing
    this.executorService = Executors.newFixedThreadPool(maxConcurrentBlocks);

    logger.info(
        "Historical Replay Tool initialized (blocks: {}, concurrent: {}, detailed: {}, fail_fast: {})",
        defaultBlockCount,
        maxConcurrentBlocks,
        enableDetailedLogging,
        failOnFirstMismatch);
  }

  /** Replay historical blocks starting from a specific block number. */
  public ReplayReport replayBlocks(long startBlockNumber) {
    return replayBlocks(startBlockNumber, defaultBlockCount);
  }

  /** Replay a specific range of historical blocks. */
  public ReplayReport replayBlocks(long startBlockNumber, int blockCount) {
    logger.info(
        "Starting historical replay: blocks {} to {} ({} blocks)",
        startBlockNumber,
        startBlockNumber + blockCount - 1,
        blockCount);

    long startTime = System.currentTimeMillis();

    try {
      // Process blocks in batches for better performance
      int batchSize = Math.min(maxConcurrentBlocks, 100);

      for (int i = 0; i < blockCount; i += batchSize) {
        int currentBatchSize = Math.min(batchSize, blockCount - i);
        long batchStartBlock = startBlockNumber + i;

        processBatch(batchStartBlock, currentBatchSize);

        // Log progress
        if (i % 1000 == 0 || enableDetailedLogging) {
          logProgress(i, blockCount);
        }

        // Check for early termination on mismatch
        if (failOnFirstMismatch && mismatches.get() > 0) {
          logger.warn("Stopping replay due to mismatch (fail_on_mismatch=true)");
          break;
        }
      }

      // Wait for all tasks to complete
      executorService.shutdown();
      if (!executorService.awaitTermination(30, TimeUnit.MINUTES)) {
        logger.warn("Some replay tasks did not complete within timeout");
        executorService.shutdownNow();
      }

    } catch (Exception e) {
      logger.error("Historical replay failed", e);
      throw new RuntimeException("Historical replay failed", e);
    }

    long endTime = System.currentTimeMillis();
    long duration = endTime - startTime;

    // Generate final report
    ReplayReport report = generateReport(startBlockNumber, blockCount, duration);

    logger.info(
        "Historical replay completed: {} blocks, {} transactions, {} mismatches in {}ms",
        totalBlocks.get(),
        totalTransactions.get(),
        mismatches.get(),
        duration);

    return report;
  }

  /** Process a batch of blocks concurrently. */
  private void processBatch(long startBlock, int batchSize) throws InterruptedException {
    List<CompletableFuture<Void>> futures = new ArrayList<>();

    for (int i = 0; i < batchSize; i++) {
      long blockNumber = startBlock + i;

      CompletableFuture<Void> future =
          CompletableFuture.runAsync(
              () -> {
                try {
                  processBlock(blockNumber);
                } catch (Exception e) {
                  logger.error("Failed to process block {}", blockNumber, e);
                }
              },
              executorService);

      futures.add(future);
    }

    // Wait for batch to complete
    CompletableFuture.allOf(futures.toArray(new CompletableFuture[0])).join();
  }

  /** Process a single block through shadow execution. */
  private void processBlock(long blockNumber) {
    currentlyProcessing.incrementAndGet();

    try {
      if (enableDetailedLogging) {
        logger.debug("Processing block {}", blockNumber);
      }

      // Load block from database/storage
      BlockCapsule block = loadBlock(blockNumber);
      if (block == null) {
        logger.warn("Block {} not found, skipping", blockNumber);
        return;
      }

      // Process all transactions in the block
      List<TransactionCapsule> transactions = block.getTransactions();
      ReplayResult blockResult = new ReplayResult(blockNumber, transactions.size());

      for (TransactionCapsule transaction : transactions) {
        processTransaction(transaction, blockResult);
      }

      // Update metrics
      totalBlocks.incrementAndGet();
      totalTransactions.addAndGet(transactions.size());

      synchronized (results) {
        results.add(blockResult);
      }

      if (enableDetailedLogging) {
        logger.debug(
            "Completed block {}: {} transactions, {} mismatches",
            blockNumber,
            transactions.size(),
            blockResult.getMismatchCount());
      }

    } finally {
      currentlyProcessing.decrementAndGet();
    }
  }

  /** Process a single transaction through shadow execution. */
  private void processTransaction(TransactionCapsule transaction, ReplayResult blockResult) {
    try {
      // Create transaction context
      TransactionContext context = createTransactionContext(transaction);

      // Execute through shadow execution
      CompletableFuture<ExecutionProgramResult> future =
          shadowExecutionSPI.executeTransaction(context);
      ExecutionProgramResult result = future.get();

      // Record result
      if (result.isSuccess()) {
        successfulTransactions.incrementAndGet();
        blockResult.addSuccessfulTransaction();
      } else {
        failedTransactions.incrementAndGet();
        blockResult.addFailedTransaction();
      }

      // Check for mismatches (if using shadow execution)
      if (shadowExecutionSPI instanceof ShadowExecutionSPI) {
        // Mismatch detection is handled internally by ShadowExecutionSPI
        // We can get mismatch stats from the SPI later
      }

    } catch (Exception e) {
      logger.error("Failed to process transaction {}", transaction.getTransactionId(), e);
      failedTransactions.incrementAndGet();
      blockResult.addFailedTransaction();
    }
  }

  /** Load a block from the database/storage. */
  private BlockCapsule loadBlock(long blockNumber) {
    // TODO: Implement actual block loading from Tron database
    // For now, return null to indicate block not found
    // In a real implementation, this would:
    // 1. Connect to Tron database
    // 2. Load block by number
    // 3. Return BlockCapsule

    logger.debug("Loading block {} (placeholder implementation)", blockNumber);
    return null; // Placeholder
  }

  /** Create a transaction context for execution. */
  private TransactionContext createTransactionContext(TransactionCapsule transaction) {
    // TODO: Implement proper transaction context creation
    // This would include block context, database state, etc.
    return new TransactionContext(null, transaction, null, false, false);
  }

  /** Log current progress. */
  private void logProgress(int processed, int total) {
    double percentage = (double) processed / total * 100;
    logger.info(
        "Progress: {}/{} blocks ({:.1f}%) - {} transactions, {} mismatches, {} active",
        processed,
        total,
        percentage,
        totalTransactions.get(),
        mismatches.get(),
        currentlyProcessing.get());
  }

  /** Generate final replay report. */
  private ReplayReport generateReport(long startBlock, int blockCount, long duration) {
    // Get mismatch stats from shadow execution if available
    String shadowStats = "";
    if (shadowExecutionSPI instanceof ShadowExecutionSPI) {
      shadowStats = ((ShadowExecutionSPI) shadowExecutionSPI).getMismatchStats();
    }

    return new ReplayReport(
        startBlock,
        blockCount,
        duration,
        totalBlocks.get(),
        totalTransactions.get(),
        successfulTransactions.get(),
        failedTransactions.get(),
        mismatches.get(),
        shadowStats,
        new ArrayList<>(results));
  }

  /** Cleanup resources. */
  public void cleanup() {
    if (!executorService.isShutdown()) {
      executorService.shutdown();
    }

    if (shadowExecutionSPI instanceof ShadowExecutionSPI) {
      ((ShadowExecutionSPI) shadowExecutionSPI).cleanup();
    }
  }

  // Configuration helper methods
  private static int getConfigInt(String key, int defaultValue) {
    return Integer.parseInt(System.getProperty(key, String.valueOf(defaultValue)));
  }

  private static boolean getConfigBoolean(String key, boolean defaultValue) {
    return Boolean.parseBoolean(System.getProperty(key, String.valueOf(defaultValue)));
  }

  /** Result for a single block replay. */
  public static class ReplayResult {
    private final long blockNumber;
    private final int totalTransactions;
    private int successfulTransactions = 0;
    private int failedTransactions = 0;
    private int mismatchCount = 0;

    public ReplayResult(long blockNumber, int totalTransactions) {
      this.blockNumber = blockNumber;
      this.totalTransactions = totalTransactions;
    }

    public void addSuccessfulTransaction() {
      successfulTransactions++;
    }

    public void addFailedTransaction() {
      failedTransactions++;
    }

    public void addMismatch() {
      mismatchCount++;
    }

    // Getters
    public long getBlockNumber() {
      return blockNumber;
    }

    public int getTotalTransactions() {
      return totalTransactions;
    }

    public int getSuccessfulTransactions() {
      return successfulTransactions;
    }

    public int getFailedTransactions() {
      return failedTransactions;
    }

    public int getMismatchCount() {
      return mismatchCount;
    }
  }

  /** Mismatch report for detailed analysis. */
  public static class MismatchReport {
    private final long blockNumber;
    private final String transactionId;
    private final String mismatchType;
    private final String details;

    public MismatchReport(
        long blockNumber, String transactionId, String mismatchType, String details) {
      this.blockNumber = blockNumber;
      this.transactionId = transactionId;
      this.mismatchType = mismatchType;
      this.details = details;
    }

    // Getters
    public long getBlockNumber() {
      return blockNumber;
    }

    public String getTransactionId() {
      return transactionId;
    }

    public String getMismatchType() {
      return mismatchType;
    }

    public String getDetails() {
      return details;
    }
  }
}
