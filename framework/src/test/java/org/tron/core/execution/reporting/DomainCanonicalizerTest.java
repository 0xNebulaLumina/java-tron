package org.tron.core.execution.reporting;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import org.junit.Test;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;

/**
 * Unit tests for DomainCanonicalizer.
 */
public class DomainCanonicalizerTest {

  // SHA-256 of empty string, used for empty array digest consistency
  private static final String EMPTY_DIGEST =
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

  @Test
  public void testEmptyDomainResult() {
    DomainCanonicalizer.DomainResult result = DomainCanonicalizer.emptyDomainResult();

    assertEquals("[]", result.getJson());
    assertEquals(0, result.getCount());
    // Empty arrays should return sha256("") for cross-tooling compatibility
    assertEquals(EMPTY_DIGEST, result.getDigest());
  }

  @Test
  public void testEmptyArrayDigestPolicy() {
    // Test that all domain types return sha256("") for empty arrays
    List<DomainCanonicalizer.AccountDelta> emptyAccountDeltas = new ArrayList<>();
    DomainCanonicalizer.DomainResult accountResult =
        DomainCanonicalizer.accountToJsonAndDigest(emptyAccountDeltas);
    assertEquals("[]", accountResult.getJson());
    assertEquals(0, accountResult.getCount());
    assertEquals(EMPTY_DIGEST, accountResult.getDigest());

    List<DomainCanonicalizer.EvmStorageDelta> emptyEvmDeltas = new ArrayList<>();
    DomainCanonicalizer.DomainResult evmResult =
        DomainCanonicalizer.evmStorageToJsonAndDigest(emptyEvmDeltas);
    assertEquals("[]", evmResult.getJson());
    assertEquals(0, evmResult.getCount());
    assertEquals(EMPTY_DIGEST, evmResult.getDigest());

    List<DomainCanonicalizer.Trc10BalanceDelta> emptyTrc10Deltas = new ArrayList<>();
    DomainCanonicalizer.DomainResult trc10Result =
        DomainCanonicalizer.trc10BalancesToJsonAndDigest(emptyTrc10Deltas);
    assertEquals("[]", trc10Result.getJson());
    assertEquals(0, trc10Result.getCount());
    assertEquals(EMPTY_DIGEST, trc10Result.getDigest());

    List<DomainCanonicalizer.VoteDelta> emptyVoteDeltas = new ArrayList<>();
    DomainCanonicalizer.DomainResult voteResult =
        DomainCanonicalizer.votesToJsonAndDigest(emptyVoteDeltas);
    assertEquals("[]", voteResult.getJson());
    assertEquals(0, voteResult.getCount());
    assertEquals(EMPTY_DIGEST, voteResult.getDigest());

    List<DomainCanonicalizer.FreezeDelta> emptyFreezeDeltas = new ArrayList<>();
    DomainCanonicalizer.DomainResult freezeResult =
        DomainCanonicalizer.freezesToJsonAndDigest(emptyFreezeDeltas);
    assertEquals("[]", freezeResult.getJson());
    assertEquals(0, freezeResult.getCount());
    assertEquals(EMPTY_DIGEST, freezeResult.getDigest());

    List<DomainCanonicalizer.GlobalResourceDelta> emptyGlobalDeltas = new ArrayList<>();
    DomainCanonicalizer.DomainResult globalResult =
        DomainCanonicalizer.globalsToJsonAndDigest(emptyGlobalDeltas);
    assertEquals("[]", globalResult.getJson());
    assertEquals(0, globalResult.getCount());
    assertEquals(EMPTY_DIGEST, globalResult.getDigest());
  }

  @Test
  public void testAccountToJsonAndDigest() {
    List<DomainCanonicalizer.AccountDelta> deltas = new ArrayList<>();

    DomainCanonicalizer.AccountDelta delta1 = new DomainCanonicalizer.AccountDelta();
    delta1.setAddressHex("41abc123");
    delta1.setOp("update");
    delta1.setOldBalance(1000L);
    delta1.setNewBalance(2000L);
    deltas.add(delta1);

    DomainCanonicalizer.AccountDelta delta2 = new DomainCanonicalizer.AccountDelta();
    delta2.setAddressHex("41def456");
    delta2.setOp("create");
    delta2.setNewBalance(500L);
    deltas.add(delta2);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.accountToJsonAndDigest(deltas);

    assertEquals(2, result.getCount());
    assertNotNull(result.getJson());
    assertNotNull(result.getDigest());
    assertTrue(result.getJson().startsWith("["));
    assertTrue(result.getJson().endsWith("]"));
    assertTrue(result.getJson().contains("41abc123"));
    assertTrue(result.getJson().contains("41def456"));
    assertTrue(result.getDigest().length() == 64); // SHA-256 hex length
  }

  @Test
  public void testAccountDeltaSorting() {
    List<DomainCanonicalizer.AccountDelta> deltas = new ArrayList<>();

    // Add in reverse order to test sorting
    DomainCanonicalizer.AccountDelta delta2 = new DomainCanonicalizer.AccountDelta();
    delta2.setAddressHex("41zzz");
    delta2.setOp("update");
    deltas.add(delta2);

    DomainCanonicalizer.AccountDelta delta1 = new DomainCanonicalizer.AccountDelta();
    delta1.setAddressHex("41aaa");
    delta1.setOp("create");
    deltas.add(delta1);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.accountToJsonAndDigest(deltas);

    // Verify sorting: 41aaa should come before 41zzz
    int aPos = result.getJson().indexOf("41aaa");
    int zPos = result.getJson().indexOf("41zzz");
    assertTrue("41aaa should appear before 41zzz", aPos < zPos);
  }

  @Test
  public void testEvmStorageToJsonAndDigest() {
    List<DomainCanonicalizer.EvmStorageDelta> deltas = new ArrayList<>();

    DomainCanonicalizer.EvmStorageDelta delta = new DomainCanonicalizer.EvmStorageDelta();
    delta.setContractAddressHex("41contract");
    delta.setSlotKeyHex("0000000000000000000000000000000000000000000000000000000000000001");
    delta.setOp("set");
    delta.setOldValueHex("");
    delta.setNewValueHex("deadbeef");
    deltas.add(delta);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.evmStorageToJsonAndDigest(deltas);

    assertEquals(1, result.getCount());
    assertTrue(result.getJson().contains("41contract"));
    assertTrue(result.getJson().contains("deadbeef"));
    assertTrue(result.getDigest().length() == 64);
  }

  @Test
  public void testEvmStorageSorting() {
    List<DomainCanonicalizer.EvmStorageDelta> deltas = new ArrayList<>();

    // Add in reverse order
    DomainCanonicalizer.EvmStorageDelta delta2 = new DomainCanonicalizer.EvmStorageDelta();
    delta2.setContractAddressHex("41bbb");
    delta2.setSlotKeyHex("0002");
    delta2.setOp("set");
    deltas.add(delta2);

    DomainCanonicalizer.EvmStorageDelta delta1 = new DomainCanonicalizer.EvmStorageDelta();
    delta1.setContractAddressHex("41aaa");
    delta1.setSlotKeyHex("0001");
    delta1.setOp("set");
    deltas.add(delta1);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.evmStorageToJsonAndDigest(deltas);

    // Verify sorting: contract 41aaa should come before 41bbb
    int aPos = result.getJson().indexOf("41aaa");
    int bPos = result.getJson().indexOf("41bbb");
    assertTrue("41aaa should appear before 41bbb", aPos < bPos);
  }

  @Test
  public void testTrc10BalancesToJsonAndDigest() {
    List<DomainCanonicalizer.Trc10BalanceDelta> deltas = new ArrayList<>();

    DomainCanonicalizer.Trc10BalanceDelta delta = new DomainCanonicalizer.Trc10BalanceDelta();
    delta.setTokenId("1002001");
    delta.setOwnerAddressHex("41owner");
    delta.setOp("increase");
    delta.setOldBalance("1000");
    delta.setNewBalance("2000");
    deltas.add(delta);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.trc10BalancesToJsonAndDigest(deltas);

    assertEquals(1, result.getCount());
    assertTrue(result.getJson().contains("1002001"));
    assertTrue(result.getJson().contains("41owner"));
    assertTrue(result.getDigest().length() == 64);
  }

  @Test
  public void testTrc10IssuanceToJsonAndDigest() {
    List<DomainCanonicalizer.Trc10IssuanceDelta> deltas = new ArrayList<>();

    DomainCanonicalizer.Trc10IssuanceDelta delta = new DomainCanonicalizer.Trc10IssuanceDelta();
    delta.setTokenId("1002001");
    delta.setField("total_supply");
    delta.setOp("create");
    delta.setNewValue("1000000000");
    deltas.add(delta);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.trc10IssuanceToJsonAndDigest(deltas);

    assertEquals(1, result.getCount());
    assertTrue(result.getJson().contains("total_supply"));
    assertTrue(result.getJson().contains("1000000000"));
  }

  @Test
  public void testVotesToJsonAndDigest() {
    List<DomainCanonicalizer.VoteDelta> deltas = new ArrayList<>();

    DomainCanonicalizer.VoteDelta delta = new DomainCanonicalizer.VoteDelta();
    delta.setVoterAddressHex("41voter");
    delta.setWitnessAddressHex("41witness");
    delta.setOp("set");
    delta.setOldVotes("0");
    delta.setNewVotes("1000");
    deltas.add(delta);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.votesToJsonAndDigest(deltas);

    assertEquals(1, result.getCount());
    assertTrue(result.getJson().contains("41voter"));
    assertTrue(result.getJson().contains("41witness"));
    assertTrue(result.getJson().contains("1000"));
  }

  @Test
  public void testFreezesToJsonAndDigest() {
    List<DomainCanonicalizer.FreezeDelta> deltas = new ArrayList<>();

    DomainCanonicalizer.FreezeDelta delta = new DomainCanonicalizer.FreezeDelta();
    delta.setOwnerAddressHex("41owner");
    delta.setResourceType("BANDWIDTH");
    delta.setOp("freeze");
    delta.setOldAmountSun("0");
    delta.setNewAmountSun("100000000");
    delta.setOldExpireTimeMs("0");
    delta.setNewExpireTimeMs("1735689600000");
    deltas.add(delta);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.freezesToJsonAndDigest(deltas);

    assertEquals(1, result.getCount());
    assertTrue(result.getJson().contains("BANDWIDTH"));
    assertTrue(result.getJson().contains("100000000"));
  }

  @Test
  public void testGlobalsToJsonAndDigest() {
    List<DomainCanonicalizer.GlobalResourceDelta> deltas = new ArrayList<>();

    DomainCanonicalizer.GlobalResourceDelta delta = new DomainCanonicalizer.GlobalResourceDelta();
    delta.setField("total_energy_limit");
    delta.setOp("update");
    delta.setOldValue("90000000000");
    delta.setNewValue("100000000000");
    deltas.add(delta);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.globalsToJsonAndDigest(deltas);

    assertEquals(1, result.getCount());
    assertTrue(result.getJson().contains("total_energy_limit"));
  }

  @Test
  public void testAccountAextToJsonAndDigest() {
    List<DomainCanonicalizer.AccountResourceUsageDelta> deltas = new ArrayList<>();

    DomainCanonicalizer.AccountResourceUsageDelta delta =
        new DomainCanonicalizer.AccountResourceUsageDelta();
    delta.setAddressHex("41account");
    delta.setOp("update");
    delta.setOldNetUsage(100L);
    delta.setNewNetUsage(200L);
    delta.setOldEnergyUsage(1000L);
    delta.setNewEnergyUsage(2000L);
    deltas.add(delta);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.accountAextToJsonAndDigest(deltas);

    assertEquals(1, result.getCount());
    assertTrue(result.getJson().contains("41account"));
    assertTrue(result.getJson().contains("net_usage"));
    assertTrue(result.getJson().contains("energy_usage"));
  }

  @Test
  public void testLogsToJsonAndDigest() {
    List<DomainCanonicalizer.LogEntryDelta> deltas = new ArrayList<>();

    DomainCanonicalizer.LogEntryDelta delta = new DomainCanonicalizer.LogEntryDelta();
    delta.setContractAddressHex("41contract");
    delta.setIndex(0);
    delta.setTopicsHex(Arrays.asList("topic1", "topic2"));
    delta.setDataHex("eventdata");
    deltas.add(delta);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.logsToJsonAndDigest(deltas);

    assertEquals(1, result.getCount());
    assertTrue(result.getJson().contains("41contract"));
    assertTrue(result.getJson().contains("topic1"));
    assertTrue(result.getJson().contains("eventdata"));
  }

  @Test
  public void testSplitStateChanges() {
    List<StateChange> stateChanges = new ArrayList<>();

    // Account change (empty key)
    stateChanges.add(new StateChange(
        new byte[]{0x41, 0x01, 0x02},
        new byte[0],
        null,
        new byte[76] // Minimum account info size
    ));

    // EVM storage change (non-empty key)
    stateChanges.add(new StateChange(
        new byte[]{0x41, 0x03, 0x04},
        new byte[]{0x00, 0x01},
        new byte[0],
        new byte[]{0x12, 0x34}
    ));

    DomainCanonicalizer.SplitStateChanges split =
        DomainCanonicalizer.splitStateChanges(stateChanges);

    assertEquals(1, split.accountChanges.size());
    assertEquals(1, split.evmStorageChanges.size());
  }

  @Test
  public void testDeterministicDigest() {
    // Create same data twice in different order
    List<DomainCanonicalizer.AccountDelta> deltas1 = new ArrayList<>();
    DomainCanonicalizer.AccountDelta d1a = new DomainCanonicalizer.AccountDelta();
    d1a.setAddressHex("41aaa");
    d1a.setOp("update");
    deltas1.add(d1a);
    DomainCanonicalizer.AccountDelta d1b = new DomainCanonicalizer.AccountDelta();
    d1b.setAddressHex("41bbb");
    d1b.setOp("create");
    deltas1.add(d1b);

    List<DomainCanonicalizer.AccountDelta> deltas2 = new ArrayList<>();
    // Add in reverse order
    DomainCanonicalizer.AccountDelta d2b = new DomainCanonicalizer.AccountDelta();
    d2b.setAddressHex("41bbb");
    d2b.setOp("create");
    deltas2.add(d2b);
    DomainCanonicalizer.AccountDelta d2a = new DomainCanonicalizer.AccountDelta();
    d2a.setAddressHex("41aaa");
    d2a.setOp("update");
    deltas2.add(d2a);

    DomainCanonicalizer.DomainResult result1 =
        DomainCanonicalizer.accountToJsonAndDigest(deltas1);
    DomainCanonicalizer.DomainResult result2 =
        DomainCanonicalizer.accountToJsonAndDigest(deltas2);

    // Same digest regardless of input order
    assertEquals(result1.getDigest(), result2.getDigest());
    assertEquals(result1.getJson(), result2.getJson());
  }

  @Test
  public void testLowercaseHex() {
    List<DomainCanonicalizer.AccountDelta> deltas = new ArrayList<>();
    DomainCanonicalizer.AccountDelta delta = new DomainCanonicalizer.AccountDelta();
    delta.setAddressHex("41ABCDEF"); // Uppercase
    delta.setOp("update");
    deltas.add(delta);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.accountToJsonAndDigest(deltas);

    // Should be lowercase in output
    assertTrue(result.getJson().contains("41abcdef"));
  }

  @Test
  public void testEmptyListReturnsEmptyResult() {
    DomainCanonicalizer.DomainResult accountResult =
        DomainCanonicalizer.accountToJsonAndDigest(new ArrayList<>());
    assertEquals("[]", accountResult.getJson());
    assertEquals(0, accountResult.getCount());
    assertEquals(EMPTY_DIGEST, accountResult.getDigest());

    DomainCanonicalizer.DomainResult evmResult =
        DomainCanonicalizer.evmStorageToJsonAndDigest(new ArrayList<>());
    assertEquals("[]", evmResult.getJson());
    assertEquals(0, evmResult.getCount());
    assertEquals(EMPTY_DIGEST, evmResult.getDigest());

    DomainCanonicalizer.DomainResult logResult =
        DomainCanonicalizer.logsToJsonAndDigest(new ArrayList<>());
    assertEquals("[]", logResult.getJson());
    assertEquals(0, logResult.getCount());
    assertEquals(EMPTY_DIGEST, logResult.getDigest());
  }

  @Test
  public void testNullListReturnsEmptyResult() {
    DomainCanonicalizer.DomainResult accountResult =
        DomainCanonicalizer.accountToJsonAndDigest(null);
    assertEquals("[]", accountResult.getJson());
    assertEquals(0, accountResult.getCount());
    assertEquals(EMPTY_DIGEST, accountResult.getDigest());
  }

  // ================================
  // AEXT Parse/Serialize Tests
  // ================================

  @Test
  public void testParseAextFromValidAccountBytes() {
    // Build account bytes: balance(32) + nonce(8) + codeHash(32) + codeLen(4) + code(0) + AEXT
    byte[] accountBytes = buildAccountBytesWithAext(
        1000000000L, // balance
        5L,          // nonce
        new byte[32], // codeHash
        0,           // codeLen
        100L,        // netUsage
        50L,         // freeNetUsage
        200L,        // energyUsage
        1640000000L, // latestConsumeTime
        1640000100L, // latestConsumeFreeTime
        1640000200L, // latestConsumeTimeForEnergy
        1000L,       // netWindowSize
        2000L,       // energyWindowSize
        true,        // netWindowOptimized
        false        // energyWindowOptimized
    );

    DomainCanonicalizer.ParsedAext aext = DomainCanonicalizer.parseAext(accountBytes);

    assertNotNull("AEXT should be parsed", aext);
    assertEquals("Net usage should match", 100L, aext.netUsage);
    assertEquals("Free net usage should match", 50L, aext.freeNetUsage);
    assertEquals("Energy usage should match", 200L, aext.energyUsage);
    assertEquals("Net window size should match", 1000L, aext.netWindowSize);
    assertEquals("Energy window size should match", 2000L, aext.energyWindowSize);
    assertTrue("Net window optimized should be true", aext.netWindowOptimized);
  }

  @Test
  public void testParseAextReturnsNullForDataWithoutAext() {
    // Build minimal account bytes without AEXT (just 76 bytes)
    byte[] minimalBytes = new byte[76];

    DomainCanonicalizer.ParsedAext aext = DomainCanonicalizer.parseAext(minimalBytes);

    assertNull("AEXT should be null for data without AEXT", aext);
  }

  @Test
  public void testParseAextReturnsNullForNullData() {
    DomainCanonicalizer.ParsedAext aext = DomainCanonicalizer.parseAext(null);
    assertNull("AEXT should be null for null data", aext);
  }

  @Test
  public void testParseAextReturnsNullForShortData() {
    byte[] shortBytes = new byte[50]; // Less than minimum 76
    DomainCanonicalizer.ParsedAext aext = DomainCanonicalizer.parseAext(shortBytes);
    assertNull("AEXT should be null for short data", aext);
  }

  @Test
  public void testParseAextReturnsNullForWrongMagic() {
    // Build account bytes with wrong AEXT magic
    byte[] accountBytes = new byte[76 + 8 + 68]; // base + header + payload
    // Set code length to 0
    accountBytes[72] = 0;
    accountBytes[73] = 0;
    accountBytes[74] = 0;
    accountBytes[75] = 0;
    // Set wrong magic at offset 76 (should be "AEXT" = 0x41 0x45 0x58 0x54)
    accountBytes[76] = 'X';
    accountBytes[77] = 'Y';
    accountBytes[78] = 'Z';
    accountBytes[79] = 'W';

    DomainCanonicalizer.ParsedAext aext = DomainCanonicalizer.parseAext(accountBytes);
    assertNull("AEXT should be null for wrong magic", aext);
  }

  @Test
  public void testParseAextIsEmptyForZeroValues() {
    byte[] accountBytes = buildAccountBytesWithAext(
        1000000000L, 5L, new byte[32], 0,
        0L, 0L, 0L, 0L, 0L, 0L, 0L, 0L, false, false
    );

    DomainCanonicalizer.ParsedAext aext = DomainCanonicalizer.parseAext(accountBytes);

    assertNotNull("AEXT should be parsed", aext);
    assertTrue("AEXT with zero values should be empty", aext.isEmpty());
  }

  @Test
  public void testExtractAccountResourceUsageFromStateChanges() {
    // Create state change with AEXT difference
    byte[] oldAccountBytes = buildAccountBytesWithAext(
        1000000000L, 5L, new byte[32], 0,
        100L, 50L, 200L, 0L, 0L, 0L, 1000L, 2000L, false, false
    );
    byte[] newAccountBytes = buildAccountBytesWithAext(
        1000000000L, 5L, new byte[32], 0,
        150L, 75L, 250L, 0L, 0L, 0L, 1000L, 2000L, false, false
    );

    List<StateChange> stateChanges = new ArrayList<>();
    stateChanges.add(new StateChange(
        new byte[]{0x41, 0x01, 0x02, 0x03},
        new byte[0], // empty key = account change
        oldAccountBytes,
        newAccountBytes
    ));

    List<DomainCanonicalizer.AccountResourceUsageDelta> deltas =
        DomainCanonicalizer.extractAccountResourceUsage(stateChanges);

    assertEquals("Should extract one delta", 1, deltas.size());
    DomainCanonicalizer.AccountResourceUsageDelta delta = deltas.get(0);
    assertEquals("Old net usage should match", Long.valueOf(100L), delta.getOldNetUsage());
    assertEquals("New net usage should match", Long.valueOf(150L), delta.getNewNetUsage());
    assertEquals("Old energy usage should match", Long.valueOf(200L), delta.getOldEnergyUsage());
    assertEquals("New energy usage should match", Long.valueOf(250L), delta.getNewEnergyUsage());
  }

  @Test
  public void testExtractAccountResourceUsageSkipsUnchangedAext() {
    // Create state change with identical AEXT
    byte[] accountBytes = buildAccountBytesWithAext(
        1000000000L, 5L, new byte[32], 0,
        100L, 50L, 200L, 0L, 0L, 0L, 1000L, 2000L, false, false
    );

    List<StateChange> stateChanges = new ArrayList<>();
    stateChanges.add(new StateChange(
        new byte[]{0x41, 0x01, 0x02, 0x03},
        new byte[0],
        accountBytes,
        accountBytes // same AEXT
    ));

    List<DomainCanonicalizer.AccountResourceUsageDelta> deltas =
        DomainCanonicalizer.extractAccountResourceUsage(stateChanges);

    assertEquals("Should skip unchanged AEXT", 0, deltas.size());
  }

  /**
   * Helper to build account bytes with AEXT.
   * Format: balance(32) + nonce(8) + codeHash(32) + codeLen(4) + code(0) + AEXT
   * AEXT: magic(4) + version(2) + length(2) + payload(68)
   */
  private byte[] buildAccountBytesWithAext(
      long balance, long nonce, byte[] codeHash, int codeLen,
      long netUsage, long freeNetUsage, long energyUsage,
      long latestConsumeTime, long latestConsumeFreeTime, long latestConsumeTimeForEnergy,
      long netWindowSize, long energyWindowSize,
      boolean netWindowOptimized, boolean energyWindowOptimized) {

    byte[] result = new byte[76 + 8 + 68]; // base + AEXT header + AEXT payload

    // Balance: first 32 bytes (big-endian, but simplified for test)
    result[31] = (byte) (balance & 0xFF);
    result[30] = (byte) ((balance >> 8) & 0xFF);
    result[29] = (byte) ((balance >> 16) & 0xFF);
    result[28] = (byte) ((balance >> 24) & 0xFF);

    // Nonce: bytes 32-39 (big-endian)
    for (int i = 0; i < 8; i++) {
      result[39 - i] = (byte) ((nonce >> (8 * i)) & 0xFF);
    }

    // Code hash: bytes 40-71
    System.arraycopy(codeHash, 0, result, 40, Math.min(32, codeHash.length));

    // Code length: bytes 72-75 (big-endian)
    result[72] = (byte) ((codeLen >> 24) & 0xFF);
    result[73] = (byte) ((codeLen >> 16) & 0xFF);
    result[74] = (byte) ((codeLen >> 8) & 0xFF);
    result[75] = (byte) (codeLen & 0xFF);

    // AEXT header at offset 76
    int offset = 76;
    // Magic: "AEXT" (0x41 0x45 0x58 0x54)
    result[offset++] = 0x41;
    result[offset++] = 0x45;
    result[offset++] = 0x58;
    result[offset++] = 0x54;
    // Version: 1 (big-endian)
    result[offset++] = 0x00;
    result[offset++] = 0x01;
    // Length: 68 (big-endian)
    result[offset++] = 0x00;
    result[offset++] = 0x44;

    // AEXT payload (68 bytes)
    offset = writeI64BigEndian(result, offset, netUsage);
    offset = writeI64BigEndian(result, offset, freeNetUsage);
    offset = writeI64BigEndian(result, offset, energyUsage);
    offset = writeI64BigEndian(result, offset, latestConsumeTime);
    offset = writeI64BigEndian(result, offset, latestConsumeFreeTime);
    offset = writeI64BigEndian(result, offset, latestConsumeTimeForEnergy);
    offset = writeI64BigEndian(result, offset, netWindowSize);
    offset = writeI64BigEndian(result, offset, energyWindowSize);
    result[offset++] = (byte) (netWindowOptimized ? 1 : 0);
    result[offset] = (byte) (energyWindowOptimized ? 1 : 0);

    return result;
  }

  private int writeI64BigEndian(byte[] dest, int offset, long value) {
    for (int i = 0; i < 8; i++) {
      dest[offset + i] = (byte) ((value >> (56 - 8 * i)) & 0xFF);
    }
    return offset + 8;
  }

  @Test
  public void testJsonKeysAreSorted() {
    List<DomainCanonicalizer.AccountDelta> deltas = new ArrayList<>();
    DomainCanonicalizer.AccountDelta delta = new DomainCanonicalizer.AccountDelta();
    delta.setAddressHex("41addr");
    delta.setOp("update");
    delta.setOldBalance(100L);
    delta.setNewBalance(200L);
    delta.setOldNonce(1L);
    delta.setNewNonce(2L);
    deltas.add(delta);

    DomainCanonicalizer.DomainResult result =
        DomainCanonicalizer.accountToJsonAndDigest(deltas);

    // Keys should be sorted: address_hex, balance_sun, nonce, op
    int addrPos = result.getJson().indexOf("address_hex");
    int balancePos = result.getJson().indexOf("balance_sun");
    int noncePos = result.getJson().indexOf("nonce");
    int opPos = result.getJson().indexOf("op");

    assertTrue("Keys should be sorted alphabetically",
        addrPos < balancePos && balancePos < noncePos && noncePos < opPos);
  }

  @Test
  public void testDigestStabilityWithPermutedInputs() {
    // Create account deltas in different orders and verify same digest
    DomainCanonicalizer.AccountDelta delta1 = new DomainCanonicalizer.AccountDelta();
    delta1.setAddressHex("41aaa");
    delta1.setOp("update");
    delta1.setOldBalance(100L);
    delta1.setNewBalance(200L);

    DomainCanonicalizer.AccountDelta delta2 = new DomainCanonicalizer.AccountDelta();
    delta2.setAddressHex("41bbb");
    delta2.setOp("create");
    delta2.setNewBalance(300L);

    DomainCanonicalizer.AccountDelta delta3 = new DomainCanonicalizer.AccountDelta();
    delta3.setAddressHex("41ccc");
    delta3.setOp("delete");
    delta3.setOldBalance(50L);

    // Order 1: delta1, delta2, delta3
    List<DomainCanonicalizer.AccountDelta> order1 = new ArrayList<>();
    order1.add(delta1);
    order1.add(delta2);
    order1.add(delta3);
    DomainCanonicalizer.DomainResult result1 =
        DomainCanonicalizer.accountToJsonAndDigest(order1);

    // Order 2: delta3, delta1, delta2 (different input order)
    List<DomainCanonicalizer.AccountDelta> order2 = new ArrayList<>();
    order2.add(delta3);
    order2.add(delta1);
    order2.add(delta2);
    DomainCanonicalizer.DomainResult result2 =
        DomainCanonicalizer.accountToJsonAndDigest(order2);

    // Order 3: delta2, delta3, delta1 (yet another input order)
    List<DomainCanonicalizer.AccountDelta> order3 = new ArrayList<>();
    order3.add(delta2);
    order3.add(delta3);
    order3.add(delta1);
    DomainCanonicalizer.DomainResult result3 =
        DomainCanonicalizer.accountToJsonAndDigest(order3);

    // All digests should be identical regardless of input order
    assertEquals("Digests should be identical for different input orders",
        result1.getDigest(), result2.getDigest());
    assertEquals("Digests should be identical for different input orders",
        result2.getDigest(), result3.getDigest());

    // JSON should also be identical
    assertEquals("JSON should be identical for different input orders",
        result1.getJson(), result2.getJson());
    assertEquals("JSON should be identical for different input orders",
        result2.getJson(), result3.getJson());
  }

  @Test
  public void testEvmStorageDigestStabilityWithPermutedInputs() {
    // Create EVM storage deltas in different orders and verify same digest
    DomainCanonicalizer.EvmStorageDelta delta1 = new DomainCanonicalizer.EvmStorageDelta();
    delta1.setContractAddressHex("41contract1");
    delta1.setSlotKeyHex("0000000000000000000000000000000000000000000000000000000000000001");
    delta1.setOp("set");
    delta1.setNewValueHex("deadbeef");

    DomainCanonicalizer.EvmStorageDelta delta2 = new DomainCanonicalizer.EvmStorageDelta();
    delta2.setContractAddressHex("41contract1");
    delta2.setSlotKeyHex("0000000000000000000000000000000000000000000000000000000000000002");
    delta2.setOp("set");
    delta2.setNewValueHex("cafebabe");

    DomainCanonicalizer.EvmStorageDelta delta3 = new DomainCanonicalizer.EvmStorageDelta();
    delta3.setContractAddressHex("41contract2");
    delta3.setSlotKeyHex("0000000000000000000000000000000000000000000000000000000000000001");
    delta3.setOp("delete");
    delta3.setOldValueHex("12345678");

    // Different input orders
    List<DomainCanonicalizer.EvmStorageDelta> order1 = new ArrayList<>();
    order1.add(delta1);
    order1.add(delta2);
    order1.add(delta3);
    DomainCanonicalizer.DomainResult result1 =
        DomainCanonicalizer.evmStorageToJsonAndDigest(order1);

    List<DomainCanonicalizer.EvmStorageDelta> order2 = new ArrayList<>();
    order2.add(delta3);
    order2.add(delta1);
    order2.add(delta2);
    DomainCanonicalizer.DomainResult result2 =
        DomainCanonicalizer.evmStorageToJsonAndDigest(order2);

    // All digests should be identical regardless of input order
    assertEquals("EVM storage digests should be identical for different input orders",
        result1.getDigest(), result2.getDigest());
    assertEquals("EVM storage JSON should be identical for different input orders",
        result1.getJson(), result2.getJson());
  }

  @Test
  public void testVoteDigestStabilityWithPermutedInputs() {
    // Create vote deltas in different orders
    DomainCanonicalizer.VoteDelta delta1 = new DomainCanonicalizer.VoteDelta();
    delta1.setVoterAddressHex("41voter1");
    delta1.setWitnessAddressHex("41witness1");
    delta1.setOp("create");
    delta1.setOldVotes("0");
    delta1.setNewVotes("100");

    DomainCanonicalizer.VoteDelta delta2 = new DomainCanonicalizer.VoteDelta();
    delta2.setVoterAddressHex("41voter1");
    delta2.setWitnessAddressHex("41witness2");
    delta2.setOp("update");
    delta2.setOldVotes("50");
    delta2.setNewVotes("75");

    List<DomainCanonicalizer.VoteDelta> order1 = new ArrayList<>();
    order1.add(delta1);
    order1.add(delta2);
    DomainCanonicalizer.DomainResult result1 =
        DomainCanonicalizer.votesToJsonAndDigest(order1);

    List<DomainCanonicalizer.VoteDelta> order2 = new ArrayList<>();
    order2.add(delta2);
    order2.add(delta1);
    DomainCanonicalizer.DomainResult result2 =
        DomainCanonicalizer.votesToJsonAndDigest(order2);

    assertEquals("Vote digests should be identical for different input orders",
        result1.getDigest(), result2.getDigest());
  }
}
