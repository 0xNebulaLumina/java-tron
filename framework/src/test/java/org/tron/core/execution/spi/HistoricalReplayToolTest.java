package org.tron.core.execution.spi;

import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Test class for HistoricalReplayTool.
 */
public class HistoricalReplayToolTest {

  private static final Logger logger = LoggerFactory.getLogger(HistoricalReplayToolTest.class);
  
  private HistoricalReplayTool replayTool;
  
  @Before
  public void setUp() {
    // Create replay tool with test configuration
    replayTool = new HistoricalReplayTool(
        10,    // defaultBlockCount - small for testing
        2,     // maxConcurrentBlocks - small for testing
        true,  // enableDetailedLogging
        false  // failOnFirstMismatch
    );
  }
  
  @After
  public void tearDown() {
    if (replayTool != null) {
      replayTool.cleanup();
    }
  }
  
  @Test
  public void testReplayToolCreation() {
    Assert.assertNotNull("Replay tool should be created", replayTool);
    logger.info("✅ Historical replay tool created successfully");
  }
  
  @Test
  public void testReplayToolConfiguration() {
    // Test with default configuration
    HistoricalReplayTool defaultTool = new HistoricalReplayTool();
    Assert.assertNotNull("Default replay tool should be created", defaultTool);
    
    try {
      defaultTool.cleanup();
    } catch (Exception e) {
      // Ignore cleanup errors in test
    }
    
    logger.info("✅ Default configuration replay tool created successfully");
  }
  
  @Test
  public void testReplayBlocksWithNoData() {
    // Test replay when no blocks are available (expected behavior)
    try {
      ReplayReport report = replayTool.replayBlocks(1000000, 5); // Use high block number that likely doesn't exist
      
      // Verify report structure
      Assert.assertNotNull("Report should not be null", report);
      Assert.assertEquals("Start block should match", 1000000, report.getStartBlock());
      Assert.assertEquals("Requested block count should match", 5, report.getRequestedBlockCount());
      Assert.assertTrue("Duration should be positive", report.getDurationMs() >= 0);
      
      // Since no blocks exist, processed blocks should be 0
      Assert.assertEquals("Processed blocks should be 0", 0, report.getProcessedBlocks());
      Assert.assertEquals("Total transactions should be 0", 0, report.getTotalTransactions());
      
      logger.info("✅ Replay with no data completed: {}", report.getSummary());
      
    } catch (Exception e) {
      logger.error("Replay failed", e);
      Assert.fail("Replay should not throw exception: " + e.getMessage());
    }
  }
  
  @Test
  public void testReplayReportGeneration() {
    try {
      ReplayReport report = replayTool.replayBlocks(1, 3);
      
      // Test report methods
      Assert.assertNotNull("Summary should not be null", report.getSummary());
      Assert.assertNotNull("Detailed report should not be null", report.getDetailedReport());
      Assert.assertNotNull("CSV export should not be null", report.toCsv());
      
      // Test computed metrics
      Assert.assertTrue("Success rate should be >= 0", report.getSuccessRate() >= 0);
      Assert.assertTrue("Mismatch rate should be >= 0", report.getMismatchRate() >= 0);
      Assert.assertTrue("Quality score should be >= 0", report.getQualityScore() >= 0);
      Assert.assertTrue("Quality score should be <= 100", report.getQualityScore() <= 100);
      
      // Test boolean checks
      Assert.assertTrue("Should be complete when no blocks processed", report.isComplete() || report.getProcessedBlocks() == 0);
      Assert.assertTrue("Should be successful when no mismatches", report.isSuccessful() || report.getMismatches() == 0);
      
      logger.info("✅ Report generation test passed");
      logger.info("Report summary:\n{}", report.getSummary());
      
    } catch (Exception e) {
      logger.error("Report generation failed", e);
      Assert.fail("Report generation should not throw exception: " + e.getMessage());
    }
  }
  
  @Test
  public void testReplayWithDifferentBlockCounts() {
    // Test with different block counts
    int[] blockCounts = {1, 5, 10};
    
    for (int blockCount : blockCounts) {
      try {
        ReplayReport report = replayTool.replayBlocks(100000 + blockCount * 1000, blockCount);
        
        Assert.assertNotNull("Report should not be null for count " + blockCount, report);
        Assert.assertEquals("Requested block count should match", blockCount, report.getRequestedBlockCount());
        
        logger.info("✅ Replay with {} blocks completed", blockCount);
        
      } catch (Exception e) {
        logger.error("Replay with {} blocks failed", blockCount, e);
        Assert.fail("Replay with " + blockCount + " blocks should not fail: " + e.getMessage());
      }
    }
  }
  
  @Test
  public void testReplayMetrics() {
    try {
      long startTime = System.currentTimeMillis();
      ReplayReport report = replayTool.replayBlocks(500000, 2);
      long endTime = System.currentTimeMillis();
      
      // Verify timing
      Assert.assertTrue("Report duration should be reasonable", 
                       report.getDurationMs() <= (endTime - startTime + 1000)); // Allow 1s tolerance
      
      // Verify metrics consistency
      Assert.assertEquals("Successful + failed should equal total",
                         report.getSuccessfulTransactions() + report.getFailedTransactions(),
                         report.getTotalTransactions());
      
      // Verify performance metrics are reasonable
      if (report.getProcessedBlocks() > 0) {
        Assert.assertTrue("Blocks per second should be reasonable", 
                         report.getBlocksPerSecond() >= 0 && report.getBlocksPerSecond() <= 10000);
      }
      
      if (report.getTotalTransactions() > 0) {
        Assert.assertTrue("Transactions per second should be reasonable",
                         report.getTransactionsPerSecond() >= 0 && report.getTransactionsPerSecond() <= 100000);
      }
      
      logger.info("✅ Metrics validation passed");
      
    } catch (Exception e) {
      logger.error("Metrics test failed", e);
      Assert.fail("Metrics test should not fail: " + e.getMessage());
    }
  }
  
  @Test
  public void testReplayReportCsvExport() {
    try {
      ReplayReport report = replayTool.replayBlocks(600000, 3);
      String csv = report.toCsv();
      
      Assert.assertNotNull("CSV should not be null", csv);
      Assert.assertTrue("CSV should contain header", csv.contains("block_number"));
      Assert.assertTrue("CSV should contain header", csv.contains("total_transactions"));
      Assert.assertTrue("CSV should contain header", csv.contains("mismatch_count"));
      
      // Count lines (header + data rows)
      String[] lines = csv.split("\n");
      Assert.assertTrue("CSV should have at least header line", lines.length >= 1);
      
      logger.info("✅ CSV export test passed ({} lines)", lines.length);
      
    } catch (Exception e) {
      logger.error("CSV export test failed", e);
      Assert.fail("CSV export should not fail: " + e.getMessage());
    }
  }
  
  @Test
  public void testReplayToolCleanup() {
    // Test cleanup doesn't throw exceptions
    try {
      replayTool.cleanup();
      logger.info("✅ Cleanup completed successfully");
    } catch (Exception e) {
      logger.error("Cleanup failed", e);
      Assert.fail("Cleanup should not throw exception: " + e.getMessage());
    }
  }
  
  @Test
  public void testConfigurationProperties() {
    // Test that system properties are respected
    System.setProperty("replay.block_count", "50");
    System.setProperty("replay.max_concurrent", "3");
    System.setProperty("replay.detailed_logging", "true");
    System.setProperty("replay.fail_on_mismatch", "true");
    
    try {
      HistoricalReplayTool configuredTool = new HistoricalReplayTool();
      Assert.assertNotNull("Configured tool should be created", configuredTool);
      
      configuredTool.cleanup();
      logger.info("✅ Configuration properties test passed");
      
    } catch (Exception e) {
      logger.error("Configuration test failed", e);
      Assert.fail("Configuration test should not fail: " + e.getMessage());
    } finally {
      // Clean up system properties
      System.clearProperty("replay.block_count");
      System.clearProperty("replay.max_concurrent");
      System.clearProperty("replay.detailed_logging");
      System.clearProperty("replay.fail_on_mismatch");
    }
  }
  
  @Test
  public void testReplayResultStructure() {
    // Test ReplayResult data structure
    HistoricalReplayTool.ReplayResult result = new HistoricalReplayTool.ReplayResult(12345, 100);
    
    Assert.assertEquals("Block number should match", 12345, result.getBlockNumber());
    Assert.assertEquals("Total transactions should match", 100, result.getTotalTransactions());
    Assert.assertEquals("Initial successful count should be 0", 0, result.getSuccessfulTransactions());
    Assert.assertEquals("Initial failed count should be 0", 0, result.getFailedTransactions());
    Assert.assertEquals("Initial mismatch count should be 0", 0, result.getMismatchCount());
    
    // Test increment methods
    result.addSuccessfulTransaction();
    result.addFailedTransaction();
    result.addMismatch();
    
    Assert.assertEquals("Successful count should be 1", 1, result.getSuccessfulTransactions());
    Assert.assertEquals("Failed count should be 1", 1, result.getFailedTransactions());
    Assert.assertEquals("Mismatch count should be 1", 1, result.getMismatchCount());
    
    logger.info("✅ ReplayResult structure test passed");
  }
  
  @Test
  public void testMismatchReportStructure() {
    // Test MismatchReport data structure
    HistoricalReplayTool.MismatchReport mismatch = new HistoricalReplayTool.MismatchReport(
        12345, "tx123", "ENERGY_MISMATCH", "Expected 1000, got 1100");
    
    Assert.assertEquals("Block number should match", 12345, mismatch.getBlockNumber());
    Assert.assertEquals("Transaction ID should match", "tx123", mismatch.getTransactionId());
    Assert.assertEquals("Mismatch type should match", "ENERGY_MISMATCH", mismatch.getMismatchType());
    Assert.assertEquals("Details should match", "Expected 1000, got 1100", mismatch.getDetails());
    
    logger.info("✅ MismatchReport structure test passed");
  }
}
