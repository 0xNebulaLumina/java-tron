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

import static org.tron.protos.contract.Common.ResourceCode.BANDWIDTH;
import static org.tron.protos.contract.Common.ResourceCode.ENERGY;

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

      // Apply resource usage fields from AEXT tail if present
      if (accountInfo.hasResourceUsage()) {
        logger.debug("Applying AEXT resource usage fields for account: {}", addressStr);

        // Set usage fields
        if (accountInfo.netUsage != null) {
          accountCapsule.setNetUsage(accountInfo.netUsage);
        }
        if (accountInfo.freeNetUsage != null) {
          accountCapsule.setFreeNetUsage(accountInfo.freeNetUsage);
        }
        if (accountInfo.energyUsage != null) {
          accountCapsule.setEnergyUsage(accountInfo.energyUsage);
        }

        // Set timing fields
        if (accountInfo.latestConsumeTime != null) {
          accountCapsule.setLatestConsumeTime(accountInfo.latestConsumeTime);
        }
        if (accountInfo.latestConsumeFreeTime != null) {
          accountCapsule.setLatestConsumeFreeTime(accountInfo.latestConsumeFreeTime);
        }
        if (accountInfo.latestConsumeTimeForEnergy != null) {
          accountCapsule.setLatestConsumeTimeForEnergy(accountInfo.latestConsumeTimeForEnergy);
        }

        // Set window size and optimization flags
        if (accountInfo.netWindowSize != null) {
          accountCapsule.setNewWindowSize(BANDWIDTH, accountInfo.netWindowSize);
        }
        if (accountInfo.energyWindowSize != null) {
          accountCapsule.setNewWindowSize(ENERGY, accountInfo.energyWindowSize);
        }
        if (accountInfo.netWindowOptimized != null) {
          accountCapsule.setWindowOptimized(BANDWIDTH, accountInfo.netWindowOptimized);
        }
        if (accountInfo.energyWindowOptimized != null) {
          accountCapsule.setWindowOptimized(ENERGY, accountInfo.energyWindowOptimized);
        }

        logger.debug("Applied resource usage to account {}: netUsage={}, freeNetUsage={}, energyUsage={}, times=[{},{},{}], windows=[{},{}], optimized=[{},{}]",
                     addressStr, accountInfo.netUsage, accountInfo.freeNetUsage, accountInfo.energyUsage,
                     accountInfo.latestConsumeTime, accountInfo.latestConsumeFreeTime, accountInfo.latestConsumeTimeForEnergy,
                     accountInfo.netWindowSize, accountInfo.energyWindowSize,
                     accountInfo.netWindowOptimized, accountInfo.energyWindowOptimized);
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
   * Extended to support AEXT (Account EXTension) resource usage fields.
   */
  private static class AccountInfo {
    public final long balance;
    public final long nonce;
    public final byte[] codeHash;
    public final byte[] code;

    // AEXT v1 resource usage fields (optional, null if not present)
    public final Long netUsage;
    public final Long freeNetUsage;
    public final Long energyUsage;
    public final Long latestConsumeTime;
    public final Long latestConsumeFreeTime;
    public final Long latestConsumeTimeForEnergy;
    public final Long netWindowSize;
    public final Long energyWindowSize;
    public final Boolean netWindowOptimized;
    public final Boolean energyWindowOptimized;

    public AccountInfo(long balance, long nonce, byte[] codeHash, byte[] code) {
      this(balance, nonce, codeHash, code, null, null, null, null, null, null, null, null, null, null);
    }

    public AccountInfo(long balance, long nonce, byte[] codeHash, byte[] code,
                       Long netUsage, Long freeNetUsage, Long energyUsage,
                       Long latestConsumeTime, Long latestConsumeFreeTime, Long latestConsumeTimeForEnergy,
                       Long netWindowSize, Long energyWindowSize,
                       Boolean netWindowOptimized, Boolean energyWindowOptimized) {
      this.balance = balance;
      this.nonce = nonce;
      this.codeHash = codeHash != null ? codeHash : new byte[0];
      this.code = code != null ? code : new byte[0];
      this.netUsage = netUsage;
      this.freeNetUsage = freeNetUsage;
      this.energyUsage = energyUsage;
      this.latestConsumeTime = latestConsumeTime;
      this.latestConsumeFreeTime = latestConsumeFreeTime;
      this.latestConsumeTimeForEnergy = latestConsumeTimeForEnergy;
      this.netWindowSize = netWindowSize;
      this.energyWindowSize = energyWindowSize;
      this.netWindowOptimized = netWindowOptimized;
      this.energyWindowOptimized = energyWindowOptimized;
    }

    public boolean hasResourceUsage() {
      return netUsage != null;
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
              offset += codeLength;
            }
          }
        }
      }

      // Try to parse optional AEXT (Account EXTension) tail for resource usage
      Long netUsage = null, freeNetUsage = null, energyUsage = null;
      Long latestConsumeTime = null, latestConsumeFreeTime = null, latestConsumeTimeForEnergy = null;
      Long netWindowSize = null, energyWindowSize = null;
      Boolean netWindowOptimized = null, energyWindowOptimized = null;

      if (offset + 4 <= data.length) {
        // Check for AEXT magic: 0x41 0x45 0x58 0x54 ("AEXT")
        if (data[offset] == 0x41 && data[offset + 1] == 0x45 &&
            data[offset + 2] == 0x58 && data[offset + 3] == 0x54) {
          offset += 4;

          try {
            // Read version (u16 big-endian)
            if (offset + 2 > data.length) {
              logger.warn("AEXT tail truncated at version field");
            } else {
              int version = ((data[offset] & 0xFF) << 8) | (data[offset + 1] & 0xFF);
              offset += 2;

              if (version != 1) {
                logger.warn("AEXT version {} not supported, skipping tail", version);
              } else {
                // Read length (u16 big-endian)
                if (offset + 2 > data.length) {
                  logger.warn("AEXT tail truncated at length field");
                } else {
                  int payloadLength = ((data[offset] & 0xFF) << 8) | (data[offset + 1] & 0xFF);
                  offset += 2;

                  if (offset + payloadLength > data.length) {
                    logger.warn("AEXT payload length {} exceeds remaining data {}", payloadLength, data.length - offset);
                  } else {
                    // Parse AEXT v1 payload (all big-endian i64 except booleans)
                    int payloadOffset = offset;

                    // Helper to read i64 big-endian
                    java.util.function.Function<Integer, Long> readI64 = (off) -> {
                      long val = 0;
                      for (int i = 0; i < 8; i++) {
                        val = (val << 8) | (data[off + i] & 0xFF);
                      }
                      return val;
                    };

                    if (payloadLength >= 68) { // Minimum payload size: 8*8 + 1 + 1 + 2 = 68 + 2 = 70 with padding
                      netUsage = readI64.apply(payloadOffset);
                      freeNetUsage = readI64.apply(payloadOffset + 8);
                      energyUsage = readI64.apply(payloadOffset + 16);
                      latestConsumeTime = readI64.apply(payloadOffset + 24);
                      latestConsumeFreeTime = readI64.apply(payloadOffset + 32);
                      latestConsumeTimeForEnergy = readI64.apply(payloadOffset + 40);
                      netWindowSize = readI64.apply(payloadOffset + 48);
                      energyWindowSize = readI64.apply(payloadOffset + 56);
                      netWindowOptimized = data[payloadOffset + 64] != 0;
                      energyWindowOptimized = data[payloadOffset + 65] != 0;
                      // Reserved/padding bytes at payloadOffset + 66, 67 are ignored

                      logger.debug("Parsed AEXT v1: netUsage={}, freeNetUsage={}, energyUsage={}, times=[{},{},{}], windows=[{},{}], optimized=[{},{}]",
                                   netUsage, freeNetUsage, energyUsage,
                                   latestConsumeTime, latestConsumeFreeTime, latestConsumeTimeForEnergy,
                                   netWindowSize, energyWindowSize,
                                   netWindowOptimized, energyWindowOptimized);
                    } else {
                      logger.warn("AEXT payload length {} too short for v1 (expected >= 68)", payloadLength);
                    }
                  }
                }
              }
            }
          } catch (Exception e) {
            logger.warn("Failed to parse AEXT tail: {}", e.getMessage());
            // Continue without resource usage fields
          }
        }
      }

      logger.debug("Deserialized AccountInfo - balance: {}, nonce: {}, codeHash length: {}, code length: {}, hasResourceUsage: {}",
                   balance, nonce, codeHash.length, code.length, (netUsage != null));

      return new AccountInfo(balance, nonce, codeHash, code,
                             netUsage, freeNetUsage, energyUsage,
                             latestConsumeTime, latestConsumeFreeTime, latestConsumeTimeForEnergy,
                             netWindowSize, energyWindowSize,
                             netWindowOptimized, energyWindowOptimized);
      
    } catch (Exception e) {
      logger.warn("Failed to deserialize AccountInfo from {} bytes: {}", data.length, e.getMessage());
      return null;
    }
  }
}
