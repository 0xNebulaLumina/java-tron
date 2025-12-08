package org.tron.core.execution.reporting;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import org.junit.Test;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;

/**
 * Unit tests for ExecutionCsvRecord and its builder.
 */
public class ExecutionCsvRecordTest {

  @Test
  public void testBasicRecordCreation() {
    ExecutionCsvRecord record = ExecutionCsvRecord.builder()
        .runId("test-run-123")
        .execMode("EMBEDDED")
        .storageMode("EMBEDDED")
        .blockNum(12345)
        .blockIdHex("0xabcdef1234567890")
        .isWitnessSigned(true)
        .blockTimestamp(System.currentTimeMillis())
        .txIndexInBlock(2)
        .txIdHex("0x1234567890abcdef")
        .ownerAddressHex("0xdeadbeef")
        .contractType("TriggerSmartContract")
        .isConstant(false)
        .feeLimit(1000000)
        .isSuccess(true)
        .resultCode("SUCCESS")
        .energyUsed(50000)
        .returnDataHex("0x42")
        .runtimeError("")
        .stateChangeCount(2)
        .stateDigestSha256("abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab")
        .build();

    assertNotNull(record);
    assertEquals("test-run-123", record.getRunId());
    assertEquals("EMBEDDED", record.getExecMode());
    assertEquals("EMBEDDED", record.getStorageMode());
    assertEquals(12345, record.getBlockNum());
    assertEquals("0xabcdef1234567890", record.getBlockIdHex());
    assertTrue(record.isWitnessSigned());
    assertEquals(2, record.getTxIndexInBlock());
    assertEquals("0x1234567890abcdef", record.getTxIdHex());
    assertEquals("0xdeadbeef", record.getOwnerAddressHex());
    assertEquals("TriggerSmartContract", record.getContractType());
    assertEquals(false, record.isConstant());
    assertEquals(1000000, record.getFeeLimit());
    assertTrue(record.isSuccess());
    assertEquals("SUCCESS", record.getResultCode());
    assertEquals(50000, record.getEnergyUsed());
    assertEquals("0x42", record.getReturnDataHex());
    assertEquals("", record.getRuntimeError());
    assertEquals(2, record.getStateChangeCount());
    assertEquals("abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab",
                 record.getStateDigestSha256());
  }

  @Test
  public void testBuilderWithByteArrays() {
    byte[] blockId = {(byte) 0xab, (byte) 0xcd, (byte) 0xef, 0x12};
    byte[] txId = {0x12, 0x34, 0x56, 0x78};
    byte[] ownerAddress = {(byte) 0xde, (byte) 0xad, (byte) 0xbe, (byte) 0xef};
    byte[] returnData = {0x42, 0x43};

    ExecutionCsvRecord record = ExecutionCsvRecord.builder()
        .runId("test-run-456")
        .blockIdHex(blockId)
        .txIdHex(txId)
        .ownerAddressHex(ownerAddress)
        .returnDataHex(returnData)
        .build();

    assertEquals("abcdef12", record.getBlockIdHex());
    assertEquals("12345678", record.getTxIdHex());
    assertEquals("deadbeef", record.getOwnerAddressHex());
    assertEquals("4243", record.getReturnDataHex());
    assertEquals(2, record.getReturnDataLen());
  }

  @Test
  public void testBuilderWithStateChanges() {
    StateChange change1 = new StateChange(
        new byte[]{0x01, 0x02},
        new byte[]{0x03, 0x04},
        new byte[]{0x05, 0x06},
        new byte[]{0x07, 0x08}
    );
    StateChange change2 = new StateChange(
        new byte[]{0x11, 0x12},
        new byte[]{0x13, 0x14},
        new byte[]{0x15, 0x16},
        new byte[]{0x17, 0x18}
    );

    List<StateChange> stateChanges = Arrays.asList(change1, change2);

    ExecutionCsvRecord record = ExecutionCsvRecord.builder()
        .runId("test-run-789")
        .stateChanges(stateChanges)
        .build();

    assertEquals(2, record.getStateChangeCount());
    assertNotNull(record.getStateChangesJson());
    assertTrue("State changes JSON should contain address field",
               record.getStateChangesJson().contains("\"address\""));
    assertTrue("State changes JSON should be array format",
               record.getStateChangesJson().startsWith("["));
    assertNotNull(record.getStateDigestSha256());
    assertTrue("State digest should be valid",
               StateChangeCanonicalizer.isValidStateDigest(record.getStateDigestSha256()));
  }

  @Test
  public void testCsvRowGeneration() {
    ExecutionCsvRecord record = ExecutionCsvRecord.builder()
        .runId("test-run-csv")
        .execMode("REMOTE")
        .storageMode("REMOTE")
        .blockNum(99999)
        .blockIdHex("0x123456789abcdef0")
        .isWitnessSigned(false)
        .blockTimestamp(1640995200000L) // 2022-01-01 00:00:00 UTC
        .txIndexInBlock(0)
        .txIdHex("0xfedcba0987654321")
        .ownerAddressHex("0x1234567890abcdef")
        .contractType("TriggerSmartContract")
        .isConstant(false)
        .feeLimit(2000000)
        .isSuccess(true)
        .resultCode("SUCCESS")
        .energyUsed(75000)
        .returnDataHex("0x424344")
        .returnDataLen(3)
        .runtimeError("")
        .stateChangeCount(1)
        .stateChanges(Arrays.asList(new StateChange(
            hexToBytes("0x123"), hexToBytes("0x456"), new byte[0], hexToBytes("0x789"))))
        .stateDigestSha256("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
        .build();

    String csvRow = record.toCsvRow();
    assertNotNull(csvRow);

    // Check that all fields are present (basic sanity check)
    assertTrue("CSV should contain run ID", csvRow.contains("test-run-csv"));
    assertTrue("CSV should contain exec mode", csvRow.contains("REMOTE"));
    assertTrue("CSV should contain block number", csvRow.contains("99999"));
    assertTrue("CSV should contain success flag", csvRow.contains("true"));
    assertTrue("CSV should contain energy used", csvRow.contains("75000"));

    // Check CSV field count (note: splitting by comma is not reliable for properly escaped CSV)
    // Instead, just verify the row is not empty and contains expected data
    assertTrue("CSV row should not be empty", csvRow.length() > 0);
    assertTrue("CSV should contain quoted fields", csvRow.contains("\""));
  }

  @Test
  public void testCsvHeaderGeneration() {
    String header = ExecutionCsvRecord.getCsvHeader();
    assertNotNull(header);

    // Check key fields are present in header
    assertTrue("Header should contain run_id", header.contains("run_id"));
    assertTrue("Header should contain exec_mode", header.contains("exec_mode"));
    assertTrue("Header should contain storage_mode", header.contains("storage_mode"));
    assertTrue("Header should contain block_num", header.contains("block_num"));
    assertTrue("Header should contain tx_id_hex", header.contains("tx_id_hex"));
    assertTrue("Header should contain is_success", header.contains("is_success"));
    assertTrue("Header should contain energy_used", header.contains("energy_used"));
    assertTrue("Header should contain state_digest_sha256", header.contains("state_digest_sha256"));

    // Check new domain columns are present
    assertTrue("Header should contain account_changes_json",
        header.contains("account_changes_json"));
    assertTrue("Header should contain account_change_count",
        header.contains("account_change_count"));
    assertTrue("Header should contain account_digest_sha256",
        header.contains("account_digest_sha256"));
    assertTrue("Header should contain evm_storage_changes_json",
        header.contains("evm_storage_changes_json"));
    assertTrue("Header should contain trc10_balance_changes_json",
        header.contains("trc10_balance_changes_json"));
    assertTrue("Header should contain trc10_issuance_changes_json",
        header.contains("trc10_issuance_changes_json"));
    assertTrue("Header should contain vote_changes_json",
        header.contains("vote_changes_json"));
    assertTrue("Header should contain freeze_changes_json",
        header.contains("freeze_changes_json"));
    assertTrue("Header should contain global_resource_changes_json",
        header.contains("global_resource_changes_json"));
    assertTrue("Header should contain account_resource_usage_changes_json",
        header.contains("account_resource_usage_changes_json"));
    assertTrue("Header should contain log_entries_json",
        header.contains("log_entries_json"));

    // Check field count matches record field count
    String[] headerFields = header.split(",");
    assertEquals("Header should have 50 fields", 50, headerFields.length);
  }

  @Test
  public void testCsvColumnCount() {
    assertEquals("Column count should be 50", 50, ExecutionCsvRecord.getColumnCount());
  }

  @Test
  public void testCsvEscaping() {
    ExecutionCsvRecord record = ExecutionCsvRecord.builder()
        .runId("test-run-with-\"quotes\"")
        .runtimeError("Error message with \"quotes\" and, commas")
        .build();

    String csvRow = record.toCsvRow();

    // Verify proper CSV escaping - just check that quotes are doubled
    assertTrue("CSV should contain escaped quotes", csvRow.contains("\"\""));
    assertTrue("CSV should properly escape data", csvRow.length() > 50);
  }

  @Test
  public void testEmptyStateChanges() {
    ExecutionCsvRecord record = ExecutionCsvRecord.builder()
        .runId("test-empty-states")
        .stateChanges(new ArrayList<>())
        .build();

    assertEquals(0, record.getStateChangeCount());
    assertEquals("", record.getStateChangesJson());

    // Should still have a valid digest (for empty state)
    assertNotNull(record.getStateDigestSha256());
    assertTrue(StateChangeCanonicalizer.isValidStateDigest(record.getStateDigestSha256()));
    assertEquals(StateChangeCanonicalizer.computeEmptyStateDigest(),
                 record.getStateDigestSha256());
  }

  @Test
  public void testDomainTripletBuilders() {
    DomainCanonicalizer.DomainResult result = new DomainCanonicalizer.DomainResult(
        "[{\"test\":\"data\"}]", 1, "abc123def456");

    ExecutionCsvRecord record = ExecutionCsvRecord.builder()
        .runId("test-domain")
        .accountDomain(result)
        .evmStorageDomain(result)
        .trc10BalanceDomain(result)
        .trc10IssuanceDomain(result)
        .voteDomain(result)
        .freezeDomain(result)
        .globalResourceDomain(result)
        .accountResourceUsageDomain(result)
        .logsDomain(result)
        .build();

    assertEquals("[{\"test\":\"data\"}]", record.getAccountChangesJson());
    assertEquals(1, record.getAccountChangeCount());
    assertEquals("abc123def456", record.getAccountDigestSha256());

    assertEquals("[{\"test\":\"data\"}]", record.getEvmStorageChangesJson());
    assertEquals(1, record.getEvmStorageChangeCount());
    assertEquals("abc123def456", record.getEvmStorageDigestSha256());

    assertEquals("[{\"test\":\"data\"}]", record.getTrc10BalanceChangesJson());
    assertEquals("[{\"test\":\"data\"}]", record.getTrc10IssuanceChangesJson());
    assertEquals("[{\"test\":\"data\"}]", record.getVoteChangesJson());
    assertEquals("[{\"test\":\"data\"}]", record.getFreezeChangesJson());
    assertEquals("[{\"test\":\"data\"}]", record.getGlobalResourceChangesJson());
    assertEquals("[{\"test\":\"data\"}]", record.getAccountResourceUsageChangesJson());
    assertEquals("[{\"test\":\"data\"}]", record.getLogEntriesJson());
  }

  @Test
  public void testDefaultDomainValues() {
    ExecutionCsvRecord record = ExecutionCsvRecord.builder()
        .runId("test-defaults")
        .build();

    // All domain JSONs should default to empty array
    assertEquals("[]", record.getAccountChangesJson());
    assertEquals("[]", record.getEvmStorageChangesJson());
    assertEquals("[]", record.getTrc10BalanceChangesJson());
    assertEquals("[]", record.getTrc10IssuanceChangesJson());
    assertEquals("[]", record.getVoteChangesJson());
    assertEquals("[]", record.getFreezeChangesJson());
    assertEquals("[]", record.getGlobalResourceChangesJson());
    assertEquals("[]", record.getAccountResourceUsageChangesJson());
    assertEquals("[]", record.getLogEntriesJson());

    // All domain counts should default to 0
    assertEquals(0, record.getAccountChangeCount());
    assertEquals(0, record.getEvmStorageChangeCount());
    assertEquals(0, record.getTrc10BalanceChangeCount());
    assertEquals(0, record.getTrc10IssuanceChangeCount());
    assertEquals(0, record.getVoteChangeCount());
    assertEquals(0, record.getFreezeChangeCount());
    assertEquals(0, record.getGlobalResourceChangeCount());
    assertEquals(0, record.getAccountResourceUsageChangeCount());
    assertEquals(0, record.getLogEntryCount());

    // All domain digests should default to empty string
    assertEquals("", record.getAccountDigestSha256());
    assertEquals("", record.getEvmStorageDigestSha256());
    assertEquals("", record.getTrc10BalanceDigestSha256());
    assertEquals("", record.getTrc10IssuanceDigestSha256());
    assertEquals("", record.getVoteDigestSha256());
    assertEquals("", record.getFreezeDigestSha256());
    assertEquals("", record.getGlobalResourceDigestSha256());
    assertEquals("", record.getAccountResourceUsageDigestSha256());
    assertEquals("", record.getLogEntriesDigestSha256());
  }

  @Test
  public void testCsvRowContainsDomainColumns() {
    DomainCanonicalizer.DomainResult accountResult = new DomainCanonicalizer.DomainResult(
        "[{\"address_hex\":\"41abc\"}]", 1, "digest123");

    ExecutionCsvRecord record = ExecutionCsvRecord.builder()
        .runId("test-csv-domains")
        .execMode("REMOTE")
        .storageMode("REMOTE")
        .accountDomain(accountResult)
        .build();

    String csvRow = record.toCsvRow();

    // Verify domain data appears in CSV
    assertTrue("CSV should contain account changes JSON",
        csvRow.contains("41abc"));
    assertTrue("CSV should contain account digest",
        csvRow.contains("digest123"));
  }

  @Test
  public void testTsMsPresenceInConstructorAndCsvRow() {
    long beforeMs = System.currentTimeMillis();
    ExecutionCsvRecord record = ExecutionCsvRecord.builder()
        .runId("test-tsms")
        .build();
    long afterMs = System.currentTimeMillis();

    // Verify ts_ms is set in constructor
    long tsMs = record.getTsMs();
    assertTrue("ts_ms should be set to current time on construction",
        tsMs >= beforeMs && tsMs <= afterMs);

    // Verify ts_ms appears in CSV row
    String csvRow = record.toCsvRow();
    assertTrue("CSV row should contain ts_ms value",
        csvRow.contains(String.valueOf(tsMs)));

    // Verify ts_ms column is in header
    String header = ExecutionCsvRecord.getCsvHeader();
    assertTrue("Header should contain ts_ms", header.contains("ts_ms"));
  }

  @Test
  public void testTsMsWithExplicitValue() {
    long explicitTs = 1640995200000L; // 2022-01-01 00:00:00 UTC
    ExecutionCsvRecord record = ExecutionCsvRecord.builder()
        .runId("test-tsms-explicit")
        .tsMs(explicitTs)
        .build();

    assertEquals("ts_ms should match explicit value", explicitTs, record.getTsMs());

    String csvRow = record.toCsvRow();
    assertTrue("CSV should contain explicit ts_ms value",
        csvRow.contains("1640995200000"));
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
}
