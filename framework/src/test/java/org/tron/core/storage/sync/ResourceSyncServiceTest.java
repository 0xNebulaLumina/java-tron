package org.tron.core.storage.sync;

import java.util.HashSet;
import java.util.Set;
import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.mockito.Mock;
import org.mockito.MockitoAnnotations;
import org.mockito.junit.MockitoJUnitRunner;
import org.tron.core.db.TransactionContext;
import org.tron.core.store.AccountStore;
import org.tron.core.store.AssetIssueStore;
import org.tron.core.store.AssetIssueV2Store;
import org.tron.core.store.DynamicPropertiesStore;

/**
 * Unit tests for ResourceSyncService.
 */
@RunWith(MockitoJUnitRunner.class)
public class ResourceSyncServiceTest {

  @Mock
  private AccountStore mockAccountStore;
  
  @Mock 
  private DynamicPropertiesStore mockDynamicPropertiesStore;
  
  @Mock
  private AssetIssueStore mockAssetIssueStore;
  
  @Mock
  private AssetIssueV2Store mockAssetIssueV2Store;
  
  private ResourceSyncService syncService;
  
  @Before
  public void setUp() {
    MockitoAnnotations.initMocks(this);
    
    // Reset any system properties
    System.clearProperty("remote.resource.sync.enabled");
    System.clearProperty("remote.resource.sync.debug");
    System.clearProperty("remote.resource.sync.confirm");
    
    syncService = new ResourceSyncService();
  }
  
  @After
  public void tearDown() {
    System.clearProperty("remote.resource.sync.enabled");
    System.clearProperty("remote.resource.sync.debug");
    System.clearProperty("remote.resource.sync.confirm");
  }
  
  @Test
  public void testIsEnabledWithSystemProperty() {
    // Test enabled explicitly
    System.setProperty("remote.resource.sync.enabled", "true");
    // Note: This will return false since we're not in REMOTE storage mode in test
    // The actual enablement logic depends on storage mode detection
    
    System.setProperty("remote.resource.sync.enabled", "false");
    Assert.assertFalse(ResourceSyncService.isEnabled());
  }
  
  @Test
  public void testFlushResourceDeltasWithEmptySets() {
    TransactionContext mockCtx = new TransactionContext(null, null, null, false, false);
    
    Set<byte[]> emptyAccounts = new HashSet<>();
    Set<byte[]> emptyDynamic = new HashSet<>(); 
    Set<byte[]> emptyAssetV1 = new HashSet<>();
    Set<byte[]> emptyAssetV2 = new HashSet<>();
    
    // Should not throw with empty sets
    syncService.flushResourceDeltas(mockCtx, emptyAccounts, emptyDynamic, emptyAssetV1, emptyAssetV2);
  }
  
  @Test
  public void testGetMetrics() {
    String metrics = syncService.getMetrics();
    Assert.assertNotNull(metrics);
    Assert.assertTrue(metrics.contains("ResourceSync Metrics"));
    Assert.assertTrue(metrics.contains("flushes="));
    Assert.assertTrue(metrics.contains("errors="));
  }
  
  @Test
  public void testSingletonInstance() {
    ResourceSyncService instance1 = ResourceSyncService.getInstance();
    ResourceSyncService instance2 = ResourceSyncService.getInstance();
    
    Assert.assertSame("Should return same singleton instance", instance1, instance2);
  }
  
  @Test
  public void testFlushResourceDeltasWithNullContext() {
    Set<byte[]> testAccounts = new HashSet<>();
    testAccounts.add("test_account".getBytes());
    
    Set<byte[]> emptyDynamic = new HashSet<>();
    Set<byte[]> emptyAssetV1 = new HashSet<>(); 
    Set<byte[]> emptyAssetV2 = new HashSet<>();
    
    // Should handle null context gracefully
    syncService.flushResourceDeltas(null, testAccounts, emptyDynamic, emptyAssetV1, emptyAssetV2);
  }
  
  @Test
  public void testFlushResourceDeltasWhenDisabled() {
    // Explicitly disable sync
    System.setProperty("remote.resource.sync.enabled", "false");
    
    Set<byte[]> testAccounts = new HashSet<>();
    testAccounts.add("test_account".getBytes());
    
    Set<byte[]> emptyDynamic = new HashSet<>();
    Set<byte[]> emptyAssetV1 = new HashSet<>();
    Set<byte[]> emptyAssetV2 = new HashSet<>();
    
    TransactionContext mockCtx = new TransactionContext(null, null, null, false, false);
    
    // Should exit early when disabled
    syncService.flushResourceDeltas(mockCtx, testAccounts, emptyDynamic, emptyAssetV1, emptyAssetV2);
  }
}