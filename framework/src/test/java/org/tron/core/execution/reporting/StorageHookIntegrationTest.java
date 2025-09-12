package org.tron.core.execution.reporting;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;

import java.util.List;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.tron.core.db.StateChangeRecorderContext;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;

/**
 * Integration test to verify storage change recording integration works correctly.
 */
public class StorageHookIntegrationTest {

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
  public void testRecorderBridgeIntegration() {
    // Initialize journal and recorder bridge
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());
    
    // Simulate storage change via StateChangeRecorderContext (how ContractState would call it)
    byte[] contractAddress = hexToBytes("0x123456789abcdef0");
    byte[] storageKey = hexToBytes("0xabcdef1234567890");
    byte[] oldValue = hexToBytes("0x1111111111111111");
    byte[] newValue = hexToBytes("0x2222222222222222");
    
    StateChangeRecorderContext.recordStorageChange(contractAddress, storageKey, oldValue, newValue);
    
    // Finalize and check captured changes
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should capture 1 storage change", 1, changes.size());
    
    StateChange change = changes.get(0);
    assertEquals("123456789abcdef0", bytesToHex(change.getAddress()));
    assertEquals("abcdef1234567890", bytesToHex(change.getKey()));
    assertEquals("1111111111111111", bytesToHex(change.getOldValue()));
    assertEquals("2222222222222222", bytesToHex(change.getNewValue()));
  }
  
  @Test
  public void testRecorderBridgeWithNullOldValue() {
    // Initialize journal and recorder bridge
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());
    
    // Simulate storage change with null old value (new slot)
    byte[] contractAddress = hexToBytes("0x123456789abcdef0");
    byte[] storageKey = hexToBytes("0xabcdef1234567890");
    byte[] oldValue = null;
    byte[] newValue = hexToBytes("0x2222222222222222");
    
    StateChangeRecorderContext.recordStorageChange(contractAddress, storageKey, oldValue, newValue);
    
    // Finalize and check captured changes
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should capture 1 storage change", 1, changes.size());
    
    StateChange change = changes.get(0);
    assertEquals("123456789abcdef0", bytesToHex(change.getAddress()));
    assertEquals("abcdef1234567890", bytesToHex(change.getKey()));
    assertEquals("", bytesToHex(change.getOldValue())); // null old value
    assertEquals("2222222222222222", bytesToHex(change.getNewValue()));
  }
  
  @Test
  public void testRecorderBridgeDeduplication() {
    // Initialize journal and recorder bridge
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());
    
    // Simulate multiple storage changes to same slot
    byte[] contractAddress = hexToBytes("0x123456789abcdef0");
    byte[] storageKey = hexToBytes("0xabcdef1234567890");
    byte[] oldValue = hexToBytes("0x1111111111111111");
    byte[] intermediateValue = hexToBytes("0x2222222222222222");
    byte[] finalValue = hexToBytes("0x3333333333333333");
    
    // Record multiple changes to same slot
    StateChangeRecorderContext.recordStorageChange(contractAddress, storageKey, oldValue, intermediateValue);
    StateChangeRecorderContext.recordStorageChange(contractAddress, storageKey, intermediateValue, finalValue);
    
    // Finalize and check captured changes - should be deduplicated
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should capture 1 deduplicated change", 1, changes.size());
    
    StateChange change = changes.get(0);
    assertEquals("123456789abcdef0", bytesToHex(change.getAddress()));
    assertEquals("abcdef1234567890", bytesToHex(change.getKey()));
    assertEquals("1111111111111111", bytesToHex(change.getOldValue())); // Original old
    assertEquals("3333333333333333", bytesToHex(change.getNewValue())); // Final new
  }
  
  @Test 
  public void testDisabledRecording() {
    // Disable recording
    System.setProperty("exec.csv.stateChanges.enabled", "false");
    
    // Should not initialize journal when disabled
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());
    
    // Try to record a storage change
    byte[] contractAddress = hexToBytes("0x123456789abcdef0");
    byte[] storageKey = hexToBytes("0xabcdef1234567890");
    byte[] oldValue = hexToBytes("0x1111111111111111");
    byte[] newValue = hexToBytes("0x2222222222222222");
    
    StateChangeRecorderContext.recordStorageChange(contractAddress, storageKey, oldValue, newValue);
    
    // Should capture no changes when disabled
    List<StateChange> changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
    assertEquals("Should capture no changes when disabled", 0, changes.size());
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