package org.tron.core.storage.sync;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * ResourceSyncContext for chainbase module.
 * This provides hooks that delegate to a framework implementation when available.
 * 
 * The chainbase module doesn't have access to full transaction context or storage SPI,
 * so this delegates to the framework's implementation via a callback interface.
 */
public class ResourceSyncContext {
  
  private static final Logger logger = LoggerFactory.getLogger(ResourceSyncContext.class);
  
  /**
   * Interface for delegation to framework implementation.
   */
  public interface ResourceSyncDelegate {
    void recordAccountDirty(byte[] address);
    void recordDynamicKeyDirty(byte[] key);
    void recordAssetIssueDirtyV1(byte[] assetName);
    void recordAssetIssueDirtyV2(byte[] assetId);
  }
  
  private static volatile ResourceSyncDelegate delegate = null;
  
  /**
   * Set the delegate implementation (called by framework during initialization).
   */
  public static void setDelegate(ResourceSyncDelegate newDelegate) {
    delegate = newDelegate;
    if (logger.isDebugEnabled()) {
      logger.debug("ResourceSyncContext delegate set: {}", newDelegate != null ? "enabled" : "disabled");
    }
  }
  
  /**
   * Check if sync is enabled (has a delegate).
   */
  public static boolean isEnabled() {
    return delegate != null;
  }
  
  /**
   * Record that an account has been modified and needs to be synced.
   */
  public static void recordAccountDirty(byte[] address) {
    ResourceSyncDelegate currentDelegate = delegate;
    if (currentDelegate != null && address != null) {
      try {
        currentDelegate.recordAccountDirty(address);
      } catch (Exception e) {
        logger.warn("Error recording dirty account", e);
      }
    }
  }
  
  /**
   * Record that a dynamic property has been modified and needs to be synced.
   */
  public static void recordDynamicKeyDirty(byte[] key) {
    ResourceSyncDelegate currentDelegate = delegate;
    if (currentDelegate != null && key != null) {
      try {
        currentDelegate.recordDynamicKeyDirty(key);
      } catch (Exception e) {
        logger.warn("Error recording dirty dynamic key", e);
      }
    }
  }
  
  /**
   * Record that an asset issue (V1) has been modified and needs to be synced.
   */
  public static void recordAssetIssueDirtyV1(byte[] assetName) {
    ResourceSyncDelegate currentDelegate = delegate;
    if (currentDelegate != null && assetName != null) {
      try {
        currentDelegate.recordAssetIssueDirtyV1(assetName);
      } catch (Exception e) {
        logger.warn("Error recording dirty asset issue V1", e);
      }
    }
  }
  
  /**
   * Record that an asset issue (V2) has been modified and needs to be synced.
   */
  public static void recordAssetIssueDirtyV2(byte[] assetId) {
    ResourceSyncDelegate currentDelegate = delegate;
    if (currentDelegate != null && assetId != null) {
      try {
        currentDelegate.recordAssetIssueDirtyV2(assetId);
      } catch (Exception e) {
        logger.warn("Error recording dirty asset issue V2", e);
      }
    }
  }
}