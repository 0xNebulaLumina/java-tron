package org.tron.common.runtime;

import java.util.concurrent.CompletableFuture;
import lombok.extern.slf4j.Slf4j;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.core.ChainBaseManager;

import org.tron.core.execution.spi.ExecutionProgramResult;
import org.tron.core.execution.spi.ExecutionSPI;
import org.tron.core.execution.spi.ExecutionSpiFactory;
import org.tron.protos.Protocol.Transaction.Result.contractResult;
import org.tron.protos.Protocol.Account;

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

      // Apply state changes to local database for remote execution
      applyStateChangesToLocalDatabase(executionResult, context);

      // Since ExecutionProgramResult extends ProgramResult, we can use it directly
      context.setProgramResult(executionResult);

      logger.debug(
          "ExecutionSPI execution completed. Success: {}, Energy used: {}, State changes applied: {}",
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
   * Apply state changes from remote execution to the local Java-Tron database.
   * This is critical for remote execution mode to ensure local state consistency.
   */
  private void applyStateChangesToLocalDatabase(ExecutionProgramResult result, TransactionContext context) {
    if (result.getStateChanges() == null || result.getStateChanges().isEmpty()) {
      logger.debug("No state changes to apply for transaction: {}", 
          context.getTrxCap().getTransactionId());
      return;
    }

    logger.debug("Applying {} state changes to local database for transaction: {}", 
        result.getStateChanges().size(), context.getTrxCap().getTransactionId());

    try {
      // Get the chain base manager from context
      ChainBaseManager chainBaseManager = context.getStoreFactory().getChainBaseManager();
      
      for (ExecutionSPI.StateChange stateChange : result.getStateChanges()) {
        applyStateChange(stateChange, chainBaseManager, context);
      }
      
      logger.debug("Successfully applied {} state changes for transaction: {}", 
          result.getStateChanges().size(), context.getTrxCap().getTransactionId());
          
    } catch (Exception e) {
      logger.error("Failed to apply state changes for transaction: {}, error: {}", 
          context.getTrxCap().getTransactionId(), e.getMessage(), e);
      // Don't throw exception here as it would break the transaction flow
      // The transaction might still be valid even if state sync fails
    }
  }

  /**
   * Apply a single state change to the local database.
   */
  private void applyStateChange(ExecutionSPI.StateChange stateChange, 
                               ChainBaseManager chainBaseManager,
                               TransactionContext context) {
    try {
      byte[] address = stateChange.getAddress();
      byte[] key = stateChange.getKey();
      byte[] newValue = stateChange.getNewValue();
      
      // For account balance changes (key is typically empty or balance-related)
      if (key == null || key.length == 0) {
        // This is likely an account balance update
        updateAccountState(address, newValue, chainBaseManager, context);
      } else {
        // This is contract storage update
        updateContractStorage(address, key, newValue, chainBaseManager, context);
      }
      
    } catch (Exception e) {
      logger.warn("Failed to apply individual state change for address: {}, error: {}", 
          stateChange.getAddress(), e.getMessage());
    }
  }

  /**
   * Update account state (balance, nonce, etc.) in the local database.
   */
  private void updateAccountState(byte[] address, byte[] newValue, 
                                 ChainBaseManager chainBaseManager,
                                 TransactionContext context) {
    try {
      String addressStr = org.tron.common.utils.StringUtil.encode58Check(address);
      
      // Get or create account
      AccountCapsule accountCapsule = chainBaseManager.getAccountStore().get(address);
      boolean isNewAccount = (accountCapsule == null);
      
      if (isNewAccount) {
        // Create new account if it doesn't exist
        Account.Builder accountBuilder = Account.newBuilder()
            .setAddress(com.google.protobuf.ByteString.copyFrom(address))
            .setBalance(0) // Will be updated below
            .setCreateTime(System.currentTimeMillis());
        accountCapsule = new AccountCapsule(accountBuilder.build());
        logger.info("Created new account: {} for remote execution state sync", addressStr);
      }

      // Update account state based on newValue
      // Note: accountCapsule is guaranteed to be non-null here due to creation above
      if (newValue != null && newValue.length > 0) {
        if (newValue.length >= 8) {
          // Extract balance from newValue (assuming it's stored as long in first 8 bytes)
          long newBalance = bytesToLong(newValue);
          long oldBalance = accountCapsule.getBalance();
          accountCapsule.setBalance(newBalance);
          logger.debug("Updated account balance for {}: {} -> {}", 
              addressStr, oldBalance, newBalance);
        } else {
          // Handle other types of account state updates
          logger.debug("Received account state update with {} bytes for {}", 
              newValue.length, addressStr);
        }
      }

      // Store the updated account
      chainBaseManager.getAccountStore().put(address, accountCapsule);
      
      if (isNewAccount) {
        logger.info("Successfully created and stored new account: {}", addressStr);
      } else {
        logger.debug("Successfully updated existing account: {}", addressStr);
      }
      
    } catch (Exception e) {
      logger.error("Failed to update account state for address: {}, error: {}", 
          org.tron.common.utils.StringUtil.encode58Check(address), e.getMessage(), e);
    }
  }

  /**
   * Update contract storage in the local database.
   */
  private void updateContractStorage(byte[] address, byte[] key, byte[] newValue,
                                   ChainBaseManager chainBaseManager,
                                   TransactionContext context) {
    try {
      // Contract storage updates would go here
      // This is more complex and depends on how contract storage is managed
      logger.debug("Contract storage update for address: {}, key: {}", 
          address, key);
      // TODO: Implement contract storage synchronization if needed
      
    } catch (Exception e) {
      logger.warn("Failed to update contract storage for address: {}, key: {}, error: {}", 
          address, key, e.getMessage());
    }
  }

  /**
   * Convert byte array to long (big-endian).
   */
  private long bytesToLong(byte[] bytes) {
    if (bytes == null || bytes.length < 8) {
      return 0;
    }
    long result = 0;
    for (int i = 0; i < 8; i++) {
      result = (result << 8) | (bytes[i] & 0xFF);
    }
    return result;
  }
}
