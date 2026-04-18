package org.tron.core.execution.spi;

import java.util.List;
import java.util.concurrent.CompletableFuture;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.core.exception.VMIllegalException;

/**
 * Execution Service Provider Interface (SPI) for abstracting EVM execution operations. This
 * interface supports embedded Java EVM, remote Rust execution, and shadow verification.
 */
public interface ExecutionSPI {

  /**
   * Execute a transaction and modify state.
   *
   * @param context Transaction context containing all necessary information
   * @return CompletableFuture with execution result (extends ProgramResult)
   * @throws ContractValidateException if transaction validation fails
   * @throws ContractExeException if transaction execution fails
   * @throws VMIllegalException if VM encounters illegal operation
   */
  CompletableFuture<ExecutionProgramResult> executeTransaction(TransactionContext context)
      throws ContractValidateException, ContractExeException, VMIllegalException;

  /**
   * Call a contract without modifying state (view call).
   *
   * @param context Transaction context for the call
   * @return CompletableFuture with call result (extends ProgramResult)
   * @throws ContractValidateException if call validation fails
   * @throws VMIllegalException if VM encounters illegal operation
   */
  CompletableFuture<ExecutionProgramResult> callContract(TransactionContext context)
      throws ContractValidateException, VMIllegalException;

  /**
   * Estimate energy required for transaction execution.
   *
   * @param context Transaction context for estimation
   * @return CompletableFuture with energy estimate
   * @throws ContractValidateException if transaction validation fails
   */
  CompletableFuture<Long> estimateEnergy(TransactionContext context)
      throws ContractValidateException;

  /**
   * Get contract code at address.
   *
   * @param address Contract address
   * @param snapshotId Optional snapshot ID for historical queries
   * @return CompletableFuture with contract code
   */
  CompletableFuture<byte[]> getCode(byte[] address, String snapshotId);

  /**
   * Get storage value at address and key.
   *
   * @param address Contract address
   * @param key Storage key
   * @param snapshotId Optional snapshot ID for historical queries
   * @return CompletableFuture with storage value
   */
  CompletableFuture<byte[]> getStorageAt(byte[] address, byte[] key, String snapshotId);

  /**
   * Get account nonce.
   *
   * @param address Account address
   * @param snapshotId Optional snapshot ID for historical queries
   * @return CompletableFuture with account nonce
   */
  CompletableFuture<Long> getNonce(byte[] address, String snapshotId);

  /**
   * Get account balance.
   *
   * @param address Account address
   * @param snapshotId Optional snapshot ID for historical queries
   * @return CompletableFuture with account balance
   */
  CompletableFuture<byte[]> getBalance(byte[] address, String snapshotId);

  /**
   * Create EVM snapshot for state rollback.
   *
   * @return CompletableFuture with snapshot ID
   */
  CompletableFuture<String> createSnapshot();

  /**
   * Revert to EVM snapshot.
   *
   * @param snapshotId Snapshot ID to revert to
   * @return CompletableFuture indicating success
   */
  CompletableFuture<Boolean> revertToSnapshot(String snapshotId);

  /**
   * Get execution health status.
   *
   * @return CompletableFuture with health status
   */
  CompletableFuture<HealthStatus> healthCheck();

  /**
   * Register metrics callback for monitoring.
   *
   * @param callback Metrics callback
   */
  void registerMetricsCallback(MetricsCallback callback);

  /** Execution result containing all execution information. */
  class ExecutionResult {
    private final boolean success;
    private final byte[] returnData;
    private final long energyUsed;
    private final long energyRefunded;
    private final List<StateChange> stateChanges;
    private final List<LogEntry> logs;
    private final String errorMessage;
    private final long bandwidthUsed;
    private final List<FreezeLedgerChange> freezeChanges;
    private final List<GlobalResourceTotalsChange> globalResourceChanges;
    private final List<Trc10Change> trc10Changes;
    private final List<VoteChange> voteChanges;
    private final List<WithdrawChange> withdrawChanges;
    // Phase 0.4: Receipt passthrough - serialized Protocol.Transaction.Result bytes
    // Contains system contract-specific fields like exchange_id, withdraw_amount, etc.
    private final byte[] tronTransactionResult;
    // Phase 2.I L2: Contract creation address (20-byte EVM address)
    // For CreateSmartContract, this is the newly created contract's address
    private final byte[] contractAddress;
    // Phase B conformance: Write mode indicates whether Rust has persisted state changes
    // When PERSISTED, Java should NOT apply state_changes to avoid double-apply
    private final WriteMode writeMode;
    // Phase B conformance: Touched keys for B-镜像 (B-mirror) support
    // Only populated when writeMode == PERSISTED
    private final List<TouchedKey> touchedKeys;

    public ExecutionResult(
        boolean success,
        byte[] returnData,
        long energyUsed,
        long energyRefunded,
        List<StateChange> stateChanges,
        List<LogEntry> logs,
        String errorMessage,
        long bandwidthUsed,
        List<FreezeLedgerChange> freezeChanges,
        List<GlobalResourceTotalsChange> globalResourceChanges,
        List<Trc10Change> trc10Changes,
        List<VoteChange> voteChanges,
        List<WithdrawChange> withdrawChanges) {
      this(success, returnData, energyUsed, energyRefunded, stateChanges, logs,
           errorMessage, bandwidthUsed, freezeChanges, globalResourceChanges,
           trc10Changes, voteChanges, withdrawChanges, null, null,
           WriteMode.COMPUTE_ONLY, new java.util.ArrayList<>());
    }

    // Constructor with tronTransactionResult (backward compatible)
    public ExecutionResult(
        boolean success,
        byte[] returnData,
        long energyUsed,
        long energyRefunded,
        List<StateChange> stateChanges,
        List<LogEntry> logs,
        String errorMessage,
        long bandwidthUsed,
        List<FreezeLedgerChange> freezeChanges,
        List<GlobalResourceTotalsChange> globalResourceChanges,
        List<Trc10Change> trc10Changes,
        List<VoteChange> voteChanges,
        List<WithdrawChange> withdrawChanges,
        byte[] tronTransactionResult) {
      this(success, returnData, energyUsed, energyRefunded, stateChanges, logs,
           errorMessage, bandwidthUsed, freezeChanges, globalResourceChanges,
           trc10Changes, voteChanges, withdrawChanges, tronTransactionResult, null,
           WriteMode.COMPUTE_ONLY, new java.util.ArrayList<>());
    }

    // Constructor with contractAddress (Phase 2.I L2)
    public ExecutionResult(
        boolean success,
        byte[] returnData,
        long energyUsed,
        long energyRefunded,
        List<StateChange> stateChanges,
        List<LogEntry> logs,
        String errorMessage,
        long bandwidthUsed,
        List<FreezeLedgerChange> freezeChanges,
        List<GlobalResourceTotalsChange> globalResourceChanges,
        List<Trc10Change> trc10Changes,
        List<VoteChange> voteChanges,
        List<WithdrawChange> withdrawChanges,
        byte[] tronTransactionResult,
        byte[] contractAddress) {
      this(success, returnData, energyUsed, energyRefunded, stateChanges, logs,
           errorMessage, bandwidthUsed, freezeChanges, globalResourceChanges,
           trc10Changes, voteChanges, withdrawChanges, tronTransactionResult, contractAddress,
           WriteMode.COMPUTE_ONLY, new java.util.ArrayList<>());
    }

    // Full constructor with writeMode and touchedKeys (Phase B conformance)
    public ExecutionResult(
        boolean success,
        byte[] returnData,
        long energyUsed,
        long energyRefunded,
        List<StateChange> stateChanges,
        List<LogEntry> logs,
        String errorMessage,
        long bandwidthUsed,
        List<FreezeLedgerChange> freezeChanges,
        List<GlobalResourceTotalsChange> globalResourceChanges,
        List<Trc10Change> trc10Changes,
        List<VoteChange> voteChanges,
        List<WithdrawChange> withdrawChanges,
        byte[] tronTransactionResult,
        byte[] contractAddress,
        WriteMode writeMode,
        List<TouchedKey> touchedKeys) {
      this.success = success;
      this.returnData = returnData;
      this.energyUsed = energyUsed;
      this.energyRefunded = energyRefunded;
      this.stateChanges = stateChanges;
      this.logs = logs;
      this.errorMessage = errorMessage;
      this.bandwidthUsed = bandwidthUsed;
      this.freezeChanges = freezeChanges;
      this.globalResourceChanges = globalResourceChanges;
      this.trc10Changes = trc10Changes;
      this.voteChanges = voteChanges;
      this.withdrawChanges = withdrawChanges;
      this.tronTransactionResult = tronTransactionResult;
      this.contractAddress = contractAddress;
      this.writeMode = writeMode;
      this.touchedKeys = touchedKeys;
    }

    // Getters
    public boolean isSuccess() {
      return success;
    }

    public byte[] getReturnData() {
      return returnData;
    }

    public long getEnergyUsed() {
      return energyUsed;
    }

    public long getEnergyRefunded() {
      return energyRefunded;
    }

    public List<StateChange> getStateChanges() {
      return stateChanges;
    }

    public List<LogEntry> getLogs() {
      return logs;
    }

    public String getErrorMessage() {
      return errorMessage;
    }

    public long getBandwidthUsed() {
      return bandwidthUsed;
    }

    public List<FreezeLedgerChange> getFreezeChanges() {
      return freezeChanges;
    }

    public List<GlobalResourceTotalsChange> getGlobalResourceChanges() {
      return globalResourceChanges;
    }

    public List<Trc10Change> getTrc10Changes() {
      return trc10Changes;
    }

    public List<VoteChange> getVoteChanges() {
      return voteChanges;
    }

    public List<WithdrawChange> getWithdrawChanges() {
      return withdrawChanges;
    }

    /**
     * Get the serialized Protocol.Transaction.Result bytes from Rust execution.
     * Phase 0.4: Receipt passthrough - contains system contract-specific fields like
     * exchange_id, withdraw_amount, withdraw_expire_amount, cancel_unfreezeV2_amount, etc.
     *
     * @return Serialized Transaction.Result bytes, or null if not provided
     */
    public byte[] getTronTransactionResult() {
      return tronTransactionResult;
    }

    /**
     * Get the contract address for CreateSmartContract transactions.
     * Phase 2.I L2: This is the 20-byte EVM address of the newly created contract.
     *
     * @return Contract address bytes, or null if this is not a contract creation
     */
    public byte[] getContractAddress() {
      return contractAddress;
    }

    /**
     * Get the write mode for Phase B conformance alignment.
     * - COMPUTE_ONLY (default): Java should apply state changes
     * - PERSISTED: Rust has already persisted, Java should NOT apply to avoid double-apply
     *
     * @return WriteMode indicating how Java should handle state changes
     */
    public WriteMode getWriteMode() {
      return writeMode;
    }

    /**
     * Get the list of touched keys for B-镜像 (B-mirror) support.
     * Only populated when writeMode == PERSISTED.
     * Java can use these to refresh its local revoking head from remote root.
     *
     * @return List of TouchedKey records, or empty list if not applicable
     */
    public List<TouchedKey> getTouchedKeys() {
      return touchedKeys;
    }
  }

  /** State change information. */
  class StateChange {
    private final byte[] address;
    private final byte[] key;
    private final byte[] oldValue;
    private final byte[] newValue;

    public StateChange(byte[] address, byte[] key, byte[] oldValue, byte[] newValue) {
      this.address = address;
      this.key = key;
      this.oldValue = oldValue;
      this.newValue = newValue;
    }

    // Getters
    public byte[] getAddress() {
      return address;
    }

    public byte[] getKey() {
      return key;
    }

    public byte[] getOldValue() {
      return oldValue;
    }

    public byte[] getNewValue() {
      return newValue;
    }
  }

  /** Log entry information. */
  class LogEntry {
    private final byte[] address;
    private final List<byte[]> topics;
    private final byte[] data;

    public LogEntry(byte[] address, List<byte[]> topics, byte[] data) {
      this.address = address;
      this.topics = topics;
      this.data = data;
    }

    // Getters
    public byte[] getAddress() {
      return address;
    }

    public List<byte[]> getTopics() {
      return topics;
    }

    public byte[] getData() {
      return data;
    }
  }

  /** Health status information. */
  class HealthStatus {
    private final boolean healthy;
    private final String message;

    public HealthStatus(boolean healthy, String message) {
      this.healthy = healthy;
      this.message = message;
    }

    // Getters
    public boolean isHealthy() {
      return healthy;
    }

    public String getMessage() {
      return message;
    }
  }

  /**
   * Freeze/resource ledger change (Phase 2: emit_freeze_ledger_changes).
   * Describes a single freeze or unfreeze operation affecting an owner's resource balance.
   */
  class FreezeLedgerChange {
    public enum Resource {
      BANDWIDTH(0),
      ENERGY(1),
      TRON_POWER(2);

      private final int value;

      Resource(int value) {
        this.value = value;
      }

      public int getValue() {
        return value;
      }

      public static Resource fromValue(int value) {
        for (Resource r : Resource.values()) {
          if (r.value == value) {
            return r;
          }
        }
        throw new IllegalArgumentException("Unknown resource value: " + value);
      }
    }

    private final byte[] ownerAddress;
    private final Resource resource;
    private final long amount;
    private final long expirationMs;
    private final boolean v2Model;

    public FreezeLedgerChange(byte[] ownerAddress, Resource resource, long amount,
                               long expirationMs, boolean v2Model) {
      this.ownerAddress = ownerAddress;
      this.resource = resource;
      this.amount = amount;
      this.expirationMs = expirationMs;
      this.v2Model = v2Model;
    }

    // Getters
    public byte[] getOwnerAddress() {
      return ownerAddress;
    }

    public Resource getResource() {
      return resource;
    }

    public long getAmount() {
      return amount;
    }

    public long getExpirationMs() {
      return expirationMs;
    }

    public boolean isV2Model() {
      return v2Model;
    }
  }

  /**
   * Global resource totals change (Phase 2: emit_freeze_ledger_changes).
   * Captures dynamic property updates for total weights and limits.
   */
  class GlobalResourceTotalsChange {
    private final long totalNetWeight;
    private final long totalNetLimit;
    private final long totalEnergyWeight;
    private final long totalEnergyLimit;

    public GlobalResourceTotalsChange(long totalNetWeight, long totalNetLimit,
                                       long totalEnergyWeight, long totalEnergyLimit) {
      this.totalNetWeight = totalNetWeight;
      this.totalNetLimit = totalNetLimit;
      this.totalEnergyWeight = totalEnergyWeight;
      this.totalEnergyLimit = totalEnergyLimit;
    }

    // Getters
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
  }

  /**
   * TRC-10 Asset Issued (Phase 2: full TRC-10 ledger semantics).
   * Describes a new TRC-10 asset issuance operation for Java-side persistence.
   */
  class Trc10AssetIssued {
    private final byte[] ownerAddress;
    private final byte[] name;
    private final byte[] abbr;
    private final long totalSupply;
    private final int trxNum;
    private final int precision;
    private final int num;
    private final long startTime;
    private final long endTime;
    private final byte[] description;
    private final byte[] url;
    private final long freeAssetNetLimit;
    private final long publicFreeAssetNetLimit;
    private final long publicFreeAssetNetUsage;
    private final long publicLatestFreeNetTime;
    private final String tokenId; // Empty if Java needs to compute via TOKEN_ID_NUM

    public Trc10AssetIssued(byte[] ownerAddress, byte[] name, byte[] abbr, long totalSupply,
                            int trxNum, int precision, int num, long startTime, long endTime,
                            byte[] description, byte[] url, long freeAssetNetLimit,
                            long publicFreeAssetNetLimit, long publicFreeAssetNetUsage,
                            long publicLatestFreeNetTime, String tokenId) {
      this.ownerAddress = ownerAddress;
      this.name = name;
      this.abbr = abbr;
      this.totalSupply = totalSupply;
      this.trxNum = trxNum;
      this.precision = precision;
      this.num = num;
      this.startTime = startTime;
      this.endTime = endTime;
      this.description = description;
      this.url = url;
      this.freeAssetNetLimit = freeAssetNetLimit;
      this.publicFreeAssetNetLimit = publicFreeAssetNetLimit;
      this.publicFreeAssetNetUsage = publicFreeAssetNetUsage;
      this.publicLatestFreeNetTime = publicLatestFreeNetTime;
      this.tokenId = tokenId;
    }

    // Getters
    public byte[] getOwnerAddress() {
      return ownerAddress;
    }

    public byte[] getName() {
      return name;
    }

    public byte[] getAbbr() {
      return abbr;
    }

    public long getTotalSupply() {
      return totalSupply;
    }

    public int getTrxNum() {
      return trxNum;
    }

    public int getPrecision() {
      return precision;
    }

    public int getNum() {
      return num;
    }

    public long getStartTime() {
      return startTime;
    }

    public long getEndTime() {
      return endTime;
    }

    public byte[] getDescription() {
      return description;
    }

    public byte[] getUrl() {
      return url;
    }

    public long getFreeAssetNetLimit() {
      return freeAssetNetLimit;
    }

    public long getPublicFreeAssetNetLimit() {
      return publicFreeAssetNetLimit;
    }

    public long getPublicFreeAssetNetUsage() {
      return publicFreeAssetNetUsage;
    }

    public long getPublicLatestFreeNetTime() {
      return publicLatestFreeNetTime;
    }

    public String getTokenId() {
      return tokenId;
    }
  }

  /**
   * TRC-10 Asset Transferred (Phase 2: TRC-10 transfer operation).
   * Describes a TRC-10 transfer for Java-side persistence of asset balance changes.
   */
  class Trc10AssetTransferred {
    private final byte[] ownerAddress;  // Sender address
    private final byte[] toAddress;     // Recipient address
    private final byte[] assetName;     // V1 path: asset name bytes
    private final String tokenId;       // V2 path: token ID if parsable from assetName
    private final long amount;          // Transfer amount

    public Trc10AssetTransferred(byte[] ownerAddress, byte[] toAddress, byte[] assetName,
                                  String tokenId, long amount) {
      this.ownerAddress = ownerAddress;
      this.toAddress = toAddress;
      this.assetName = assetName;
      this.tokenId = tokenId;
      this.amount = amount;
    }

    // Getters
    public byte[] getOwnerAddress() {
      return ownerAddress;
    }

    public byte[] getToAddress() {
      return toAddress;
    }

    public byte[] getAssetName() {
      return assetName;
    }

    public String getTokenId() {
      return tokenId;
    }

    public long getAmount() {
      return amount;
    }
  }

  /**
   * TRC-10 Change (union type for different TRC-10 operations).
   * Phase 2: Supports AssetIssued and AssetTransferred.
   * Future: add Trc10Participated, Trc10Updated variants.
   */
  class Trc10Change {
    private final Trc10AssetIssued assetIssued;
    private final Trc10AssetTransferred assetTransferred;

    public Trc10Change(Trc10AssetIssued assetIssued) {
      this.assetIssued = assetIssued;
      this.assetTransferred = null;
    }

    public Trc10Change(Trc10AssetTransferred assetTransferred) {
      this.assetIssued = null;
      this.assetTransferred = assetTransferred;
    }

    public Trc10AssetIssued getAssetIssued() {
      return assetIssued;
    }

    public boolean hasAssetIssued() {
      return assetIssued != null;
    }

    public Trc10AssetTransferred getAssetTransferred() {
      return assetTransferred;
    }

    public boolean hasAssetTransferred() {
      return assetTransferred != null;
    }
  }

  /**
   * Vote entry for VoteChange - represents a single vote for a witness.
   */
  class VoteEntry {
    private final byte[] voteAddress;  // 21-byte Tron witness address
    private final long voteCount;

    public VoteEntry(byte[] voteAddress, long voteCount) {
      this.voteAddress = voteAddress;
      this.voteCount = voteCount;
    }

    public byte[] getVoteAddress() {
      return voteAddress;
    }

    public long getVoteCount() {
      return voteCount;
    }
  }

  /**
   * VoteChange carries updated votes for an account after VoteWitness execution.
   * Java should apply this to Account.votes to maintain parity with embedded mode.
   * This ensures correct old_votes seeding on subsequent votes in the same or later epochs.
   */
  class VoteChange {
    private final byte[] ownerAddress;  // 21-byte Tron address of the voter
    private final java.util.List<VoteEntry> votes;  // New votes list (replaces Account.votes)

    public VoteChange(byte[] ownerAddress, java.util.List<VoteEntry> votes) {
      this.ownerAddress = ownerAddress;
      this.votes = votes;
    }

    public byte[] getOwnerAddress() {
      return ownerAddress;
    }

    public java.util.List<VoteEntry> getVotes() {
      return votes;
    }
  }

  /**
   * WithdrawChange carries withdrawal info for applying allowance and latestWithdrawTime updates.
   * Used for WithdrawBalanceContract remote execution - Java applies this to Account fields.
   * Balance delta is already handled by AccountChange; this sidecar handles the allowance/time reset.
   */
  class WithdrawChange {
    private final byte[] ownerAddress;       // 21-byte Tron address of the witness withdrawing
    private final long amount;               // The withdrawn amount (= Account.allowance before operation)
    private final long latestWithdrawTime;   // Timestamp to set as Account.latestWithdrawTime

    public WithdrawChange(byte[] ownerAddress, long amount, long latestWithdrawTime) {
      this.ownerAddress = ownerAddress;
      this.amount = amount;
      this.latestWithdrawTime = latestWithdrawTime;
    }

    public byte[] getOwnerAddress() {
      return ownerAddress;
    }

    public long getAmount() {
      return amount;
    }

    public long getLatestWithdrawTime() {
      return latestWithdrawTime;
    }
  }

  /** Metrics callback interface. */
  interface MetricsCallback {
    void onMetric(String name, double value);
  }

  /**
   * Write mode returned by remote execution.
   * Determines how Java should handle state changes from the Rust backend.
   *
   * <p>See planning/close_loop.phase1_freeze.md §13 for the canonical
   * write-ownership policy.
   */
  enum WriteMode {
    /**
     * Rust computed state changes but did not persist them.
     * Java must apply the returned state_changes / sidecars.
     *
     * <p>In Phase 1 this path is reached for VM txs when
     * {@code rust_persist_enabled=false} (legacy / transitional only), and
     * for any revert / error / commit-failure path regardless of the flag.
     */
    COMPUTE_ONLY(0),

    /**
     * Rust already persisted state changes on a successful commit.
     * Java must NOT re-apply state_changes / sidecars to avoid double-apply.
     * Java can use touched_keys to mirror remote state to local revoking head.
     *
     * <p>Under the Phase 1 RR canonical config
     * ({@code rust_persist_enabled=true}), successful VM txs and non-VM txs
     * return this mode.
     */
    PERSISTED(1);

    private final int value;

    WriteMode(int value) {
      this.value = value;
    }

    public int getValue() {
      return value;
    }

    public static WriteMode fromValue(int value) {
      for (WriteMode wm : WriteMode.values()) {
        if (wm.value == value) {
          return wm;
        }
      }
      return COMPUTE_ONLY; // Default to compute-only for unknown values
    }
  }

  /**
   * Touched key record for B-镜像 (B-mirror) support.
   * Reports a database key that was modified during execution.
   * Java uses this to refresh its local revoking head from remote root.
   */
  class TouchedKey {
    private final String db;      // Database name (canonical, from db_names constants)
    private final byte[] key;     // The key that was touched
    private final boolean isDelete; // True if this was a delete operation

    public TouchedKey(String db, byte[] key, boolean isDelete) {
      this.db = db;
      this.key = key;
      this.isDelete = isDelete;
    }

    public String getDb() {
      return db;
    }

    public byte[] getKey() {
      return key;
    }

    public boolean isDelete() {
      return isDelete;
    }
  }
}
