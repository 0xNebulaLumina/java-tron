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
    private boolean flushed = false;
    
    ResourceSyncData(TransactionContext ctx) {
      this.transactionContext = ctx;
    }
    
    void clear() {
      dirtyAccounts.clear();
      dirtyDynamicKeys.clear();
      dirtyAssetIssueV1Keys.clear();
      dirtyAssetIssueV2Keys.clear();
      flushed = false;
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
      if (this == obj) return true;
      if (!(obj instanceof ByteArrayWrapper)) return false;
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
      logger.warn("ResourceSyncContext already exists for current thread, clearing previous context");
      existing.clear();
    }
    
    ResourceSyncData syncData = new ResourceSyncData(ctx);
    contextThreadLocal.set(syncData);
    
    if (logger.isDebugEnabled()) {
      logger.debug("Initialized ResourceSyncContext for transaction thread {} tx={}",
                   Thread.currentThread().getId(),
                   ctx != null && ctx.getTrxCap() != null ? 
                   org.tron.common.utils.ByteArray.toHexString(ctx.getTrxCap().getTransactionId().getBytes()) : "null");
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
      
      if (logger.isTraceEnabled()) {
        logger.trace("Recorded dirty asset issue V2: {}",
                     org.tron.common.utils.ByteArray.toHexString(assetId));
      }
    }
  }
  
  /**
   * Flush all recorded resource mutations to remote storage.
   * This should be called after all pre-execution resource consumption
   * and before any remote execution operations.
   * 
   * @throws RuntimeException if flush fails
   */
  public static void flushPreExec() {
    ResourceSyncData syncData = contextThreadLocal.get();
    if (syncData == null) {
      return; // No context, nothing to flush
    }
    
    if (syncData.flushed) {
      logger.debug("ResourceSyncContext already flushed for this transaction");
      return;
    }
    
    try {
      ResourceSyncService.getInstance().flushResourceDeltas(
          syncData.transactionContext,
          extractByteArrays(syncData.dirtyAccounts),
          extractByteArrays(syncData.dirtyDynamicKeys),
          extractByteArrays(syncData.dirtyAssetIssueV1Keys),
          extractByteArrays(syncData.dirtyAssetIssueV2Keys)
      );
      
      syncData.flushed = true;
      
      if (logger.isDebugEnabled()) {
        logger.debug("Flushed ResourceSyncContext: {} accounts, {} dynamic keys, {} asset V1, {} asset V2",
                     syncData.dirtyAccounts.size(),
                     syncData.dirtyDynamicKeys.size(),
                     syncData.dirtyAssetIssueV1Keys.size(),
                     syncData.dirtyAssetIssueV2Keys.size());
      }
      
    } catch (RuntimeException e) {
      logger.error("Failed to flush ResourceSyncContext", e);
      throw e;
    } catch (Exception e) {
      logger.error("Failed to flush ResourceSyncContext", e);
      throw new RuntimeException("Failed to flush ResourceSyncContext", e);
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
    return String.format("ResourceSync: %d accounts, %d dynamic keys, %d asset V1, %d asset V2, flushed=%s",
                         syncData.dirtyAccounts.size(),
                         syncData.dirtyDynamicKeys.size(),
                         syncData.dirtyAssetIssueV1Keys.size(),
                         syncData.dirtyAssetIssueV2Keys.size(),
                         syncData.flushed);
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
