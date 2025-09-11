package org.tron.core.storage.sync;

import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.tron.core.db.TransactionContext;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.store.StoreFactory;

/**
 * Unit tests for ResourceSyncContext.
 */
public class ResourceSyncContextTest {

  private TransactionContext mockTxContext;
  
  @Before
  public void setUp() {
    // Clear any existing context
    ResourceSyncContext.clearForCurrentTransaction();
    
    // Create a minimal mock transaction context
    mockTxContext = new TransactionContext(
        null, // blockCap
        null, // trxCap  
        null, // storeFactory
        false, // isStatic
        false  // eventPluginLoaded
    );
  }
  
  @After
  public void tearDown() {
    ResourceSyncContext.clearForCurrentTransaction();
  }
  
  @Test
  public void testBeginAndFinishLifecycle() {
    // Initially no active context
    Assert.assertFalse(ResourceSyncContext.hasActiveContext());
    
    // Begin should create context
    ResourceSyncContext.begin(mockTxContext);
    Assert.assertTrue(ResourceSyncContext.hasActiveContext());
    
    // Finish should clear context
    ResourceSyncContext.finish();
    Assert.assertFalse(ResourceSyncContext.hasActiveContext());
  }
  
  @Test
  public void testRecordDirtyOperations() {
    ResourceSyncContext.begin(mockTxContext);
    
    // Record various dirty operations
    byte[] testAccount = "test_account".getBytes();
    byte[] testDynamicKey = "test_dynamic_key".getBytes();
    byte[] testAssetV1 = "test_asset_v1".getBytes();
    byte[] testAssetV2 = "test_asset_v2".getBytes();
    
    ResourceSyncContext.recordAccountDirty(testAccount);
    ResourceSyncContext.recordDynamicKeyDirty(testDynamicKey);
    ResourceSyncContext.recordAssetIssueDirtyV1(testAssetV1);
    ResourceSyncContext.recordAssetIssueDirtyV2(testAssetV2);
    
    // Context should still be active
    Assert.assertTrue(ResourceSyncContext.hasActiveContext());
    
    // Verify metrics show non-empty context
    String metrics = ResourceSyncContext.getCurrentMetrics();
    Assert.assertTrue(metrics.contains("1 accounts"));
    Assert.assertTrue(metrics.contains("1 dynamic keys"));
    Assert.assertTrue(metrics.contains("1 asset V1"));
    Assert.assertTrue(metrics.contains("1 asset V2"));
    
    ResourceSyncContext.finish();
  }
  
  @Test
  public void testFlushPreExecWithoutContext() {
    // Should not throw when no context exists
    ResourceSyncContext.flushPreExec();
  }
  
  @Test
  public void testRecordOperationsWithoutContext() {
    // Should not throw when no context exists
    ResourceSyncContext.recordAccountDirty("test".getBytes());
    ResourceSyncContext.recordDynamicKeyDirty("test".getBytes());
    ResourceSyncContext.recordAssetIssueDirtyV1("test".getBytes());
    ResourceSyncContext.recordAssetIssueDirtyV2("test".getBytes());
  }
  
  @Test
  public void testThreadLocalIsolation() {
    ResourceSyncContext.begin(mockTxContext);
    ResourceSyncContext.recordAccountDirty("account1".getBytes());
    
    // Create another thread and verify it has no context
    final boolean[] otherThreadHasContext = {false};
    Thread otherThread = new Thread(() -> {
      otherThreadHasContext[0] = ResourceSyncContext.hasActiveContext();
    });
    
    try {
      otherThread.start();
      otherThread.join();
    } catch (InterruptedException e) {
      Thread.currentThread().interrupt();
    }
    
    // Other thread should not see our context
    Assert.assertFalse(otherThreadHasContext[0]);
    
    // Current thread should still have context
    Assert.assertTrue(ResourceSyncContext.hasActiveContext());
    
    ResourceSyncContext.finish();
  }
  
  @Test
  public void testDoubleBeginWarning() {
    ResourceSyncContext.begin(mockTxContext);
    Assert.assertTrue(ResourceSyncContext.hasActiveContext());
    
    // Second begin should warn but not fail
    ResourceSyncContext.begin(mockTxContext);
    Assert.assertTrue(ResourceSyncContext.hasActiveContext());
    
    ResourceSyncContext.finish();
  }
  
  @Test
  public void testClearForCurrentTransaction() {
    ResourceSyncContext.begin(mockTxContext);
    ResourceSyncContext.recordAccountDirty("test".getBytes());
    Assert.assertTrue(ResourceSyncContext.hasActiveContext());
    
    ResourceSyncContext.clearForCurrentTransaction();
    Assert.assertFalse(ResourceSyncContext.hasActiveContext());
  }
  
  @Test
  public void testDeduplication() {
    ResourceSyncContext.begin(mockTxContext);
    
    byte[] testAccount = "test_account".getBytes();
    
    // Record same account multiple times
    ResourceSyncContext.recordAccountDirty(testAccount);
    ResourceSyncContext.recordAccountDirty(testAccount);
    ResourceSyncContext.recordAccountDirty(testAccount);
    
    // Should still show only 1 account in metrics
    String metrics = ResourceSyncContext.getCurrentMetrics();
    Assert.assertTrue(metrics.contains("1 accounts"));
    
    ResourceSyncContext.finish();
  }
}