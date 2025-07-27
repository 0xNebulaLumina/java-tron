package org.tron.common.runtime;

import java.util.concurrent.CompletableFuture;
import lombok.extern.slf4j.Slf4j;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;

import org.tron.core.execution.spi.ExecutionProgramResult;
import org.tron.core.execution.spi.ExecutionSPI;
import org.tron.core.execution.spi.ExecutionSpiFactory;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.core.vm.repository.Repository;
import org.tron.core.vm.repository.RepositoryImpl;
import org.tron.core.capsule.AccountCapsule;
import org.tron.common.runtime.vm.DataWord;
import org.tron.protos.Protocol.Transaction.Result.contractResult;

/**
 * ExecutionSPI-aware Runtime implementation that maintains the existing Runtime interface while
 * delegating execution to the configured ExecutionSPI implementation (EMBEDDED, REMOTE, or SHADOW).
 *
 * <p>This class provides backward compatibility by using ExecutionProgramResult, which extends
 * ProgramResult, eliminating the need for type conversion.
 */
@Slf4j(topic = "VM")
public class RuntimeSpiImpl implements Runtime {

  private final ExecutionSPI executionSPI;
  private TransactionContext context;
  private ExecutionProgramResult executionResult;
  private String runtimeError;



  /**
   * Constructor that ensures ExecutionSPI factory is properly initialized.
   * The execution mode is determined dynamically from configuration sources during factory initialization.
   * This maintains the singleton pattern for efficiency while supporting dynamic configuration.
   */
  public RuntimeSpiImpl() {
    // // Ensure factory is initialized (this will determine execution mode from configuration)
    // ExecutionSpiFactory.initialize();

    this.executionSPI = ExecutionSpiFactory.getInstance();
    if (this.executionSPI == null) {
      throw new RuntimeException(
          "ExecutionSPI not initialized. Call ExecutionSpiFactory.initialize() first.");
    }
    logger.info(
        "RuntimeSpiImpl initialized with execution mode: {}",
        ExecutionSpiFactory.determineExecutionMode());
  }

  @Override
  public void execute(TransactionContext context)
      throws ContractValidateException, ContractExeException {
    this.context = context;

    try {
      logger.debug(
          "Executing transaction with ExecutionSPI: {}", context.getTrxCap().getTransactionId());

      // Use ExecutionSPI for execution
      CompletableFuture<ExecutionProgramResult> future =
          executionSPI.executeTransaction(context);
      this.executionResult = future.get(); // Synchronous execution

      // Store runtime error if execution failed
      if (!executionResult.isSuccess()) {
        this.runtimeError = executionResult.getErrorMessage();
      }

      // Since ExecutionProgramResult extends ProgramResult, we can use it directly
      context.setProgramResult(executionResult);

      // Apply state changes to repository (similar to VMActuator.rootRepository.commit())
      if (executionResult.isSuccess()) {
        applyStateChangesToRepository(context, executionResult);
      }

      logger.info(
          "ExecutionSPI execution completed. Success: {}, Energy used: {}, State changes: {}",
          executionResult.isSuccess(),
          executionResult.getEnergyUsed(),
          executionResult.getStateChanges() != null ? executionResult.getStateChanges().size() : 0);

    } catch (Exception e) {
      logger.error(
          "ExecutionSPI execution failed for transaction: {}",
          context.getTrxCap().getTransactionId(),
          e);

      // Create a failed ExecutionProgramResult for compatibility
      this.executionResult = createFailedExecutionProgramResult(e.getMessage());
      context.setProgramResult(executionResult);
      this.runtimeError = e.getMessage();

      throw new ContractExeException("Execution failed: " + e.getMessage());
    }
  }

  @Override
  public ProgramResult getResult() {
    if (context == null) {
      return ProgramResult.createEmpty();
    }
    return context.getProgramResult();
  }

  @Override
  public String getRuntimeError() {
    return runtimeError;
  }



  /** Create a failed ExecutionProgramResult when ExecutionSPI execution fails. */
  private ExecutionProgramResult createFailedExecutionProgramResult(String errorMessage) {
    ExecutionProgramResult result = new ExecutionProgramResult();

    // Set failure state
    result.setResultCode(contractResult.REVERT);
    result.setRevert();
    result.setRuntimeError(errorMessage);
    result.setException(new RuntimeException(errorMessage));

    logger.debug("Created failed ExecutionProgramResult with error: {}", errorMessage);
    return result;
  }

  /**
   * Apply state changes from remote execution to the Java-side repository.
   * This is equivalent to VMActuator calling rootRepository.commit() after successful execution.
   *
   * @param context The transaction context containing the store factory
   * @param executionResult The execution result containing state changes from remote execution
   */
  private void applyStateChangesToRepository(TransactionContext context, ExecutionProgramResult executionResult) {
    logger.info("Starting state synchronization for transaction: {}", context.getTrxCap().getTransactionId());
    try {
      // Create repository instance (similar to VMActuator line 141)
      Repository repository = RepositoryImpl.createRoot(context.getStoreFactory());

      // Apply state changes from the execution result
      if (executionResult.getStateChanges() != null) {
        for (StateChange stateChange : executionResult.getStateChanges()) {
          applyStateChange(repository, stateChange);
        }
      }

      // Commit all changes to the database (equivalent to rootRepository.commit())
      repository.commit();

      logger.info("Applied {} state changes to repository for transaction: {}",
          executionResult.getStateChanges() != null ? executionResult.getStateChanges().size() : 0,
          context.getTrxCap().getTransactionId());

    } catch (Exception e) {
      logger.error("Failed to apply state changes to repository for transaction: {}",
          context.getTrxCap().getTransactionId(), e);
      throw new RuntimeException("State synchronization failed: " + e.getMessage(), e);
    }
  }

  /**
   * Apply a single state change to the repository.
   *
   * @param repository The repository to apply changes to
   * @param stateChange The state change to apply
   */
  private void applyStateChange(Repository repository, StateChange stateChange) {
    byte[] address = stateChange.getAddress();
    byte[] key = stateChange.getKey();
    byte[] newValue = stateChange.getNewValue();

    if (key.length == 0) {
      // Account-level change (balance, nonce, etc.)
      applyAccountChange(repository, address, newValue);
    } else {
      // Storage-level change
      applyStorageChange(repository, address, key, newValue);
    }
  }

  /**
   * Apply account-level state changes (balance, nonce, etc.).
   *
   * @param repository The repository to apply changes to
   * @param address The account address
   * @param newValue The new account state (serialized)
   */
  private void applyAccountChange(Repository repository, byte[] address, byte[] newValue) {
    // Get existing account or create new one
    AccountCapsule account = repository.getAccount(address);
    if (account == null) {
      account = repository.createNormalAccount(address);
    }

    // TODO: Deserialize newValue and update account fields
    // This requires understanding the serialization format used by the Rust backend
    // For now, we'll update the account in the repository
    repository.updateAccount(address, account);

    logger.info("Applied account change for address: {}",
        org.tron.common.utils.StringUtil.encode58Check(address));
  }

  /**
   * Apply storage-level state changes.
   *
   * @param repository The repository to apply changes to
   * @param address The contract address
   * @param key The storage key
   * @param newValue The new storage value
   */
  private void applyStorageChange(Repository repository, byte[] address, byte[] key, byte[] newValue) {
    // Convert key and value to DataWord format
    DataWord keyWord = new DataWord(key);
    DataWord valueWord = new DataWord(newValue);

    // Apply storage change
    repository.putStorageValue(address, keyWord, valueWord);

    logger.debug("Applied storage change for address: {}, key: {}",
        org.tron.common.utils.StringUtil.encode58Check(address),
        keyWord.toString());
  }
}
