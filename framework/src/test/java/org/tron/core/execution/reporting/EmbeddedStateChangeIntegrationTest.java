package org.tron.core.execution.reporting;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

import java.util.List;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.tron.common.runtime.ProgramResult;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.db.StateChangeRecorderContext;
import org.tron.core.execution.spi.ExecutionProgramResult;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.protos.Protocol.Account;

/**
 * Integration test to verify embedded execution results include journaled state changes.
 */
public class EmbeddedStateChangeIntegrationTest {

  @Before
  public void setUp() {
    // Enable state change collection
    System.setProperty("exec.csv.stateChanges.enabled", "true");
    // Clean state
    StateChangeJournalRegistry.clearForCurrentTransaction();
    StateChangeRecorderContext.clear();
  }

  @After
  public void tearDown() {
    // Clean up
    StateChangeJournalRegistry.clearForCurrentTransaction();
    StateChangeRecorderContext.clear();
    System.clearProperty("exec.csv.stateChanges.enabled");
  }

  @Test
  public void testExecutionProgramResultIncludesJournaledStateChanges() {
    // Initialize journal and recorder bridge
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());

    // Create some test state changes
    byte[] contractAddress = hexToBytes("0x123456789abcdef0");
    byte[] storageKey = hexToBytes("0xabcdef1234567890");
    byte[] oldValue = hexToBytes("0x1111111111111111");
    byte[] newValue = hexToBytes("0x2222222222222222");
    
    // Record storage change
    StateChangeRecorderContext.recordStorageChange(contractAddress, storageKey, oldValue, newValue);

    // Record account change  
    byte[] accountAddress = hexToBytes("0x789def");
    AccountCapsule oldAccount = createTestAccount(1000L, System.currentTimeMillis());
    AccountCapsule newAccount = createTestAccount(2000L, System.currentTimeMillis());
    StateChangeRecorderContext.recordAccountChange(accountAddress, oldAccount, newAccount);

    // Create a basic ProgramResult (as would come from embedded execution)
    ProgramResult programResult = ProgramResult.createEmpty();
    programResult.setHReturn(hexToBytes("0x123456"));
    programResult.spendEnergy(5000);

    // Convert to ExecutionProgramResult (this should now include journal state changes)
    ExecutionProgramResult executionResult = ExecutionProgramResult.fromProgramResult(programResult);

    // Verify that state changes are included
    List<StateChange> stateChanges = executionResult.getStateChanges();
    assertNotNull("State changes should not be null", stateChanges);
    assertEquals("Should have 2 state changes (1 storage + 1 account)", 2, stateChanges.size());

    // Find and verify storage change
    StateChange storageChange = stateChanges.stream()
        .filter(change -> change.getKey().length > 0)
        .findFirst()
        .orElse(null);
    
    assertNotNull("Should have storage change", storageChange);
    assertEquals("123456789abcdef0", bytesToHex(storageChange.getAddress()));
    assertEquals("abcdef1234567890", bytesToHex(storageChange.getKey()));
    assertEquals("1111111111111111", bytesToHex(storageChange.getOldValue()));
    assertEquals("2222222222222222", bytesToHex(storageChange.getNewValue()));

    // Find and verify account change
    StateChange accountChange = stateChanges.stream()
        .filter(change -> change.getKey().length == 0)
        .findFirst()
        .orElse(null);
    
    assertNotNull("Should have account change", accountChange);
    assertEquals("789def", bytesToHex(accountChange.getAddress()));
    assertEquals("", bytesToHex(accountChange.getKey()));
    assertTrue("Old account should be serialized", accountChange.getOldValue().length > 0);
    assertTrue("New account should be serialized", accountChange.getNewValue().length > 0);

    // Verify other ExecutionProgramResult fields are preserved
    assertEquals("Return data should be preserved", "123456", bytesToHex(executionResult.getHReturn()));
    assertEquals("Energy used should be preserved", 5000, executionResult.getEnergyUsed());
  }

  @Test
  public void testExecutionProgramResultWithoutJournal() {
    // Don't initialize journal - test backwards compatibility
    
    // Create a basic ProgramResult
    ProgramResult programResult = ProgramResult.createEmpty();
    programResult.setHReturn(hexToBytes("0x789abc"));
    programResult.spendEnergy(3000);

    // Convert to ExecutionProgramResult
    ExecutionProgramResult executionResult = ExecutionProgramResult.fromProgramResult(programResult);

    // Should have empty state changes when no journal
    List<StateChange> stateChanges = executionResult.getStateChanges();
    assertNotNull("State changes should not be null", stateChanges);
    assertEquals("Should have no state changes without journal", 0, stateChanges.size());

    // Verify other fields are preserved
    assertEquals("Return data should be preserved", "789abc", bytesToHex(executionResult.getHReturn()));
    assertEquals("Energy used should be preserved", 3000, executionResult.getEnergyUsed());
  }

  @Test
  public void testExecutionProgramResultWithDisabledJournal() {
    // Disable journal
    System.setProperty("exec.csv.stateChanges.enabled", "false");
    
    // Try to initialize (should be no-op when disabled)
    StateChangeJournalRegistry.initializeForCurrentTransaction();
    StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());

    // Try to record state changes (should be no-op when disabled)
    byte[] address = hexToBytes("0x123456");
    StateChangeRecorderContext.recordStorageChange(address, hexToBytes("0xabcdef"), 
                                                   hexToBytes("0x111111"), hexToBytes("0x222222"));

    // Create ProgramResult and convert
    ProgramResult programResult = ProgramResult.createEmpty();
    ExecutionProgramResult executionResult = ExecutionProgramResult.fromProgramResult(programResult);

    // Should have empty state changes when disabled
    List<StateChange> stateChanges = executionResult.getStateChanges();
    assertNotNull("State changes should not be null", stateChanges);
    assertEquals("Should have no state changes when disabled", 0, stateChanges.size());
  }

  private AccountCapsule createTestAccount(long balance, long createTime) {
    Account.Builder accountBuilder = Account.newBuilder();
    accountBuilder.setBalance(balance);
    accountBuilder.setCreateTime(createTime);
    return new AccountCapsule(accountBuilder.build());
  }

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