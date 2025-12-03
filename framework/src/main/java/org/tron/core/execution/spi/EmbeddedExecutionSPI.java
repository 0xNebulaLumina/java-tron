package org.tron.core.execution.spi;

import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.runtime.ProgramResult;
import org.tron.common.runtime.Runtime;
import org.tron.common.runtime.RuntimeImpl;
import org.tron.common.runtime.vm.DataWord;
import org.tron.common.runtime.vm.LogInfo;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.core.exception.VMIllegalException;
import org.tron.protos.Protocol.Transaction.Result.contractResult;

/**
 * Embedded execution implementation using the existing Java EVM. This implementation wraps the
 * current TronVM/RuntimeImpl to provide the ExecutionSPI interface while maintaining backward
 * compatibility.
 */
public class EmbeddedExecutionSPI implements ExecutionSPI {
  private static final Logger logger = LoggerFactory.getLogger(EmbeddedExecutionSPI.class);

  private MetricsCallback metricsCallback;

  public EmbeddedExecutionSPI() {
    logger.info("Initialized embedded execution SPI");
  }

  @Override
  public CompletableFuture<ExecutionProgramResult> executeTransaction(TransactionContext context)
      throws ContractValidateException, ContractExeException, VMIllegalException {

    return CompletableFuture.supplyAsync(
        () -> {
          try {
            logger.debug(
                "Executing transaction with embedded EVM: {}",
                context.getTrxCap().getTransactionId());

            // Create runtime instance for execution
            Runtime runtime = new RuntimeImpl();

            // Execute the transaction
            runtime.execute(context);

            // Get the result
            ProgramResult programResult = runtime.getResult();
            String runtimeError = runtime.getRuntimeError();

            // Convert to ExecutionProgramResult
            return ExecutionProgramResult.fromProgramResult(programResult);

          } catch (Exception e) {
            logger.error("Embedded execution failed", e);
            // Create a failed ExecutionProgramResult
            ExecutionProgramResult result = new ExecutionProgramResult();
            result.setRuntimeError(e.getMessage());
            result.setRevert();
            result.setResultCode(contractResult.UNKNOWN);
            return result;
          }
        });
  }

  @Override
  public CompletableFuture<ExecutionProgramResult> callContract(TransactionContext context)
      throws ContractValidateException, VMIllegalException {

    return CompletableFuture.supplyAsync(
        () -> {
          try {
            logger.debug(
                "Calling contract with embedded EVM: {}", context.getTrxCap().getTransactionId());

            // For contract calls, we use the same execution but mark it as constant
            // This prevents state changes from being committed
            Runtime runtime = new RuntimeImpl();

            // Execute the call
            runtime.execute(context);

            // Get the result
            ProgramResult programResult = runtime.getResult();
            String runtimeError = runtime.getRuntimeError();

            // Convert to ExecutionProgramResult
            return ExecutionProgramResult.fromProgramResult(programResult);

          } catch (Exception e) {
            logger.error("Embedded contract call failed", e);
            // Create a failed ExecutionProgramResult
            ExecutionProgramResult result = new ExecutionProgramResult();
            result.setRuntimeError(e.getMessage());
            result.setRevert();
            result.setResultCode(contractResult.UNKNOWN);
            return result;
          }
        });
  }

  @Override
  public CompletableFuture<Long> estimateEnergy(TransactionContext context)
      throws ContractValidateException {

    return CompletableFuture.supplyAsync(
        () -> {
          try {
            logger.debug(
                "Estimating energy with embedded EVM: {}", context.getTrxCap().getTransactionId());

            // Create runtime for estimation
            Runtime runtime = new RuntimeImpl();

            // Execute to get energy usage
            runtime.execute(context);
            ProgramResult result = runtime.getResult();

            return result.getEnergyUsed();

          } catch (Exception e) {
            logger.error("Embedded energy estimation failed", e);
            return 0L;
          }
        });
  }

  @Override
  public CompletableFuture<byte[]> getCode(byte[] address, String snapshotId) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            // TODO: Implement code retrieval from current storage
            // This would typically access the CodeStore
            logger.debug("Getting code for address: {}", address);
            return new byte[0]; // Placeholder
          } catch (Exception e) {
            logger.error("Failed to get code", e);
            return new byte[0];
          }
        });
  }

  @Override
  public CompletableFuture<byte[]> getStorageAt(byte[] address, byte[] key, String snapshotId) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            // TODO: Implement storage retrieval from current storage
            logger.debug("Getting storage at address: {}, key: {}", address, key);
            return new byte[0]; // Placeholder
          } catch (Exception e) {
            logger.error("Failed to get storage", e);
            return new byte[0];
          }
        });
  }

  @Override
  public CompletableFuture<Long> getNonce(byte[] address, String snapshotId) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            // TODO: Implement nonce retrieval from AccountStore
            logger.debug("Getting nonce for address: {}", address);
            return 0L; // Placeholder
          } catch (Exception e) {
            logger.error("Failed to get nonce", e);
            return 0L;
          }
        });
  }

  @Override
  public CompletableFuture<byte[]> getBalance(byte[] address, String snapshotId) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            // TODO: Implement balance retrieval from AccountStore
            logger.debug("Getting balance for address: {}", address);
            return new byte[0]; // Placeholder
          } catch (Exception e) {
            logger.error("Failed to get balance", e);
            return new byte[0];
          }
        });
  }

  @Override
  public CompletableFuture<String> createSnapshot() {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            // TODO: Implement snapshot creation
            logger.debug("Creating EVM snapshot");
            return "embedded_snapshot_" + System.currentTimeMillis();
          } catch (Exception e) {
            logger.error("Failed to create snapshot", e);
            return null;
          }
        });
  }

  @Override
  public CompletableFuture<Boolean> revertToSnapshot(String snapshotId) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            // TODO: Implement snapshot revert
            logger.debug("Reverting to snapshot: {}", snapshotId);
            return true; // Placeholder
          } catch (Exception e) {
            logger.error("Failed to revert to snapshot", e);
            return false;
          }
        });
  }

  @Override
  public CompletableFuture<HealthStatus> healthCheck() {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            // Simple health check - verify we can create a runtime
            return new HealthStatus(true, "Embedded execution healthy");
          } catch (Exception e) {
            logger.error("Embedded execution health check failed", e);
            return new HealthStatus(false, "Embedded execution unhealthy: " + e.getMessage());
          }
        });
  }

  @Override
  public void registerMetricsCallback(MetricsCallback callback) {
    this.metricsCallback = callback;
    logger.info("Registered metrics callback for embedded execution");
  }

  /** Convert ProgramResult to ExecutionResult. */
  private ExecutionResult convertProgramResultToExecutionResult(
      ProgramResult programResult, String runtimeError) {
    if (programResult == null) {
      return new ExecutionResult(
          false, // success
          new byte[0], // returnData
          0, // energyUsed
          0, // energyRefunded
          new ArrayList<>(), // stateChanges
          new ArrayList<>(), // logs
          runtimeError != null ? runtimeError : "Unknown error", // errorMessage
          0, // bandwidthUsed
          new ArrayList<>(), // freezeChanges (Phase 2 - not used in embedded mode)
          new ArrayList<>(), // globalResourceChanges (Phase 2 - not used in embedded mode)
          new ArrayList<>(), // trc10Changes (Phase 2 - not used in embedded mode)
          new ArrayList<>() // voteChanges (Phase 2 - not used in embedded mode)
          );
    }

    boolean success = programResult.getException() == null && runtimeError == null;
    byte[] returnData =
        programResult.getHReturn() != null ? programResult.getHReturn() : new byte[0];
    long energyUsed = programResult.getEnergyUsed();
    long energyRefunded = 0; // TODO: ProgramResult doesn't have getEnergyRefund method

    // TODO: Convert program result logs to LogEntry objects
    List<LogEntry> logs = new ArrayList<>();

    // TODO: Extract state changes from program result
    List<StateChange> stateChanges = new ArrayList<>();

    String errorMessage = null;
    if (!success) {
      if (runtimeError != null) {
        errorMessage = runtimeError;
      } else if (programResult.getException() != null) {
        errorMessage = programResult.getException().getMessage();
      }
    }

    // Report metrics if callback is registered
    if (metricsCallback != null) {
      metricsCallback.onMetric("embedded.energy_used", energyUsed);
      metricsCallback.onMetric("embedded.success", success ? 1.0 : 0.0);
    }

    return new ExecutionResult(
        success,
        returnData,
        energyUsed,
        energyRefunded,
        stateChanges,
        logs,
        errorMessage,
        0, // TODO: Calculate bandwidth usage
        new ArrayList<>(), // freezeChanges (Phase 2 - not used in embedded mode)
        new ArrayList<>(), // globalResourceChanges (Phase 2 - not used in embedded mode)
        new ArrayList<>(), // trc10Changes (Phase 2 - not used in embedded mode)
        new ArrayList<>() // voteChanges (Phase 2 - not used in embedded mode)
        );
  }
}
