package org.tron.core.execution.spi;

import java.util.List;
import java.util.stream.Collectors;

/**
 * Comprehensive report for historical replay execution.
 */
public class ReplayReport {
  
  private final long startBlock;
  private final int requestedBlockCount;
  private final long durationMs;
  
  // Block-level metrics
  private final long processedBlocks;
  private final long totalTransactions;
  private final long successfulTransactions;
  private final long failedTransactions;
  private final long mismatches;
  
  // Shadow execution stats
  private final String shadowExecutionStats;
  
  // Detailed results
  private final List<HistoricalReplayTool.ReplayResult> blockResults;
  
  public ReplayReport(long startBlock, int requestedBlockCount, long durationMs,
                     long processedBlocks, long totalTransactions,
                     long successfulTransactions, long failedTransactions,
                     long mismatches, String shadowExecutionStats,
                     List<HistoricalReplayTool.ReplayResult> blockResults) {
    this.startBlock = startBlock;
    this.requestedBlockCount = requestedBlockCount;
    this.durationMs = durationMs;
    this.processedBlocks = processedBlocks;
    this.totalTransactions = totalTransactions;
    this.successfulTransactions = successfulTransactions;
    this.failedTransactions = failedTransactions;
    this.mismatches = mismatches;
    this.shadowExecutionStats = shadowExecutionStats;
    this.blockResults = blockResults;
  }
  
  // Getters
  public long getStartBlock() { return startBlock; }
  public int getRequestedBlockCount() { return requestedBlockCount; }
  public long getDurationMs() { return durationMs; }
  public long getProcessedBlocks() { return processedBlocks; }
  public long getTotalTransactions() { return totalTransactions; }
  public long getSuccessfulTransactions() { return successfulTransactions; }
  public long getFailedTransactions() { return failedTransactions; }
  public long getMismatches() { return mismatches; }
  public String getShadowExecutionStats() { return shadowExecutionStats; }
  public List<HistoricalReplayTool.ReplayResult> getBlockResults() { return blockResults; }
  
  // Computed metrics
  public double getSuccessRate() {
    return totalTransactions > 0 ? (double) successfulTransactions / totalTransactions * 100.0 : 0.0;
  }
  
  public double getMismatchRate() {
    return totalTransactions > 0 ? (double) mismatches / totalTransactions * 100.0 : 0.0;
  }
  
  public double getBlocksPerSecond() {
    return durationMs > 0 ? (double) processedBlocks / (durationMs / 1000.0) : 0.0;
  }
  
  public double getTransactionsPerSecond() {
    return durationMs > 0 ? (double) totalTransactions / (durationMs / 1000.0) : 0.0;
  }
  
  public long getEndBlock() {
    return startBlock + processedBlocks - 1;
  }
  
  /**
   * Get blocks with mismatches for detailed analysis.
   */
  public List<HistoricalReplayTool.ReplayResult> getBlocksWithMismatches() {
    return blockResults.stream()
        .filter(result -> result.getMismatchCount() > 0)
        .collect(Collectors.toList());
  }
  
  /**
   * Get summary statistics.
   */
  public String getSummary() {
    return String.format(
        "Historical Replay Summary:\n" +
        "  Blocks: %d/%d processed (%.1f%%)\n" +
        "  Range: %d to %d\n" +
        "  Duration: %.2f seconds\n" +
        "  Performance: %.1f blocks/sec, %.1f tx/sec\n" +
        "  Transactions: %d total (%d success, %d failed)\n" +
        "  Success Rate: %.2f%%\n" +
        "  Mismatches: %d (%.4f%%)\n" +
        "  Shadow Stats: %s",
        processedBlocks, requestedBlockCount, 
        (double) processedBlocks / requestedBlockCount * 100.0,
        startBlock, getEndBlock(),
        durationMs / 1000.0,
        getBlocksPerSecond(), getTransactionsPerSecond(),
        totalTransactions, successfulTransactions, failedTransactions,
        getSuccessRate(),
        mismatches, getMismatchRate(),
        shadowExecutionStats
    );
  }
  
  /**
   * Get detailed report with block-by-block breakdown.
   */
  public String getDetailedReport() {
    StringBuilder report = new StringBuilder();
    report.append(getSummary()).append("\n\n");
    
    if (!getBlocksWithMismatches().isEmpty()) {
      report.append("Blocks with Mismatches:\n");
      for (HistoricalReplayTool.ReplayResult result : getBlocksWithMismatches()) {
        report.append(String.format("  Block %d: %d/%d transactions, %d mismatches\n",
            result.getBlockNumber(), result.getSuccessfulTransactions(),
            result.getTotalTransactions(), result.getMismatchCount()));
      }
      report.append("\n");
    }
    
    // Performance breakdown
    report.append("Performance Breakdown:\n");
    report.append(String.format("  Average transactions per block: %.1f\n",
        processedBlocks > 0 ? (double) totalTransactions / processedBlocks : 0.0));
    report.append(String.format("  Average processing time per block: %.2f ms\n",
        processedBlocks > 0 ? (double) durationMs / processedBlocks : 0.0));
    report.append(String.format("  Average processing time per transaction: %.2f ms\n",
        totalTransactions > 0 ? (double) durationMs / totalTransactions : 0.0));
    
    return report.toString();
  }
  
  /**
   * Export report to CSV format for analysis.
   */
  public String toCsv() {
    StringBuilder csv = new StringBuilder();
    
    // Header
    csv.append("block_number,total_transactions,successful_transactions,failed_transactions,mismatch_count\n");
    
    // Data rows
    for (HistoricalReplayTool.ReplayResult result : blockResults) {
      csv.append(String.format("%d,%d,%d,%d,%d\n",
          result.getBlockNumber(),
          result.getTotalTransactions(),
          result.getSuccessfulTransactions(),
          result.getFailedTransactions(),
          result.getMismatchCount()));
    }
    
    return csv.toString();
  }
  
  /**
   * Check if the replay was successful (no mismatches).
   */
  public boolean isSuccessful() {
    return mismatches == 0;
  }
  
  /**
   * Check if the replay completed all requested blocks.
   */
  public boolean isComplete() {
    return processedBlocks == requestedBlockCount;
  }
  
  /**
   * Get quality score (0-100) based on success rate and completion.
   */
  public double getQualityScore() {
    double completionScore = isComplete() ? 50.0 : (double) processedBlocks / requestedBlockCount * 50.0;
    double successScore = getSuccessRate() * 0.3; // 30% weight for success rate
    double mismatchPenalty = getMismatchRate() * 0.2; // 20% penalty for mismatches
    
    return Math.max(0.0, Math.min(100.0, completionScore + successScore - mismatchPenalty));
  }
  
  @Override
  public String toString() {
    return getSummary();
  }
}
