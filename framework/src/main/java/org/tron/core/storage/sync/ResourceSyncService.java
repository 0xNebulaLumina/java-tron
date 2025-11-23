package org.tron.core.storage.sync;

import java.util.HashMap;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;
import java.util.stream.Collectors;
import java.nio.charset.StandardCharsets;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Component;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.AssetIssueCapsule;
import org.tron.core.capsule.BytesCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.store.AccountStore;
import org.tron.core.store.AssetIssueStore;
import org.tron.core.store.AssetIssueV2Store;
import org.tron.core.store.DynamicPropertiesStore;
import org.tron.core.storage.spi.StorageMode;
import org.tron.core.storage.spi.StorageSPI;
import org.tron.core.storage.spi.StorageSpiFactory;

/**
 * Service for synchronizing resource mutations to remote storage before EVM execution.
 * 
 * <p>This service handles the batching and flushing of dirty account, dynamic property,
 * and asset issue keys to remote storage to ensure consistency between Java-side
 * resource mutations and subsequent remote execution operations.
 * 
 * <p>The service is enabled only when storage mode is REMOTE and the sync feature
 * is enabled via configuration flags.
 */
@Component
public class ResourceSyncService {
  
  private static final Logger logger = LoggerFactory.getLogger(ResourceSyncService.class);
  
  // Database names as used by the stores
  private static final String ACCOUNT_DB = "account";
  private static final String PROPERTIES_DB = "properties"; 
  private static final String ASSET_ISSUE_DB = "asset-issue";
  private static final String ASSET_ISSUE_V2_DB = "asset-issue-v2";
  
  // Configuration property keys
  private static final String SYNC_ENABLED_PROPERTY = "remote.resource.sync.enabled";
  private static final String SYNC_DEBUG_PROPERTY = "remote.resource.sync.debug";
  private static final String SYNC_CONFIRM_PROPERTY = "remote.resource.sync.confirm";
  
  // Default values
  private static final boolean DEFAULT_SYNC_ENABLED_REMOTE = true;
  private static final boolean DEFAULT_SYNC_ENABLED_EMBEDDED = false;
  private static final boolean DEFAULT_SYNC_DEBUG = false;
  private static final boolean DEFAULT_SYNC_CONFIRM = false;
  
  // Error handling
  private static final int FAILURE_THRESHOLD = 10;
  private static final long FAILURE_WINDOW_MS = 60000; // 1 minute
  
  private static volatile ResourceSyncService instance;
  
  @Autowired
  private AccountStore accountStore;
  
  @Autowired
  private DynamicPropertiesStore dynamicPropertiesStore;
  
  @Autowired
  private AssetIssueStore assetIssueStore;
  
  @Autowired
  private AssetIssueV2Store assetIssueV2Store;
  
  // Thread pool for async operations
  private final ExecutorService executor = Executors.newSingleThreadExecutor(r -> {
    Thread t = new Thread(r, "ResourceSync");
    t.setDaemon(true);
    return t;
  });
  
  // Metrics and circuit breaker
  private final AtomicLong flushCount = new AtomicLong(0);
  private final AtomicLong errorCount = new AtomicLong(0);
  private final AtomicInteger consecutiveFailures = new AtomicInteger(0);
  private volatile long lastFailureTime = 0;
  private volatile boolean circuitBreakerOpen = false;
  
  public ResourceSyncService() {
    instance = this;
  }
  
  /**
   * Get the singleton instance of ResourceSyncService.
   * 
   * @return the service instance
   */
  public static ResourceSyncService getInstance() {
    return instance;
  }
  
  /**
   * Check if resource synchronization is enabled.
   * 
   * @return true if sync is enabled
   */
  public static boolean isEnabled() {
    try {
      StorageMode mode = StorageSpiFactory.determineStorageMode();
      
      // First check if we're in remote mode
      if (mode != StorageMode.REMOTE) {
        // In embedded mode, sync is disabled by default but can be overridden
        return getBooleanProperty(SYNC_ENABLED_PROPERTY, DEFAULT_SYNC_ENABLED_EMBEDDED);
      }
      
      // In remote mode, sync is enabled by default but can be disabled
      return getBooleanProperty(SYNC_ENABLED_PROPERTY, DEFAULT_SYNC_ENABLED_REMOTE);
      
    } catch (Exception e) {
      logger.debug("Error determining if resource sync is enabled: {}", e.getMessage());
      return false;
    }
  }
  
  /**
   * Flush resource deltas for the given dirty keys to remote storage.
   * 
   * @param ctx the transaction context
   * @param dirtyAccounts set of dirty account addresses
   * @param dirtyDynamicKeys set of dirty dynamic property keys
   * @param dirtyAssetIssueV1Keys set of dirty asset issue V1 keys
   * @param dirtyAssetIssueV2Keys set of dirty asset issue V2 keys
   */
  public void flushResourceDeltas(TransactionContext ctx,
                                  Set<byte[]> dirtyAccounts,
                                  Set<byte[]> dirtyDynamicKeys,
                                  Set<byte[]> dirtyAssetIssueV1Keys,
                                  Set<byte[]> dirtyAssetIssueV2Keys) {
    
    if (!isEnabled()) {
      logger.trace("Resource sync is disabled, skipping flush");
      return;
    }
    
    if (circuitBreakerOpen && isInFailureWindow()) {
      logger.debug("Resource sync circuit breaker is open, skipping flush");
      return;
    }
    
    long startTime = System.currentTimeMillis();
    flushCount.incrementAndGet();
    
    try {
      StorageSPI storageSPI = getStorageSPI();
      if (storageSPI == null) {
        logger.warn("StorageSPI not available for resource sync");
        return;
      }
      
      boolean debugEnabled = getBooleanProperty(SYNC_DEBUG_PROPERTY, DEFAULT_SYNC_DEBUG);
      boolean confirmEnabled = getBooleanProperty(SYNC_CONFIRM_PROPERTY, DEFAULT_SYNC_CONFIRM);

      // Temporary visibility: log exact dirty accounts at INFO to confirm blackhole inclusion
      if (debugEnabled) {
        try {
          byte[] blackhole = accountStore != null && accountStore.getBlackhole() != null
              ? accountStore.getBlackhole().getAddress().toByteArray()
              : null;

          final String blackholeBase58 = blackhole != null
              ? org.tron.common.utils.StringUtil.encode58Check(blackhole)
              : "<unknown>";

          boolean includesBlackhole = false;
          // Prepare account list in Base58 for readability
          java.util.List<String> dirtyAccountsB58 = dirtyAccounts.stream()
              .map(addr -> {
                if (blackhole != null && java.util.Arrays.equals(addr, blackhole)) {
                  // mark blackhole explicitly
                  return org.tron.common.utils.StringUtil.encode58Check(addr) + " (blackhole)";
                }
                return org.tron.common.utils.StringUtil.encode58Check(addr);
              })
              .collect(Collectors.toList());

          if (blackhole != null) {
            includesBlackhole = dirtyAccounts.stream().anyMatch(a -> java.util.Arrays.equals(a, blackhole));
          }

          // Dynamic keys (mostly ASCII identifiers) for additional context
          java.util.List<String> dynamicKeysPretty = dirtyDynamicKeys.stream()
              .map(k -> {
                try {
                  String s = new String(k, StandardCharsets.UTF_8);
                  // If looks readable ASCII uppercase/underscore, keep as-is; else hex encode
                  boolean ascii = s.chars().allMatch(ch -> ch >= 32 && ch <= 126);
                  return ascii ? s : org.tron.common.utils.ByteArray.toHexString(k);
                } catch (Exception e) {
                  return org.tron.common.utils.ByteArray.toHexString(k);
                }
              })
              .collect(Collectors.toList());

          logger.info("ResourceSync pre-exec flush: accounts={}, dynamic_keys={}, assetV1={}, assetV2={}, includes_blackhole={}, blackhole={}",
              dirtyAccountsB58.size(), dirtyDynamicKeys.size(), dirtyAssetIssueV1Keys.size(), dirtyAssetIssueV2Keys.size(),
              includesBlackhole, blackholeBase58);
          if (!dirtyAccountsB58.isEmpty()) {
            logger.info("ResourceSync pre-exec flush accounts: {}", String.join(", ", dirtyAccountsB58));
          }
          if (!dynamicKeysPretty.isEmpty()) {
            logger.info("ResourceSync pre-exec flush dynamic keys: {}", String.join(", ", dynamicKeysPretty));
          }
        } catch (Exception logEx) {
          logger.debug("Failed to compose ResourceSync visibility logs: {}", logEx.getMessage());
        }
      }
      
      // Build batches in the correct order: asset issues -> accounts -> dynamic props
      CompletableFuture<Void> flushFuture = CompletableFuture.completedFuture(null);
      
      // 1. Flush asset issue V1 changes
      if (!dirtyAssetIssueV1Keys.isEmpty()) {
        Map<byte[], byte[]> assetV1Batch = buildAssetIssueV1Batch(dirtyAssetIssueV1Keys);
        if (!assetV1Batch.isEmpty()) {
          flushFuture = flushFuture.thenCompose(v -> storageSPI.batchWrite(ASSET_ISSUE_DB, assetV1Batch));
          if (debugEnabled) {
            logger.debug("Batched {} asset issue V1 changes", assetV1Batch.size());
          }
        }
      }
      
      // 2. Flush asset issue V2 changes
      if (!dirtyAssetIssueV2Keys.isEmpty()) {
        Map<byte[], byte[]> assetV2Batch = buildAssetIssueV2Batch(dirtyAssetIssueV2Keys);
        if (!assetV2Batch.isEmpty()) {
          flushFuture = flushFuture.thenCompose(v -> storageSPI.batchWrite(ASSET_ISSUE_V2_DB, assetV2Batch));
          if (debugEnabled) {
            logger.debug("Batched {} asset issue V2 changes", assetV2Batch.size());
          }
        }
      }
      
      // 3. Flush account changes
      if (!dirtyAccounts.isEmpty()) {
        Map<byte[], byte[]> accountBatch = buildAccountBatch(dirtyAccounts);
        if (!accountBatch.isEmpty()) {
          flushFuture = flushFuture.thenCompose(v -> storageSPI.batchWrite(ACCOUNT_DB, accountBatch));
          if (debugEnabled) {
            logger.debug("Batched {} account changes", accountBatch.size());
          }
        }
      }
      
      // 4. Flush dynamic property changes
      if (!dirtyDynamicKeys.isEmpty()) {
        Map<byte[], byte[]> dynamicBatch = buildDynamicPropertiesBatch(dirtyDynamicKeys);
        if (!dynamicBatch.isEmpty()) {
          flushFuture = flushFuture.thenCompose(v -> storageSPI.batchWrite(PROPERTIES_DB, dynamicBatch));
          if (debugEnabled) {
            logger.debug("Batched {} dynamic property changes", dynamicBatch.size());
          }
        }
      }
      
      // Wait for all flushes to complete
      flushFuture.get();
      
      // Optional confirmation reads
      if (confirmEnabled) {
        performConfirmationReads(storageSPI, dirtyAccounts, dirtyDynamicKeys, 
                                dirtyAssetIssueV1Keys, dirtyAssetIssueV2Keys);
      }
      
      // Reset circuit breaker on success
      consecutiveFailures.set(0);
      circuitBreakerOpen = false;
      
      long duration = System.currentTimeMillis() - startTime;
      if (debugEnabled) {
        String txId = (ctx != null && ctx.getTrxCap() != null) ? 
            org.tron.common.utils.ByteArray.toHexString(ctx.getTrxCap().getTransactionId().getBytes()) : "unknown";
        logger.debug("Flushed resource deltas for tx {} in {}ms: {} accounts, {} dynamic, {} asset V1, {} asset V2",
                     txId, duration, 
                     dirtyAccounts.size(), dirtyDynamicKeys.size(), 
                     dirtyAssetIssueV1Keys.size(), dirtyAssetIssueV2Keys.size());
      }
      
    } catch (Exception e) {
      errorCount.incrementAndGet();
      int failures = consecutiveFailures.incrementAndGet();
      lastFailureTime = System.currentTimeMillis();
      
      if (failures >= FAILURE_THRESHOLD) {
        circuitBreakerOpen = true;
        logger.warn("Resource sync circuit breaker opened after {} consecutive failures", failures);
      }
      
      logger.error("Failed to flush resource deltas (failure count: {})", failures, e);
    }
  }
  
  /**
   * Get the current StorageSPI instance.
   */
  private StorageSPI getStorageSPI() {
    try {
      return StorageSpiFactory.createStorage();
    } catch (Exception e) {
      logger.error("Failed to get StorageSPI instance", e);
      return null;
    }
  }
  
  /**
   * Build batch of account changes.
   */
  private Map<byte[], byte[]> buildAccountBatch(Set<byte[]> dirtyAccounts) {
    Map<byte[], byte[]> batch = new HashMap<>();
    
    for (byte[] address : dirtyAccounts) {
      try {
        AccountCapsule account = accountStore.getUnchecked(address);
        if (account != null) {
          batch.put(address, account.getData());
        }
      } catch (Exception e) {
        logger.warn("Failed to read account for sync: {}", 
                    org.tron.common.utils.ByteArray.toHexString(address), e);
      }
    }
    
    return batch;
  }
  
  /**
   * Build batch of dynamic property changes.
   */
  private Map<byte[], byte[]> buildDynamicPropertiesBatch(Set<byte[]> dirtyKeys) {
    Map<byte[], byte[]> batch = new HashMap<>();
    
    for (byte[] key : dirtyKeys) {
      try {
        BytesCapsule value = dynamicPropertiesStore.getUnchecked(key);
        if (value != null) {
          batch.put(key, value.getData());
        }
      } catch (Exception e) {
        logger.warn("Failed to read dynamic property for sync: {}", 
                    org.tron.common.utils.ByteArray.toHexString(key), e);
      }
    }
    
    return batch;
  }
  
  /**
   * Build batch of asset issue V1 changes.
   */
  private Map<byte[], byte[]> buildAssetIssueV1Batch(Set<byte[]> dirtyKeys) {
    Map<byte[], byte[]> batch = new HashMap<>();
    
    for (byte[] key : dirtyKeys) {
      try {
        AssetIssueCapsule asset = assetIssueStore.get(key);
        if (asset != null) {
          batch.put(key, asset.getData());
        }
      } catch (Exception e) {
        logger.warn("Failed to read asset issue V1 for sync: {}", 
                    org.tron.common.utils.ByteArray.toHexString(key), e);
      }
    }
    
    return batch;
  }
  
  /**
   * Build batch of asset issue V2 changes.
   */
  private Map<byte[], byte[]> buildAssetIssueV2Batch(Set<byte[]> dirtyKeys) {
    Map<byte[], byte[]> batch = new HashMap<>();
    
    for (byte[] key : dirtyKeys) {
      try {
        AssetIssueCapsule asset = assetIssueV2Store.get(key);
        if (asset != null) {
          batch.put(key, asset.getData());
        }
      } catch (Exception e) {
        logger.warn("Failed to read asset issue V2 for sync: {}", 
                    org.tron.common.utils.ByteArray.toHexString(key), e);
      }
    }
    
    return batch;
  }
  
  /**
   * Perform confirmation reads to verify flush success.
   */
  private void performConfirmationReads(StorageSPI storageSPI,
                                       Set<byte[]> dirtyAccounts,
                                       Set<byte[]> dirtyDynamicKeys,
                                       Set<byte[]> dirtyAssetIssueV1Keys,
                                       Set<byte[]> dirtyAssetIssueV2Keys) {
    try {
      int confirmed = 0, missed = 0;
      
      // Check a few accounts
      int checked = 0;
      for (byte[] address : dirtyAccounts) {
        if (checked++ >= 3) break; // Limit confirmation checks
        
        try {
          byte[] remoteValue = storageSPI.get(ACCOUNT_DB, address).get();
          if (remoteValue != null) {
            confirmed++;
          } else {
            missed++;
            logger.debug("Confirmation miss for account: {}", 
                        org.tron.common.utils.ByteArray.toHexString(address));
          }
        } catch (Exception e) {
          logger.debug("Confirmation check failed for account: {}", 
                       org.tron.common.utils.ByteArray.toHexString(address), e);
        }
      }
      
      // Check a few dynamic properties
      checked = 0;
      for (byte[] key : dirtyDynamicKeys) {
        if (checked++ >= 3) break; // Limit confirmation checks
        
        try {
          byte[] remoteValue = storageSPI.get(PROPERTIES_DB, key).get();
          if (remoteValue != null) {
            confirmed++;
          } else {
            missed++;
            logger.debug("Confirmation miss for dynamic property: {}", 
                        org.tron.common.utils.ByteArray.toHexString(key));
          }
        } catch (Exception e) {
          logger.debug("Confirmation check failed for dynamic property: {}", 
                       org.tron.common.utils.ByteArray.toHexString(key), e);
        }
      }
      
      if (confirmed > 0 || missed > 0) {
        logger.debug("Confirmation results: {} confirmed, {} missed", confirmed, missed);
      }
      
    } catch (Exception e) {
      logger.debug("Error during confirmation reads", e);
    }
  }
  
  /**
   * Check if we're still in the failure window for circuit breaker.
   */
  private boolean isInFailureWindow() {
    return (System.currentTimeMillis() - lastFailureTime) < FAILURE_WINDOW_MS;
  }
  
  /**
   * Get metrics about sync operations.
   */
  public String getMetrics() {
    return String.format("ResourceSync Metrics: flushes=%d, errors=%d, failures=%d, circuitOpen=%s",
                         flushCount.get(), errorCount.get(), consecutiveFailures.get(), circuitBreakerOpen);
  }
  
  /**
   * Get boolean property with default value.
   */
  private static boolean getBooleanProperty(String key, boolean defaultValue) {
    try {
      String value = System.getProperty(key);
      if (value != null) {
        return Boolean.parseBoolean(value.trim());
      }
    } catch (Exception e) {
      logger.debug("Error reading property {}: {}", key, e.getMessage());
    }
    return defaultValue;
  }
  
  /**
   * Shutdown the service and its thread pool.
   */
  public void shutdown() {
    executor.shutdown();
  }
}
