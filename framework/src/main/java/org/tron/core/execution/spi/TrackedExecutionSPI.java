package org.tron.core.execution.spi;

import java.io.IOException;
import java.util.Arrays;
import java.util.concurrent.CompletableFuture;
import lombok.extern.slf4j.Slf4j;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.core.exception.VMIllegalException;
import org.tron.core.execution.spi.ExecutionSPI.HealthStatus;
import org.tron.core.execution.spi.ExecutionSPI.MetricsCallback;

/**
 * Decorator that adds execution tracking to any ExecutionSPI implementation.
 *
 * <p>This decorator wraps an existing ExecutionSPI implementation and logs detailed metrics
 * for every transaction execution to CSV files for later analysis and comparison between
 * different execution modes.
 *
 * <p>Features:
 * - Non-invasive: No modification to wrapped ExecutionSPI implementation
 * - Comprehensive tracking: Captures ExecutionProgramResult, TransactionContext, and state digest
 * - Performance monitoring: Measures execution time with minimal overhead
 * - State consistency: Computes state digest for execution result verification
 * - Thread-safe: Safe for concurrent execution tracking
 */
@Slf4j
public class TrackedExecutionSPI implements ExecutionSPI, AutoCloseable {

  private final ExecutionSPI delegate;
  private final ExecutionMetricsLogger metricsLogger;
  private final String executionMode;
  private final StateDigestJni stateDigest;
  private final boolean computeStateDigest;

  /**
   * Create a tracked ExecutionSPI wrapper.
   *
   * @param delegate The ExecutionSPI implementation to wrap
   * @param metricsLogger The logger for writing metrics
   * @param executionMode The execution mode name (EMBEDDED, REMOTE, etc.)
   * @param computeStateDigest Whether to compute state digest for verification
   */
  public TrackedExecutionSPI(
      ExecutionSPI delegate,
      ExecutionMetricsLogger metricsLogger,
      String executionMode,
      boolean computeStateDigest) {
    this.delegate = delegate;
    this.metricsLogger = metricsLogger;
    this.executionMode = executionMode;
    this.computeStateDigest = computeStateDigest;
    this.stateDigest = computeStateDigest ? new StateDigestJni() : null;

    logger.info("TrackedExecutionSPI initialized for mode: {}, state digest: {}",
        executionMode, computeStateDigest);
  }

  /**
   * Create a tracked ExecutionSPI wrapper with default settings.
   *
   * @param delegate The ExecutionSPI implementation to wrap
   * @param outputDirectory Directory for CSV output
   * @param executionMode The execution mode name
   * @throws IOException if metrics logger cannot be created
   */
  public static TrackedExecutionSPI create(
      ExecutionSPI delegate,
      String outputDirectory,
      String executionMode) throws IOException {
    ExecutionMetricsLogger logger = new ExecutionMetricsLogger(outputDirectory);
    return new TrackedExecutionSPI(delegate, logger, executionMode, true);
  }

  @Override
  public CompletableFuture<ExecutionProgramResult> executeTransaction(TransactionContext context)
      throws ContractValidateException, ContractExeException, VMIllegalException {

    long startTime = System.currentTimeMillis();

    return delegate.executeTransaction(context)
        .thenApply(result -> {
          long executionTime = System.currentTimeMillis() - startTime;
          trackExecution(context, result, executionTime);
          return result;
        })
        .exceptionally(throwable -> {
          long executionTime = System.currentTimeMillis() - startTime;
          trackExecutionError(context, throwable, executionTime);
          if (throwable instanceof RuntimeException) {
            throw (RuntimeException) throwable;
          } else {
            throw new RuntimeException("Execution failed", throwable);
          }
        });
  }

  @Override
  public CompletableFuture<ExecutionProgramResult> callContract(TransactionContext context)
      throws ContractValidateException, VMIllegalException {

    long startTime = System.currentTimeMillis();

    return delegate.callContract(context)
        .thenApply(result -> {
          long executionTime = System.currentTimeMillis() - startTime;
          trackExecution(context, result, executionTime);
          return result;
        })
        .exceptionally(throwable -> {
          long executionTime = System.currentTimeMillis() - startTime;
          trackExecutionError(context, throwable, executionTime);
          if (throwable instanceof RuntimeException) {
            throw (RuntimeException) throwable;
          } else {
            throw new RuntimeException("Call failed", throwable);
          }
        });
  }

  @Override
  public CompletableFuture<Long> estimateEnergy(TransactionContext context)
      throws ContractValidateException {
    // Note: Energy estimation typically doesn't modify state, so we don't track it
    return delegate.estimateEnergy(context);
  }

  @Override
  public CompletableFuture<byte[]> getCode(byte[] address, String snapshotId) {
    return delegate.getCode(address, snapshotId);
  }

  @Override
  public CompletableFuture<byte[]> getStorageAt(byte[] address, byte[] key, String snapshotId) {
    return delegate.getStorageAt(address, key, snapshotId);
  }

  @Override
  public CompletableFuture<Long> getNonce(byte[] address, String snapshotId) {
    return delegate.getNonce(address, snapshotId);
  }

  @Override
  public CompletableFuture<byte[]> getBalance(byte[] address, String snapshotId) {
    return delegate.getBalance(address, snapshotId);
  }

  @Override
  public CompletableFuture<String> createSnapshot() {
    return delegate.createSnapshot();
  }

  @Override
  public CompletableFuture<Boolean> revertToSnapshot(String snapshotId) {
    return delegate.revertToSnapshot(snapshotId);
  }

  @Override
  public CompletableFuture<HealthStatus> healthCheck() {
    return delegate.healthCheck();
  }

  @Override
  public void registerMetricsCallback(MetricsCallback callback) {
    delegate.registerMetricsCallback(callback);
  }

  @Override
  public void close() {
    logger.info("Closing TrackedExecutionSPI...");

    // Close metrics logger
    if (metricsLogger != null) {
      metricsLogger.close();
    }

    // Close state digest
    if (stateDigest != null) {
      stateDigest.destroy();
    }

    // Close delegate if it's also AutoCloseable
    if (delegate instanceof AutoCloseable) {
      try {
        ((AutoCloseable) delegate).close();
      } catch (Exception e) {
        logger.warn("Error closing delegate ExecutionSPI", e);
      }
    }

    logger.info("TrackedExecutionSPI closed");
  }

  /**
   * Track successful execution.
   */
  private void trackExecution(
      TransactionContext context,
      ExecutionProgramResult result,
      long executionTimeMs) {

    try {
      String stateDigestValue = null;

      // Compute state digest if enabled
      if (computeStateDigest && stateDigest != null) {
        stateDigestValue = computeStateDigest(result);
      }

      // Create metrics
      ExecutionMetrics metrics = ExecutionMetrics.create(
          executionMode,
          result,
          context,
          stateDigestValue,
          executionTimeMs
      );

      // Log metrics asynchronously
      metricsLogger.log(metrics);

      logger.debug("Tracked execution for transaction: {}, success: {}, energy: {}, time: {}ms",
          metrics.getTransactionId(), result.isSuccess(), result.getEnergyUsed(), executionTimeMs);

    } catch (Exception e) {
      logger.error("Failed to track execution metrics", e);
    }
  }

  /**
   * Track execution error.
   */
  private void trackExecutionError(
      TransactionContext context,
      Throwable throwable,
      long executionTimeMs) {

    try {
      String errorMessage = throwable != null ? throwable.getMessage() : "Unknown error";

      ExecutionMetrics metrics = ExecutionMetrics.createError(
          executionMode,
          context,
          errorMessage,
          executionTimeMs
      );

      metricsLogger.log(metrics);

      logger.debug("Tracked execution error for transaction: {}, error: {}, time: {}ms",
          metrics.getTransactionId(), errorMessage, executionTimeMs);

    } catch (Exception e) {
      logger.error("Failed to track execution error metrics", e);
    }
  }

  /**
   * Compute state digest from execution result.
   */
  private String computeStateDigest(ExecutionProgramResult result) {
    try {
      if (result == null || result.getStateChanges() == null || result.getStateChanges().isEmpty()) {
        return "";
      }

      // Clear previous state
      stateDigest.clear();

      // Add each modified account to the digest
      for (ExecutionSPI.StateChange change : result.getStateChanges()) {
        // Note: This is a simplified approach that only captures storage changes
        // In a full implementation, you might need account balance, nonce, and code hash
        stateDigest.addAccount(
            change.getAddress(),
            new byte[32], // placeholder balance
            0L, // placeholder nonce
            new byte[32], // placeholder code hash
            Arrays.asList(change.getKey()),
            Arrays.asList(change.getNewValue())
        );
      }

      return stateDigest.computeHashHex();

    } catch (Exception e) {
      logger.warn("Failed to compute state digest", e);
      return "error:" + e.getMessage();
    }
  }

  /**
   * Get the wrapped ExecutionSPI implementation.
   */
  public ExecutionSPI getDelegate() {
    return delegate;
  }

  /**
   * Get the execution mode name.
   */
  public String getExecutionMode() {
    return executionMode;
  }

  /**
   * Get metrics logger queue size for monitoring.
   */
  public int getMetricsQueueSize() {
    return metricsLogger != null ? metricsLogger.getQueueSize() : 0;
  }

  /**
   * Check if metrics logging is active.
   */
  public boolean isMetricsLoggingActive() {
    return metricsLogger != null && metricsLogger.isRunning();
  }
}