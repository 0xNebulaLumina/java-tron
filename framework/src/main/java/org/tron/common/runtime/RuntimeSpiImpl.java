package org.tron.common.runtime;

import static org.tron.protos.contract.Common.ResourceCode.BANDWIDTH;
import static org.tron.protos.contract.Common.ResourceCode.ENERGY;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import lombok.extern.slf4j.Slf4j;
import org.tron.core.ChainBaseManager;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.db.ByteArrayWrapper;
import org.tron.core.db.TransactionContext;
import org.tron.core.db.TronStoreWithRevoking;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.core.execution.reporting.PreStateSnapshotRegistry;
import org.tron.core.execution.spi.ExecutionMode;
import org.tron.core.execution.spi.ExecutionProgramResult;
import org.tron.core.execution.spi.ExecutionSPI;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.core.execution.spi.ExecutionSpiFactory;
import org.tron.core.storage.spi.StorageMode;
import org.tron.core.storage.spi.StorageSPI;
import org.tron.core.storage.spi.StorageSpiFactory;
import org.tron.protos.Protocol.Transaction.Result.contractResult;
import org.tron.protos.Protocol.Vote;

/**
 * REMOTE ExecutionSPI runtime that treats Rust as the state writer and refreshes Java's
 * read-side mirror after persisted execution.
 *
 * <p>This class uses ExecutionProgramResult, which extends ProgramResult, eliminating the need for
 * type conversion.
 */
@Slf4j(topic = "VM")
public class RuntimeSpiImpl implements Runtime {

  private static volatile StorageSPI mirrorRemoteStorageSPI;

  private final ExecutionSPI executionSPI;
  private TransactionContext context;
  private ExecutionProgramResult executionResult;
  private String runtimeError;

  /**
   * Constructor that requires a pre-initialized REMOTE ExecutionSPI.
   */
  public RuntimeSpiImpl() {
    if (ExecutionSpiFactory.getInitializedMode() != ExecutionMode.REMOTE) {
      throw new RuntimeException("RuntimeSpiImpl requires REMOTE ExecutionSPI mode");
    }

    this.executionSPI = ExecutionSpiFactory.getInstance();
    if (this.executionSPI == null) {
      throw new RuntimeException(
          "ExecutionSPI not initialized. Call ExecutionSpiFactory.initialize() first.");
    }
    logger.info(
        "RuntimeSpiImpl initialized with execution mode: {}",
        ExecutionSpiFactory.getInitializedMode());
  }

  @Override
  public void execute(TransactionContext context)
      throws ContractValidateException, ContractExeException {
    this.context = context;

    try {
      logger.debug(
          "Executing transaction with ExecutionSPI: {}", context.getTrxCap().getTransactionId());

      if (!isPostExecMirrorEnabled()) {
        throw new RemoteExecutionOwnershipException(
            "REMOTE execution requires post-exec mirror to be enabled");
      }

      // Use ExecutionSPI for execution
      CompletableFuture<ExecutionProgramResult> future =
          executionSPI.executeTransaction(context);
      this.executionResult = future.get(); // Synchronous execution

      // Store runtime error if execution failed
      if (!executionResult.isSuccess()) {
        this.runtimeError = executionResult.getErrorMessage();
      }

      ExecutionSPI.WriteMode writeMode = executionResult.getWriteMode();
      if (writeMode != ExecutionSPI.WriteMode.PERSISTED
          && (executionResult.isSuccess() || hasRemoteStateEffects(executionResult))) {
        throw new RemoteExecutionOwnershipException(
            "Remote execution returned non-persisted state after Java-side apply was removed");
      }

      capturePreStateSnapshot(executionResult, context);

      if (writeMode == ExecutionSPI.WriteMode.PERSISTED) {
        logger.info("Refreshing Java read-side mirror for persisted remote transaction: {}",
            context.getTrxCap().getTransactionId());
        postExecMirror(executionResult, context);
      } else {
        logger.debug("No persisted state effects to mirror for unsuccessful remote execution");
      }

      // Since ExecutionProgramResult extends ProgramResult, we can use it directly
      context.setProgramResult(executionResult);

      logger.debug(
          "ExecutionSPI execution completed. Success: {}, Energy used: {}, "
              + "State changes reported: {}",
          executionResult.isSuccess(),
          executionResult.getEnergyUsed(),
          executionResult.getStateChanges() != null ? executionResult.getStateChanges().size() : 0);

    } catch (RemoteExecutionOwnershipException e) {
      throw e;
    } catch (Exception e) {
      logger.error(
          "ExecutionSPI execution failed for transaction: {}",
          context.getTrxCap().getTransactionId(),
          e);

      if (executionResult != null
          && executionResult.getWriteMode() == ExecutionSPI.WriteMode.PERSISTED) {
        throw new IllegalStateException(
            "Persisted remote execution state could not be mirrored", e);
      }

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

  private static class RemoteExecutionOwnershipException extends IllegalStateException {
    RemoteExecutionOwnershipException(String message) {
      super(message);
    }
  }

  /** Create a failed ExecutionProgramResult when ExecutionSPI execution fails. */
  private ExecutionProgramResult createFailedExecutionProgramResult(String errorMessage) {
    ExecutionProgramResult result = new ExecutionProgramResult();

    result.setResultCode(contractResult.UNKNOWN);
    result.setRuntimeError(errorMessage);
    result.setException(new RuntimeException(errorMessage));

    logger.debug("Created failed ExecutionProgramResult with error: {}", errorMessage);
    return result;
  }

  /**
   * Capture pre-state snapshot for CSV reporting in remote execution mode.
   * This captures TRC-10 balances, votes, and global totals before mirror refresh,
   * allowing the builder to compute absolute old/new values for domain triplets.
   *
   * <p>Gated by: -Dremote.exec.prestate.snapshot.enabled=true (default true).
   */
  private void capturePreStateSnapshot(ExecutionProgramResult result, TransactionContext context) {
    // Check JVM gate: default true
    boolean captureEnabled = Boolean.parseBoolean(
        System.getProperty("remote.exec.prestate.snapshot.enabled", "true"));

    if (!captureEnabled) {
      logger.debug("Pre-state snapshot capture disabled by JVM property for transaction: {}",
          context.getTrxCap().getTransactionId());
      return;
    }

    try {
      ChainBaseManager chainBaseManager = context.getStoreFactory().getChainBaseManager();
      org.tron.core.store.AccountStore accountStore = chainBaseManager.getAccountStore();
      org.tron.core.store.DynamicPropertiesStore dynamicStore =
          chainBaseManager.getDynamicPropertiesStore();

      // Initialize snapshot for this transaction
      PreStateSnapshotRegistry.initializeForCurrentTransaction();

      // 1. Capture TRC-10 balances for addresses involved in transfers
      if (result.getTrc10Changes() != null) {
        for (ExecutionSPI.Trc10Change trc10Change : result.getTrc10Changes()) {
          if (trc10Change.hasAssetTransferred()) {
            ExecutionSPI.Trc10AssetTransferred transfer = trc10Change.getAssetTransferred();
            String tokenId = transfer.getTokenId();

            // If tokenId is missing (V1 path), derive it from AssetIssueStore using asset name.
            if (tokenId == null || tokenId.isEmpty()) {
              try {
                org.tron.core.store.AssetIssueStore assetIssueStore =
                    chainBaseManager.getAssetIssueStore();
                if (assetIssueStore != null && transfer.getAssetName() != null) {
                  org.tron.core.capsule.AssetIssueCapsule assetIssue =
                      assetIssueStore.get(transfer.getAssetName());
                  if (assetIssue != null && assetIssue.getId() != null) {
                    tokenId = assetIssue.getId();
                    logger.debug("Derived TRC-10 tokenId '{}' from asset name for prestate snapshot",
                        tokenId);
                  }
                }
              } catch (Exception e) {
                logger.warn("Failed to derive tokenId from AssetIssueStore: {}", e.getMessage());
              }
            }

            // Capture owner's pre-state balance
            byte[] ownerAddress = transfer.getOwnerAddress();
            AccountCapsule ownerAccount = accountStore.get(ownerAddress);
            if (ownerAccount != null && tokenId != null) {
              Map<String, Long> assetV2Map = ownerAccount.getAssetMapV2();
              Long ownerBalance = assetV2Map.get(tokenId);
              PreStateSnapshotRegistry.captureTrc10Balance(
                  ownerAddress, tokenId, ownerBalance != null ? ownerBalance : 0L);
            }

            // Capture recipient's pre-state balance
            byte[] toAddress = transfer.getToAddress();
            AccountCapsule recipientAccount = accountStore.get(toAddress);
            if (recipientAccount != null && tokenId != null) {
              Map<String, Long> recipientAssetV2Map = recipientAccount.getAssetMapV2();
              Long recipientBalance = recipientAssetV2Map.get(tokenId);
              PreStateSnapshotRegistry.captureTrc10Balance(
                  toAddress, tokenId, recipientBalance != null ? recipientBalance : 0L);
            } else if (tokenId != null) {
              // Recipient account doesn't exist yet, balance is 0
              PreStateSnapshotRegistry.captureTrc10Balance(toAddress, tokenId, 0L);
            }
          }
        }
      }

      // 2. Capture votes for voters involved in vote changes
      if (result.getVoteChanges() != null) {
        for (ExecutionSPI.VoteChange voteChange : result.getVoteChanges()) {
          byte[] voterAddress = voteChange.getOwnerAddress();
          AccountCapsule voterAccount = accountStore.get(voterAddress);
          if (voterAccount != null) {
            // Capture all existing votes for this voter
            java.util.List<Vote> existingVotes = voterAccount.getVotesList();
            PreStateSnapshotRegistry.captureVotes(voterAddress, existingVotes);
          }
        }
      }

      // 3. Capture global resource totals (for freeze/global resource changes)
      boolean hasFreezeChanges = result.getFreezeChanges() != null
          && !result.getFreezeChanges().isEmpty();
      boolean hasGlobalChanges = result.getGlobalResourceChanges() != null
          && !result.getGlobalResourceChanges().isEmpty();

      if (hasFreezeChanges || hasGlobalChanges) {
        long totalNetWeight = dynamicStore.getTotalNetWeight();
        long totalNetLimit = dynamicStore.getTotalNetLimit();
        long totalEnergyWeight = dynamicStore.getTotalEnergyWeight();
        long totalEnergyLimit = dynamicStore.getTotalEnergyCurrentLimit();
        long totalTronPowerWeight = dynamicStore.getTotalTronPowerWeight();

        PreStateSnapshotRegistry.captureGlobalTotals(
            totalNetWeight, totalNetLimit, totalEnergyWeight, totalEnergyLimit, totalTronPowerWeight);
      }

      // 3b. Capture freeze snapshots (owner/resource old amount + expire) before mirror refresh
      if (result.getFreezeChanges() != null) {
        for (ExecutionSPI.FreezeLedgerChange freezeChange : result.getFreezeChanges()) {
          byte[] ownerAddress = freezeChange.getOwnerAddress();
          if (ownerAddress == null) {
            continue;
          }

          AccountCapsule ownerAccount = accountStore.get(ownerAddress);
          long oldAmount = 0L;
          long oldExpireTimeMs = 0L;

          if (ownerAccount != null) {
            switch (freezeChange.getResource()) {
              case BANDWIDTH:
                if (freezeChange.isV2Model()) {
                  // V2 has no expiration
                  oldAmount = ownerAccount.getFrozenV2BalanceForBandwidth();
                  oldExpireTimeMs = 0L;
                } else {
                  oldAmount = ownerAccount.getFrozenBalance();
                  java.util.List<org.tron.protos.Protocol.Account.Frozen> frozenList = ownerAccount.getFrozenList();
                  oldExpireTimeMs = (frozenList != null && !frozenList.isEmpty())
                      ? frozenList.get(0).getExpireTime() : 0L;
                }
                break;
              case ENERGY:
                if (freezeChange.isV2Model()) {
                  oldAmount = ownerAccount.getFrozenV2BalanceForEnergy();
                  oldExpireTimeMs = 0L;
                } else {
                  oldAmount = ownerAccount.getEnergyFrozenBalance();
                  oldExpireTimeMs = ownerAccount.getAccountResource()
                      .getFrozenBalanceForEnergy().getExpireTime();
                }
                break;
              case TRON_POWER:
                if (freezeChange.isV2Model()) {
                  oldAmount = ownerAccount.getTronPowerFrozenV2Balance();
                  oldExpireTimeMs = 0L;
                } else {
                  oldAmount = ownerAccount.getTronPowerFrozenBalance();
                  oldExpireTimeMs = ownerAccount.getInstance().getTronPower().getExpireTime();
                }
                break;
              default:
                // Unknown resource type: leave zeros
                break;
            }
          }

          // Match embedded execution reporting: UnfreezeBalanceActuator records V1 unfreeze
          // journal entries with zeroed expire times, even when the account still carries
          // a historical V1 expiration in its pre-state.
          if (!freezeChange.isV2Model() && isV1UnfreezeContract(context)) {
            oldExpireTimeMs = 0L;
          }

          // Note: recipient is not provided by FreezeLedgerChange; use self-freeze (null recipient)
          PreStateSnapshotRegistry.captureFreeze(
              ownerAddress, freezeChange.getResource().name(), null, oldAmount, oldExpireTimeMs);
        }
      }

      // 4. Capture per-account frozen totals for limit computation
      // Build set of affected addresses from state changes (empty key = account changes)
      // and freeze changes owners
      Set<String> affectedAddresses = new HashSet<>();
      List<StateChange> stateChanges = result.getStateChanges();
      if (stateChanges != null) {
        for (StateChange sc : stateChanges) {
          // Empty key means account state change
          if (sc.getKey() == null || sc.getKey().length == 0) {
            if (sc.getAddress() != null) {
              affectedAddresses.add(org.tron.common.utils.ByteArray.toHexString(sc.getAddress()).toLowerCase());
            }
          }
        }
      }
      // Also include freeze change owners
      if (result.getFreezeChanges() != null) {
        for (ExecutionSPI.FreezeLedgerChange freezeChange : result.getFreezeChanges()) {
          if (freezeChange.getOwnerAddress() != null) {
            affectedAddresses.add(
                org.tron.common.utils.ByteArray.toHexString(freezeChange.getOwnerAddress()).toLowerCase());
          }
        }
      }

      // Capture frozen totals for each affected address
      for (String addressHex : affectedAddresses) {
        byte[] addressBytes = org.tron.common.utils.ByteArray.fromHexString(addressHex);
        AccountCapsule account = accountStore.get(addressBytes);
        if (account != null) {
          long frozenForBandwidth = account.getAllFrozenBalanceForBandwidth();
          long frozenForEnergy = account.getAllFrozenBalanceForEnergy();
          PreStateSnapshotRegistry.captureAccountFrozenTotals(
              addressBytes, frozenForBandwidth, frozenForEnergy);
        }
      }

      logger.debug("Captured pre-state snapshot for transaction: {} - {}",
          context.getTrxCap().getTransactionId(),
          PreStateSnapshotRegistry.getCurrentSnapshotMetrics());

    } catch (Exception e) {
      logger.warn("Failed to capture pre-state snapshot for transaction: {}, error: {}",
          context.getTrxCap().getTransactionId(), e.getMessage());
      // Don't fail the transaction - snapshot is for reporting only
    }
  }

  // =========================================================================
  // Phase B Mirror - batchGet optimization constants and feature flags
  // =========================================================================

  /**
   * JVM property to enable batchGet-based mirror reads.
   * When true (default), uses batchGet() for O(#dbs) RPC calls per tx.
   * When false, falls back to per-key get() for O(#keys) RPC calls per tx.
   */
  private static final String PROP_BATCH_GET_ENABLED = "remote.exec.postexec.mirror.batchGet";
  private static final boolean DEFAULT_BATCH_GET_ENABLED = true;

  /**
   * JVM property for maximum keys per batchGet chunk.
   * Conservative default to avoid oversized gRPC messages.
   */
  private static final String PROP_BATCH_MAX_KEYS = "remote.exec.postexec.mirror.batchGet.maxKeys";
  private static final int DEFAULT_BATCH_MAX_KEYS = 256;

  /**
   * JVM property to control fallback behavior on batchGet failure.
   * When true (default), falls back to per-key get() for failed chunks.
   * When false, counts errors but doesn't retry (risky for correctness).
   */
  private static final String PROP_FALLBACK_ENABLED = "remote.exec.postexec.mirror.batchGet.fallbackToSingleGet";
  private static final boolean DEFAULT_FALLBACK_ENABLED = true;

  /**
   * Post-execution mirror for B-镜像 (B-mirror) support.
   * When Rust has persisted state changes (write_mode=PERSISTED), Java refreshes its
   * local revoking head from the remote root to keep local views consistent.
   *
   * <p>This optimized implementation uses batchGet() to reduce gRPC calls from
   * O(#touched keys) to O(#touched dbs) per transaction.
   *
   * <p>Gate: Enabled when -Dremote.exec.postexec.mirror=true (default true).
   *
   * @param result ExecutionProgramResult containing touched keys
   * @param context Transaction context with access to stores
   */
  private void postExecMirror(ExecutionProgramResult result, TransactionContext context) {
    List<ExecutionSPI.TouchedKey> touchedKeys = result.getTouchedKeys();
    if (touchedKeys == null || touchedKeys.isEmpty()) {
      if (hasRemoteStateEffects(result)) {
        throw new IllegalStateException(
            "Persisted remote execution returned effects without touched keys");
      }
      logger.debug("No touched keys for post-exec mirror");
      return;
    }

    // Feature flags
    boolean batchGetEnabled = Boolean.parseBoolean(
        System.getProperty(PROP_BATCH_GET_ENABLED, String.valueOf(DEFAULT_BATCH_GET_ENABLED)));
    int maxBatchKeys = Integer.parseInt(
        System.getProperty(PROP_BATCH_MAX_KEYS, String.valueOf(DEFAULT_BATCH_MAX_KEYS)));
    if (maxBatchKeys <= 0) {
      throw new IllegalArgumentException(PROP_BATCH_MAX_KEYS + " must be > 0");
    }
    boolean fallbackEnabled = Boolean.parseBoolean(
        System.getProperty(PROP_FALLBACK_ENABLED, String.valueOf(DEFAULT_FALLBACK_ENABLED)));

    logger.debug("Phase B mirror: Refreshing {} touched keys for tx={}, batchGet={}, maxKeys={}, fallback={}",
        touchedKeys.size(), context.getTrxCap().getTransactionId(),
        batchGetEnabled, maxBatchKeys, fallbackEnabled);

    try {
      ChainBaseManager chainBaseManager = context.getStoreFactory().getChainBaseManager();
      StorageSPI storageSPI = getMirrorRemoteStorageSPI();

      // Step A: Normalize and dedupe touched keys by db (last-write-wins)
      // Map<dbName, Map<KeyWrapper, KeyOperation>> where KeyOperation contains the final byte[] and isDelete
      Map<String, LinkedHashMap<ByteArrayWrapper, KeyOperation>> normalizedByDb = normalizeTouchedKeys(touchedKeys);

      int successCount = 0;
      int errorCount = 0;
      int batchGetCalls = 0;
      int fallbackGetCalls = 0;

      // Process each database's keys
      for (Map.Entry<String, LinkedHashMap<ByteArrayWrapper, KeyOperation>> dbEntry : normalizedByDb.entrySet()) {
        String dbName = dbEntry.getKey();
        LinkedHashMap<ByteArrayWrapper, KeyOperation> keyOps = dbEntry.getValue();

        // Get the local store for this database
        TronStoreWithRevoking<?> store = getStoreByDbName(dbName, chainBaseManager);
        if (store == null) {
          throw new IllegalStateException(
              "Phase B mirror: Unknown database '" + dbName + "' for " + keyOps.size() + " keys");
        }

        // Step B: Split keys into deletes (no remote read needed) and reads
        List<KeyOperation> deleteOps = new ArrayList<>();
        List<KeyOperation> readOps = new ArrayList<>();

        for (KeyOperation op : keyOps.values()) {
          if (op.isDelete) {
            deleteOps.add(op);
          } else {
            readOps.add(op);
          }
        }

        // Apply deletes locally (no remote calls)
        for (KeyOperation op : deleteOps) {
          store.delete(op.keyBytes);
          successCount++;
        }

        // Process reads: use batchGet or per-key get based on feature flag
        if (readOps.isEmpty()) {
          continue;
        }

        if (batchGetEnabled) {
          // Step C: Batch fetch with chunking
          int[] counts = processBatchReads(dbName, store, storageSPI, readOps, maxBatchKeys, fallbackEnabled);
          successCount += counts[0];
          errorCount += counts[1];
          batchGetCalls += counts[2];
          fallbackGetCalls += counts[3];
        } else {
          // Legacy per-key get path
          int[] counts = processPerKeyReads(dbName, store, storageSPI, readOps);
          successCount += counts[0];
          errorCount += counts[1];
        }
      }

      logger.debug("Phase B mirror: Completed {} keys (success={}, errors={}, batchGetCalls={}, fallbackCalls={})",
          touchedKeys.size(), successCount, errorCount, batchGetCalls, fallbackGetCalls);

    } catch (Exception e) {
      logger.error("Phase B mirror: Failed to refresh local state: {}", e.getMessage(), e);
      throw new RuntimeException("Phase B mirror failed to refresh local state", e);
    }
  }

  private boolean isPostExecMirrorEnabled() {
    return Boolean.parseBoolean(System.getProperty("remote.exec.postexec.mirror", "true"));
  }

  private boolean hasRemoteStateEffects(ExecutionProgramResult result) {
    return (result.getTouchedKeys() != null && !result.getTouchedKeys().isEmpty())
        || (result.getStateChanges() != null && !result.getStateChanges().isEmpty())
        || (result.getFreezeChanges() != null && !result.getFreezeChanges().isEmpty())
        || (result.getGlobalResourceChanges() != null
            && !result.getGlobalResourceChanges().isEmpty())
        || (result.getTrc10Changes() != null && !result.getTrc10Changes().isEmpty())
        || (result.getVoteChanges() != null && !result.getVoteChanges().isEmpty())
        || (result.getWithdrawChanges() != null && !result.getWithdrawChanges().isEmpty())
        || (result.getContractAddress() != null && result.getContractAddress().length > 0);
  }

  private boolean isV1UnfreezeContract(TransactionContext context) {
    if (context == null || context.getTrxCap() == null) {
      return false;
    }

    try {
      return context.getTrxCap().getInstance().getRawData().getContractCount() > 0
          && context.getTrxCap().getInstance().getRawData().getContract(0).getType()
          == org.tron.protos.Protocol.Transaction.Contract.ContractType.UnfreezeBalanceContract;
    } catch (Exception e) {
      logger.debug("Failed to inspect contract type for freeze snapshot parity: {}", e.getMessage());
      return false;
    }
  }

  /**
   * Normalize and dedupe touched keys by database, using last-write-wins semantics.
   *
   * <p>When a key appears multiple times (possibly with mixed delete/update operations),
   * only the last occurrence determines the final operation to apply.
   *
   * @param touchedKeys raw touched keys from execution result
   * @return Map from dbName to LinkedHashMap of KeyWrapper to KeyOperation (preserves last-write order)
   */
  private Map<String, LinkedHashMap<ByteArrayWrapper, KeyOperation>> normalizeTouchedKeys(
      List<ExecutionSPI.TouchedKey> touchedKeys) {

    Map<String, LinkedHashMap<ByteArrayWrapper, KeyOperation>> result = new HashMap<>();

    for (ExecutionSPI.TouchedKey tk : touchedKeys) {
      String dbName = tk.getDb();
      byte[] keyBytes = tk.getKey();
      boolean isDelete = tk.isDelete();

      LinkedHashMap<ByteArrayWrapper, KeyOperation> dbMap =
          result.computeIfAbsent(dbName, k -> new LinkedHashMap<>());

      // Last-write-wins: always overwrite with the latest operation
      ByteArrayWrapper keyWrapper = new ByteArrayWrapper(keyBytes);
      dbMap.put(keyWrapper, new KeyOperation(keyBytes, isDelete));
    }

    return result;
  }

  /**
   * Process batch reads for a database using batchGet with chunking.
   *
   * @return int array: [successCount, errorCount, batchGetCalls, fallbackGetCalls]
   */
  private int[] processBatchReads(
      String dbName,
      TronStoreWithRevoking<?> store,
      StorageSPI storageSPI,
      List<KeyOperation> readOps,
      int maxBatchKeys,
      boolean fallbackEnabled) {

    int successCount = 0;
    int errorCount = 0;
    int batchGetCalls = 0;
    int fallbackGetCalls = 0;

    // Step C: Chunk readOps into batches of up to maxBatchKeys
    for (int i = 0; i < readOps.size(); i += maxBatchKeys) {
      int end = Math.min(i + maxBatchKeys, readOps.size());
      List<KeyOperation> chunk = readOps.subList(i, end);

      // Prepare key list for batchGet
      List<byte[]> chunkKeys = new ArrayList<>(chunk.size());
      for (KeyOperation op : chunk) {
        chunkKeys.add(op.keyBytes);
      }

      try {
        // Call batchGet for this chunk
        Map<byte[], byte[]> values = storageSPI.batchGet(dbName, chunkKeys).get();
        batchGetCalls++;

        // Step D: Apply results using identity-based lookup (RemoteStorageSPI now preserves key identity)
        for (KeyOperation op : chunk) {
          byte[] value = values.get(op.keyBytes);
          if (value != null) {
            store.putRawBytes(op.keyBytes, value);
          } else {
            // Not found in remote - treat as delete (same as current per-key semantics)
            store.delete(op.keyBytes);
          }
          successCount++;
        }

      } catch (Exception e) {
        // Step E: batchGet failed - fallback or count errors
        logger.warn("Phase B mirror: batchGet failed for db={}, chunk size={}: {}",
            dbName, chunk.size(), e.getMessage());

        if (fallbackEnabled) {
          // Fallback to per-key get for this chunk
          int[] fallbackCounts = processPerKeyReads(dbName, store, storageSPI, chunk);
          successCount += fallbackCounts[0];
          errorCount += fallbackCounts[1];
          fallbackGetCalls += chunk.size();
        } else {
          throw new RuntimeException("Phase B mirror batchGet failed for db=" + dbName, e);
        }
      }
    }

    return new int[]{successCount, errorCount, batchGetCalls, fallbackGetCalls};
  }

  /**
   * Process reads using legacy per-key get path (used as fallback or when batchGet disabled).
   *
   * @return int array: [successCount, errorCount]
   */
  private int[] processPerKeyReads(
      String dbName,
      TronStoreWithRevoking<?> store,
      StorageSPI storageSPI,
      List<KeyOperation> readOps) {

    int successCount = 0;
    int errorCount = 0;

    for (KeyOperation op : readOps) {
      try {
        byte[] value = storageSPI.get(dbName, op.keyBytes).get();
        if (value != null) {
          store.putRawBytes(op.keyBytes, value);
        } else {
          // Key doesn't exist in remote, treat as delete
          store.delete(op.keyBytes);
        }
        successCount++;
      } catch (Exception e) {
        throw new RuntimeException("Phase B mirror failed to mirror key in db=" + dbName, e);
      }
    }

    return new int[]{successCount, errorCount};
  }

  /**
   * Helper class to hold normalized key operation (key bytes and final delete flag).
   */
  private static class KeyOperation {
    final byte[] keyBytes;
    final boolean isDelete;

    KeyOperation(byte[] keyBytes, boolean isDelete) {
      this.keyBytes = keyBytes;
      this.isDelete = isDelete;
    }
  }

  private static StorageSPI getMirrorRemoteStorageSPI() {
    StorageSPI current = mirrorRemoteStorageSPI;
    if (current != null) {
      return current;
    }

    synchronized (RuntimeSpiImpl.class) {
      if (mirrorRemoteStorageSPI != null) {
        return mirrorRemoteStorageSPI;
      }

      mirrorRemoteStorageSPI = StorageSpiFactory.createStorage(StorageMode.REMOTE);
      return mirrorRemoteStorageSPI;
    }
  }

  /**
   * Get the store instance for a given database name.
   * Maps canonical db names to ChainBaseManager store instances.
   *
   * @param dbName The database name (from touched_keys)
   * @param chainBaseManager The chain base manager
   * @return The store instance, or null if not found
   */
  private TronStoreWithRevoking<?> getStoreByDbName(String dbName, ChainBaseManager chainBaseManager) {
    // Map db names to stores based on db_names.rs constants
    switch (dbName) {
      // Account stores
      case "account":
        return chainBaseManager.getAccountStore();
      case "account-index":
        return chainBaseManager.getAccountIndexStore();
      case "accountid-index":
        return chainBaseManager.getAccountIdIndexStore();

      // Contract/TVM stores
      case "contract":
        return chainBaseManager.getContractStore();
      case "abi":
        return chainBaseManager.getAbiStore();
      case "code":
        return chainBaseManager.getCodeStore();
      case "contract-state":
        return chainBaseManager.getContractStateStore();
      case "storage-row":
        return chainBaseManager.getStorageRowStore();

      // Governance stores
      case "witness":
        return chainBaseManager.getWitnessStore();
      case "votes":
        return chainBaseManager.getVotesStore();
      case "proposal":
        return chainBaseManager.getProposalStore();

      // Asset/TRC-10 stores
      case "asset-issue":
        return chainBaseManager.getAssetIssueStore();
      case "asset-issue-v2":
        return chainBaseManager.getAssetIssueV2Store();

      // Delegation stores
      case "DelegatedResource":
        return chainBaseManager.getDelegatedResourceStore();
      case "DelegatedResourceAccountIndex":
        return chainBaseManager.getDelegatedResourceAccountIndexStore();
      case "delegation":
        return chainBaseManager.getDelegationStore();

      // Exchange stores
      case "exchange":
        return chainBaseManager.getExchangeStore();
      case "exchange-v2":
        return chainBaseManager.getExchangeV2Store();

      // Market stores
      case "market_account":
        return chainBaseManager.getMarketAccountStore();
      case "market_order":
        return chainBaseManager.getMarketOrderStore();
      case "market_pair_to_price":
        return chainBaseManager.getMarketPairToPriceStore();
      case "market_pair_price_to_order":
        return chainBaseManager.getMarketPairPriceToOrderStore();

      // System stores
      case "properties":
        return chainBaseManager.getDynamicPropertiesStore();

      default:
        logger.debug("Phase B mirror: No mapping for database '{}'", dbName);
        return null;
    }
  }
}
