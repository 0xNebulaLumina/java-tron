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
      byte[] oldValue = stateChange.getOldValue();
      byte[] newValue = stateChange.getNewValue();
      
      // Log state change details for debugging
      logger.debug("Applying state change - address: {}, key length: {}, oldValue length: {}, newValue length: {}",
          org.tron.common.utils.ByteArray.toHexString(address),
          key != null ? key.length : 0,
          oldValue != null ? oldValue.length : 0,
          newValue != null ? newValue.length : 0);
      
      // For account balance changes (key is typically empty or null)
      // This indicates an account-level change (balance, nonce, code, etc.)
      if (key == null || key.length == 0) {
        // This is an account balance/state update
        logger.debug("Processing account state change for address: {}", 
                    org.tron.common.utils.StringUtil.encode58Check(address));
        updateAccountState(address, newValue, chainBaseManager, context);
      } else {
        // This is a storage update (contract storage slot)
        logger.debug("Processing storage change for address: {}, key: {}", 
                    org.tron.common.utils.StringUtil.encode58Check(address),
                    org.tron.common.utils.ByteArray.toHexString(key));
        updateAccountStorage(address, key, newValue, chainBaseManager, context);
      }
      
    } catch (Exception e) {
      logger.error("Failed to apply individual state change for address: {}, error: {}", 
          org.tron.common.utils.ByteArray.toHexString(stateChange.getAddress()), 
          e.getMessage(), e);
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
      logger.info("Updating account state for address (length: {}): {}, newValue length: {}", 
          address.length, org.tron.common.utils.ByteArray.toHexString(address), 
          newValue != null ? newValue.length : 0);
      
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
      
      // Deserialize the AccountInfo from the serialized format first
      AccountInfo accountInfo = deserializeAccountInfo(newValue);
      if (accountInfo == null) {
        logger.error("Failed to deserialize AccountInfo for address: {} from {} bytes", addressStr, newValue.length);
        // Don't proceed if we can't deserialize the account info
        return;
      }
      
      // Get or create account
      AccountCapsule accountCapsule = chainBaseManager.getAccountStore().get(address);
      boolean isNewAccount = (accountCapsule == null);
      
      if (isNewAccount) {
        // Create new account if it doesn't exist with the balance from AccountInfo
        Account.Builder accountBuilder = Account.newBuilder()
            .setAddress(com.google.protobuf.ByteString.copyFrom(address))
            .setBalance(accountInfo.balance) // Use balance from AccountInfo
            .setCreateTime(System.currentTimeMillis())
            .setType(org.tron.protos.Protocol.AccountType.Normal); // Set account type
        accountCapsule = new AccountCapsule(accountBuilder.build());
        logger.info("Created new account: {} with balance: {} for remote execution state sync", 
                   addressStr, accountInfo.balance);
      } else {
        // Update existing account
        long oldBalance = accountCapsule.getBalance();        
        // Update balance
        accountCapsule.setBalance(accountInfo.balance);
        
        logger.info("Updated existing account {}: balance {} -> {}", 
                   addressStr, oldBalance, accountInfo.balance);
      }
      
      // Note: TRON doesn't have explicit nonce like Ethereum, so we'll just track it for logging
      // Note: Getting/Setting contract code in TRON requires different mechanisms than just accessing AccountCapsule
      // This would typically involve ContractStore and other TRON-specific storage
      if (accountInfo.code != null && accountInfo.code.length > 0) {
        logger.debug("Account {} has contract code: {} bytes, codeHash: {}", 
                    addressStr, accountInfo.code.length, 
                    org.tron.common.utils.ByteArray.toHexString(accountInfo.codeHash));
        // TODO: Handle contract code storage if needed
      }

      // Store the updated account
      chainBaseManager.getAccountStore().put(address, accountCapsule);
      
      if (isNewAccount) {
        logger.info("Successfully created and stored new account: {} with balance: {}", 
                   addressStr, accountInfo.balance);
      } else {
        logger.info("Successfully updated existing account: {} with new balance: {}", 
                   addressStr, accountInfo.balance);
      }
      
    } catch (Exception e) {
      logger.error("Failed to update account state for address: {}, error: {}", 
          org.tron.common.utils.StringUtil.encode58Check(address), e.getMessage(), e);
    }
  }

  /**
   * Update account storage in the local database.
   * Handles freeze ledger records and dynamic properties from remote execution.
   */
  private void updateAccountStorage(byte[] address, byte[] key, byte[] newValue,
                                   ChainBaseManager chainBaseManager,
                                   TransactionContext context) {
    try {
      // Decode the key to check if it's a special freeze/dynamic property key
      String keyStr = decodeStorageKey(key);

      logger.debug("Processing storage update - address: {}, key decoded: '{}', value length: {}",
          org.tron.common.utils.ByteArray.toHexString(address),
          keyStr,
          newValue != null ? newValue.length : 0);

      // Handle freeze ledger records (FREEZE:BW, FREEZE:EN, FREEZE:TP)
      if (keyStr.startsWith("FREEZE:")) {
        applyFreezeRecord(address, keyStr, newValue, chainBaseManager);
        return;
      }

      // Handle dynamic properties (DYNPROPS sentinel address)
      if (isSentinelDynPropsAddress(address)) {
        applyDynamicProperty(keyStr, newValue, chainBaseManager);
        return;
      }

      // Handle contract storage slots (standard EVM storage)
      logger.debug("Standard contract storage update for address: {}, key: {}",
          org.tron.common.utils.ByteArray.toHexString(address),
          org.tron.common.utils.ByteArray.toHexString(key));
      // TODO: Implement contract storage synchronization if needed

    } catch (Exception e) {
      logger.error("Failed to update account storage for address: {}, key: {}, error: {}",
          org.tron.common.utils.ByteArray.toHexString(address),
          org.tron.common.utils.ByteArray.toHexString(key),
          e.getMessage(), e);
    }
  }

  /**
   * Apply a freeze record to an account's freeze ledger.
   * Format: FREEZE:BW (bandwidth), FREEZE:EN (energy), FREEZE:TP (tron power)
   * Value: 16 bytes = [amount(8 bytes big-endian)] + [expiration(8 bytes big-endian)]
   */
  private void applyFreezeRecord(byte[] address, String freezeKey, byte[] newValue,
                                ChainBaseManager chainBaseManager) {
    String addressStr = org.tron.common.utils.StringUtil.encode58Check(address);

    logger.info("Applying freeze record for account: {}, key: {}, value length: {}",
        addressStr, freezeKey, newValue != null ? newValue.length : 0);

    // Get or create account
    AccountCapsule accountCapsule = chainBaseManager.getAccountStore().get(address);
    if (accountCapsule == null) {
      logger.warn("Account does not exist for freeze record: {}", addressStr);
      // Optionally create account with default balance
      accountCapsule = new AccountCapsule(com.google.protobuf.ByteString.copyFrom(address),
          org.tron.protos.Protocol.AccountType.Normal);
    }

    // Handle deletion (empty newValue)
    if (newValue == null || newValue.length == 0) {
      logger.info("Clearing freeze record for account: {}, resource: {}", addressStr, freezeKey);
      // Set frozen balance to 0
      if (freezeKey.equals("FREEZE:BW")) {
        accountCapsule.setFrozenForBandwidth(0, 0);
      } else if (freezeKey.equals("FREEZE:EN")) {
        accountCapsule.setFrozenForEnergy(0, 0);
      } else if (freezeKey.equals("FREEZE:TP")) {
        accountCapsule.setFrozenForTronPower(0, 0);
      }
    } else {
      // Parse freeze record: 16 bytes = amount(8) + expiration(8)
      if (newValue.length < 16) {
        logger.error("Invalid freeze record length: {} bytes, expected 16", newValue.length);
        return;
      }

      // Extract amount (first 8 bytes, big-endian)
      long amount = 0;
      for (int i = 0; i < 8; i++) {
        amount = (amount << 8) | (newValue[i] & 0xFF);
      }

      // Extract expiration (next 8 bytes, big-endian, signed i64)
      long expiration = 0;
      for (int i = 8; i < 16; i++) {
        expiration = (expiration << 8) | (newValue[i] & 0xFF);
      }

      logger.info("Parsed freeze record: amount={} SUN, expiration={} ms, resource={}",
          amount, expiration, freezeKey);

      // Apply freeze to account based on resource type
      if (freezeKey.equals("FREEZE:BW")) {
        accountCapsule.setFrozenForBandwidth(amount, expiration);
        logger.info("Set frozen bandwidth for {}: amount={}, expiration={}",
            addressStr, amount, expiration);
      } else if (freezeKey.equals("FREEZE:EN")) {
        accountCapsule.setFrozenForEnergy(amount, expiration);
        logger.info("Set frozen energy for {}: amount={}, expiration={}",
            addressStr, amount, expiration);
      } else if (freezeKey.equals("FREEZE:TP")) {
        accountCapsule.setFrozenForTronPower(amount, expiration);
        logger.info("Set frozen tron power for {}: amount={}, expiration={}",
            addressStr, amount, expiration);
      }
    }

    // Persist updated account
    chainBaseManager.getAccountStore().put(address, accountCapsule);
    logger.info("Successfully applied freeze record for account: {}", addressStr);
  }

  /**
   * Apply a dynamic property update.
   * Handles TOTAL_NET_WEIGHT, TOTAL_NET_LIMIT, etc.
   */
  private void applyDynamicProperty(String propertyKey, byte[] newValue,
                                   ChainBaseManager chainBaseManager) {
    logger.info("Applying dynamic property: key={}, value length={}",
        propertyKey, newValue != null ? newValue.length : 0);

    // Handle deletion (empty newValue)
    if (newValue == null || newValue.length == 0) {
      logger.info("Clearing dynamic property: {}", propertyKey);
      // For now, we don't delete dynamic properties
      return;
    }

    // Parse value as u64 (8 bytes, big-endian)
    if (newValue.length < 8) {
      logger.error("Invalid dynamic property value length: {} bytes, expected 8", newValue.length);
      return;
    }

    long value = 0;
    for (int i = 0; i < 8; i++) {
      value = (value << 8) | (newValue[i] & 0xFF);
    }

    logger.info("Parsed dynamic property: {}={}", propertyKey, value);

    // Apply to DynamicPropertiesStore
    if (propertyKey.equals("TOTAL_NET_WEIGHT")) {
      chainBaseManager.getDynamicPropertiesStore().saveTotalNetWeight(value);
      logger.info("Set TOTAL_NET_WEIGHT to {}", value);
    } else if (propertyKey.equals("TOTAL_NET_LIMIT")) {
      chainBaseManager.getDynamicPropertiesStore().saveTotalNetLimit(value);
      logger.info("Set TOTAL_NET_LIMIT to {}", value);
    } else if (propertyKey.equals("TOTAL_ENERGY_WEIGHT")) {
      chainBaseManager.getDynamicPropertiesStore().saveTotalEnergyWeight(value);
      logger.info("Set TOTAL_ENERGY_WEIGHT to {}", value);
    } else if (propertyKey.equals("TOTAL_ENERGY_LIMIT")) {
      // Note: Check if this method exists in DynamicPropertiesStore
      logger.info("TOTAL_ENERGY_LIMIT update requested but may not be supported yet");
    } else {
      logger.warn("Unknown dynamic property key: {}", propertyKey);
    }
  }

  /**
   * Decode a storage key from U256 bytes to a string.
   * The key may be ASCII padded to 32 bytes (right-aligned).
   */
  private String decodeStorageKey(byte[] key) {
    if (key == null || key.length == 0) {
      return "";
    }

    // Find the first non-zero byte (skip leading padding)
    int start = 0;
    while (start < key.length && key[start] == 0) {
      start++;
    }

    if (start >= key.length) {
      return ""; // All zeros
    }

    // Extract the non-zero portion
    byte[] keyBytes = new byte[key.length - start];
    System.arraycopy(key, start, keyBytes, 0, keyBytes.length);

    // Try to decode as ASCII
    try {
      String decoded = new String(keyBytes, java.nio.charset.StandardCharsets.US_ASCII);
      // Check if it's printable ASCII
      if (decoded.matches("^[\\x20-\\x7E]+$")) {
        return decoded;
      }
    } catch (Exception e) {
      // Not ASCII, return hex representation
    }

    // Return hex if not ASCII
    return org.tron.common.utils.ByteArray.toHexString(keyBytes);
  }

  /**
   * Check if an address is the sentinel DYNPROPS address.
   * Could be ASCII "DYNPROPS" or 21-byte all-zeros.
   */
  private boolean isSentinelDynPropsAddress(byte[] address) {
    if (address == null) {
      return false;
    }

    // Check if it's ASCII "DYNPROPS" (padded)
    String addressStr = decodeStorageKey(address);
    if (addressStr.equals("DYNPROPS")) {
      return true;
    }

    // Check if it's all zeros (21 bytes)
    if (address.length == 21) {
      for (byte b : address) {
        if (b != 0) {
          return false;
        }
      }
      return true;
    }

    return false;
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
    // Handle empty data for account deletion cases
    if (data == null || data.length == 0) {
      return null;
    }
    
    // Handle minimal accounts (balance only) - at least 32 bytes for balance
    if (data.length < 32) {
      logger.warn("AccountInfo data too short: {} bytes. Expected at least 32 bytes for balance.", data.length);
      return null;
    }
    
    try {
      int offset = 0;
      
      // Extract balance (32 bytes, big-endian)
      byte[] balanceBytes = new byte[32];
      System.arraycopy(data, offset, balanceBytes, 0, 32);
      long balance = bytesToLongFromBalance(balanceBytes);
      offset += 32;
      
      // Default values for optional fields
      long nonce = 0;
      byte[] codeHash = new byte[32]; // Default to zero hash
      byte[] code = new byte[0]; // Default to empty code
      
      // Extract nonce if present (8 bytes, big-endian)
      if (data.length >= offset + 8) {
        for (int i = 0; i < 8; i++) {
          nonce = (nonce << 8) | (data[offset + i] & 0xFF);
        }
        offset += 8;
        
        // Extract code hash if present (32 bytes)
        if (data.length >= offset + 32) {
          System.arraycopy(data, offset, codeHash, 0, 32);
          offset += 32;
          
          // Extract code length and code if present (4 bytes for length, then variable)
          if (data.length >= offset + 4) {
            int codeLength = 0;
            for (int i = 0; i < 4; i++) {
              codeLength = (codeLength << 8) | (data[offset + i] & 0xFF);
            }
            offset += 4;
            
            // Extract code (variable length)
            if (codeLength > 0 && data.length >= offset + codeLength) {
              code = new byte[codeLength];
              System.arraycopy(data, offset, code, 0, codeLength);
            }
          }
        }
      }
      
      logger.debug("Deserialized AccountInfo - balance: {}, nonce: {}, codeHash length: {}, code length: {}", 
                   balance, nonce, codeHash.length, code.length);
      
      return new AccountInfo(balance, nonce, codeHash, code);
      
    } catch (Exception e) {
      logger.warn("Failed to deserialize AccountInfo from {} bytes: {}", data.length, e.getMessage());
      return null;
    }
  }
}
