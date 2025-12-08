package org.tron.core.execution.reporting;

import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.utils.ByteArray;
import org.tron.protos.Protocol.Vote;

/**
 * Thread-local registry for pre-state snapshots used in remote execution mode.
 *
 * <p>In remote execution, the Rust backend applies state changes and returns deltas.
 * To compute absolute old/new values for CSV reporting, we need to capture the
 * pre-execution state before applying those changes.
 *
 * <p>This registry captures:
 * <ul>
 *   <li>TRC-10 balances per (owner, token_id)</li>
 *   <li>Votes per (voter, witness)</li>
 *   <li>Global resource totals from DynamicPropertiesStore</li>
 * </ul>
 *
 * <p>Usage pattern:
 * <pre>
 *   // Before applying remote execution results
 *   PreStateSnapshotRegistry.initializeForCurrentTransaction();
 *   PreStateSnapshotRegistry.captureTrc10Balance(ownerAddr, tokenId, balance);
 *   PreStateSnapshotRegistry.captureVote(voterAddr, witnessAddr, voteCount);
 *   PreStateSnapshotRegistry.captureGlobalTotals(netWeight, netLimit, energyWeight, energyLimit);
 *
 *   // During CSV record building
 *   Long oldBalance = PreStateSnapshotRegistry.getTrc10Balance(ownerAddr, tokenId);
 *   Long oldVotes = PreStateSnapshotRegistry.getVote(voterAddr, witnessAddr);
 *   GlobalSnapshot globals = PreStateSnapshotRegistry.getGlobalTotals();
 *
 *   // After CSV logging
 *   PreStateSnapshotRegistry.clearForCurrentTransaction();
 * </pre>
 */
public class PreStateSnapshotRegistry {

  private static final Logger logger = LoggerFactory.getLogger(PreStateSnapshotRegistry.class);

  private static final ThreadLocal<PreStateSnapshot> snapshotThreadLocal = new ThreadLocal<>();

  /**
   * Container for pre-state snapshots.
   */
  public static class PreStateSnapshot {
    // Key: addressHex + ":" + tokenId -> balance
    private final Map<String, Long> trc10Balances = new HashMap<>();

    // Key: voterHex + ":" + witnessHex -> voteCount
    private final Map<String, Long> votes = new HashMap<>();

    // Key: ownerHex + ":" + resourceType + ":" + recipientHex -> FreezeSnapshot
    private final Map<String, FreezeSnapshot> freezes = new HashMap<>();

    // Key: addressHex -> AccountFrozenTotals (per-account frozen sums for limit computation)
    private final Map<String, AccountFrozenTotals> accountFrozenTotals = new HashMap<>();

    // Global resource totals
    private long totalNetWeight;
    private long totalNetLimit;
    private long totalEnergyWeight;
    private long totalEnergyLimit;
    private long totalTronPowerWeight;
    private boolean globalsInitialized = false;

    public void clear() {
      trc10Balances.clear();
      votes.clear();
      freezes.clear();
      accountFrozenTotals.clear();
      globalsInitialized = false;
    }
  }

  /**
   * Global resource totals snapshot.
   */
  public static class GlobalSnapshot {
    private final long totalNetWeight;
    private final long totalNetLimit;
    private final long totalEnergyWeight;
    private final long totalEnergyLimit;
    private final long totalTronPowerWeight;

    public GlobalSnapshot(long totalNetWeight, long totalNetLimit,
                          long totalEnergyWeight, long totalEnergyLimit,
                          long totalTronPowerWeight) {
      this.totalNetWeight = totalNetWeight;
      this.totalNetLimit = totalNetLimit;
      this.totalEnergyWeight = totalEnergyWeight;
      this.totalEnergyLimit = totalEnergyLimit;
      this.totalTronPowerWeight = totalTronPowerWeight;
    }

    public long getTotalNetWeight() {
      return totalNetWeight;
    }

    public long getTotalNetLimit() {
      return totalNetLimit;
    }

    public long getTotalEnergyWeight() {
      return totalEnergyWeight;
    }

    public long getTotalEnergyLimit() {
      return totalEnergyLimit;
    }

    public long getTotalTronPowerWeight() {
      return totalTronPowerWeight;
    }
  }

  /**
   * Freeze snapshot for a specific (owner, resource, recipient) tuple.
   */
  public static class FreezeSnapshot {
    private final long amount;
    private final long expireTimeMs;

    public FreezeSnapshot(long amount, long expireTimeMs) {
      this.amount = amount;
      this.expireTimeMs = expireTimeMs;
    }

    public long getAmount() {
      return amount;
    }

    public long getExpireTimeMs() {
      return expireTimeMs;
    }
  }

  /**
   * Per-account frozen totals for computing net_limit and energy_limit.
   * These are the inputs to BandwidthProcessor.calculateGlobalNetLimit() and
   * EnergyProcessor.calculateGlobalEnergyLimit().
   */
  public static class AccountFrozenTotals {
    private final long frozenForBandwidth;
    private final long frozenForEnergy;

    public AccountFrozenTotals(long frozenForBandwidth, long frozenForEnergy) {
      this.frozenForBandwidth = frozenForBandwidth;
      this.frozenForEnergy = frozenForEnergy;
    }

    public long getFrozenForBandwidth() {
      return frozenForBandwidth;
    }

    public long getFrozenForEnergy() {
      return frozenForEnergy;
    }
  }

  // ================================
  // Lifecycle Management
  // ================================

  /**
   * Initialize a new snapshot for the current transaction thread.
   * Should be called once at the beginning of each transaction before applying remote results.
   */
  public static void initializeForCurrentTransaction() {
    PreStateSnapshot existing = snapshotThreadLocal.get();
    if (existing != null) {
      logger.warn("PreStateSnapshot already exists for current thread, clearing previous snapshot");
      existing.clear();
    }

    PreStateSnapshot snapshot = new PreStateSnapshot();
    snapshotThreadLocal.set(snapshot);

    if (logger.isDebugEnabled()) {
      logger.debug("Initialized PreStateSnapshot for transaction thread {}",
                   Thread.currentThread().getId());
    }
  }

  /**
   * Clear snapshot for current thread.
   * Should be called after CSV logging is complete.
   */
  public static void clearForCurrentTransaction() {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      snapshot.clear();
      snapshotThreadLocal.remove();
      if (logger.isDebugEnabled()) {
        logger.debug("Cleared PreStateSnapshot for transaction thread {}",
                     Thread.currentThread().getId());
      }
    }
  }

  /**
   * Check if a snapshot is active for the current thread.
   */
  public static boolean hasActiveSnapshot() {
    return snapshotThreadLocal.get() != null;
  }

  // ================================
  // TRC-10 Balance Capture
  // ================================

  /**
   * Capture a TRC-10 balance for later lookup.
   *
   * @param ownerAddress owner address bytes
   * @param tokenId token ID string
   * @param balance pre-execution balance
   */
  public static void captureTrc10Balance(byte[] ownerAddress, String tokenId, long balance) {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      String key = makeKey(ownerAddress, tokenId);
      snapshot.trc10Balances.put(key, balance);
      if (logger.isTraceEnabled()) {
        logger.trace("Captured TRC-10 balance: owner={}, token={}, balance={}",
                     ByteArray.toHexString(ownerAddress), tokenId, balance);
      }
    }
  }

  /**
   * Get captured TRC-10 balance for an owner/token pair.
   *
   * @param ownerAddress owner address bytes
   * @param tokenId token ID string
   * @return pre-execution balance, or null if not captured
   */
  public static Long getTrc10Balance(byte[] ownerAddress, String tokenId) {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      String key = makeKey(ownerAddress, tokenId);
      return snapshot.trc10Balances.get(key);
    }
    return null;
  }

  /**
   * Get all captured TRC-10 balances.
   */
  public static Map<String, Long> getAllTrc10Balances() {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      return Collections.unmodifiableMap(snapshot.trc10Balances);
    }
    return Collections.emptyMap();
  }

  // ================================
  // Vote Capture
  // ================================

  /**
   * Capture a vote for later lookup.
   *
   * @param voterAddress voter address bytes
   * @param witnessAddress witness address bytes
   * @param voteCount pre-execution vote count
   */
  public static void captureVote(byte[] voterAddress, byte[] witnessAddress, long voteCount) {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      String key = makeVoteKey(voterAddress, witnessAddress);
      snapshot.votes.put(key, voteCount);
      if (logger.isTraceEnabled()) {
        logger.trace("Captured vote: voter={}, witness={}, votes={}",
                     ByteArray.toHexString(voterAddress),
                     ByteArray.toHexString(witnessAddress), voteCount);
      }
    }
  }

  /**
   * Capture all votes for a voter from their vote list.
   *
   * @param voterAddress voter address bytes
   * @param votesList list of votes from Account.getVotesList()
   */
  public static void captureVotes(byte[] voterAddress, List<Vote> votesList) {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null && votesList != null) {
      for (Vote vote : votesList) {
        byte[] witnessAddr = vote.getVoteAddress().toByteArray();
        String key = makeVoteKey(voterAddress, witnessAddr);
        snapshot.votes.put(key, vote.getVoteCount());
        if (logger.isTraceEnabled()) {
          logger.trace("Captured vote: voter={}, witness={}, votes={}",
                       ByteArray.toHexString(voterAddress),
                       ByteArray.toHexString(witnessAddr), vote.getVoteCount());
        }
      }
    }
  }

  /**
   * Get captured vote count for a voter/witness pair.
   *
   * @param voterAddress voter address bytes
   * @param witnessAddress witness address bytes
   * @return pre-execution vote count, or null if not captured
   */
  public static Long getVote(byte[] voterAddress, byte[] witnessAddress) {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      String key = makeVoteKey(voterAddress, witnessAddress);
      return snapshot.votes.get(key);
    }
    return null;
  }

  /**
   * Get all captured votes.
   */
  public static Map<String, Long> getAllVotes() {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      return Collections.unmodifiableMap(snapshot.votes);
    }
    return Collections.emptyMap();
  }

  // ================================
  // Global Totals Capture
  // ================================

  /**
   * Capture global resource totals for later lookup.
   */
  public static void captureGlobalTotals(long totalNetWeight, long totalNetLimit,
                                         long totalEnergyWeight, long totalEnergyLimit,
                                         long totalTronPowerWeight) {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      snapshot.totalNetWeight = totalNetWeight;
      snapshot.totalNetLimit = totalNetLimit;
      snapshot.totalEnergyWeight = totalEnergyWeight;
      snapshot.totalEnergyLimit = totalEnergyLimit;
      snapshot.totalTronPowerWeight = totalTronPowerWeight;
      snapshot.globalsInitialized = true;

      if (logger.isTraceEnabled()) {
        logger.trace("Captured global totals: netWeight={}, netLimit={}, "
                     + "energyWeight={}, energyLimit={}, tronPowerWeight={}",
                     totalNetWeight, totalNetLimit, totalEnergyWeight,
                     totalEnergyLimit, totalTronPowerWeight);
      }
    }
  }

  /**
   * Get captured global totals.
   *
   * @return GlobalSnapshot if initialized, or null if not captured
   */
  public static GlobalSnapshot getGlobalTotals() {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null && snapshot.globalsInitialized) {
      return new GlobalSnapshot(
          snapshot.totalNetWeight,
          snapshot.totalNetLimit,
          snapshot.totalEnergyWeight,
          snapshot.totalEnergyLimit,
          snapshot.totalTronPowerWeight
      );
    }
    return null;
  }

  /**
   * Check if global totals have been captured.
   */
  public static boolean hasGlobalTotals() {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    return snapshot != null && snapshot.globalsInitialized;
  }

  // ================================
  // Freeze Capture
  // ================================

  /**
   * Capture a freeze state for later lookup.
   *
   * @param ownerAddress owner address bytes
   * @param resourceType resource type (BANDWIDTH, ENERGY, TRON_POWER)
   * @param recipientAddress recipient address bytes (null if self-freeze)
   * @param amount pre-execution frozen amount in SUN
   * @param expireTimeMs pre-execution expiration timestamp in ms
   */
  public static void captureFreeze(byte[] ownerAddress, String resourceType,
                                   byte[] recipientAddress, long amount, long expireTimeMs) {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      String key = makeFreezeKey(ownerAddress, resourceType, recipientAddress);
      snapshot.freezes.put(key, new FreezeSnapshot(amount, expireTimeMs));
      if (logger.isTraceEnabled()) {
        logger.trace("Captured freeze: owner={}, resource={}, recipient={}, amount={}, expire={}",
                     ByteArray.toHexString(ownerAddress), resourceType,
                     recipientAddress != null ? ByteArray.toHexString(recipientAddress) : "self",
                     amount, expireTimeMs);
      }
    }
  }

  /**
   * Get captured freeze state for an owner/resource/recipient tuple.
   *
   * @param ownerAddress owner address bytes
   * @param resourceType resource type (BANDWIDTH, ENERGY, TRON_POWER)
   * @param recipientAddress recipient address bytes (null if self-freeze)
   * @return FreezeSnapshot if captured, or null if not found
   */
  public static FreezeSnapshot getFreeze(byte[] ownerAddress, String resourceType,
                                         byte[] recipientAddress) {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      String key = makeFreezeKey(ownerAddress, resourceType, recipientAddress);
      return snapshot.freezes.get(key);
    }
    return null;
  }

  /**
   * Get all captured freeze snapshots.
   */
  public static Map<String, FreezeSnapshot> getAllFreezes() {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      return Collections.unmodifiableMap(snapshot.freezes);
    }
    return Collections.emptyMap();
  }

  // ================================
  // Account Frozen Totals Capture
  // ================================

  /**
   * Capture per-account frozen totals for computing net_limit and energy_limit.
   *
   * @param address account address bytes
   * @param frozenForBandwidth total frozen balance for bandwidth (from getAllFrozenBalanceForBandwidth)
   * @param frozenForEnergy total frozen balance for energy (from getAllFrozenBalanceForEnergy)
   */
  public static void captureAccountFrozenTotals(byte[] address, long frozenForBandwidth,
                                                long frozenForEnergy) {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      String key = ByteArray.toHexString(address).toLowerCase();
      snapshot.accountFrozenTotals.put(key, new AccountFrozenTotals(frozenForBandwidth, frozenForEnergy));
      if (logger.isTraceEnabled()) {
        logger.trace("Captured account frozen totals: address={}, frozenBw={}, frozenEnergy={}",
                     key, frozenForBandwidth, frozenForEnergy);
      }
    }
  }

  /**
   * Get captured frozen totals for an account.
   *
   * @param address account address bytes
   * @return AccountFrozenTotals if captured, or null if not found
   */
  public static AccountFrozenTotals getAccountFrozenTotals(byte[] address) {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      String key = ByteArray.toHexString(address).toLowerCase();
      return snapshot.accountFrozenTotals.get(key);
    }
    return null;
  }

  /**
   * Get all captured account frozen totals.
   */
  public static Map<String, AccountFrozenTotals> getAllAccountFrozenTotals() {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot != null) {
      return Collections.unmodifiableMap(snapshot.accountFrozenTotals);
    }
    return Collections.emptyMap();
  }

  // ================================
  // Helper Methods
  // ================================

  private static String makeKey(byte[] address, String tokenId) {
    return ByteArray.toHexString(address).toLowerCase() + ":" + tokenId;
  }

  private static String makeVoteKey(byte[] voterAddress, byte[] witnessAddress) {
    return ByteArray.toHexString(voterAddress).toLowerCase() + ":"
           + ByteArray.toHexString(witnessAddress).toLowerCase();
  }

  private static String makeFreezeKey(byte[] ownerAddress, String resourceType,
                                      byte[] recipientAddress) {
    String ownerHex = ByteArray.toHexString(ownerAddress).toLowerCase();
    String recipientHex = recipientAddress != null
        ? ByteArray.toHexString(recipientAddress).toLowerCase() : "";
    return ownerHex + ":" + resourceType + ":" + recipientHex;
  }

  /**
   * Get current snapshot metrics (for monitoring).
   */
  public static String getCurrentSnapshotMetrics() {
    PreStateSnapshot snapshot = snapshotThreadLocal.get();
    if (snapshot == null) {
      return "No pre-state snapshot active";
    }
    return String.format(
        "PreStateSnapshot: %d TRC-10 balances, %d votes, %d freezes, %d account frozen totals, globals=%s",
        snapshot.trc10Balances.size(),
        snapshot.votes.size(),
        snapshot.freezes.size(),
        snapshot.accountFrozenTotals.size(),
        snapshot.globalsInitialized ? "captured" : "not captured");
  }
}
