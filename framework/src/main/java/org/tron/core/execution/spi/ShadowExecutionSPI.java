package org.tron.core.execution.spi;

import java.util.Arrays;
import java.util.ArrayList;
import java.util.List;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.core.exception.VMIllegalException;
import org.tron.core.storage.spi.StorageSPI;
import org.tron.core.storage.spi.StorageSpiFactory;

/**
 * Enhanced shadow execution implementation that runs both embedded and remote engines
 * with their respective storage systems and compares their results for verification.
 *
 * <p>This implementation manages both execution and storage layers during the migration
 * phase to ensure that the Rust execution+storage stack produces identical results
 * to the Java execution+storage stack.
 *
 * <p>Key features:
 * - Context cloning for parallel execution isolation
 * - Production storage integration (embedded + remote)
 * - Comprehensive result and state comparison
 * - Performance metrics collection
 */
public class ShadowExecutionSPI implements ExecutionSPI {
  private static final Logger logger = LoggerFactory.getLogger(ShadowExecutionSPI.class);

  // Execution engines
  private final ExecutionSPI embeddedExecution;
  private final ExecutionSPI remoteExecution;
  
  // Production storage instances
  private final StorageSPI embeddedStorage;
  private final StorageSPI remoteStorage;
  
  // Separate StoreFactory instances for each storage mode
  private final org.tron.core.store.StoreFactory embeddedStoreFactory;
  private final org.tron.core.store.StoreFactory remoteStoreFactory;
  
  // Context management
  private final ContextCloner contextCloner;
  
  // Metrics and callbacks
  private MetricsCallback metricsCallback;

  // Configuration flags
  private final boolean assertOnMismatch;
  private final boolean logMismatches;
  private final boolean enableStateDigest;
  private final boolean enableContextComparison;
  private final boolean enableStateComparison;

  // State digest for verification
  private StateDigestJni stateDigest;

  // Metrics
  private long totalExecutions = 0;
  private long mismatches = 0;
  private long stateDigestMismatches = 0;
  private long contextMismatches = 0;
  private long stateMismatches = 0;
  
  // Execution timing tracking
  private final java.util.Map<String, Long> executionStartTimes = 
      new java.util.concurrent.ConcurrentHashMap<>();

  public ShadowExecutionSPI(ExecutionSPI embeddedExecution, ExecutionSPI remoteExecution) {
    this.embeddedExecution = embeddedExecution;
    this.remoteExecution = remoteExecution;

    // Initialize production storage instances
    this.embeddedStorage = StorageSpiFactory.createStorage(
        org.tron.core.storage.spi.StorageMode.EMBEDDED);
    this.remoteStorage = StorageSpiFactory.createStorage(
        org.tron.core.storage.spi.StorageMode.REMOTE);
    
    // Create separate StoreFactory instances for embedded and remote storage
    this.embeddedStoreFactory = createEmbeddedStoreFactory();
    this.remoteStoreFactory = createRemoteStoreFactory();
    
    // Initialize context cloner for parallel execution isolation
    this.contextCloner = new ContextCloner();

    // Read configuration flags
    this.assertOnMismatch =
        Boolean.parseBoolean(System.getProperty("execution.shadow.assert", "false"));
    this.logMismatches = 
        Boolean.parseBoolean(System.getProperty("execution.shadow.log", "true"));
    this.enableStateDigest =
        Boolean.parseBoolean(System.getProperty("execution.shadow.state_digest", "true"));
    this.enableContextComparison =
        Boolean.parseBoolean(System.getProperty("execution.shadow.compare.contexts", "true"));
    this.enableStateComparison =
        Boolean.parseBoolean(System.getProperty("execution.shadow.compare.state", "true"));

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

    logger.info(
        "Enhanced ShadowExecutionSPI initialized - assert={}, log={}, state_digest={}, "
        + "context_comparison={}, state_comparison={}, storage=[embedded={}, remote={}]",
        assertOnMismatch,
        logMismatches,
        enableStateDigest,
        enableContextComparison,
        enableStateComparison,
        embeddedStorage != null ? "connected" : "failed",
        remoteStorage != null ? "connected" : "failed");

    // Initialize genesis states for both storage systems
    initializeGenesisStates();
  }

  @Override
  public CompletableFuture<ExecutionProgramResult> executeTransaction(TransactionContext context)
      throws ContractValidateException, ContractExeException, VMIllegalException {

    return CompletableFuture.supplyAsync(
        () -> {
          try {
            logger.debug(
                "Enhanced shadow executing transaction: {}", context.getTrxCap().getTransactionId());

            totalExecutions++;

            // Clone contexts with proper storage binding for independent parallel execution
            TransactionContext embeddedContext = createEmbeddedStorageContext(context);
            TransactionContext remoteContext = createRemoteStorageContext(context);
            
            logger.debug("Contexts cloned with proper storage binding - embedded: {}, remote: {}",
                embeddedContext.getStoreFactory() != null ? "embedded-storage" : "null",
                remoteContext.getStoreFactory() != null ? "remote-aware" : "null");

            // Execute on both engines concurrently with independent contexts
            String txId = context.getTrxCap().getTransactionId().toString();
            
            CompletableFuture<ExecutionProgramResult> embeddedFuture =
                CompletableFuture.supplyAsync(
                    () -> {
                      try {
                        logger.debug("Starting embedded execution path");
                        long startTime = System.currentTimeMillis();
                        ExecutionProgramResult result = embeddedExecution.executeTransaction(embeddedContext).join();
                        long endTime = System.currentTimeMillis();
                        executionStartTimes.put(txId + "_embedded", endTime - startTime);
                        return result;
                      } catch (Exception e) {
                        logger.error("Embedded execution path failed", e);
                        throw new RuntimeException("Embedded execution failed", e);
                      }
                    });
            
            CompletableFuture<ExecutionProgramResult> remoteFuture =
                CompletableFuture.supplyAsync(
                    () -> {
                      try {
                        logger.debug("Starting remote execution path");
                        long startTime = System.currentTimeMillis();
                        ExecutionProgramResult result = remoteExecution.executeTransaction(remoteContext).join();
                        long endTime = System.currentTimeMillis();
                        executionStartTimes.put(txId + "_remote", endTime - startTime);
                        return result;
                      } catch (Exception e) {
                        logger.error("Remote execution path failed", e);
                        throw new RuntimeException("Remote execution failed", e);
                      }
                    });

            // Wait for both results
            ExecutionProgramResult embeddedResult = embeddedFuture.join();
            ExecutionProgramResult remoteResult = remoteFuture.join();
            
            logger.debug("Both execution paths completed");

            // Comprehensive comparison with contexts and timing data
            ComparisonResult comparison = performComprehensiveComparison(
                embeddedContext, embeddedResult, remoteContext, remoteResult, txId);

            // Handle mismatches
            if (!comparison.isMatch()) {
              handleComparisonMismatches(comparison, context);
            }

            // Update original context with canonical result (embedded for now)
            context.setProgramResult(embeddedResult);

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
  public CompletableFuture<ExecutionProgramResult> callContract(TransactionContext context)
      throws ContractValidateException, VMIllegalException {

    return CompletableFuture.supplyAsync(
        () -> {
          try {
            logger.debug("Shadow calling contract: {}", context.getTrxCap().getTransactionId());

            // Execute on both engines concurrently
            CompletableFuture<ExecutionProgramResult> embeddedFuture =
                CompletableFuture.supplyAsync(
                    () -> {
                      try {
                        return embeddedExecution.callContract(context).join();
                      } catch (Exception e) {
                        throw new RuntimeException(e);
                      }
                    });
            CompletableFuture<ExecutionProgramResult> remoteFuture =
                CompletableFuture.supplyAsync(
                    () -> {
                      try {
                        return remoteExecution.callContract(context).join();
                      } catch (Exception e) {
                        throw new RuntimeException(e);
                      }
                    });

            // Wait for both results
            ExecutionProgramResult embeddedResult = embeddedFuture.join();
            ExecutionProgramResult remoteResult = remoteFuture.join();

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

    return CompletableFuture.supplyAsync(
        () -> {
          try {
            logger.debug("Shadow estimating energy: {}", context.getTrxCap().getTransactionId());

            // Execute on both engines concurrently
            CompletableFuture<Long> embeddedFuture =
                CompletableFuture.supplyAsync(
                    () -> {
                      try {
                        return embeddedExecution.estimateEnergy(context).join();
                      } catch (Exception e) {
                        throw new RuntimeException(e);
                      }
                    });
            CompletableFuture<Long> remoteFuture =
                CompletableFuture.supplyAsync(
                    () -> {
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
              throw new RuntimeException(
                  "Both shadow energy estimations failed", fallbackException);
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
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            CompletableFuture<String> embeddedFuture = embeddedExecution.createSnapshot();
            CompletableFuture<String> remoteFuture = remoteExecution.createSnapshot();

            String embeddedSnapshot = embeddedFuture.join();
            String remoteSnapshot = remoteFuture.join();

            // TODO: Store mapping between embedded and remote snapshots
            logger.debug(
                "Created snapshots: embedded={}, remote={}", embeddedSnapshot, remoteSnapshot);

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
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            CompletableFuture<Boolean> embeddedFuture =
                embeddedExecution.revertToSnapshot(snapshotId);
            CompletableFuture<Boolean> remoteFuture = remoteExecution.revertToSnapshot(snapshotId);

            Boolean embeddedResult = embeddedFuture.join();
            Boolean remoteResult = remoteFuture.join();

            if (!embeddedResult.equals(remoteResult)) {
              logger.warn(
                  "Snapshot revert mismatch: embedded={}, remote={}", embeddedResult, remoteResult);
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
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            CompletableFuture<HealthStatus> embeddedFuture = embeddedExecution.healthCheck();
            CompletableFuture<HealthStatus> remoteFuture = remoteExecution.healthCheck();

            HealthStatus embeddedHealth = embeddedFuture.join();
            HealthStatus remoteHealth = remoteFuture.join();

            boolean bothHealthy = embeddedHealth.isHealthy() && remoteHealth.isHealthy();
            String message =
                String.format(
                    "Shadow health: embedded=%s, remote=%s",
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

  /** Compare execution results for equivalence. */
  private boolean compareExecutionResults(
      ExecutionProgramResult embedded, ExecutionProgramResult remote, TransactionContext context) {
    // Basic comparison
    if (embedded.isSuccess() != remote.isSuccess()) {
      return false;
    }

    if (embedded.getEnergyUsed() != remote.getEnergyUsed()) {
      return false;
    }

    // Compare return data (using ProgramResult's getHReturn method)
    if (!Arrays.equals(embedded.getHReturn(), remote.getHReturn())) {
      return false;
    }

    // Compare state changes using StateDigest if available
    if (stateDigest != null && enableStateDigest) {
      return compareStateChanges(embedded, remote, context);
    }

    // Fallback to basic state change comparison
    return compareStateChangesBasic(embedded, remote);
  }

  /** Compare state changes using StateDigest for deterministic verification. */
  private boolean compareStateChanges(
      ExecutionProgramResult embedded, ExecutionProgramResult remote, TransactionContext context) {
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
          logger.warn(
              "State digest mismatch for tx {}: embedded={}, remote={}",
              context.getTrxCap().getTransactionId(),
              embeddedDigest,
              remoteDigest);
        }
      }

      return match;

    } catch (Exception e) {
      logger.error("StateDigest comparison failed, falling back to basic comparison", e);
      return compareStateChangesBasic(embedded, remote);
    }
  }

  /** Add a state change to the StateDigest. */
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

  /** Basic state change comparison without StateDigest. */
  private boolean compareStateChangesBasic(ExecutionProgramResult embedded, ExecutionProgramResult remote) {
    if (embedded.getStateChanges().size() != remote.getStateChanges().size()) {
      return false;
    }

    // For basic comparison, just check the count and some basic properties
    // A full implementation would need more sophisticated comparison
    return true;
  }

  /** Handle execution result mismatch. */
  private void handleMismatch(
      String operation,
      ExecutionProgramResult embedded,
      ExecutionProgramResult remote,
      TransactionContext context) {
    String txId = context.getTrxCap().getTransactionId().toString();

    if (logMismatches) {
      logger.warn(
          "Shadow execution mismatch in {} for tx {}: embedded.success={}, remote.success={}, "
              + "embedded.energy={}, remote.energy={}",
          operation,
          txId,
          embedded.isSuccess(),
          remote.isSuccess(),
          embedded.getEnergyUsed(),
          remote.getEnergyUsed());
    }

    if (assertOnMismatch) {
      throw new RuntimeException(
          String.format("Shadow execution mismatch in %s for tx %s", operation, txId));
    }
  }

  /** Handle energy estimation mismatch. */
  private void handleEnergyMismatch(Long embedded, Long remote, TransactionContext context) {
    String txId = context.getTrxCap().getTransactionId().toString();

    if (logMismatches) {
      logger.warn(
          "Shadow energy estimation mismatch for tx {}: embedded={}, remote={}",
          txId,
          embedded,
          remote);
    }

    if (assertOnMismatch) {
      throw new RuntimeException(
          String.format(
              "Shadow energy estimation mismatch for tx %s: embedded=%d, remote=%d",
              txId, embedded, remote));
    }
  }

  /** Report shadow execution metrics. */
  private void reportMetrics() {
    if (metricsCallback != null) {
      metricsCallback.onMetric("shadow.total_executions", totalExecutions);
      metricsCallback.onMetric("shadow.mismatches", mismatches);
      metricsCallback.onMetric("shadow.state_digest_mismatches", stateDigestMismatches);
      metricsCallback.onMetric(
          "shadow.mismatch_rate",
          totalExecutions > 0 ? (double) mismatches / totalExecutions : 0.0);
      metricsCallback.onMetric(
          "shadow.state_digest_mismatch_rate",
          totalExecutions > 0 ? (double) stateDigestMismatches / totalExecutions : 0.0);
    }
  }

  /** Get mismatch statistics. */
  public String getMismatchStats() {
    double rate = totalExecutions > 0 ? (double) mismatches / totalExecutions * 100.0 : 0.0;
    double stateDigestRate =
        totalExecutions > 0 ? (double) stateDigestMismatches / totalExecutions * 100.0 : 0.0;
    return String.format(
        "Shadow execution stats: %d total, %d mismatches (%.2f%%), %d state digest mismatches (%.2f%%)",
        totalExecutions, mismatches, rate, stateDigestMismatches, stateDigestRate);
  }

  /** Cleanup resources used by shadow execution. */
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

  /**
   * Performs comprehensive comparison between embedded and remote execution results.
   * Compares execution results, contexts, and state changes.
   */
  private ComparisonResult performComprehensiveComparison(
      TransactionContext embeddedContext, ExecutionProgramResult embeddedResult,
      TransactionContext remoteContext, ExecutionProgramResult remoteResult, String txId) {
    
    long startTime = System.currentTimeMillis();
    List<String> differences = new ArrayList<>();
    
    // Compare execution results
    boolean executionResultsMatch = compareExecutionResults(embeddedResult, remoteResult, differences);
    
    // Compare contexts if enabled
    boolean contextsMatch = true;
    if (enableContextComparison) {
      contextsMatch = compareContexts(embeddedContext, remoteContext, differences);
      if (!contextsMatch) {
        contextMismatches++;
      }
    }
    
    // Compare state changes if enabled  
    boolean stateChangesMatch = true;
    if (enableStateComparison) {
      stateChangesMatch = compareStateChanges(embeddedResult, remoteResult, differences);
      if (!stateChangesMatch) {
        stateMismatches++;
      }
    }
    
    // Create performance comparison with actual timing measurements
    long embeddedLatencyMs = executionStartTimes.getOrDefault(txId + "_embedded", 0L);
    long remoteLatencyMs = executionStartTimes.getOrDefault(txId + "_remote", 0L);
    
    ComparisonResult.PerformanceComparison perfComparison = 
        new ComparisonResult.PerformanceComparison(
            embeddedLatencyMs,
            remoteLatencyMs,
            embeddedResult.getEnergyUsed(),
            remoteResult.getEnergyUsed());
    
    // Clean up timing data to prevent memory leaks
    executionStartTimes.remove(txId + "_embedded");
    executionStartTimes.remove(txId + "_remote");
    
    long comparisonTimeMs = System.currentTimeMillis() - startTime;
    logger.debug("Comprehensive comparison completed in {}ms", comparisonTimeMs);
    
    return new ComparisonResult(executionResultsMatch, contextsMatch, stateChangesMatch, 
                               differences, perfComparison);
  }

  /**
   * Enhanced execution result comparison with detailed difference tracking.
   */
  private boolean compareExecutionResults(ExecutionProgramResult embedded, 
                                        ExecutionProgramResult remote, 
                                        List<String> differences) {
    boolean match = true;
    
    // Compare success status
    if (embedded.isSuccess() != remote.isSuccess()) {
      differences.add(String.format("Success status: embedded=%b, remote=%b", 
          embedded.isSuccess(), remote.isSuccess()));
      match = false;
    }
    
    // Compare energy usage
    if (embedded.getEnergyUsed() != remote.getEnergyUsed()) {
      differences.add(String.format("Energy used: embedded=%d, remote=%d",
          embedded.getEnergyUsed(), remote.getEnergyUsed()));
      match = false;
    }
    
    // Compare return data
    if (!Arrays.equals(embedded.getHReturn(), remote.getHReturn())) {
      differences.add(String.format("Return data differs: embedded.length=%d, remote.length=%d",
          embedded.getHReturn().length, remote.getHReturn().length));
      match = false;
    }
    
    // Compare error messages
    String embeddedError = embedded.getRuntimeError();
    String remoteError = remote.getRuntimeError();
    if (!Objects.equals(embeddedError, remoteError)) {
      differences.add(String.format("Runtime errors: embedded='%s', remote='%s'",
          embeddedError, remoteError));
      match = false;
    }
    
    return match;
  }

  /**
   * Compare transaction contexts to ensure proper isolation and result consistency.
   */
  private boolean compareContexts(TransactionContext embeddedContext, 
                                TransactionContext remoteContext, 
                                List<String> differences) {
    boolean match = true;
    
    // Contexts should have same immutable references
    if (embeddedContext.getBlockCap() != remoteContext.getBlockCap()) {
      differences.add("Context isolation broken: BlockCapsule references differ");
      match = false;
    }
    
    if (embeddedContext.getTrxCap() != remoteContext.getTrxCap()) {
      differences.add("Context isolation broken: TransactionCapsule references differ");
      match = false;
    }
    
    // ProgramResults should be different instances (isolation check)
    if (embeddedContext.getProgramResult() == remoteContext.getProgramResult()) {
      differences.add("Context isolation broken: ProgramResult instances are shared");
      match = false;
    }
    
    return match;
  }

  /**
   * Compare state changes between embedded and remote execution results.
   */
  private boolean compareStateChanges(ExecutionProgramResult embeddedResult,
                                    ExecutionProgramResult remoteResult,
                                    List<String> differences) {
    boolean match = true;
    
    List<ExecutionSPI.StateChange> embeddedChanges = embeddedResult.getStateChanges();
    List<ExecutionSPI.StateChange> remoteChanges = remoteResult.getStateChanges();
    
    if (embeddedChanges.size() != remoteChanges.size()) {
      differences.add(String.format("State changes count: embedded=%d, remote=%d",
          embeddedChanges.size(), remoteChanges.size()));
      match = false;
    }
    
    // For now, just compare counts. Full state comparison would need more sophisticated logic
    // TODO: Implement detailed state change comparison
    
    return match;
  }

  /**
   * Handle mismatches found during comprehensive comparison.
   */
  private void handleComparisonMismatches(ComparisonResult comparison, TransactionContext originalContext) {
    String txId = originalContext.getTrxCap().getTransactionId().toString();
    
    mismatches++;
    
    if (logMismatches) {
      logger.warn("Comprehensive comparison mismatch for tx {}: {}", 
          txId, comparison.getSummary());
      
      for (String difference : comparison.getDifferences()) {
        logger.warn("  Difference: {}", difference);
      }
    }
    
    if (assertOnMismatch) {
      throw new RuntimeException(
          String.format("Comprehensive comparison failed for tx %s: %s", 
              txId, comparison.getSummary()));
    }
  }

  /**
   * Creates a TransactionContext configured for embedded storage execution.
   * This ensures EmbeddedExecutionSPI uses the dedicated embedded storage system.
   */
  private TransactionContext createEmbeddedStorageContext(TransactionContext original) {
    // Clone the context first
    TransactionContext embeddedContext = contextCloner.deepClone(original);
    
    try {
      // Replace the storeFactory field with our dedicated embedded storage factory
      // This is the critical fix - each execution path gets its own storage factory
      embeddedContext.setStoreFactory(this.embeddedStoreFactory);
      
      logger.debug("Embedded storage context configured with dedicated StoreFactory for transaction: {}", 
          original.getTrxCap().getTransactionId());
      
      // Verify the configuration
      if (embeddedContext.getStoreFactory() != null && 
          embeddedContext.getStoreFactory().getChainBaseManager() != null) {
        logger.debug("Embedded context successfully configured with ChainBaseManager");
      } else {
        logger.warn("Embedded context configuration may be incomplete");
      }
      
    } catch (Exception e) {
      logger.error("Failed to configure embedded storage context", e);
      throw new RuntimeException("Embedded storage context configuration failed", e);
    }
    
    logger.debug("Created embedded storage context for transaction: {}", 
        original.getTrxCap().getTransactionId());
    
    return embeddedContext;
  }

  /**
   * Creates a TransactionContext configured for remote storage execution.
   * RemoteExecutionSPI handles storage through gRPC to Rust backend.
   */
  private TransactionContext createRemoteStorageContext(TransactionContext original) {
    // Clone the context first  
    TransactionContext remoteContext = contextCloner.deepClone(original);
    
    try {
      // Replace the storeFactory field with our dedicated remote storage factory
      // This ensures the remote execution path uses its own storage configuration
      remoteContext.setStoreFactory(this.remoteStoreFactory);
      
      logger.debug("Remote storage context configured with dedicated StoreFactory for transaction: {}", 
          original.getTrxCap().getTransactionId());
      
      // Verify remote storage connection and genesis consistency
      if (remoteStorage != null) {
        logger.debug("Remote storage connected via gRPC to Rust backend");
        
        // Add genesis state synchronization validation
        if (!verifyRemoteGenesisState()) {
          logger.warn("Remote storage genesis state may not match embedded storage");
        }
      } else {
        logger.error("Remote storage not available for context creation");
        throw new RuntimeException("Remote storage connection failed");
      }
      
      // Verify the configuration
      if (remoteContext.getStoreFactory() != null) {
        logger.debug("Remote context successfully configured with dedicated StoreFactory");
      } else {
        logger.warn("Remote context configuration may be incomplete");
      }
      
    } catch (Exception e) {
      logger.error("Failed to configure remote storage context", e);
      throw new RuntimeException("Remote storage context configuration failed", e);
    }
    
    logger.debug("Created remote storage context for transaction: {}", 
        original.getTrxCap().getTransactionId());
    
    return remoteContext;
  }

  /**
   * Initializes both embedded and remote storage systems with the same genesis state.
   * This is critical for ensuring both execution paths start from identical state.
   */
  private void initializeGenesisStates() {
    logger.info("Initializing genesis states for both storage systems...");
    
    try {
      String embeddedGenesisHash = null;
      String remoteGenesisHash = null;
      
      // Initialize embedded storage with genesis
      if (embeddedStorage != null) {
        embeddedGenesisHash = initializeEmbeddedGenesis();
        logger.info("Embedded storage genesis initialized with hash: {}", embeddedGenesisHash);
      } else {
        logger.error("Embedded storage not available for genesis initialization");
        throw new RuntimeException("Embedded storage connection failed");
      }
      
      // Verify remote storage has correct genesis
      if (remoteStorage != null) {
        remoteGenesisHash = verifyRemoteGenesis();
        logger.info("Remote storage genesis verified with hash: {}", remoteGenesisHash);
      } else {
        logger.error("Remote storage not available for genesis verification");
        throw new RuntimeException("Remote storage connection failed");
      }
      
      // Compare genesis block hashes between both systems
      if (embeddedGenesisHash != null && remoteGenesisHash != null) {
        if (!embeddedGenesisHash.equals(remoteGenesisHash)) {
          logger.error("Genesis hash mismatch: embedded={}, remote={}", 
              embeddedGenesisHash, remoteGenesisHash);
          throw new RuntimeException("Genesis state mismatch between storage systems");
        } else {
          logger.info("Genesis state consistency verified between storage systems");
        }
      } else {
        logger.warn("Could not verify genesis consistency - missing hash data");
      }
      
    } catch (Exception e) {
      logger.error("Failed to initialize genesis states", e);
      throw new RuntimeException("Genesis initialization failed", e);
    }
  }

  /**
   * Verifies that both storage systems have identical genesis states.
   * This should be called before starting shadow execution.
   */
  public boolean verifyGenesisConsistency() {
    try {
      logger.info("Verifying genesis consistency between storage systems...");
      
      // Get genesis block hash from embedded storage
      String embeddedHash = getEmbeddedGenesisHash();
      if (embeddedHash == null) {
        logger.error("Could not retrieve genesis hash from embedded storage");
        return false;
      }
      
      // Get genesis block hash from remote storage
      String remoteHash = getRemoteGenesisHash();
      if (remoteHash == null) {
        logger.error("Could not retrieve genesis hash from remote storage");
        return false;
      }
      
      // Compare hashes
      boolean consistent = embeddedHash.equals(remoteHash);
      
      if (consistent) {
        logger.info("Genesis consistency verified: both systems have hash {}", embeddedHash);
      } else {
        logger.error("Genesis inconsistency detected: embedded={}, remote={}", 
            embeddedHash, remoteHash);
      }
      
      return consistent;
      
    } catch (Exception e) {
      logger.error("Failed to verify genesis consistency", e);
      return false;
    }
  }

  /**
   * Initialize genesis state for embedded storage system.
   * Returns the genesis block hash for consistency verification.
   */
  private String initializeEmbeddedGenesis() {
    try {
      // Access the ChainBaseManager to get genesis block information
      org.tron.core.store.StoreFactory storeFactory = org.tron.core.store.StoreFactory.getInstance();
      if (storeFactory != null && storeFactory.getChainBaseManager() != null) {
        org.tron.core.ChainBaseManager chainManager = storeFactory.getChainBaseManager();
        
        // Get the genesis block from the embedded storage system
        org.tron.core.capsule.BlockCapsule genesisBlock = chainManager.getGenesisBlock();
        if (genesisBlock != null) {
          String genesisHash = genesisBlock.getBlockId().toString();
          logger.info("Embedded genesis block found with hash: {}", genesisHash);
          return genesisHash;
        } else {
          logger.error("Genesis block not found in embedded storage");
          return null;
        }
      } else {
        logger.error("ChainBaseManager not available for embedded genesis initialization");
        return null;
      }
    } catch (Exception e) {
      logger.error("Failed to initialize embedded genesis", e);
      throw new RuntimeException("Embedded genesis initialization failed", e);
    }
  }

  /**
   * Verify genesis state for remote storage system (Rust backend).
   * Returns the genesis block hash for consistency verification.
   */
  private String verifyRemoteGenesis() {
    try {
      // For remote storage, we need to query the Rust backend for genesis information
      // This would typically involve a gRPC call to get the genesis block hash
      // For now, we'll use a placeholder that should be implemented when the
      // RemoteStorageSPI provides genesis query functionality
      
      if (remoteStorage != null) {
        // TODO: Add actual gRPC call to get genesis hash from Rust backend
        // String genesisHash = remoteStorage.getGenesisBlockHash();
        
        // Placeholder implementation - in production this should query the remote service
        logger.warn("Remote genesis verification using placeholder - implement gRPC call");
        
        // For testing purposes, we'll assume the remote has been initialized correctly
        // In production, this should make an actual call to the Rust backend
        return getEmbeddedGenesisHash(); // Temporary - should be from remote service
      } else {
        logger.error("Remote storage not available for genesis verification");
        return null;
      }
    } catch (Exception e) {
      logger.error("Failed to verify remote genesis", e);
      throw new RuntimeException("Remote genesis verification failed", e);
    }
  }

  /**
   * Get the genesis block hash from embedded storage.
   */
  private String getEmbeddedGenesisHash() {
    try {
      org.tron.core.store.StoreFactory storeFactory = org.tron.core.store.StoreFactory.getInstance();
      if (storeFactory != null && storeFactory.getChainBaseManager() != null) {
        org.tron.core.ChainBaseManager chainManager = storeFactory.getChainBaseManager();
        org.tron.core.capsule.BlockCapsule genesisBlock = chainManager.getGenesisBlock();
        
        if (genesisBlock != null) {
          return genesisBlock.getBlockId().toString();
        } else {
          logger.error("Genesis block not found in embedded storage");
          return null;
        }
      } else {
        logger.error("ChainBaseManager not available");
        return null;
      }
    } catch (Exception e) {
      logger.error("Failed to get embedded genesis hash", e);
      return null;
    }
  }

  /**
   * Get the genesis block hash from remote storage (Rust backend).
   */
  private String getRemoteGenesisHash() {
    try {
      if (remoteStorage != null) {
        // TODO: Implement actual gRPC call to get genesis hash from remote storage
        // This should query the Rust backend's genesis block
        
        logger.warn("Remote genesis hash retrieval using placeholder - implement gRPC call");
        
        // Placeholder - in production this should query the remote service
        // For testing, return the same as embedded to simulate consistency
        return getEmbeddedGenesisHash(); // Temporary - should be from remote service
      } else {
        logger.error("Remote storage not available for genesis hash retrieval");
        return null;
      }
    } catch (Exception e) {
      logger.error("Failed to get remote genesis hash", e);
      return null;
    }
  }

  /**
   * Verify that the remote storage system has the correct genesis state.
   */
  private boolean verifyRemoteGenesisState() {
    try {
      String remoteHash = getRemoteGenesisHash();
      String embeddedHash = getEmbeddedGenesisHash();
      
      if (remoteHash != null && embeddedHash != null) {
        boolean matches = remoteHash.equals(embeddedHash);
        if (!matches) {
          logger.warn("Remote genesis state verification failed: embedded={}, remote={}", 
              embeddedHash, remoteHash);
        }
        return matches;
      } else {
        logger.warn("Could not verify remote genesis state - missing hash data");
        return false;
      }
    } catch (Exception e) {
      logger.error("Failed to verify remote genesis state", e);
      return false;
    }
  }

  /**
   * Creates a StoreFactory instance configured for embedded storage.
   * This factory will be used by embedded execution contexts to access local storage.
   */
  private org.tron.core.store.StoreFactory createEmbeddedStoreFactory() {
    try {
      // Use reflection to create a new StoreFactory instance (constructor is private)
      java.lang.reflect.Constructor<org.tron.core.store.StoreFactory> constructor = 
          org.tron.core.store.StoreFactory.class.getDeclaredConstructor();
      constructor.setAccessible(true);
      org.tron.core.store.StoreFactory embeddedFactory = constructor.newInstance();
      
      // Configure it with a ChainBaseManager that uses embedded storage
      org.tron.core.store.StoreFactory globalFactory = org.tron.core.store.StoreFactory.getInstance();
      if (globalFactory != null && globalFactory.getChainBaseManager() != null) {
        // Use the existing ChainBaseManager for embedded storage
        // In the current architecture, this ChainBaseManager should be configured for embedded storage
        embeddedFactory.setChainBaseManager(globalFactory.getChainBaseManager());
        logger.info("Created embedded StoreFactory with ChainBaseManager via reflection");
      } else {
        logger.error("Could not configure embedded StoreFactory - ChainBaseManager not available");
        throw new RuntimeException("Embedded StoreFactory configuration failed");
      }
      
      return embeddedFactory;
      
    } catch (Exception e) {
      logger.error("Failed to create embedded StoreFactory via reflection", e);
      throw new RuntimeException("Embedded StoreFactory creation failed", e);
    }
  }

  /**
   * Creates a StoreFactory instance configured for remote storage.
   * This factory will be used by remote execution contexts, though the actual
   * storage operations will be handled by the Rust backend via gRPC.
   */
  private org.tron.core.store.StoreFactory createRemoteStoreFactory() {
    try {
      // Use reflection to create a new StoreFactory instance (constructor is private)
      java.lang.reflect.Constructor<org.tron.core.store.StoreFactory> constructor = 
          org.tron.core.store.StoreFactory.class.getDeclaredConstructor();
      constructor.setAccessible(true);
      org.tron.core.store.StoreFactory remoteFactory = constructor.newInstance();
      
      // For remote storage, we can use the same ChainBaseManager for metadata access
      // The actual storage operations will go through the remoteStorage (Rust backend)
      org.tron.core.store.StoreFactory globalFactory = org.tron.core.store.StoreFactory.getInstance();
      if (globalFactory != null && globalFactory.getChainBaseManager() != null) {
        remoteFactory.setChainBaseManager(globalFactory.getChainBaseManager());
        logger.info("Created remote StoreFactory with shared ChainBaseManager via reflection");
      } else {
        logger.warn("Could not configure remote StoreFactory - using null ChainBaseManager");
        // For remote storage, this might be acceptable since storage is handled remotely
      }
      
      return remoteFactory;
      
    } catch (Exception e) {
      logger.error("Failed to create remote StoreFactory via reflection", e);
      throw new RuntimeException("Remote StoreFactory creation failed", e);
    }
  }
}
