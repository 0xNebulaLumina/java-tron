package org.tron.core.execution.reporting;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;

import java.util.List;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.db.StateChangeRecorderContext;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.protos.Protocol.Account;

/**
 * Integration test to verify account change recording integration works correctly.
 */
public class AccountHookIntegrationTest {

  @Before
  public void setUp() {
    // Enable state change collection for tests
    System.setProperty("exec.csv.stateChanges.enabled", "true");
    // Clean state
    StateChangeJournalRegistry.clearForCurrentTransaction();
    StateChangeRecorderContext.clear();
  }

  @After
  public void tearDown() {
    // Clean up system properties and state
    StateChangeJournalRegistry.clearForCurrentTransaction();
    StateChangeRecorderContext.clear();
    System.clearProperty("exec.csv.stateChanges.enabled");
  }

  @Test
  public void testAccountCreationRecording() {
    // Initialize journal and recorder bridge
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());
    
    // Simulate account creation (via StateChangeRecorderContext)
    byte[] address = hexToBytes("0x123456789abcdef0");
    AccountCapsule newAccount = createTestAccount(1000L, System.currentTimeMillis());
    
    StateChangeRecorderContext.recordAccountChange(address, null, newAccount);
    
    // Finalize and check captured changes
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should capture 1 account creation", 1, changes.size());
    
    StateChange change = changes.get(0);
    assertEquals("123456789abcdef0", bytesToHex(change.getAddress()));
    assertEquals("", bytesToHex(change.getKey())); // Empty key for account change
    assertEquals("", bytesToHex(change.getOldValue())); // null old account
    assertTrue("New account should be serialized", change.getNewValue().length > 0);
  }
  
  @Test
  public void testAccountBalanceChangeRecording() {
    // Initialize journal and recorder bridge
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());
    
    // Simulate balance change
    byte[] address = hexToBytes("0x123456789abcdef0");
    AccountCapsule oldAccount = createTestAccount(1000L, System.currentTimeMillis());
    AccountCapsule newAccount = createTestAccount(2000L, System.currentTimeMillis());
    
    StateChangeRecorderContext.recordAccountChange(address, oldAccount, newAccount);
    
    // Finalize and check captured changes
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should capture 1 account change", 1, changes.size());
    
    StateChange change = changes.get(0);
    assertEquals("123456789abcdef0", bytesToHex(change.getAddress()));
    assertEquals("", bytesToHex(change.getKey())); // Empty key for account change
    assertTrue("Old account should be serialized", change.getOldValue().length > 0);
    assertTrue("New account should be serialized", change.getNewValue().length > 0);
  }
  
  @Test
  public void testAccountChangeDeduplication() {
    // Initialize journal and recorder bridge
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());
    
    // Simulate multiple changes to same account
    byte[] address = hexToBytes("0x123456789abcdef0");
    AccountCapsule originalAccount = createTestAccount(1000L, System.currentTimeMillis());
    AccountCapsule intermediateAccount = createTestAccount(2000L, System.currentTimeMillis());
    AccountCapsule finalAccount = createTestAccount(3000L, System.currentTimeMillis());
    
    // Record multiple changes to same account
    StateChangeRecorderContext.recordAccountChange(address, originalAccount, intermediateAccount);
    StateChangeRecorderContext.recordAccountChange(address, intermediateAccount, finalAccount);
    
    // Finalize and check captured changes - should be deduplicated
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should capture 1 deduplicated account change", 1, changes.size());
    
    StateChange change = changes.get(0);
    assertEquals("123456789abcdef0", bytesToHex(change.getAddress()));
    assertEquals("", bytesToHex(change.getKey())); // Empty key for account change
    
    // Should preserve original old account and final new account
    assertTrue("Old account should be serialized", change.getOldValue().length > 0);
    assertTrue("New account should be serialized", change.getNewValue().length > 0);
  }
  
  @Test
  public void testMixedStorageAndAccountChanges() {
    // Initialize journal and recorder bridge
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());
    
    // Record a storage change
    byte[] contractAddress = hexToBytes("0xabc123");
    byte[] storageKey = hexToBytes("0xdef456");
    byte[] storageOldValue = hexToBytes("0x111111");
    byte[] storageNewValue = hexToBytes("0x222222");
    
    StateChangeRecorderContext.recordStorageChange(contractAddress, storageKey, 
                                                  storageOldValue, storageNewValue);
    
    // Record an account change
    byte[] accountAddress = hexToBytes("0x789def");
    AccountCapsule oldAccount = createTestAccount(1000L, System.currentTimeMillis());
    AccountCapsule newAccount = createTestAccount(2000L, System.currentTimeMillis());
    
    StateChangeRecorderContext.recordAccountChange(accountAddress, oldAccount, newAccount);
    
    // Finalize and check captured changes
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should capture 2 changes (1 storage + 1 account)", 2, changes.size());
    
    // Find storage change (non-empty key)
    StateChange storageChange = changes.stream()
        .filter(c -> c.getKey().length > 0)
        .findFirst()
        .orElse(null);
    
    assertNotNull("Should have storage change", storageChange);
    assertEquals("abc123", bytesToHex(storageChange.getAddress()));
    assertEquals("def456", bytesToHex(storageChange.getKey()));
    assertEquals("111111", bytesToHex(storageChange.getOldValue()));
    assertEquals("222222", bytesToHex(storageChange.getNewValue()));
    
    // Find account change (empty key)
    StateChange accountChange = changes.stream()
        .filter(c -> c.getKey().length == 0)
        .findFirst()
        .orElse(null);
    
    assertNotNull("Should have account change", accountChange);
    assertEquals("789def", bytesToHex(accountChange.getAddress()));
    assertEquals("", bytesToHex(accountChange.getKey()));
    assertTrue("Old account should be serialized", accountChange.getOldValue().length > 0);
    assertTrue("New account should be serialized", accountChange.getNewValue().length > 0);
  }
  
  @Test
  public void testDisabledAccountRecording() {
    // Disable recording
    System.setProperty("exec.csv.stateChanges.enabled", "false");
    
    // Should not initialize journal when disabled
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());
    
    // Try to record account change
    byte[] address = hexToBytes("0x123456789abcdef0");
    AccountCapsule oldAccount = createTestAccount(1000L, System.currentTimeMillis());
    AccountCapsule newAccount = createTestAccount(2000L, System.currentTimeMillis());
    
    StateChangeRecorderContext.recordAccountChange(address, oldAccount, newAccount);
    
    // Should capture no changes when disabled
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should capture no changes when disabled", 0, changes.size());
  }
  
  /**
   * Helper method to create test AccountCapsule.
   */
  private AccountCapsule createTestAccount(long balance, long createTime) {
    Account.Builder accountBuilder = Account.newBuilder();
    accountBuilder.setBalance(balance);
    accountBuilder.setCreateTime(createTime);
    return new AccountCapsule(accountBuilder.build());
  }
  
  private void assertNotNull(String message, Object object) {
    if (object == null) {
      throw new AssertionError(message);
    }
  }
  
  private void assertTrue(String message, boolean condition) {
    if (!condition) {
      throw new AssertionError(message);
    }
  }
  
  /**
   * Helper method to convert hex string to byte array.
   */
  private byte[] hexToBytes(String hex) {
    if (hex == null || hex.isEmpty()) {
      return new byte[0];
    }
    // Remove 0x prefix if present
    if (hex.startsWith("0x")) {
      hex = hex.substring(2);
    }
    // Ensure even length
    if (hex.length() % 2 != 0) {
      hex = "0" + hex;
    }
    
    byte[] bytes = new byte[hex.length() / 2];
    for (int i = 0; i < bytes.length; i++) {
      bytes[i] = (byte) Integer.parseInt(hex.substring(2 * i, 2 * i + 2), 16);
    }
    return bytes;
  }
  
  /**
   * Helper method to convert byte array to hex string.
   */
  private String bytesToHex(byte[] bytes) {
    if (bytes == null || bytes.length == 0) {
      return "";
    }
    StringBuilder sb = new StringBuilder();
    for (byte b : bytes) {
      sb.append(String.format("%02x", b & 0xff));
    }
    return sb.toString();
  }
}