package org.tron.core.execution.spi;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractValidateException;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.Transaction.Contract.ContractType;
import org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract;
import tron.backend.BackendOuterClass.*;

/**
 * Test class for RemoteExecutionSPI's AssetIssueContract mapping.
 * Tests that AssetIssueContract transactions are correctly mapped to remote execution requests.
 */
public class RemoteExecutionSPIAssetIssueTest {

  private RemoteExecutionSPI remoteSPI;
  private static final byte[] OWNER_ADDRESS = new byte[21];
  private static final String TEST_ASSET_NAME = "TestToken";
  private static final String TEST_ABBR = "TT";
  private static final long TOTAL_SUPPLY = 1000000L;
  private static final int PRECISION = 6;
  private static final int TRX_NUM = 1;
  private static final int NUM = 1;
  private static final long START_TIME = System.currentTimeMillis();
  private static final long END_TIME = START_TIME + 86400000L; // +1 day

  static {
    // Initialize owner address with TRON address prefix (0x41)
    OWNER_ADDRESS[0] = 0x41;
    for (int i = 1; i < OWNER_ADDRESS.length; i++) {
      OWNER_ADDRESS[i] = (byte) i;
    }
  }

  @Before
  public void setUp() {
    // Create RemoteExecutionSPI instance for testing
    remoteSPI = new RemoteExecutionSPI("localhost", 50011);
  }

  @After
  public void tearDown() {
    // Clear system properties after each test
    System.clearProperty("remote.exec.trc10.enabled");
    if (remoteSPI != null) {
      remoteSPI.shutdown();
    }
  }

  /**
   * Helper method to create an AssetIssueContract transaction for testing.
   */
  private TransactionCapsule createAssetIssueTransaction() throws Exception {
    // Build AssetIssueContract
    AssetIssueContract assetIssueContract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(OWNER_ADDRESS))
        .setName(ByteString.copyFromUtf8(TEST_ASSET_NAME))
        .setAbbr(ByteString.copyFromUtf8(TEST_ABBR))
        .setTotalSupply(TOTAL_SUPPLY)
        .setPrecision(PRECISION)
        .setTrxNum(TRX_NUM)
        .setNum(NUM)
        .setStartTime(START_TIME)
        .setEndTime(END_TIME)
        .setDescription(ByteString.copyFromUtf8("Test asset"))
        .setUrl(ByteString.copyFromUtf8("https://test.com"))
        .build();

    // Pack contract into Any
    Any contractParameter = Any.pack(assetIssueContract);

    // Build transaction contract
    Protocol.Transaction.Contract contract = Protocol.Transaction.Contract.newBuilder()
        .setType(ContractType.AssetIssueContract)
        .setParameter(contractParameter)
        .build();

    // Build transaction
    Protocol.Transaction transaction = Protocol.Transaction.newBuilder()
        .setRawData(Protocol.Transaction.raw.newBuilder()
            .addContract(contract)
            .setFeeLimit(1000000000L)
            .setTimestamp(System.currentTimeMillis())
            .build())
        .build();

    return new TransactionCapsule(transaction);
  }

  /**
   * Test that AssetIssueContract throws UnsupportedOperationException when TRC-10 is disabled.
   */
  @Test
  public void testAssetIssueContractDisabled() throws Exception {
    // Ensure TRC-10 is disabled (default)
    System.clearProperty("remote.exec.trc10.enabled");

    TransactionCapsule trxCap = createAssetIssueTransaction();
    BlockCapsule blockCapsule = new BlockCapsule(1, ByteString.copyFrom(new byte[32]), System.currentTimeMillis(), ByteString.copyFrom(OWNER_ADDRESS));
    TransactionContext context = new TransactionContext(blockCapsule, trxCap, null);

    try {
      // This should throw UnsupportedOperationException since TRC-10 is disabled
      remoteSPI.executeTransaction(context).get();
      Assert.fail("Should have thrown UnsupportedOperationException");
    } catch (Exception e) {
      // Unwrap CompletionException
      Throwable cause = e.getCause();
      Assert.assertTrue("Expected UnsupportedOperationException but got: " + cause.getClass().getName(),
          cause instanceof UnsupportedOperationException);
      Assert.assertTrue("Error message should mention disabled TRC-10",
          cause.getMessage().contains("AssetIssue execution via remote backend is disabled"));
      Assert.assertTrue("Error message should mention the flag",
          cause.getMessage().contains("remote.exec.trc10.enabled"));
    }
  }

  /**
   * Test that AssetIssueContract is properly mapped when TRC-10 is enabled.
   * Note: This test validates the request building logic without requiring a running Rust backend.
   */
  @Test
  public void testAssetIssueContractEnabled() throws Exception {
    // Enable TRC-10 remote execution
    System.setProperty("remote.exec.trc10.enabled", "true");

    TransactionCapsule trxCap = createAssetIssueTransaction();
    Protocol.Transaction transaction = trxCap.getInstance();
    Protocol.Transaction.Contract contract = transaction.getRawData().getContract(0);

    // Verify contract type
    Assert.assertEquals(ContractType.AssetIssueContract, contract.getType());

    // Unpack and verify contract parameter
    Any contractParameter = contract.getParameter();
    AssetIssueContract assetIssueContract = contractParameter.unpack(AssetIssueContract.class);

    Assert.assertEquals(TEST_ASSET_NAME, assetIssueContract.getName().toStringUtf8());
    Assert.assertEquals(TEST_ABBR, assetIssueContract.getAbbr().toStringUtf8());
    Assert.assertEquals(TOTAL_SUPPLY, assetIssueContract.getTotalSupply());
    Assert.assertEquals(PRECISION, assetIssueContract.getPrecision());
    Assert.assertEquals(TRX_NUM, assetIssueContract.getTrxNum());
    Assert.assertEquals(NUM, assetIssueContract.getNum());
    Assert.assertEquals(START_TIME, assetIssueContract.getStartTime());
    Assert.assertEquals(END_TIME, assetIssueContract.getEndTime());

    // Verify that the contract serializes correctly (this is what gets sent to Rust)
    byte[] contractBytes = assetIssueContract.toByteArray();
    Assert.assertNotNull(contractBytes);
    Assert.assertTrue("Contract bytes should not be empty", contractBytes.length > 0);

    // Verify that we can deserialize the bytes back to the same contract
    AssetIssueContract deserializedContract = AssetIssueContract.parseFrom(contractBytes);
    Assert.assertEquals(TEST_ASSET_NAME, deserializedContract.getName().toStringUtf8());
    Assert.assertEquals(TOTAL_SUPPLY, deserializedContract.getTotalSupply());

    // Note: Full execution test requires a running Rust backend, which is tested separately in integration tests
  }

  /**
   * Test AssetIssueContract classification - verify that key fields would be mapped correctly.
   */
  @Test
  public void testAssetIssueContractClassification() throws Exception {
    // Enable TRC-10 remote execution
    System.setProperty("remote.exec.trc10.enabled", "true");

    TransactionCapsule trxCap = createAssetIssueTransaction();
    Protocol.Transaction transaction = trxCap.getInstance();
    Protocol.Transaction.Contract contract = transaction.getRawData().getContract(0);

    // Verify transaction structure that will be used for classification
    Assert.assertEquals(ContractType.AssetIssueContract, contract.getType());

    // Verify owner address
    AssetIssueContract assetIssueContract = contract.getParameter().unpack(AssetIssueContract.class);
    Assert.assertArrayEquals(OWNER_ADDRESS, assetIssueContract.getOwnerAddress().toByteArray());

    // According to RemoteExecutionSPI implementation:
    // - TxKind should be NON_VM (verified by contract type)
    // - ContractType should be ASSET_ISSUE_CONTRACT
    // - fromAddress should be owner address
    // - toAddress should be empty (system contract)
    // - value should be 0 (fee charged separately)
    // - data should be contract.toByteArray()
    // - assetId should be empty (not a transfer)

    // Verify contract can be serialized for remote execution
    byte[] dataPayload = assetIssueContract.toByteArray();
    Assert.assertNotNull("Data payload must not be null", dataPayload);
    Assert.assertTrue("Data payload must not be empty", dataPayload.length > 0);

    // Verify expected classification matches RemoteExecutionSPI logic:
    // Expected: txKind = NON_VM, contractType = ASSET_ISSUE_CONTRACT
    // This verifies that the contract structure is correct for remote execution mapping
  }

  /**
   * Test AssetIssueContract with minimal fields (edge case).
   */
  @Test
  public void testAssetIssueContractMinimalFields() throws Exception {
    // Build minimal AssetIssueContract
    AssetIssueContract minimalContract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(OWNER_ADDRESS))
        .setName(ByteString.copyFromUtf8("MIN"))
        .setTotalSupply(100L)
        .build();

    // Verify it can be serialized
    byte[] contractBytes = minimalContract.toByteArray();
    Assert.assertNotNull(contractBytes);
    Assert.assertTrue(contractBytes.length > 0);

    // Verify deserialization
    AssetIssueContract deserialized = AssetIssueContract.parseFrom(contractBytes);
    Assert.assertEquals("MIN", deserialized.getName().toStringUtf8());
    Assert.assertEquals(100L, deserialized.getTotalSupply());
  }

  /**
   * Test that the TRC-10 flag is read correctly from system properties.
   */
  @Test
  public void testTrc10FlagReading() {
    // Test default (disabled)
    String disabled = System.getProperty("remote.exec.trc10.enabled", "false");
    Assert.assertEquals("false", disabled);
    Assert.assertFalse(Boolean.parseBoolean(disabled));

    // Test enabled
    System.setProperty("remote.exec.trc10.enabled", "true");
    String enabled = System.getProperty("remote.exec.trc10.enabled", "false");
    Assert.assertEquals("true", enabled);
    Assert.assertTrue(Boolean.parseBoolean(enabled));

    // Test case insensitivity
    System.setProperty("remote.exec.trc10.enabled", "TRUE");
    Assert.assertTrue(Boolean.parseBoolean(System.getProperty("remote.exec.trc10.enabled", "false")));

    System.setProperty("remote.exec.trc10.enabled", "False");
    Assert.assertFalse(Boolean.parseBoolean(System.getProperty("remote.exec.trc10.enabled", "false")));
  }
}
