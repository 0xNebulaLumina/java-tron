package org.tron.core.execution.reporting;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import java.util.List;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.protos.Protocol.Account;

/**
 * Unit tests for StateChangeJournal.
 */
public class StateChangeJournalTest {

  @Before
  public void setUp() {
    // Enable state change collection for tests
    System.setProperty("exec.csv.stateChanges.enabled", "true");
  }

  @After
  public void tearDown() {
    // Clean up system properties
    System.clearProperty("exec.csv.stateChanges.enabled");
  }

  @Test
  public void testStorageChangeRecording() {
    StateChangeJournal journal = new StateChangeJournal();
    
    byte[] address = hexToBytes("0x123456");
    byte[] key = hexToBytes("0xabcdef");
    byte[] oldValue = hexToBytes("0x111111");
    byte[] newValue = hexToBytes("0x222222");
    
    journal.recordStorageChange(address, key, oldValue, newValue);
    
    assertEquals(1, journal.getStorageChangeCount());
    assertEquals(0, journal.getAccountChangeCount());
  }
  
  @Test
  public void testStorageChangeMerging() {
    StateChangeJournal journal = new StateChangeJournal();
    
    byte[] address = hexToBytes("0x123456");
    byte[] key = hexToBytes("0xabcdef");
    byte[] oldValue = hexToBytes("0x111111");
    byte[] intermediateValue = hexToBytes("0x222222");
    byte[] finalValue = hexToBytes("0x333333");
    
    // First change
    journal.recordStorageChange(address, key, oldValue, intermediateValue);
    assertEquals(1, journal.getStorageChangeCount());
    
    // Second change to same slot - should merge
    journal.recordStorageChange(address, key, intermediateValue, finalValue);
    assertEquals(1, journal.getStorageChangeCount());
    
    // Finalize and check merged result
    List<StateChange> changes = journal.finalizeChanges();
    assertEquals(1, changes.size());
    
    StateChange change = changes.get(0);
    assertEquals("123456", bytesToHex(change.getAddress()));
    assertEquals("abcdef", bytesToHex(change.getKey()));
    assertEquals("111111", bytesToHex(change.getOldValue())); // Original old value preserved
    assertEquals("333333", bytesToHex(change.getNewValue())); // Final new value
  }
  
  @Test
  public void testAccountChangeRecording() {
    StateChangeJournal journal = new StateChangeJournal();
    
    byte[] address = hexToBytes("0x123456");
    AccountCapsule oldAccount = createTestAccount(1000L, 1000L);
    AccountCapsule newAccount = createTestAccount(2000L, 1000L);
    
    journal.recordAccountChange(address, oldAccount, newAccount);
    
    assertEquals(0, journal.getStorageChangeCount());
    assertEquals(1, journal.getAccountChangeCount());
  }
  
  @Test
  public void testAccountChangeMerging() {
    StateChangeJournal journal = new StateChangeJournal();
    
    byte[] address = hexToBytes("0x123456");
    AccountCapsule oldAccount = createTestAccount(1000L, 1000L);
    AccountCapsule intermediateAccount = createTestAccount(2000L, 1000L);
    AccountCapsule finalAccount = createTestAccount(3000L, 1000L);
    
    // First change
    journal.recordAccountChange(address, oldAccount, intermediateAccount);
    assertEquals(1, journal.getAccountChangeCount());
    
    // Second change to same account - should merge
    journal.recordAccountChange(address, intermediateAccount, finalAccount);
    assertEquals(1, journal.getAccountChangeCount());
    
    // Finalize and check merged result
    List<StateChange> changes = journal.finalizeChanges();
    assertEquals(1, changes.size());
    
    StateChange change = changes.get(0);
    assertEquals("123456", bytesToHex(change.getAddress()));
    assertEquals("", bytesToHex(change.getKey())); // Empty key for account change
    
    // Should preserve original old account and final new account
    // We can't easily verify the serialized account bytes without complex parsing,
    // but we can verify the structure is correct
    assertTrue("Old value should not be empty", change.getOldValue().length > 0);
    assertTrue("New value should not be empty", change.getNewValue().length > 0);
  }
  
  @Test
  public void testMixedChanges() {
    StateChangeJournal journal = new StateChangeJournal();
    
    // Add storage change
    byte[] contractAddress = hexToBytes("0x123456");
    byte[] storageKey = hexToBytes("0xabcdef");
    byte[] storageOld = hexToBytes("0x111111");
    byte[] storageNew = hexToBytes("0x222222");
    journal.recordStorageChange(contractAddress, storageKey, storageOld, storageNew);
    
    // Add account change
    byte[] accountAddress = hexToBytes("0x789abc");
    AccountCapsule oldAccount = createTestAccount(1000L, 1000L);
    AccountCapsule newAccount = createTestAccount(2000L, 1000L);
    journal.recordAccountChange(accountAddress, oldAccount, newAccount);
    
    assertEquals(1, journal.getStorageChangeCount());
    assertEquals(1, journal.getAccountChangeCount());
    
    List<StateChange> changes = journal.finalizeChanges();
    assertEquals(2, changes.size());
    
    // Find storage change
    StateChange storageChange = changes.stream()
        .filter(c -> c.getKey().length > 0)
        .findFirst()
        .orElse(null);
    
    assertTrue("Should have storage change", storageChange != null);
    assertEquals("123456", bytesToHex(storageChange.getAddress()));
    assertEquals("abcdef", bytesToHex(storageChange.getKey()));
    
    // Find account change
    StateChange accountChange = changes.stream()
        .filter(c -> c.getKey().length == 0)
        .findFirst()
        .orElse(null);
    
    assertTrue("Should have account change", accountChange != null);
    assertEquals("789abc", bytesToHex(accountChange.getAddress()));
    assertEquals("", bytesToHex(accountChange.getKey()));
  }
  
  @Test
  public void testAccountSerialization() {
    StateChangeJournal journal = new StateChangeJournal();
    
    byte[] address = hexToBytes("0x123456");
    AccountCapsule account = createTestAccount(1000L, 1000L);
    
    journal.recordAccountChange(address, null, account);
    List<StateChange> changes = journal.finalizeChanges();
    
    assertEquals(1, changes.size());
    StateChange change = changes.get(0);
    
    // Check serialized format structure
    byte[] serialized = change.getNewValue();
    assertTrue("Serialized account should have minimum size", serialized.length >= 44); // 32+8+4 = 44
    
    // Check that old value is empty for account creation
    assertEquals(0, change.getOldValue().length);
  }
  
  @Test
  public void testDisabledCollection() {
    // Disable collection
    System.setProperty("exec.csv.stateChanges.enabled", "false");
    StateChangeJournal journal = new StateChangeJournal();
    
    assertFalse("Collection should be disabled", StateChangeJournal.isEnabled());
    
    // Try to record changes
    journal.recordStorageChange(hexToBytes("0x123"), hexToBytes("0x456"), null, hexToBytes("0x789"));
    journal.recordAccountChange(hexToBytes("0xabc"), null, createTestAccount(1000L, 1000L));
    
    // Should have no changes
    assertEquals(0, journal.getStorageChangeCount());
    assertEquals(0, journal.getAccountChangeCount());
    
    List<StateChange> changes = journal.finalizeChanges();
    assertEquals(0, changes.size());
  }
  
  @Test
  public void testJournalClear() {
    StateChangeJournal journal = new StateChangeJournal();
    
    journal.recordStorageChange(hexToBytes("0x123"), hexToBytes("0x456"), null, hexToBytes("0x789"));
    journal.recordAccountChange(hexToBytes("0xabc"), null, createTestAccount(1000L, 1000L));
    
    assertEquals(1, journal.getStorageChangeCount());
    assertEquals(1, journal.getAccountChangeCount());
    
    journal.clear();
    
    assertEquals(0, journal.getStorageChangeCount());
    assertEquals(0, journal.getAccountChangeCount());
  }
  
  @Test(expected = IllegalStateException.class)
  public void testDoubleFinalize() {
    StateChangeJournal journal = new StateChangeJournal();
    journal.recordStorageChange(hexToBytes("0x123"), hexToBytes("0x456"), null, hexToBytes("0x789"));
    
    journal.finalizeChanges(); // First finalize - should work
    journal.finalizeChanges(); // Second finalize - should throw
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