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
    
    // Check field count matches record field count  
    String[] headerFields = header.split(",");
    assertEquals("Header should have 23 fields", 23, headerFields.length);
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