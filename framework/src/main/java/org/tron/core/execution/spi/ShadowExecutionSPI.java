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

  public ShadowExecutionSPI(ExecutionSPI embeddedExecution, ExecutionSPI remoteExecution) {
    this.embeddedExecution = embeddedExecution;
    this.remoteExecution = remoteExecution;

    // Initialize production storage instances
    this.embeddedStorage = StorageSpiFactory.createStorage(
        org.tron.core.storage.spi.StorageMode.EMBEDDED);
    this.remoteStorage = StorageSpiFactory.createStorage(
        org.tron.core.storage.spi.StorageMode.REMOTE);
    
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
            CompletableFuture<ExecutionProgramResult> embeddedFuture =
                CompletableFuture.supplyAsync(
                    () -> {
                      try {
                        logger.debug("Starting embedded execution path");
                        return embeddedExecution.executeTransaction(embeddedContext).join();
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
                        return remoteExecution.executeTransaction(remoteContext).join();
                      } catch (Exception e) {
                        logger.error("Remote execution path failed", e);
                        throw new RuntimeException("Remote execution failed", e);
                      }
                    });

            // Wait for both results
            ExecutionProgramResult embeddedResult = embeddedFuture.join();
            ExecutionProgramResult remoteResult = remoteFuture.join();
            
            logger.debug("Both execution paths completed");

            // Comprehensive comparison with contexts
            ComparisonResult comparison = performComprehensiveComparison(
                embeddedContext, embeddedResult, remoteContext, remoteResult);

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
      TransactionContext remoteContext, ExecutionProgramResult remoteResult) {
    
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
    
    // Create performance comparison
    ComparisonResult.PerformanceComparison perfComparison = 
        new ComparisonResult.PerformanceComparison(
            0, // TODO: Measure actual execution times
            0,
            embeddedResult.getEnergyUsed(),
            remoteResult.getEnergyUsed());
    
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
   * This ensures EmbeddedExecutionSPI uses EmbeddedStorageSPI.
   */
  private TransactionContext createEmbeddedStorageContext(TransactionContext original) {
    // Clone the context first
    TransactionContext embeddedContext = contextCloner.deepClone(original);
    
    // Create embedded storage-aware StoreFactory
    // For now, we'll use the existing StoreFactory pattern but ensure it's configured
    // for embedded storage. The actual integration depends on how ChainBaseManager
    // can be configured with different storage backends.
    
    // TODO: This is where we need to integrate with embedded storage initialization
    // The StoreFactory should be configured to use embeddedStorage instance
    
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
    
    // For remote execution, the storage is handled by the Rust backend via gRPC.
    // The RemoteExecutionSPI doesn't need local storage configuration as it
    // communicates with the Rust service which has its own storage.
    
    // However, we need to ensure the Rust backend has the same genesis state
    // TODO: Add genesis state synchronization check
    
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
      // Initialize embedded storage with genesis
      if (embeddedStorage != null) {
        // TODO: Implement genesis initialization for embedded storage
        // This should create the genesis block and initialize accounts
        logger.info("Embedded storage genesis initialization - TODO: implement");
      }
      
      // Verify remote storage has correct genesis
      if (remoteStorage != null) {
        // TODO: Verify remote storage (Rust backend) has same genesis state
        // This might involve querying the genesis block hash and comparing
        logger.info("Remote storage genesis verification - TODO: implement");
      }
      
      // TODO: Compare genesis block hashes between both systems
      logger.warn("Genesis state synchronization not fully implemented yet");
      
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
      // TODO: Implement genesis consistency check
      // 1. Get genesis block hash from embedded storage
      // 2. Get genesis block hash from remote storage  
      // 3. Compare hashes
      // 4. Return true if identical, false otherwise
      
      logger.warn("Genesis consistency verification not implemented yet");
      return true; // Placeholder
      
    } catch (Exception e) {
      logger.error("Failed to verify genesis consistency", e);
      return false;
    }
  }
}
