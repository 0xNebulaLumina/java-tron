package org.tron.core.execution.spi;

import java.util.Arrays;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.core.exception.VMIllegalException;

/**
 * Shadow execution implementation that runs both embedded and remote engines
 * and compares their results for verification purposes.
 * 
 * This implementation is used during the migration phase to ensure that
 * the Rust execution engine produces identical results to the Java engine.
 */
public class ShadowExecutionSPI implements ExecutionSPI {
  private static final Logger logger = LoggerFactory.getLogger(ShadowExecutionSPI.class);

  private final ExecutionSPI embeddedExecution;
  private final ExecutionSPI remoteExecution;
  private MetricsCallback metricsCallback;

  // Configuration flags
  private final boolean assertOnMismatch;
  private final boolean logMismatches;
  private final boolean enableStateDigest;

  // State digest for verification
  private StateDigestJni stateDigest;

  // Metrics
  private long totalExecutions = 0;
  private long mismatches = 0;
  private long stateDigestMismatches = 0;

  public ShadowExecutionSPI(ExecutionSPI embeddedExecution, ExecutionSPI remoteExecution) {
    this.embeddedExecution = embeddedExecution;
    this.remoteExecution = remoteExecution;

    // Read configuration flags
    this.assertOnMismatch = Boolean.parseBoolean(
        System.getProperty("execution.shadow.assert", "false"));
    this.logMismatches = Boolean.parseBoolean(
        System.getProperty("execution.shadow.log", "true"));
    this.enableStateDigest = Boolean.parseBoolean(
        System.getProperty("execution.shadow.state_digest", "true"));

    // Initialize state digest if enabled
    if (enableStateDigest) {
      try {
        this.stateDigest = new StateDigestJni();
        logger.info("StateDigest enabled for shadow verification");
      } catch (Exception e) {
        logger.warn("Failed to initialize StateDigest, falling back to basic comparison", e);
        this.stateDigest = null;
      }
    } else {
      this.stateDigest = null;
    }

    logger.info("Initialized shadow execution SPI (assert={}, log={}, state_digest={})",
               assertOnMismatch, logMismatches, enableStateDigest);
  }

  @Override
  public CompletableFuture<ExecutionResult> executeTransaction(TransactionContext context)
      throws ContractValidateException, ContractExeException, VMIllegalException {
    
    return CompletableFuture.supplyAsync(() -> {
      try {
        logger.debug("Shadow executing transaction: {}", 
                    context.getTrxCap().getTransactionId());
        
        totalExecutions++;
        
        // Execute on both engines concurrently
        CompletableFuture<ExecutionResult> embeddedFuture =
            CompletableFuture.supplyAsync(() -> {
              try {
                return embeddedExecution.executeTransaction(context).join();
              } catch (Exception e) {
                throw new RuntimeException(e);
              }
            });
        CompletableFuture<ExecutionResult> remoteFuture =
            CompletableFuture.supplyAsync(() -> {
              try {
                return remoteExecution.executeTransaction(context).join();
              } catch (Exception e) {
                throw new RuntimeException(e);
              }
            });
        
        // Wait for both results
        ExecutionResult embeddedResult = embeddedFuture.join();
        ExecutionResult remoteResult = remoteFuture.join();
        
        // Compare results
        boolean resultsMatch = compareExecutionResults(embeddedResult, remoteResult, context);
        
        if (!resultsMatch) {
          mismatches++;
          handleMismatch("executeTransaction", embeddedResult, remoteResult, context);
        }
        
        // Report metrics
        reportMetrics();
        
        // Return the embedded result (canonical for now)
        return embeddedResult;
        
      } catch (CompletionException e) {
        logger.error("Shadow execution failed", e);
        // If one engine fails, fall back to embedded
        try {
          return embeddedExecution.executeTransaction(context).join();
        } catch (Exception fallbackException) {
          logger.error("Fallback to embedded execution also failed", fallbackException);
          throw new RuntimeException("Both shadow executions failed", fallbackException);
        }
      }
    });
  }

  @Override
  public CompletableFuture<ExecutionResult> callContract(TransactionContext context)
      throws ContractValidateException, VMIllegalException {
    
    return CompletableFuture.supplyAsync(() -> {
      try {
        logger.debug("Shadow calling contract: {}", 
                    context.getTrxCap().getTransactionId());
        
        // Execute on both engines concurrently
        CompletableFuture<ExecutionResult> embeddedFuture =
            CompletableFuture.supplyAsync(() -> {
              try {
                return embeddedExecution.callContract(context).join();
              } catch (Exception e) {
                throw new RuntimeException(e);
              }
            });
        CompletableFuture<ExecutionResult> remoteFuture =
            CompletableFuture.supplyAsync(() -> {
              try {
                return remoteExecution.callContract(context).join();
              } catch (Exception e) {
                throw new RuntimeException(e);
              }
            });
        
        // Wait for both results
        ExecutionResult embeddedResult = embeddedFuture.join();
        ExecutionResult remoteResult = remoteFuture.join();
        
        // Compare results
        boolean resultsMatch = compareExecutionResults(embeddedResult, remoteResult, context);
        
        if (!resultsMatch) {
          handleMismatch("callContract", embeddedResult, remoteResult, context);
        }
        
        // Return the embedded result (canonical for now)
        return embeddedResult;
        
      } catch (CompletionException e) {
        logger.error("Shadow contract call failed", e);
        // Fall back to embedded
        try {
          return embeddedExecution.callContract(context).join();
        } catch (Exception fallbackException) {
          logger.error("Fallback to embedded contract call also failed", fallbackException);
          throw new RuntimeException("Both shadow contract calls failed", fallbackException);
        }
      }
    });
  }

  @Override
  public CompletableFuture<Long> estimateEnergy(TransactionContext context)
      throws ContractValidateException {
    
    return CompletableFuture.supplyAsync(() -> {
      try {
        logger.debug("Shadow estimating energy: {}", 
                    context.getTrxCap().getTransactionId());
        
        // Execute on both engines concurrently
        CompletableFuture<Long> embeddedFuture =
            CompletableFuture.supplyAsync(() -> {
              try {
                return embeddedExecution.estimateEnergy(context).join();
              } catch (Exception e) {
                throw new RuntimeException(e);
              }
            });
        CompletableFuture<Long> remoteFuture =
            CompletableFuture.supplyAsync(() -> {
              try {
                return remoteExecution.estimateEnergy(context).join();
              } catch (Exception e) {
                throw new RuntimeException(e);
              }
            });
        
        // Wait for both results
        Long embeddedResult = embeddedFuture.join();
        Long remoteResult = remoteFuture.join();
        
        // Compare results
        if (!embeddedResult.equals(remoteResult)) {
          handleEnergyMismatch(embeddedResult, remoteResult, context);
        }
        
        // Return the embedded result (canonical for now)
        return embeddedResult;
        
      } catch (CompletionException e) {
        logger.error("Shadow energy estimation failed", e);
        // Fall back to embedded
        try {
          return embeddedExecution.estimateEnergy(context).join();
        } catch (Exception fallbackException) {
          logger.error("Fallback to embedded energy estimation also failed", fallbackException);
          throw new RuntimeException("Both shadow energy estimations failed", fallbackException);
        }
      }
    });
  }

  @Override
  public CompletableFuture<byte[]> getCode(byte[] address, String snapshotId) {
    // For read operations, we can just use the embedded implementation
    // since state should be identical
    return embeddedExecution.getCode(address, snapshotId);
  }

  @Override
  public CompletableFuture<byte[]> getStorageAt(byte[] address, byte[] key, String snapshotId) {
    // For read operations, we can just use the embedded implementation
    return embeddedExecution.getStorageAt(address, key, snapshotId);
  }

  @Override
  public CompletableFuture<Long> getNonce(byte[] address, String snapshotId) {
    // For read operations, we can just use the embedded implementation
    return embeddedExecution.getNonce(address, snapshotId);
  }

  @Override
  public CompletableFuture<byte[]> getBalance(byte[] address, String snapshotId) {
    // For read operations, we can just use the embedded implementation
    return embeddedExecution.getBalance(address, snapshotId);
  }

  @Override
  public CompletableFuture<String> createSnapshot() {
    // Create snapshots on both engines
    return CompletableFuture.supplyAsync(() -> {
      try {
        CompletableFuture<String> embeddedFuture = embeddedExecution.createSnapshot();
        CompletableFuture<String> remoteFuture = remoteExecution.createSnapshot();
        
        String embeddedSnapshot = embeddedFuture.join();
        String remoteSnapshot = remoteFuture.join();
        
        // TODO: Store mapping between embedded and remote snapshots
        logger.debug("Created snapshots: embedded={}, remote={}", embeddedSnapshot, remoteSnapshot);
        
        return embeddedSnapshot;
      } catch (Exception e) {
        logger.error("Shadow snapshot creation failed", e);
        return embeddedExecution.createSnapshot().join();
      }
    });
  }

  @Override
  public CompletableFuture<Boolean> revertToSnapshot(String snapshotId) {
    // Revert on both engines
    return CompletableFuture.supplyAsync(() -> {
      try {
        CompletableFuture<Boolean> embeddedFuture = embeddedExecution.revertToSnapshot(snapshotId);
        CompletableFuture<Boolean> remoteFuture = remoteExecution.revertToSnapshot(snapshotId);
        
        Boolean embeddedResult = embeddedFuture.join();
        Boolean remoteResult = remoteFuture.join();
        
        if (!embeddedResult.equals(remoteResult)) {
          logger.warn("Snapshot revert mismatch: embedded={}, remote={}", embeddedResult, remoteResult);
        }
        
        return embeddedResult;
      } catch (Exception e) {
        logger.error("Shadow snapshot revert failed", e);
        return embeddedExecution.revertToSnapshot(snapshotId).join();
      }
    });
  }

  @Override
  public CompletableFuture<HealthStatus> healthCheck() {
    return CompletableFuture.supplyAsync(() -> {
      try {
        CompletableFuture<HealthStatus> embeddedFuture = embeddedExecution.healthCheck();
        CompletableFuture<HealthStatus> remoteFuture = remoteExecution.healthCheck();
        
        HealthStatus embeddedHealth = embeddedFuture.join();
        HealthStatus remoteHealth = remoteFuture.join();
        
        boolean bothHealthy = embeddedHealth.isHealthy() && remoteHealth.isHealthy();
        String message = String.format("Shadow health: embedded=%s, remote=%s", 
                                     embeddedHealth.isHealthy(), remoteHealth.isHealthy());
        
        return new HealthStatus(bothHealthy, message);
      } catch (Exception e) {
        logger.error("Shadow health check failed", e);
        return new HealthStatus(false, "Shadow health check failed: " + e.getMessage());
      }
    });
  }

  @Override
  public void registerMetricsCallback(MetricsCallback callback) {
    this.metricsCallback = callback;
    // Register with both underlying implementations
    embeddedExecution.registerMetricsCallback(callback);
    remoteExecution.registerMetricsCallback(callback);
    logger.info("Registered metrics callback for shadow execution");
  }

  /**
   * Compare execution results for equivalence.
   */
  private boolean compareExecutionResults(ExecutionResult embedded, ExecutionResult remote,
                                        TransactionContext context) {
    // Basic comparison
    if (embedded.isSuccess() != remote.isSuccess()) {
      return false;
    }

    if (embedded.getEnergyUsed() != remote.getEnergyUsed()) {
      return false;
    }

    // Compare return data
    if (!Arrays.equals(embedded.getReturnData(), remote.getReturnData())) {
      return false;
    }

    // Compare state changes using StateDigest if available
    if (stateDigest != null && enableStateDigest) {
      return compareStateChanges(embedded, remote, context);
    }

    // Fallback to basic state change comparison
    return compareStateChangesBasic(embedded, remote);
  }

  /**
   * Compare state changes using StateDigest for deterministic verification.
   */
  private boolean compareStateChanges(ExecutionResult embedded, ExecutionResult remote,
                                    TransactionContext context) {
    try {
      // Clear previous state
      stateDigest.clear();

      // Add embedded execution state changes
      for (StateChange change : embedded.getStateChanges()) {
        addStateChangeToDigest(change, "embedded");
      }
      String embeddedDigest = stateDigest.computeHashHex();

      // Clear and add remote execution state changes
      stateDigest.clear();
      for (StateChange change : remote.getStateChanges()) {
        addStateChangeToDigest(change, "remote");
      }
      String remoteDigest = stateDigest.computeHashHex();

      boolean match = embeddedDigest.equals(remoteDigest);
      if (!match) {
        stateDigestMismatches++;
        if (logMismatches) {
          logger.warn("State digest mismatch for tx {}: embedded={}, remote={}",
                     context.getTrxCap().getTransactionId(), embeddedDigest, remoteDigest);
        }
      }

      return match;

    } catch (Exception e) {
      logger.error("StateDigest comparison failed, falling back to basic comparison", e);
      return compareStateChangesBasic(embedded, remote);
    }
  }

  /**
   * Add a state change to the StateDigest.
   */
  private void addStateChangeToDigest(StateChange change, String source) {
    try {
      // For now, create a simple account representation
      // In a full implementation, this would extract proper account info
      byte[] address = change.getAddress();
      byte[] balance = new byte[32]; // TODO: Extract actual balance
      long nonce = 0; // TODO: Extract actual nonce
      byte[] codeHash = new byte[32]; // TODO: Extract actual code hash

      stateDigest.addAccount(address, balance, nonce, codeHash);

    } catch (Exception e) {
      logger.warn("Failed to add state change to digest from {}: {}", source, e.getMessage());
    }
  }

  /**
   * Basic state change comparison without StateDigest.
   */
  private boolean compareStateChangesBasic(ExecutionResult embedded, ExecutionResult remote) {
    if (embedded.getStateChanges().size() != remote.getStateChanges().size()) {
      return false;
    }

    // For basic comparison, just check the count and some basic properties
    // A full implementation would need more sophisticated comparison
    return true;
  }

  /**
   * Handle execution result mismatch.
   */
  private void handleMismatch(String operation, ExecutionResult embedded, ExecutionResult remote, 
                            TransactionContext context) {
    String txId = context.getTrxCap().getTransactionId().toString();
    
    if (logMismatches) {
      logger.warn("Shadow execution mismatch in {} for tx {}: embedded.success={}, remote.success={}, " +
                 "embedded.energy={}, remote.energy={}", 
                 operation, txId, embedded.isSuccess(), remote.isSuccess(),
                 embedded.getEnergyUsed(), remote.getEnergyUsed());
    }
    
    if (assertOnMismatch) {
      throw new RuntimeException(String.format(
          "Shadow execution mismatch in %s for tx %s", operation, txId));
    }
  }

  /**
   * Handle energy estimation mismatch.
   */
  private void handleEnergyMismatch(Long embedded, Long remote, TransactionContext context) {
    String txId = context.getTrxCap().getTransactionId().toString();
    
    if (logMismatches) {
      logger.warn("Shadow energy estimation mismatch for tx {}: embedded={}, remote={}", 
                 txId, embedded, remote);
    }
    
    if (assertOnMismatch) {
      throw new RuntimeException(String.format(
          "Shadow energy estimation mismatch for tx %s: embedded=%d, remote=%d", 
          txId, embedded, remote));
    }
  }

  /**
   * Report shadow execution metrics.
   */
  private void reportMetrics() {
    if (metricsCallback != null) {
      metricsCallback.onMetric("shadow.total_executions", totalExecutions);
      metricsCallback.onMetric("shadow.mismatches", mismatches);
      metricsCallback.onMetric("shadow.state_digest_mismatches", stateDigestMismatches);
      metricsCallback.onMetric("shadow.mismatch_rate",
                              totalExecutions > 0 ? (double) mismatches / totalExecutions : 0.0);
      metricsCallback.onMetric("shadow.state_digest_mismatch_rate",
                              totalExecutions > 0 ? (double) stateDigestMismatches / totalExecutions : 0.0);
    }
  }

  /**
   * Get mismatch statistics.
   */
  public String getMismatchStats() {
    double rate = totalExecutions > 0 ? (double) mismatches / totalExecutions * 100.0 : 0.0;
    double stateDigestRate = totalExecutions > 0 ? (double) stateDigestMismatches / totalExecutions * 100.0 : 0.0;
    return String.format("Shadow execution stats: %d total, %d mismatches (%.2f%%), %d state digest mismatches (%.2f%%)",
                        totalExecutions, mismatches, rate, stateDigestMismatches, stateDigestRate);
  }

  /**
   * Cleanup resources used by shadow execution.
   */
  public void cleanup() {
    if (stateDigest != null) {
      stateDigest.destroy();
      stateDigest = null;
    }
  }

  @Override
  protected void finalize() throws Throwable {
    try {
      cleanup();
    } finally {
      super.finalize();
    }
  }
}
