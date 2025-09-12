package org.tron.core.execution.reporting;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.common.utils.ByteArray;

/**
 * Per-transaction journal for capturing state changes during embedded execution.
 * 
 * <p>Tracks both storage changes (SSTORE operations) and account-level changes
 * (balance updates, contract creation/deletion, code changes). Provides deduplication
 * and merge logic to handle multiple updates to the same storage slot or account.
 * 
 * <p>Usage pattern:
 * <pre>
 *   StateChangeJournal journal = new StateChangeJournal();
 *   // During execution...
 *   journal.recordStorageChange(address, key, oldValue, newValue);
 *   journal.recordAccountChange(address, oldAccount, newAccount);
 *   // After execution...
 *   List&lt;StateChange&gt; changes = journal.finalizeChanges();
 * </pre>
 */
public class StateChangeJournal {
  
  private static final Logger logger = LoggerFactory.getLogger(StateChangeJournal.class);
  
  // Storage changes: (address + key) -> StorageChangeEntry
  private final Map<String, StorageChangeEntry> storageChanges = new HashMap<>();
  
  // Account changes: address -> AccountChangeEntry  
  private final Map<String, AccountChangeEntry> accountChanges = new HashMap<>();
  
  private final Object lock = new Object();
  private volatile boolean finalized = false;
  
  /**
   * Check if state change collection is enabled.
   */
  public static boolean isEnabled() {
    String enabledProp = System.getProperty("exec.csv.stateChanges.enabled", "false");
    return "true".equalsIgnoreCase(enabledProp);
  }
  
  /**
   * Record a storage slot change (SSTORE operation).
   * 
   * @param contractAddress Contract address
   * @param storageKey Storage slot key
   * @param oldValue Previous value (null for new slots)
   * @param newValue New value (null for deletion)
   */
  public void recordStorageChange(byte[] contractAddress, byte[] storageKey, 
                                  byte[] oldValue, byte[] newValue) {
    if (!isEnabled() || finalized) {
      return;
    }
    
    try {
      String addressHex = ByteArray.toHexString(contractAddress).toLowerCase();
      String keyHex = ByteArray.toHexString(storageKey).toLowerCase();
      String compositeKey = addressHex + ":" + keyHex;
      
      synchronized (lock) {
        StorageChangeEntry existing = storageChanges.get(compositeKey);
        if (existing != null) {
          // Merge: keep original old value, update new value
          existing.newValue = newValue != null ? newValue.clone() : null;
        } else {
          // New entry
          StorageChangeEntry entry = new StorageChangeEntry();
          entry.address = contractAddress.clone();
          entry.key = storageKey.clone();
          entry.oldValue = oldValue != null ? oldValue.clone() : null;
          entry.newValue = newValue != null ? newValue.clone() : null;
          storageChanges.put(compositeKey, entry);
        }
      }
      
      if (logger.isDebugEnabled()) {
        logger.debug("Recorded storage change: {} key={} old={} new={}", 
                     addressHex, keyHex,
                     oldValue != null ? ByteArray.toHexString(oldValue) : "null",
                     newValue != null ? ByteArray.toHexString(newValue) : "null");
      }
    } catch (Exception e) {
      logger.warn("Failed to record storage change", e);
    }
  }
  
  /**
   * Record an account-level change (balance, code, creation, deletion).
   * 
   * @param address Account address
   * @param oldAccount Previous account state (null for creation)
   * @param newAccount New account state (null for deletion)
   */
  public void recordAccountChange(byte[] address, AccountCapsule oldAccount, AccountCapsule newAccount) {
    if (!isEnabled() || finalized) {
      return;
    }
    
    try {
      String addressHex = ByteArray.toHexString(address).toLowerCase();
      
      synchronized (lock) {
        AccountChangeEntry existing = accountChanges.get(addressHex);
        if (existing != null) {
          // Merge: keep original old state, update new state
          existing.newAccount = newAccount;
        } else {
          // New entry
          AccountChangeEntry entry = new AccountChangeEntry();
          entry.address = address.clone();
          entry.oldAccount = oldAccount;
          entry.newAccount = newAccount;
          accountChanges.put(addressHex, entry);
        }
      }
      
      if (logger.isDebugEnabled()) {
        logger.debug("Recorded account change: {} old={} new={}", 
                     addressHex,
                     oldAccount != null ? oldAccount.getBalance() : "null",
                     newAccount != null ? newAccount.getBalance() : "null");
      }
    } catch (Exception e) {
      logger.warn("Failed to record account change", e);
    }
  }
  
  /**
   * Get current state changes without finalizing the journal.
   * 
   * @return List of current state changes (snapshot)
   */
  public List<StateChange> getCurrentChanges() {
    synchronized (lock) {
      List<StateChange> result = new ArrayList<>();
      
      // Add storage changes
      for (StorageChangeEntry entry : storageChanges.values()) {
        StateChange change = new StateChange(
            entry.address,
            entry.key,
            entry.oldValue,
            entry.newValue
        );
        result.add(change);
      }
      
      // Add account changes (using empty key to distinguish from storage)
      for (AccountChangeEntry entry : accountChanges.values()) {
        byte[] oldAccountBytes = serializeAccountInfo(entry.oldAccount);
        byte[] newAccountBytes = serializeAccountInfo(entry.newAccount);
        
        StateChange change = new StateChange(
            entry.address,
            new byte[0], // Empty key indicates account change
            oldAccountBytes,
            newAccountBytes
        );
        result.add(change);
      }
      
      logger.debug("Retrieved current changes: {} storage, {} account, {} total",
                   storageChanges.size(), accountChanges.size(), result.size());
      
      return result;
    }
  }
  
  /**
   * Finalize the journal and convert to StateChange list.
   * 
   * @return List of state changes for CSV logging
   */
  public List<StateChange> finalizeChanges() {
    synchronized (lock) {
      if (finalized) {
        throw new IllegalStateException("Journal already finalized");
      }
      finalized = true;
      
      List<StateChange> result = new ArrayList<>();
      
      // Add storage changes
      for (StorageChangeEntry entry : storageChanges.values()) {
        StateChange change = new StateChange(
            entry.address,
            entry.key,
            entry.oldValue,
            entry.newValue
        );
        result.add(change);
      }
      
      // Add account changes (using empty key to distinguish from storage)
      for (AccountChangeEntry entry : accountChanges.values()) {
        byte[] oldAccountBytes = serializeAccountInfo(entry.oldAccount);
        byte[] newAccountBytes = serializeAccountInfo(entry.newAccount);
        
        StateChange change = new StateChange(
            entry.address,
            new byte[0], // Empty key indicates account change
            oldAccountBytes,
            newAccountBytes
        );
        result.add(change);
      }
      
      logger.info("Finalized journal: {} storage changes, {} account changes, {} total",
                  storageChanges.size(), accountChanges.size(), result.size());
      
      return result;
    }
  }
  
  /**
   * Get current change counts for metrics.
   */
  public int getStorageChangeCount() {
    synchronized (lock) {
      return storageChanges.size();
    }
  }
  
  public int getAccountChangeCount() {
    synchronized (lock) {
      return accountChanges.size();
    }
  }
  
  /**
   * Clear journal state (for testing).
   */
  public void clear() {
    synchronized (lock) {
      storageChanges.clear();
      accountChanges.clear();
      finalized = false;
    }
  }
  
  /**
   * Serialize AccountCapsule to byte array aligned with remote format.
   * Format: [balance(32)] + [create_time(8)] + [code_length(4)] + [code(variable)]
   * Note: TRON accounts don't have nonce or code hash, using create_time instead
   */
  private byte[] serializeAccountInfo(AccountCapsule account) {
    if (account == null) {
      return new byte[0];
    }
    
    try {
      // Get account data
      long balance = account.getBalance();
      long createTime = account.getInstance().getCreateTime();
      byte[] code = account.getInstance().getCode().toByteArray();
      
      // Calculate total size
      int totalSize = 32 + 8 + 4 + (code != null ? code.length : 0);
      byte[] result = new byte[totalSize];
      int offset = 0;
      
      // Balance (32 bytes, big-endian)
      byte[] balanceBytes = longToBytes32(balance);
      System.arraycopy(balanceBytes, 0, result, offset, 32);
      offset += 32;
      
      // Create time (8 bytes, big-endian)
      byte[] createTimeBytes = longToBytes8(createTime);
      System.arraycopy(createTimeBytes, 0, result, offset, 8);
      offset += 8;
      
      // Code length (4 bytes, big-endian)
      int codeLength = code != null ? code.length : 0;
      result[offset++] = (byte) (codeLength >>> 24);
      result[offset++] = (byte) (codeLength >>> 16);
      result[offset++] = (byte) (codeLength >>> 8);
      result[offset++] = (byte) codeLength;
      
      // Code (variable length)
      if (code != null && code.length > 0) {
        System.arraycopy(code, 0, result, offset, code.length);
      }
      
      return result;
    } catch (Exception e) {
      logger.warn("Failed to serialize account info", e);
      return new byte[0];
    }
  }
  
  private byte[] longToBytes32(long value) {
    byte[] bytes = new byte[32];
    // Store as big-endian in the last 8 bytes
    for (int i = 0; i < 8; i++) {
      bytes[31 - i] = (byte) (value >>> (i * 8));
    }
    return bytes;
  }
  
  private byte[] longToBytes8(long value) {
    byte[] bytes = new byte[8];
    for (int i = 0; i < 8; i++) {
      bytes[7 - i] = (byte) (value >>> (i * 8));
    }
    return bytes;
  }
  
  /**
   * Internal storage change entry.
   */
  private static class StorageChangeEntry {
    byte[] address;
    byte[] key;
    byte[] oldValue;
    byte[] newValue;
  }
  
  /**
   * Internal account change entry.
   */
  private static class AccountChangeEntry {
    byte[] address;
    AccountCapsule oldAccount;
    AccountCapsule newAccount;
  }
}