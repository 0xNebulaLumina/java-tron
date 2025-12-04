package org.tron.core.execution.reporting;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

import java.util.List;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.tron.core.execution.reporting.DomainCanonicalizer.FreezeDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.GlobalResourceDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.Trc10BalanceDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.Trc10IssuanceDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.VoteDelta;

/**
 * Unit tests for DomainChangeJournal merge semantics.
 *
 * <p>Tests verify that multiple updates within a transaction are properly merged:
 * - First old value is kept
 * - Last new value is kept
 * - Correct operation type is determined
 */
public class DomainChangeJournalTest {

  @Before
  public void setUp() {
    // Enable domain change recording
    System.setProperty("exec.csv.stateChanges.enabled", "true");
  }

  @After
  public void tearDown() {
    System.clearProperty("exec.csv.stateChanges.enabled");
  }

  // ================================
  // TRC-10 Balance Merge Semantics
  // ================================

  @Test
  public void testTrc10BalanceMergeKeepsFirstOldAndLastNew() {
    DomainChangeJournal journal = new DomainChangeJournal();
    byte[] owner = new byte[]{0x41, 0x01, 0x02};
    String tokenId = "1002001";

    // First update: 100 -> 150
    journal.recordTrc10BalanceChange(owner, tokenId, 100L, 150L);
    // Second update: 150 -> 200
    journal.recordTrc10BalanceChange(owner, tokenId, 150L, 200L);
    // Third update: 200 -> 250
    journal.recordTrc10BalanceChange(owner, tokenId, 200L, 250L);

    List<Trc10BalanceDelta> deltas = journal.getTrc10BalanceChanges();

    assertEquals("Should have one merged entry", 1, deltas.size());
    Trc10BalanceDelta delta = deltas.get(0);
    assertEquals("Old balance should be first old value", "100", delta.getOldBalance());
    assertEquals("New balance should be last new value", "250", delta.getNewBalance());
    assertEquals("Token ID should match", tokenId, delta.getTokenId());
  }

  @Test
  public void testTrc10BalanceOpDetermination() {
    DomainChangeJournal journal = new DomainChangeJournal();
    byte[] owner = new byte[]{0x41, 0x01};

    // Increase: 0 -> 100
    journal.recordTrc10BalanceChange(owner, "token1", 0L, 100L);

    // Decrease: 100 -> 50
    journal.recordTrc10BalanceChange(new byte[]{0x41, 0x02}, "token2", 100L, 50L);

    // Delete: 50 -> 0
    journal.recordTrc10BalanceChange(new byte[]{0x41, 0x03}, "token3", 50L, 0L);

    List<Trc10BalanceDelta> deltas = journal.getTrc10BalanceChanges();
    assertEquals(3, deltas.size());

    // Find each delta by token ID
    for (Trc10BalanceDelta delta : deltas) {
      if (delta.getTokenId().equals("token1")) {
        assertEquals("increase", delta.getOp());
      } else if (delta.getTokenId().equals("token2")) {
        assertEquals("decrease", delta.getOp());
      } else if (delta.getTokenId().equals("token3")) {
        assertEquals("delete", delta.getOp());
      }
    }
  }

  @Test
  public void testTrc10BalanceMultipleTokensSameOwner() {
    DomainChangeJournal journal = new DomainChangeJournal();
    byte[] owner = new byte[]{0x41, 0x01};

    journal.recordTrc10BalanceChange(owner, "token1", 100L, 200L);
    journal.recordTrc10BalanceChange(owner, "token2", 50L, 75L);

    List<Trc10BalanceDelta> deltas = journal.getTrc10BalanceChanges();
    assertEquals("Should have two separate entries", 2, deltas.size());
  }

  // ================================
  // Vote Merge Semantics
  // ================================

  @Test
  public void testVoteMergeKeepsFirstOldAndLastNew() {
    DomainChangeJournal journal = new DomainChangeJournal();
    byte[] voter = new byte[]{0x41, 0x01};
    byte[] witness = new byte[]{0x41, 0x02};

    // First update: 0 -> 100
    journal.recordVoteChange(voter, witness, 0L, 100L);
    // Second update: 100 -> 150
    journal.recordVoteChange(voter, witness, 100L, 150L);

    List<VoteDelta> deltas = journal.getVoteChanges();

    assertEquals("Should have one merged entry", 1, deltas.size());
    VoteDelta delta = deltas.get(0);
    assertEquals("Old votes should be first old value", "0", delta.getOldVotes());
    assertEquals("New votes should be last new value", "150", delta.getNewVotes());
  }

  @Test
  public void testVoteOpDetermination() {
    DomainChangeJournal journal = new DomainChangeJournal();
    byte[] voter = new byte[]{0x41, 0x01};

    // Set (new vote): 0 -> 100
    journal.recordVoteChange(voter, new byte[]{0x41, 0x02}, 0L, 100L);

    // Increase: 100 -> 150
    journal.recordVoteChange(voter, new byte[]{0x41, 0x03}, 100L, 150L);

    // Decrease: 150 -> 50
    journal.recordVoteChange(voter, new byte[]{0x41, 0x04}, 150L, 50L);

    // Delete: 50 -> 0
    journal.recordVoteChange(voter, new byte[]{0x41, 0x05}, 50L, 0L);

    List<VoteDelta> deltas = journal.getVoteChanges();
    assertEquals(4, deltas.size());
  }

  @Test
  public void testVoteMultipleWitnessesSameVoter() {
    DomainChangeJournal journal = new DomainChangeJournal();
    byte[] voter = new byte[]{0x41, 0x01};

    journal.recordVoteChange(voter, new byte[]{0x41, 0x02}, 0L, 100L);
    journal.recordVoteChange(voter, new byte[]{0x41, 0x03}, 0L, 200L);

    List<VoteDelta> deltas = journal.getVoteChanges();
    assertEquals("Should have two separate entries", 2, deltas.size());
  }

  // ================================
  // Freeze Merge Semantics
  // ================================

  @Test
  public void testFreezeMergeKeepsFirstOldAndLastNew() {
    DomainChangeJournal journal = new DomainChangeJournal();
    byte[] owner = new byte[]{0x41, 0x01};
    String resource = "BANDWIDTH";

    // First freeze: 0 -> 1000, expire 0 -> 1000000
    journal.recordFreezeChange(owner, resource, null, 0L, 1000L, 0L, 1000000L, "freeze");
    // Second update: 1000 -> 2000, expire 1000000 -> 2000000
    journal.recordFreezeChange(owner, resource, null, 1000L, 2000L, 1000000L, 2000000L, "update");

    List<FreezeDelta> deltas = journal.getFreezeChanges();

    assertEquals("Should have one merged entry", 1, deltas.size());
    FreezeDelta delta = deltas.get(0);
    assertEquals("Old amount should be first old value", "0", delta.getOldAmountSun());
    assertEquals("New amount should be last new value", "2000", delta.getNewAmountSun());
    assertEquals("Old expire should be first old value", "0", delta.getOldExpireTimeMs());
    assertEquals("New expire should be last new value", "2000000", delta.getNewExpireTimeMs());
  }

  @Test
  public void testFreezeMultipleResourcesSameOwner() {
    DomainChangeJournal journal = new DomainChangeJournal();
    byte[] owner = new byte[]{0x41, 0x01};

    journal.recordFreezeChange(owner, "BANDWIDTH", null, 0L, 1000L, 0L, 1000000L, "freeze");
    journal.recordFreezeChange(owner, "ENERGY", null, 0L, 2000L, 0L, 2000000L, "freeze");

    List<FreezeDelta> deltas = journal.getFreezeChanges();
    assertEquals("Should have two separate entries", 2, deltas.size());
  }

  @Test
  public void testFreezeWithRecipient() {
    DomainChangeJournal journal = new DomainChangeJournal();
    byte[] owner = new byte[]{0x41, 0x01};
    byte[] recipient = new byte[]{0x41, 0x02};

    journal.recordFreezeChange(owner, "BANDWIDTH", recipient, 0L, 1000L, 0L, 1000000L, "freeze");

    List<FreezeDelta> deltas = journal.getFreezeChanges();
    assertEquals(1, deltas.size());
    assertNotNull(deltas.get(0).getRecipientAddressHex());
    assertFalse(deltas.get(0).getRecipientAddressHex().isEmpty());
  }

  // ================================
  // Global Resource Merge Semantics
  // ================================

  @Test
  public void testGlobalResourceMergeKeepsFirstOldAndLastNew() {
    DomainChangeJournal journal = new DomainChangeJournal();

    // First update: 1000 -> 1100
    journal.recordGlobalResourceChange("total_energy_weight", 1000L, 1100L);
    // Second update: 1100 -> 1200
    journal.recordGlobalResourceChange("total_energy_weight", 1100L, 1200L);

    List<GlobalResourceDelta> deltas = journal.getGlobalResourceChanges();

    assertEquals("Should have one merged entry", 1, deltas.size());
    GlobalResourceDelta delta = deltas.get(0);
    assertEquals("Old value should be first old value", "1000", delta.getOldValue());
    assertEquals("New value should be last new value", "1200", delta.getNewValue());
  }

  @Test
  public void testGlobalResourceMultipleFields() {
    DomainChangeJournal journal = new DomainChangeJournal();

    journal.recordGlobalResourceChange("total_energy_weight", 1000L, 1100L);
    journal.recordGlobalResourceChange("total_net_weight", 2000L, 2100L);

    List<GlobalResourceDelta> deltas = journal.getGlobalResourceChanges();
    assertEquals("Should have two separate entries", 2, deltas.size());
  }

  // ================================
  // TRC-10 Issuance Merge Semantics
  // ================================

  @Test
  public void testTrc10IssuanceMergeKeepsFirstOldAndLastNew() {
    DomainChangeJournal journal = new DomainChangeJournal();
    String tokenId = "1002001";

    // First update: null -> initial total_supply
    journal.recordTrc10IssuanceChange(tokenId, "total_supply", null, "1000000", "create");
    // Second update: update total_supply
    journal.recordTrc10IssuanceChange(tokenId, "total_supply", "1000000", "2000000", "update");

    List<Trc10IssuanceDelta> deltas = journal.getTrc10IssuanceChanges();

    assertEquals("Should have one merged entry", 1, deltas.size());
    Trc10IssuanceDelta delta = deltas.get(0);
    assertEquals("Old value should be first old value (null)", null, delta.getOldValue());
    assertEquals("New value should be last new value", "2000000", delta.getNewValue());
  }

  // ================================
  // Lifecycle Tests
  // ================================

  @Test
  public void testClearResetsAllState() {
    DomainChangeJournal journal = new DomainChangeJournal();
    byte[] owner = new byte[]{0x41, 0x01};

    journal.recordTrc10BalanceChange(owner, "token1", 0L, 100L);
    journal.recordVoteChange(owner, new byte[]{0x41, 0x02}, 0L, 50L);
    journal.recordFreezeChange(owner, "BANDWIDTH", null, 0L, 1000L, 0L, 1000000L, "freeze");
    journal.recordGlobalResourceChange("total_energy_weight", 1000L, 1100L);

    // Verify counts before clear
    assertEquals(1, journal.getTrc10BalanceChangeCount());
    assertEquals(1, journal.getVoteChangeCount());
    assertEquals(1, journal.getFreezeChangeCount());
    assertEquals(1, journal.getGlobalResourceChangeCount());

    journal.clear();

    // Verify all cleared
    assertEquals(0, journal.getTrc10BalanceChangeCount());
    assertEquals(0, journal.getVoteChangeCount());
    assertEquals(0, journal.getFreezeChangeCount());
    assertEquals(0, journal.getGlobalResourceChangeCount());
    assertTrue(journal.getTrc10BalanceChanges().isEmpty());
    assertTrue(journal.getVoteChanges().isEmpty());
    assertTrue(journal.getFreezeChanges().isEmpty());
    assertTrue(journal.getGlobalResourceChanges().isEmpty());
  }

  @Test
  public void testFinalizedJournalRejectsNewRecords() {
    DomainChangeJournal journal = new DomainChangeJournal();
    byte[] owner = new byte[]{0x41, 0x01};

    journal.recordTrc10BalanceChange(owner, "token1", 0L, 100L);
    journal.markFinalized();

    // Try to add more after finalization
    journal.recordTrc10BalanceChange(owner, "token2", 0L, 200L);

    // Should still only have the first entry
    assertEquals(1, journal.getTrc10BalanceChangeCount());
    assertTrue(journal.isFinalized());
  }

  @Test
  public void testIsEnabledRespectsSystemProperty() {
    // Already set in setUp
    assertTrue(DomainChangeJournal.isEnabled());

    System.setProperty("exec.csv.stateChanges.enabled", "false");
    assertFalse(DomainChangeJournal.isEnabled());

    // Reset for other tests
    System.setProperty("exec.csv.stateChanges.enabled", "true");
  }
}
