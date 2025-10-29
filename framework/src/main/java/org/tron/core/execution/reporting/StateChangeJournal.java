package org.tron.core.execution.reporting;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.bouncycastle.crypto.digests.KeccakDigest;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.common.utils.ByteArray;

import static org.tron.protos.contract.Common.ResourceCode.BANDWIDTH;
import static org.tron.protos.contract.Common.ResourceCode.ENERGY;

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
   * Format: [balance(32)] + [nonce(8)] + [code_hash(32)] + [code_length(4)] + [code(variable)]
   *         + optional [AEXT tail] for resource usage
   *
   * AEXT tail format (v1):
   * - magic: "AEXT" (4 bytes)
   * - version: 1 (u16 big-endian, 2 bytes)
   * - length: 68 (u16 big-endian, 2 bytes)
   * - payload: resource usage fields (68 bytes)
   *
   * Notes:
   * - TRON accounts don't use Ethereum nonce; we emit 0.
   * - code_hash is Keccak-256 of the contract code bytes (empty code => KECCAK_EMPTY).
   * - AEXT tail is controlled by system property: remote.exec.accountinfo.resources.enabled (default: true)
   */
  private byte[] serializeAccountInfo(AccountCapsule account) {
    if (account == null) {
      return new byte[0];
    }

    try {
      // Get account data
      long balance = account.getBalance();
      // TRON does not use Ethereum-style nonces for accounts; emit 0
      long nonce = 0L;
      byte[] code = account.getInstance().getCode().toByteArray();
      byte[] codeHash = keccak256(code != null ? code : new byte[0]);

      // Calculate base size: balance(32) + nonce(8) + code_hash(32) + code_len(4) + code
      int codeLength = (code != null ? code.length : 0);
      int baseSize = 32 + 8 + 32 + 4 + codeLength;

      // Check if AEXT tail should be appended (default true)
      boolean includeResourceUsage = Boolean.parseBoolean(
          System.getProperty("remote.exec.accountinfo.resources.enabled", "true"));

      int totalSize = baseSize;
      byte[] aextTail = null;

      if (includeResourceUsage) {
        try {
          aextTail = serializeAextTail(account);
          totalSize += aextTail.length;
        } catch (Exception e) {
          logger.warn("Failed to serialize AEXT tail, falling back to base format: {}", e.getMessage());
          // Continue with base format only
        }
      }

      byte[] result = new byte[totalSize];
      int offset = 0;

      // Balance (32 bytes, big-endian)
      byte[] balanceBytes = longToBytes32(balance);
      System.arraycopy(balanceBytes, 0, result, offset, 32);
      offset += 32;

      // Nonce (8 bytes, big-endian)
      byte[] nonceBytes = longToBytes8(nonce);
      System.arraycopy(nonceBytes, 0, result, offset, 8);
      offset += 8;

      // Code hash (32 bytes)
      System.arraycopy(codeHash, 0, result, offset, 32);
      offset += 32;

      // Code length (4 bytes, big-endian)
      result[offset++] = (byte) (codeLength >>> 24);
      result[offset++] = (byte) (codeLength >>> 16);
      result[offset++] = (byte) (codeLength >>> 8);
      result[offset++] = (byte) codeLength;

      // Code (variable length)
      if (code != null && code.length > 0) {
        System.arraycopy(code, 0, result, offset, code.length);
        offset += code.length;
      }

      // Append AEXT tail if present
      if (aextTail != null && aextTail.length > 0) {
        System.arraycopy(aextTail, 0, result, offset, aextTail.length);
        logger.debug("Appended AEXT tail ({} bytes) to account serialization", aextTail.length);
      }

      return result;
    } catch (Exception e) {
      logger.warn("Failed to serialize account info", e);
      return new byte[0];
    }
  }

  /**
   * Serialize AEXT (Account EXTension) v1 tail with resource usage fields.
   * Format: magic(4) + version(2) + length(2) + payload(68)
   * Total: 76 bytes
   */
  private byte[] serializeAextTail(AccountCapsule account) {
    // AEXT v1 payload size: 8*8 (i64 fields) + 1 + 1 (booleans) + 2 (padding) = 68 bytes
    int payloadSize = 68;
    int totalSize = 4 + 2 + 2 + payloadSize; // magic + version + length + payload = 76 bytes
    byte[] result = new byte[totalSize];
    int offset = 0;

    // Magic: "AEXT" (0x41 0x45 0x58 0x54)
    result[offset++] = 0x41; // 'A'
    result[offset++] = 0x45; // 'E'
    result[offset++] = 0x58; // 'X'
    result[offset++] = 0x54; // 'T'

    // Version: 1 (u16 big-endian)
    result[offset++] = 0x00;
    result[offset++] = 0x01;

    // Length: 68 (u16 big-endian)
    result[offset++] = 0x00;
    result[offset++] = 0x44; // 0x44 = 68 in decimal

    // Payload: resource usage fields (all i64 big-endian except booleans)
    // netUsage (8 bytes)
    long netUsage = account.getNetUsage();
    offset = writeI64BigEndian(result, offset, netUsage);

    // freeNetUsage (8 bytes)
    long freeNetUsage = account.getFreeNetUsage();
    offset = writeI64BigEndian(result, offset, freeNetUsage);

    // energyUsage (8 bytes)
    long energyUsage = account.getEnergyUsage();
    offset = writeI64BigEndian(result, offset, energyUsage);

    // latestConsumeTime (8 bytes)
    long latestConsumeTime = account.getLatestConsumeTime();
    offset = writeI64BigEndian(result, offset, latestConsumeTime);

    // latestConsumeFreeTime (8 bytes)
    long latestConsumeFreeTime = account.getLatestConsumeFreeTime();
    offset = writeI64BigEndian(result, offset, latestConsumeFreeTime);

    // latestConsumeTimeForEnergy (8 bytes)
    long latestConsumeTimeForEnergy = account.getAccountResource().getLatestConsumeTimeForEnergy();
    offset = writeI64BigEndian(result, offset, latestConsumeTimeForEnergy);

    // netWindowSize (8 bytes) - use getWindowSize for logical units
    long netWindowSize = account.getWindowSize(BANDWIDTH);
    offset = writeI64BigEndian(result, offset, netWindowSize);

    // energyWindowSize (8 bytes)
    long energyWindowSize = account.getWindowSize(ENERGY);
    offset = writeI64BigEndian(result, offset, energyWindowSize);

    // netWindowOptimized (1 byte boolean)
    boolean netWindowOptimized = account.getWindowOptimized(BANDWIDTH);
    result[offset++] = (byte) (netWindowOptimized ? 0x01 : 0x00);

    // energyWindowOptimized (1 byte boolean)
    boolean energyWindowOptimized = account.getWindowOptimized(ENERGY);
    result[offset++] = (byte) (energyWindowOptimized ? 0x01 : 0x00);

    // Reserved/padding (2 bytes)
    result[offset++] = 0x00;
    result[offset++] = 0x00;

    logger.debug("Serialized AEXT v1: netUsage={}, freeNetUsage={}, energyUsage={}, times=[{},{},{}], windows=[{},{}], optimized=[{},{}]",
                 netUsage, freeNetUsage, energyUsage,
                 latestConsumeTime, latestConsumeFreeTime, latestConsumeTimeForEnergy,
                 netWindowSize, energyWindowSize,
                 netWindowOptimized, energyWindowOptimized);

    return result;
  }

  /**
   * Write an i64 value in big-endian format to the byte array.
   * Returns the new offset after writing.
   */
  private int writeI64BigEndian(byte[] buffer, int offset, long value) {
    for (int i = 7; i >= 0; i--) {
      buffer[offset++] = (byte) (value >>> (i * 8));
    }
    return offset;
  }

  private byte[] keccak256(byte[] data) {
    KeccakDigest digest = new KeccakDigest(256);
    if (data != null && data.length > 0) {
      digest.update(data, 0, data.length);
    }
    byte[] out = new byte[32];
    digest.doFinal(out, 0);
    return out;
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
