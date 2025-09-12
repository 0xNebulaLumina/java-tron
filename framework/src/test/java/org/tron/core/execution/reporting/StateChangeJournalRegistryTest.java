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
 * Unit tests for StateChangeJournalRegistry thread-local management.
 */
public class StateChangeJournalRegistryTest {

  @Before
  public void setUp() {
    // Enable state change collection for tests
    System.setProperty("exec.csv.stateChanges.enabled", "true");
    // Ensure clean state
    StateChangeJournalRegistry.clearForCurrentTransaction();
  }

  @After
  public void tearDown() {
    // Clean up system properties and thread-local state
    StateChangeJournalRegistry.clearForCurrentTransaction();
    System.clearProperty("exec.csv.stateChanges.enabled");
  }

  @Test
  public void testJournalLifecycle() {
    // Initially no active journal
    assertFalse("Should not have active journal initially", 
                StateChangeJournalRegistry.hasActiveJournal());
    
    // Initialize journal
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    assertTrue("Should have active journal after init", 
               StateChangeJournalRegistry.hasActiveJournal());
    
    // Record some changes
    byte[] address = hexToBytes("0x123456");
    byte[] key = hexToBytes("0xabcdef");
    byte[] oldValue = hexToBytes("0x111111");
    byte[] newValue = hexToBytes("0x222222");
    
    StateChangeJournalRegistry.recordStorageChange(address, key, oldValue, newValue);
    
    // Finalize and check results
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should have recorded 1 change", 1, changes.size());
    
    StateChange change = changes.get(0);
    assertEquals("123456", bytesToHex(change.getAddress()));
    assertEquals("abcdef", bytesToHex(change.getKey()));
    assertEquals("111111", bytesToHex(change.getOldValue()));
    assertEquals("222222", bytesToHex(change.getNewValue()));
    
    // Journal should be cleared after finalize
    assertFalse("Should not have active journal after finalize", 
                StateChangeJournalRegistry.hasActiveJournal());
  }
  
  @Test
  public void testAccountChange() {
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    
    byte[] address = hexToBytes("0x123456");
    AccountCapsule oldAccount = createTestAccount(1000L, 1000L);
    AccountCapsule newAccount = createTestAccount(2000L, 1000L);
    
    StateChangeJournalRegistry.recordAccountChange(address, oldAccount, newAccount);
    
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should have 1 account change", 1, changes.size());
    
    StateChange change = changes.get(0);
    assertEquals("123456", bytesToHex(change.getAddress()));
    assertEquals("", bytesToHex(change.getKey())); // Empty key for account change
    
    // Account serialization should have non-empty old/new values
    assertTrue("Old account should be serialized", change.getOldValue().length > 0);
    assertTrue("New account should be serialized", change.getNewValue().length > 0);
  }
  
  @Test
  public void testMixedChanges() {
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    
    // Storage change
    StateChangeJournalRegistry.recordStorageChange(
        hexToBytes("0x123456"), hexToBytes("0xabcdef"), 
        hexToBytes("0x111111"), hexToBytes("0x222222"));
    
    // Account change  
    StateChangeJournalRegistry.recordAccountChange(
        hexToBytes("0x789abc"), 
        createTestAccount(1000L, 1000L), 
        createTestAccount(2000L, 1000L));
    
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should have 2 changes", 2, changes.size());
    
    // Find storage vs account changes
    long storageChanges = changes.stream().filter(c -> c.getKey().length > 0).count();
    long accountChanges = changes.stream().filter(c -> c.getKey().length == 0).count();
    
    assertEquals("Should have 1 storage change", 1, storageChanges);
    assertEquals("Should have 1 account change", 1, accountChanges);
  }
  
  @Test
  public void testDisabledCollection() {
    // Disable collection
    System.setProperty("exec.csv.stateChanges.enabled", "false");
    
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    assertFalse("Should not have active journal when disabled", 
                StateChangeJournalRegistry.hasActiveJournal());
    
    // Try to record changes
    StateChangeJournalRegistry.recordStorageChange(
        hexToBytes("0x123"), hexToBytes("0x456"), 
        null, hexToBytes("0x789"));
    
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should have no changes when disabled", 0, changes.size());
  }
  
  @Test
  public void testDoubleInitialization() {
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    assertTrue("Should have active journal", 
               StateChangeJournalRegistry.hasActiveJournal());
    
    // Record a change
    StateChangeJournalRegistry.recordStorageChange(
        hexToBytes("0x123"), hexToBytes("0x456"), 
        null, hexToBytes("0x789"));
    
    // Initialize again - should clear previous journal
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    assertTrue("Should still have active journal", 
               StateChangeJournalRegistry.hasActiveJournal());
    
    // Should have no changes from previous initialization
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should have no changes after re-initialization", 0, changes.size());
  }
  
  @Test
  public void testMetrics() {
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    
    String initialMetrics = StateChangeJournalRegistry.getCurrentJournalMetrics();
    assertTrue("Initial metrics should show 0 changes", 
               initialMetrics.contains("0 storage changes") && 
               initialMetrics.contains("0 account changes"));
    
    // Add changes
    StateChangeJournalRegistry.recordStorageChange(
        hexToBytes("0x123"), hexToBytes("0x456"), 
        null, hexToBytes("0x789"));
    StateChangeJournalRegistry.recordAccountChange(
        hexToBytes("0xabc"), null, createTestAccount(1000L, 1000L));
    
    String updatedMetrics = StateChangeJournalRegistry.getCurrentJournalMetrics();
    assertTrue("Updated metrics should show 1 of each", 
               updatedMetrics.contains("1 storage changes") && 
               updatedMetrics.contains("1 account changes"));
    
    StateChangeJournalRegistry.finalizeForCurrentTransaction();
    
    String finalMetrics = StateChangeJournalRegistry.getCurrentJournalMetrics();
    assertEquals("Should show no journal after finalization", 
                 "No journal active", finalMetrics);
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