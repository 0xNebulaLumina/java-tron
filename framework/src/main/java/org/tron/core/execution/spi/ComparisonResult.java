package org.tron.core.execution.spi;

import java.util.ArrayList;
import java.util.List;

/**
 * Result of a comprehensive comparison between embedded and remote execution paths.
 * Contains detailed information about what matched and what differed.
 */
public class ComparisonResult {
  private final boolean overallMatch;
  private final boolean executionResultsMatch;
  private final boolean contextsMatch;
  private final boolean stateChangesMatch;
  
  private final List<String> differences;
  private final PerformanceComparison performanceComparison;
  
  public ComparisonResult(boolean executionResultsMatch, boolean contextsMatch, 
                         boolean stateChangesMatch, List<String> differences,
                         PerformanceComparison performanceComparison) {
    this.executionResultsMatch = executionResultsMatch;
    this.contextsMatch = contextsMatch;
    this.stateChangesMatch = stateChangesMatch;
    this.overallMatch = executionResultsMatch && contextsMatch && stateChangesMatch;
    this.differences = differences != null ? differences : new ArrayList<>();
    this.performanceComparison = performanceComparison;
  }
  
  public boolean isMatch() {
    return overallMatch;
  }
  
  public boolean areExecutionResultsMatch() {
    return executionResultsMatch;
  }
  
  public boolean areContextsMatch() {
    return contextsMatch;
  }
  
  public boolean areStateChangesMatch() {
    return stateChangesMatch;
  }
  
  public List<String> getDifferences() {
    return new ArrayList<>(differences);
  }
  
  public PerformanceComparison getPerformanceComparison() {
    return performanceComparison;
  }
  
  public String getSummary() {
    StringBuilder sb = new StringBuilder();
    sb.append("ComparisonResult: overall=").append(overallMatch);
    sb.append(", execution=").append(executionResultsMatch);
    sb.append(", contexts=").append(contextsMatch);
    sb.append(", state=").append(stateChangesMatch);
    sb.append(", differences=").append(differences.size());
    return sb.toString();
  }
  
  /**
   * Performance comparison metrics between embedded and remote execution.
   */
  public static class PerformanceComparison {
    private final long embeddedLatencyMs;
    private final long remoteLatencyMs;
    private final long embeddedEnergyUsed;
    private final long remoteEnergyUsed;
    
    public PerformanceComparison(long embeddedLatencyMs, long remoteLatencyMs,
                                long embeddedEnergyUsed, long remoteEnergyUsed) {
      this.embeddedLatencyMs = embeddedLatencyMs;
      this.remoteLatencyMs = remoteLatencyMs;
      this.embeddedEnergyUsed = embeddedEnergyUsed;
      this.remoteEnergyUsed = remoteEnergyUsed;
    }
    
    public long getEmbeddedLatencyMs() {
      return embeddedLatencyMs;
    }
    
    public long getRemoteLatencyMs() {
      return remoteLatencyMs;
    }
    
    public long getEmbeddedEnergyUsed() {
      return embeddedEnergyUsed;
    }
    
    public long getRemoteEnergyUsed() {
      return remoteEnergyUsed;
    }
    
    public double getLatencyRatio() {
      return embeddedLatencyMs > 0 ? (double) remoteLatencyMs / embeddedLatencyMs : 0.0;
    }
    
    public long getLatencyDifferenceMs() {
      return remoteLatencyMs - embeddedLatencyMs;
    }
    
    public long getEnergyDifference() {
      return remoteEnergyUsed - embeddedEnergyUsed;
    }
  }
}