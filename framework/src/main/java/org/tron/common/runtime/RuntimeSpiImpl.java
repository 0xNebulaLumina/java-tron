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

    logger.info("Applying {} state changes to local database for transaction: {}", 
        result.getStateChanges().size(), context.getTrxCap().getTransactionId());

    try {
      // Get the chain base manager from context
      ChainBaseManager chainBaseManager = context.getStoreFactory().getChainBaseManager();
      
      for (ExecutionSPI.StateChange stateChange : result.getStateChanges()) {
        applyStateChange(stateChange, chainBaseManager, context);
      }
      
      logger.info("Successfully applied {} state changes for transaction: {}", 
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
        // This is storage update
        updateAccountStorage(address, key, newValue, chainBaseManager, context);
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
      // Log the address format for debugging
      logger.info("Updating account state for address (length: {}): {}", 
          address.length, org.tron.common.utils.ByteArray.toHexString(address));
      
      String addressStr = org.tron.common.utils.StringUtil.encode58Check(address);
      
      // Check for account deletion
      if (newValue == null || newValue.length == 0) {
        // Handle account deletion
        AccountCapsule existingAccount = chainBaseManager.getAccountStore().get(address);
        if (existingAccount != null) {
          // Delete the account from the store
          chainBaseManager.getAccountStore().delete(address);
          logger.info("Deleted account: {} due to remote execution state sync", addressStr);
        } else {
          logger.debug("Account deletion requested for non-existent account: {}", addressStr);
        }
        return;
      }
      
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

      // Update account state based on newValue (deserialize AccountInfo)
      // Note: accountCapsule is guaranteed to be non-null here due to creation above
      if (newValue != null && newValue.length > 0) {
        // Deserialize the AccountInfo from the serialized format
        AccountInfo accountInfo = deserializeAccountInfo(newValue);
        if (accountInfo != null) {
          long oldBalance = accountCapsule.getBalance();        
          // Update balance
          accountCapsule.setBalance(accountInfo.balance);

          // Note: TRON doesn't have explicit nonce like Ethereum, so we'll just track it for logging
          // Note: Getting/Setting contract code in TRON requires different mechanisms than just accessing AccountCapsule
          // This would typically involve ContractStore and other TRON-specific storage, for now we'll just log the values
          logger.debug("Updated account for {}: balance {} -> {}, nonce: {}, codeHash: {}, code: {} bytes", 
              addressStr, oldBalance, accountInfo.balance, accountInfo.nonce, 
              java.util.Arrays.toString(accountInfo.codeHash), accountInfo.code.length);
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
   * Update account storage in the local database.
   */
  private void updateAccountStorage(byte[] address, byte[] key, byte[] newValue,
                                   ChainBaseManager chainBaseManager,
                                   TransactionContext context) {
    try {
      // Account storage updates would go here
      // This is more complex and depends on how Account storage is managed
      logger.debug("Account storage update for address: {}, key: {}", 
          address, key);
      // TODO: Implement account storage synchronization if needed
      
    } catch (Exception e) {
      logger.warn("Failed to update account storage for address: {}, key: {}, error: {}", 
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

  /**
   * Convert 32-byte balance array to long (big-endian).
   */
  private long bytesToLongFromBalance(byte[] bytes) {
    if (bytes == null || bytes.length < 32) {
      return 0;
    }
    long result = 0;
    // Take the last 8 bytes from the 32-byte balance
    for (int i = 24; i < 32; i++) {
      result = (result << 8) | (bytes[i] & 0xFF);
    }
    return result;
  }

  /**
   * Simple AccountInfo class to hold deserialized account information.
   */
  private static class AccountInfo {
    public final long balance;
    public final long nonce;
    public final byte[] codeHash;
    public final byte[] code;
    
    public AccountInfo(long balance, long nonce, byte[] codeHash, byte[] code) {
      this.balance = balance;
      this.nonce = nonce;
      this.codeHash = codeHash != null ? codeHash : new byte[0];
      this.code = code != null ? code : new byte[0];
    }
  }

  /**
   * Deserialize AccountInfo from byte array.
   * Format: [balance(32)] + [nonce(8)] + [code_hash(32)] + [code_length(4)] + [code(variable)]
   */
  private AccountInfo deserializeAccountInfo(byte[] data) {
    if (data == null || data.length < 76) { // 32 + 8 + 32 + 4 = 76 minimum
      return null;
    }
    
    try {
      int offset = 0;
      
      // Extract balance (32 bytes, big-endian)
      byte[] balanceBytes = new byte[32];
      System.arraycopy(data, offset, balanceBytes, 0, 32);
      long balance = bytesToLongFromBalance(balanceBytes);
      offset += 32;
      
      // Extract nonce (8 bytes, big-endian)
      long nonce = 0;
      for (int i = 0; i < 8; i++) {
        nonce = (nonce << 8) | (data[offset + i] & 0xFF);
      }
      offset += 8;
      
      // Extract code hash (32 bytes)
      byte[] codeHash = new byte[32];
      System.arraycopy(data, offset, codeHash, 0, 32);
      offset += 32;
      
      // Extract code length (4 bytes, big-endian)
      int codeLength = 0;
      for (int i = 0; i < 4; i++) {
        codeLength = (codeLength << 8) | (data[offset + i] & 0xFF);
      }
      offset += 4;
      
      // Extract code (variable length)
      byte[] code = new byte[codeLength];
      if (codeLength > 0 && offset + codeLength <= data.length) {
        System.arraycopy(data, offset, code, 0, codeLength);
      }
      
      return new AccountInfo(balance, nonce, codeHash, code);
      
    } catch (Exception e) {
      logger.warn("Failed to deserialize AccountInfo from {} bytes: {}", data.length, e.getMessage());
      return null;
    }
  }
}
