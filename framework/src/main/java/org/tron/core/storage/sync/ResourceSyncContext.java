package org.tron.core.storage.sync;

import java.util.HashSet;
import java.util.Set;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.db.TransactionContext;

/**
 * Thread-local context for tracking resource mutations that need to be synced
 * to remote storage before EVM execution.
 * 
 * <p>This class tracks dirty keys for accounts, dynamic properties, and asset issues
 * that have been modified during pre-execution resource consumption (bandwidth,
 * energy, fees) and need to be flushed to remote storage to ensure consistency
 * before any subsequent remote execution.
 * 
 * <p>This implementation also acts as a delegate for the chainbase ResourceSyncContext,
 * allowing chainbase processors to record dirty keys through delegation.
 * 
 * <p>Usage pattern:
 * <pre>
 *   // At transaction start (before resource consumption)
 *   ResourceSyncContext.begin(transactionContext);
 *   
 *   // During resource consumption
 *   ResourceSyncContext.recordAccountDirty(ownerAddress);
 *   ResourceSyncContext.recordDynamicKeyDirty(TOTAL_TRANSACTION_COST);
 *   
 *   // Before remote execution
 *   ResourceSyncContext.flushPreExec();
 *   
 *   // At transaction end
 *   ResourceSyncContext.finish();
 * </pre>
 */
public class ResourceSyncContext {
  
  private static final Logger logger = LoggerFactory.getLogger(ResourceSyncContext.class);
  
  private static final ThreadLocal<ResourceSyncData> contextThreadLocal = new ThreadLocal<>();
  
  /**
   * Thread-local data structure holding dirty resource keys for the current transaction.
   */
  private static class ResourceSyncData {
    private final TransactionContext transactionContext;
    private final Set<ByteArrayWrapper> dirtyAccounts = new HashSet<>();
    private final Set<ByteArrayWrapper> dirtyDynamicKeys = new HashSet<>();
    private final Set<ByteArrayWrapper> dirtyAssetIssueV1Keys = new HashSet<>();
    private final Set<ByteArrayWrapper> dirtyAssetIssueV2Keys = new HashSet<>();
    private boolean dirtySinceFlush = false;

    ResourceSyncData(TransactionContext ctx) {
      this.transactionContext = ctx;
    }

    void clear() {
      dirtyAccounts.clear();
      dirtyDynamicKeys.clear();
      dirtyAssetIssueV1Keys.clear();
      dirtyAssetIssueV2Keys.clear();
      dirtySinceFlush = false;
    }
  }
  
  /**
   * Wrapper for byte arrays to allow proper equality/hashing in HashSet.
   */
  private static class ByteArrayWrapper {
    private final byte[] data;
    
    ByteArrayWrapper(byte[] data) {
      this.data = data != null ? data.clone() : new byte[0];
    }
    
    byte[] getData() {
      return data.clone();
    }
    
    @Override
    public boolean equals(Object obj) {
      if (this == obj) {
        return true;
      }
      if (!(obj instanceof ByteArrayWrapper)) {
        return false;
      }
      return java.util.Arrays.equals(data, ((ByteArrayWrapper) obj).data);
    }
    
    @Override
    public int hashCode() {
      return java.util.Arrays.hashCode(data);
    }
  }
  
  /**
   * Initialize a new resource sync context for the current transaction thread.
   * This should be called once at the beginning of each transaction, before
   * any resource consumption operations.
   * 
   * @param ctx the transaction context
   */
  public static void begin(TransactionContext ctx) {
    if (!ResourceSyncService.isEnabled()) {
      return; // Don't create context if sync is disabled
    }
    
    ResourceSyncData existing = contextThreadLocal.get();
    if (existing != null) {
      logger.warn("ResourceSyncContext already exists for current thread, "
          + "clearing previous context");
      existing.clear();
    }
    
    ResourceSyncData syncData = new ResourceSyncData(ctx);
    contextThreadLocal.set(syncData);
    
    if (logger.isDebugEnabled()) {
      String txId = ctx != null && ctx.getTrxCap() != null
          ? org.tron.common.utils.ByteArray.toHexString(
              ctx.getTrxCap().getTransactionId().getBytes())
          : "null";
      logger.debug("Initialized ResourceSyncContext for transaction thread {} tx={}",
                   Thread.currentThread().getId(), txId);
    }
  }
  
  /**
   * Record that an account has been modified and needs to be synced.
   *
   * @param address the account address
   */
  public static void recordAccountDirty(byte[] address) {
    ResourceSyncData syncData = contextThreadLocal.get();
    if (syncData != null && address != null) {
      syncData.dirtyAccounts.add(new ByteArrayWrapper(address));
      syncData.dirtySinceFlush = true;

      if (logger.isTraceEnabled()) {
        logger.trace("Recorded dirty account: {}",
                     org.tron.common.utils.ByteArray.toHexString(address));
      }
    }
  }
  
  /**
   * Record that a dynamic property has been modified and needs to be synced.
   *
   * @param key the dynamic property key
   */
  public static void recordDynamicKeyDirty(byte[] key) {
    ResourceSyncData syncData = contextThreadLocal.get();
    if (syncData != null && key != null) {
      syncData.dirtyDynamicKeys.add(new ByteArrayWrapper(key));
      syncData.dirtySinceFlush = true;

      if (logger.isTraceEnabled()) {
        logger.trace("Recorded dirty dynamic key: {}",
                     org.tron.common.utils.ByteArray.toHexString(key));
      }
    }
  }
  
  /**
   * Record that an asset issue (V1) has been modified and needs to be synced.
   *
   * @param assetName the asset name
   */
  public static void recordAssetIssueDirtyV1(byte[] assetName) {
    ResourceSyncData syncData = contextThreadLocal.get();
    if (syncData != null && assetName != null) {
      syncData.dirtyAssetIssueV1Keys.add(new ByteArrayWrapper(assetName));
      syncData.dirtySinceFlush = true;

      if (logger.isTraceEnabled()) {
        logger.trace("Recorded dirty asset issue V1: {}",
                     org.tron.common.utils.ByteArray.toHexString(assetName));
      }
    }
  }
  
  /**
   * Record that an asset issue (V2) has been modified and needs to be synced.
   *
   * @param assetId the asset ID
   */
  public static void recordAssetIssueDirtyV2(byte[] assetId) {
    ResourceSyncData syncData = contextThreadLocal.get();
    if (syncData != null && assetId != null) {
      syncData.dirtyAssetIssueV2Keys.add(new ByteArrayWrapper(assetId));
      syncData.dirtySinceFlush = true;

      if (logger.isTraceEnabled()) {
        logger.trace("Recorded dirty asset issue V2: {}",
                     org.tron.common.utils.ByteArray.toHexString(assetId));
      }
    }
  }
  
  /**
   * Flush all recorded resource mutations to remote storage (pre-execution phase).
   * This should be called after all pre-execution resource consumption
   * and before any remote execution operations.
   *
   * @return true if flush was performed, false if skipped (nothing dirty)
   */
  public static boolean flushPreExec() {
    return flushInternal("pre-exec");
  }

  /**
   * Flush all recorded resource mutations to remote storage (post-execution phase).
   * This should be called after remote execution has applied TRC-10 mutations
   * to ensure they are visible to the next transaction.
   *
   * @return true if flush was performed, false if skipped (nothing dirty)
   */
  public static boolean flushPostExec() {
    return flushInternal("post-exec");
  }

  /**
   * Internal flush implementation supporting multi-phase flushing.
   * This method flushes mutations to remote storage only if there are dirty keys
   * that have been recorded since the last flush.
   *
   * @param stage the flush stage name (for logging)
   * @return true if flush was performed, false if skipped
   */
  private static boolean flushInternal(String stage) {
    ResourceSyncData syncData = contextThreadLocal.get();
    if (syncData == null) {
      if (logger.isDebugEnabled()) {
        logger.debug("No ResourceSyncContext active, skipping {} flush", stage);
      }
      return false; // No context, nothing to flush
    }

    // Check if there are any dirty keys AND if anything changed since last flush
    boolean hasDirtyKeys = !syncData.dirtyAccounts.isEmpty()
        || !syncData.dirtyDynamicKeys.isEmpty()
        || !syncData.dirtyAssetIssueV1Keys.isEmpty()
        || !syncData.dirtyAssetIssueV2Keys.isEmpty();

    if (!hasDirtyKeys || !syncData.dirtySinceFlush) {
      if (logger.isDebugEnabled()) {
        logger.debug("Skipping {} flush: hasDirtyKeys={}, dirtySinceFlush={}",
                     stage, hasDirtyKeys, syncData.dirtySinceFlush);
      }
      return false;
    }

    try {
      int accountCount = syncData.dirtyAccounts.size();
      int dynamicCount = syncData.dirtyDynamicKeys.size();
      int assetV1Count = syncData.dirtyAssetIssueV1Keys.size();
      int assetV2Count = syncData.dirtyAssetIssueV2Keys.size();

      ResourceSyncService.getInstance().flushResourceDeltas(
          syncData.transactionContext,
          extractByteArrays(syncData.dirtyAccounts),
          extractByteArrays(syncData.dirtyDynamicKeys),
          extractByteArrays(syncData.dirtyAssetIssueV1Keys),
          extractByteArrays(syncData.dirtyAssetIssueV2Keys)
      );

      // Clear dirty sets and reset flag after successful flush
      syncData.dirtyAccounts.clear();
      syncData.dirtyDynamicKeys.clear();
      syncData.dirtyAssetIssueV1Keys.clear();
      syncData.dirtyAssetIssueV2Keys.clear();
      syncData.dirtySinceFlush = false;

      if (logger.isDebugEnabled()) {
        logger.debug("Successfully flushed {} mutations: {} accounts, {} dynamic keys, "
            + "{} asset V1, {} asset V2",
            stage, accountCount, dynamicCount, assetV1Count, assetV2Count);
      }

      return true;

    } catch (Exception e) {
      logger.error("Failed to flush {} mutations", stage, e);
      // Don't throw - this shouldn't abort transaction execution
      return false;
    }
  }
  
  /**
   * Clear the resource sync context for the current thread.
   * This should be called at the end of transaction processing.
   */
  public static void finish() {
    ResourceSyncData syncData = contextThreadLocal.get();
    if (syncData != null) {
      syncData.clear();
      contextThreadLocal.remove();
      
      if (logger.isDebugEnabled()) {
        logger.debug("Finished ResourceSyncContext for transaction thread {}", 
                     Thread.currentThread().getId());
      }
    }
  }
  
  /**
   * Get current context metrics (for monitoring).
   */
  public static String getCurrentMetrics() {
    ResourceSyncData syncData = contextThreadLocal.get();
    if (syncData == null) {
      return "No resource sync context active";
    }
    return String.format("ResourceSync: %d accounts, %d dynamic keys, %d asset V1, "
        + "%d asset V2, dirtySinceFlush=%s",
        syncData.dirtyAccounts.size(),
        syncData.dirtyDynamicKeys.size(),
        syncData.dirtyAssetIssueV1Keys.size(),
        syncData.dirtyAssetIssueV2Keys.size(),
        syncData.dirtySinceFlush);
  }

  /**
   * Get count of dirty accounts in current context.
   */
  public static int getDirtyAccountCount() {
    ResourceSyncData syncData = contextThreadLocal.get();
    return syncData != null ? syncData.dirtyAccounts.size() : 0;
  }

  /**
   * Get count of dirty dynamic keys in current context.
   */
  public static int getDirtyDynamicKeyCount() {
    ResourceSyncData syncData = contextThreadLocal.get();
    return syncData != null ? syncData.dirtyDynamicKeys.size() : 0;
  }

  /**
   * Get count of dirty asset V1 keys in current context.
   */
  public static int getDirtyAssetV1Count() {
    ResourceSyncData syncData = contextThreadLocal.get();
    return syncData != null ? syncData.dirtyAssetIssueV1Keys.size() : 0;
  }

  /**
   * Get count of dirty asset V2 keys in current context.
   */
  public static int getDirtyAssetV2Count() {
    ResourceSyncData syncData = contextThreadLocal.get();
    return syncData != null ? syncData.dirtyAssetIssueV2Keys.size() : 0;
  }
  
  /**
   * Check if a resource sync context is active for the current thread.
   */
  public static boolean hasActiveContext() {
    return contextThreadLocal.get() != null;
  }
  
  /**
   * Clear context for current thread (emergency cleanup).
   */
  public static void clearForCurrentTransaction() {
    ResourceSyncData syncData = contextThreadLocal.get();
    if (syncData != null) {
      syncData.clear();
      contextThreadLocal.remove();
      logger.debug("Cleared ResourceSyncContext for transaction thread {}",
                   Thread.currentThread().getId());
    }
  }
  
  /**
   * Extract byte arrays from a set of wrappers.
   */
  private static Set<byte[]> extractByteArrays(Set<ByteArrayWrapper> wrappers) {
    Set<byte[]> result = new HashSet<>();
    for (ByteArrayWrapper wrapper : wrappers) {
      result.add(wrapper.getData());
    }
    return result;
  }
}