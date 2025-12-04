package org.tron.core.execution.reporting;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.utils.ByteArray;
import org.tron.core.execution.reporting.DomainCanonicalizer.Trc10BalanceDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.Trc10IssuanceDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.VoteDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.FreezeDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.GlobalResourceDelta;

/**
 * Per-transaction journal for capturing domain-specific changes during embedded execution.
 *
 * <p>Tracks: TRC-10 balances, TRC-10 issuance, votes, freezes, and global resources.
 * Provides deduplication and merge logic to handle multiple updates within a transaction.
 *
 * <p>Usage pattern:
 * <pre>
 *   DomainChangeJournal journal = new DomainChangeJournal();
 *   // During execution...
 *   journal.recordTrc10BalanceChange(owner, tokenId, oldBalance, newBalance);
 *   journal.recordVoteChange(voter, witness, oldVotes, newVotes);
 *   // After execution...
 *   List&lt;Trc10BalanceDelta&gt; trc10Changes = journal.getTrc10BalanceChanges();
 * </pre>
 */
public class DomainChangeJournal {

  private static final Logger logger = LoggerFactory.getLogger(DomainChangeJournal.class);

  // TRC-10 balance changes: (tokenId + ownerAddress) -> entry
  private final Map<String, Trc10BalanceEntry> trc10BalanceChanges = new HashMap<>();

  // TRC-10 issuance changes: (tokenId + field) -> entry
  private final Map<String, Trc10IssuanceEntry> trc10IssuanceChanges = new HashMap<>();

  // Vote changes: (voterAddress + witnessAddress) -> entry
  private final Map<String, VoteEntry> voteChanges = new HashMap<>();

  // Freeze changes: (ownerAddress + resourceType + recipientAddress) -> entry
  private final Map<String, FreezeEntry> freezeChanges = new HashMap<>();

  // Global resource changes: field -> entry
  private final Map<String, GlobalResourceEntry> globalResourceChanges = new HashMap<>();

  private final Object lock = new Object();
  private volatile boolean finalized = false;

  /**
   * Check if domain change collection is enabled.
   * Uses the same flag as StateChangeJournal for unified control.
   */
  public static boolean isEnabled() {
    String enabledProp = System.getProperty("exec.csv.stateChanges.enabled", "false");
    return "true".equalsIgnoreCase(enabledProp);
  }

  // ================================
  // TRC-10 Balance Changes
  // ================================

  /**
   * Record a TRC-10 balance change.
   *
   * @param ownerAddress Account address
   * @param tokenId Token ID as string
   * @param oldBalance Previous balance
   * @param newBalance New balance
   */
  public void recordTrc10BalanceChange(byte[] ownerAddress, String tokenId,
                                       long oldBalance, long newBalance) {
    if (!isEnabled() || finalized) {
      return;
    }

    try {
      String addressHex = ByteArray.toHexString(ownerAddress).toLowerCase();
      String compositeKey = tokenId + ":" + addressHex;

      synchronized (lock) {
        Trc10BalanceEntry existing = trc10BalanceChanges.get(compositeKey);
        if (existing != null) {
          // Merge: keep original old value, update new value
          existing.newBalance = newBalance;
        } else {
          // New entry
          Trc10BalanceEntry entry = new Trc10BalanceEntry();
          entry.ownerAddress = ownerAddress.clone();
          entry.tokenId = tokenId;
          entry.oldBalance = oldBalance;
          entry.newBalance = newBalance;
          trc10BalanceChanges.put(compositeKey, entry);
        }
      }

      if (logger.isDebugEnabled()) {
        logger.debug("Recorded TRC-10 balance change: token={} owner={} old={} new={}",
                     tokenId, addressHex, oldBalance, newBalance);
      }
    } catch (Exception e) {
      logger.warn("Failed to record TRC-10 balance change", e);
    }
  }

  // ================================
  // TRC-10 Issuance Changes
  // ================================

  /**
   * Record a TRC-10 issuance metadata change.
   *
   * @param tokenId Token ID as string
   * @param field Field name (e.g., total_supply, description)
   * @param oldValue Previous value
   * @param newValue New value
   * @param op Operation: create, update, delete
   */
  public void recordTrc10IssuanceChange(String tokenId, String field,
                                        String oldValue, String newValue, String op) {
    if (!isEnabled() || finalized) {
      return;
    }

    try {
      String compositeKey = tokenId + ":" + field;

      synchronized (lock) {
        Trc10IssuanceEntry existing = trc10IssuanceChanges.get(compositeKey);
        if (existing != null) {
          // Merge: keep original old value, update new value
          existing.newValue = newValue;
        } else {
          // New entry
          Trc10IssuanceEntry entry = new Trc10IssuanceEntry();
          entry.tokenId = tokenId;
          entry.field = field;
          entry.oldValue = oldValue;
          entry.newValue = newValue;
          entry.op = op;
          trc10IssuanceChanges.put(compositeKey, entry);
        }
      }

      if (logger.isDebugEnabled()) {
        logger.debug("Recorded TRC-10 issuance change: token={} field={} op={} old={} new={}",
                     tokenId, field, op, oldValue, newValue);
      }
    } catch (Exception e) {
      logger.warn("Failed to record TRC-10 issuance change", e);
    }
  }

  // ================================
  // Vote Changes
  // ================================

  /**
   * Record a vote change.
   *
   * @param voterAddress Voter account address
   * @param witnessAddress Witness address being voted for
   * @param oldVotes Previous vote count
   * @param newVotes New vote count
   */
  public void recordVoteChange(byte[] voterAddress, byte[] witnessAddress,
                               long oldVotes, long newVotes) {
    if (!isEnabled() || finalized) {
      return;
    }

    try {
      String voterHex = ByteArray.toHexString(voterAddress).toLowerCase();
      String witnessHex = ByteArray.toHexString(witnessAddress).toLowerCase();
      String compositeKey = voterHex + ":" + witnessHex;

      synchronized (lock) {
        VoteEntry existing = voteChanges.get(compositeKey);
        if (existing != null) {
          // Merge: keep original old value, update new value
          existing.newVotes = newVotes;
        } else {
          // New entry
          VoteEntry entry = new VoteEntry();
          entry.voterAddress = voterAddress.clone();
          entry.witnessAddress = witnessAddress.clone();
          entry.oldVotes = oldVotes;
          entry.newVotes = newVotes;
          voteChanges.put(compositeKey, entry);
        }
      }

      if (logger.isDebugEnabled()) {
        logger.debug("Recorded vote change: voter={} witness={} old={} new={}",
                     voterHex, witnessHex, oldVotes, newVotes);
      }
    } catch (Exception e) {
      logger.warn("Failed to record vote change", e);
    }
  }

  // ================================
  // Freeze Changes
  // ================================

  /**
   * Record a freeze/unfreeze change.
   *
   * @param ownerAddress Owner account address
   * @param resourceType Resource type: BANDWIDTH, ENERGY, or TRON_POWER
   * @param recipientAddress Recipient address (null if self-freeze)
   * @param oldAmount Previous frozen amount in SUN
   * @param newAmount New frozen amount in SUN
   * @param oldExpireTime Previous expiration timestamp (ms)
   * @param newExpireTime New expiration timestamp (ms)
   * @param op Operation: freeze, unfreeze, update
   */
  public void recordFreezeChange(byte[] ownerAddress, String resourceType,
                                 byte[] recipientAddress,
                                 long oldAmount, long newAmount,
                                 long oldExpireTime, long newExpireTime,
                                 String op) {
    if (!isEnabled() || finalized) {
      return;
    }

    try {
      String ownerHex = ByteArray.toHexString(ownerAddress).toLowerCase();
      String recipientHex = recipientAddress != null
          ? ByteArray.toHexString(recipientAddress).toLowerCase() : "";
      String compositeKey = ownerHex + ":" + resourceType + ":" + recipientHex;

      synchronized (lock) {
        FreezeEntry existing = freezeChanges.get(compositeKey);
        if (existing != null) {
          // Merge: keep original old values, update new values
          existing.newAmount = newAmount;
          existing.newExpireTime = newExpireTime;
        } else {
          // New entry
          FreezeEntry entry = new FreezeEntry();
          entry.ownerAddress = ownerAddress.clone();
          entry.resourceType = resourceType;
          entry.recipientAddress = recipientAddress != null ? recipientAddress.clone() : null;
          entry.oldAmount = oldAmount;
          entry.newAmount = newAmount;
          entry.oldExpireTime = oldExpireTime;
          entry.newExpireTime = newExpireTime;
          entry.op = op;
          freezeChanges.put(compositeKey, entry);
        }
      }

      if (logger.isDebugEnabled()) {
        logger.debug("Recorded freeze change: owner={} resource={} op={} oldAmt={} newAmt={}",
                     ownerHex, resourceType, op, oldAmount, newAmount);
      }
    } catch (Exception e) {
      logger.warn("Failed to record freeze change", e);
    }
  }

  // ================================
  // Global Resource Changes
  // ================================

  /**
   * Record a global resource change.
   *
   * @param field Field name (e.g., total_energy_limit, total_net_weight)
   * @param oldValue Previous value
   * @param newValue New value
   */
  public void recordGlobalResourceChange(String field, long oldValue, long newValue) {
    if (!isEnabled() || finalized) {
      return;
    }

    try {
      synchronized (lock) {
        GlobalResourceEntry existing = globalResourceChanges.get(field);
        if (existing != null) {
          // Merge: keep original old value, update new value
          existing.newValue = newValue;
        } else {
          // New entry
          GlobalResourceEntry entry = new GlobalResourceEntry();
          entry.field = field;
          entry.oldValue = oldValue;
          entry.newValue = newValue;
          globalResourceChanges.put(field, entry);
        }
      }

      if (logger.isDebugEnabled()) {
        logger.debug("Recorded global resource change: field={} old={} new={}",
                     field, oldValue, newValue);
      }
    } catch (Exception e) {
      logger.warn("Failed to record global resource change", e);
    }
  }

  // ================================
  // Getters for Domain Changes
  // ================================

  /**
   * Get TRC-10 balance changes as delta list.
   */
  public List<Trc10BalanceDelta> getTrc10BalanceChanges() {
    synchronized (lock) {
      List<Trc10BalanceDelta> deltas = new ArrayList<>();
      for (Trc10BalanceEntry entry : trc10BalanceChanges.values()) {
        Trc10BalanceDelta delta = new Trc10BalanceDelta();
        delta.setTokenId(entry.tokenId);
        delta.setOwnerAddressHex(ByteArray.toHexString(entry.ownerAddress));
        delta.setOldBalance(String.valueOf(entry.oldBalance));
        delta.setNewBalance(String.valueOf(entry.newBalance));
        // Determine op based on old/new values
        if (entry.oldBalance == 0 && entry.newBalance > 0) {
          delta.setOp("increase");
        } else if (entry.oldBalance > 0 && entry.newBalance == 0) {
          delta.setOp("delete");
        } else if (entry.newBalance > entry.oldBalance) {
          delta.setOp("increase");
        } else if (entry.newBalance < entry.oldBalance) {
          delta.setOp("decrease");
        } else {
          delta.setOp("set");
        }
        deltas.add(delta);
      }
      return deltas;
    }
  }

  /**
   * Get TRC-10 issuance changes as delta list.
   */
  public List<Trc10IssuanceDelta> getTrc10IssuanceChanges() {
    synchronized (lock) {
      List<Trc10IssuanceDelta> deltas = new ArrayList<>();
      for (Trc10IssuanceEntry entry : trc10IssuanceChanges.values()) {
        Trc10IssuanceDelta delta = new Trc10IssuanceDelta();
        delta.setTokenId(entry.tokenId);
        delta.setField(entry.field);
        delta.setOldValue(entry.oldValue);
        delta.setNewValue(entry.newValue);
        delta.setOp(entry.op);
        deltas.add(delta);
      }
      return deltas;
    }
  }

  /**
   * Get vote changes as delta list.
   */
  public List<VoteDelta> getVoteChanges() {
    synchronized (lock) {
      List<VoteDelta> deltas = new ArrayList<>();
      for (VoteEntry entry : voteChanges.values()) {
        VoteDelta delta = new VoteDelta();
        delta.setVoterAddressHex(ByteArray.toHexString(entry.voterAddress));
        delta.setWitnessAddressHex(ByteArray.toHexString(entry.witnessAddress));
        delta.setOldVotes(String.valueOf(entry.oldVotes));
        delta.setNewVotes(String.valueOf(entry.newVotes));
        // Determine op based on old/new values
        if (entry.oldVotes == 0 && entry.newVotes > 0) {
          delta.setOp("set");
        } else if (entry.oldVotes > 0 && entry.newVotes == 0) {
          delta.setOp("delete");
        } else if (entry.newVotes > entry.oldVotes) {
          delta.setOp("increase");
        } else if (entry.newVotes < entry.oldVotes) {
          delta.setOp("decrease");
        } else {
          delta.setOp("set");
        }
        deltas.add(delta);
      }
      return deltas;
    }
  }

  /**
   * Get freeze changes as delta list.
   */
  public List<FreezeDelta> getFreezeChanges() {
    synchronized (lock) {
      List<FreezeDelta> deltas = new ArrayList<>();
      for (FreezeEntry entry : freezeChanges.values()) {
        FreezeDelta delta = new FreezeDelta();
        delta.setOwnerAddressHex(ByteArray.toHexString(entry.ownerAddress));
        delta.setResourceType(entry.resourceType);
        if (entry.recipientAddress != null) {
          delta.setRecipientAddressHex(ByteArray.toHexString(entry.recipientAddress));
        }
        delta.setOldAmountSun(String.valueOf(entry.oldAmount));
        delta.setNewAmountSun(String.valueOf(entry.newAmount));
        delta.setOldExpireTimeMs(String.valueOf(entry.oldExpireTime));
        delta.setNewExpireTimeMs(String.valueOf(entry.newExpireTime));
        delta.setOp(entry.op);
        deltas.add(delta);
      }
      return deltas;
    }
  }

  /**
   * Get global resource changes as delta list.
   */
  public List<GlobalResourceDelta> getGlobalResourceChanges() {
    synchronized (lock) {
      List<GlobalResourceDelta> deltas = new ArrayList<>();
      for (GlobalResourceEntry entry : globalResourceChanges.values()) {
        GlobalResourceDelta delta = new GlobalResourceDelta();
        delta.setField(entry.field);
        delta.setOldValue(String.valueOf(entry.oldValue));
        delta.setNewValue(String.valueOf(entry.newValue));
        delta.setOp("update");
        deltas.add(delta);
      }
      return deltas;
    }
  }

  // ================================
  // Count Getters for Metrics
  // ================================

  public int getTrc10BalanceChangeCount() {
    synchronized (lock) {
      return trc10BalanceChanges.size();
    }
  }

  public int getTrc10IssuanceChangeCount() {
    synchronized (lock) {
      return trc10IssuanceChanges.size();
    }
  }

  public int getVoteChangeCount() {
    synchronized (lock) {
      return voteChanges.size();
    }
  }

  public int getFreezeChangeCount() {
    synchronized (lock) {
      return freezeChanges.size();
    }
  }

  public int getGlobalResourceChangeCount() {
    synchronized (lock) {
      return globalResourceChanges.size();
    }
  }

  /**
   * Clear journal state.
   */
  public void clear() {
    synchronized (lock) {
      trc10BalanceChanges.clear();
      trc10IssuanceChanges.clear();
      voteChanges.clear();
      freezeChanges.clear();
      globalResourceChanges.clear();
      finalized = false;
    }
  }

  /**
   * Mark journal as finalized (no more changes allowed).
   */
  public void markFinalized() {
    synchronized (lock) {
      finalized = true;
      logger.info("Finalized DomainChangeJournal: {} TRC-10 balance, {} TRC-10 issuance, " +
                  "{} votes, {} freezes, {} global resources",
                  trc10BalanceChanges.size(), trc10IssuanceChanges.size(),
                  voteChanges.size(), freezeChanges.size(), globalResourceChanges.size());
    }
  }

  /**
   * Check if journal is finalized.
   */
  public boolean isFinalized() {
    return finalized;
  }

  // ================================
  // Internal Entry Classes
  // ================================

  private static class Trc10BalanceEntry {
    byte[] ownerAddress;
    String tokenId;
    long oldBalance;
    long newBalance;
  }

  private static class Trc10IssuanceEntry {
    String tokenId;
    String field;
    String oldValue;
    String newValue;
    String op;
  }

  private static class VoteEntry {
    byte[] voterAddress;
    byte[] witnessAddress;
    long oldVotes;
    long newVotes;
  }

  private static class FreezeEntry {
    byte[] ownerAddress;
    String resourceType;
    byte[] recipientAddress; // null if self-freeze
    long oldAmount;
    long newAmount;
    long oldExpireTime;
    long newExpireTime;
    String op;
  }

  private static class GlobalResourceEntry {
    String field;
    long oldValue;
    long newValue;
  }
}
