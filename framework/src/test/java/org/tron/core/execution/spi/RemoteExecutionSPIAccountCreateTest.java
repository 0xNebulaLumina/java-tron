package org.tron.core.execution.spi;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.tron.common.utils.Sha256Hash;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction.Contract.ContractType;
import org.tron.protos.contract.AccountContract.AccountCreateContract;
import tron.backend.BackendOuterClass.*;

/**
 * Test class for RemoteExecutionSPI's AccountCreateContract mapping.
 * Verifies that AccountCreateContract transactions are correctly mapped
 * to remote execution requests with proper TxKind, ContractType, addresses,
 * and data fields.
 */
public class RemoteExecutionSPIAccountCreateTest {

  private RemoteExecutionSPI remoteSPI;
  private static final byte[] OWNER_ADDRESS = new byte[21];
  private static final byte[] TARGET_ADDRESS = new byte[21];
  private static final byte[] WITNESS_ADDRESS = new byte[21];

  static {
    // Initialize owner address with TRON mainnet prefix (0x41)
    OWNER_ADDRESS[0] = 0x41;
    for (int i = 1; i < OWNER_ADDRESS.length; i++) {
      OWNER_ADDRESS[i] = (byte) (i + 0x10);
    }
    // Initialize target address
    TARGET_ADDRESS[0] = 0x41;
    for (int i = 1; i < TARGET_ADDRESS.length; i++) {
      TARGET_ADDRESS[i] = (byte) (i + 0x20);
    }
    // Initialize witness address
    WITNESS_ADDRESS[0] = 0x41;
    for (int i = 1; i < WITNESS_ADDRESS.length; i++) {
      WITNESS_ADDRESS[i] = (byte) (i + 0x30);
    }
  }

  @Before
  public void setUp() {
    // Disable AEXT collection so tests don't need AccountStore
    System.setProperty("remote.exec.preexec.aext.enabled", "false");
    remoteSPI = new RemoteExecutionSPI("localhost", 50011);
  }

  @After
  public void tearDown() {
    System.clearProperty("remote.exec.preexec.aext.enabled");
    if (remoteSPI != null) {
      remoteSPI.shutdown();
    }
  }

  /**
   * Helper method to create an AccountCreateContract transaction.
   */
  private TransactionCapsule createAccountCreateTransaction(
      byte[] ownerAddr, byte[] targetAddr, AccountType type) throws Exception {
    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerAddr))
        .setAccountAddress(ByteString.copyFrom(targetAddr))
        .setType(type)
        .build();

    Any contractParameter = Any.pack(contract);

    Protocol.Transaction.Contract txContract = Protocol.Transaction.Contract.newBuilder()
        .setType(ContractType.AccountCreateContract)
        .setParameter(contractParameter)
        .build();

    Protocol.Transaction transaction = Protocol.Transaction.newBuilder()
        .setRawData(Protocol.Transaction.raw.newBuilder()
            .addContract(txContract)
            .setTimestamp(System.currentTimeMillis())
            .build())
        .build();

    return new TransactionCapsule(transaction);
  }

  /**
   * Helper to build a TransactionContext with a dummy BlockCapsule (no StoreFactory).
   */
  private TransactionContext buildContext(TransactionCapsule trxCap) {
    BlockCapsule blockCap = new BlockCapsule(
        100L,
        Sha256Hash.ZERO_HASH,
        System.currentTimeMillis(),
        ByteString.copyFrom(WITNESS_ADDRESS));
    return new TransactionContext(blockCap, trxCap, null, false, false);
  }

  // ==========================================================================
  // Request mapping assertions — these exercise buildExecuteTransactionRequest
  // ==========================================================================

  /**
   * Verify AccountCreateContract maps to TxKind.NON_VM in the actual request.
   */
  @Test
  public void testAccountCreateMapsToNonVmTxKind() throws Exception {
    TransactionCapsule trxCap = createAccountCreateTransaction(
        OWNER_ADDRESS, TARGET_ADDRESS, AccountType.Normal);
    TransactionContext ctx = buildContext(trxCap);

    ExecuteTransactionRequest request = remoteSPI.buildExecuteTransactionRequest(ctx);
    TronTransaction tx = request.getTransaction();

    Assert.assertEquals(
        "TxKind must be NON_VM for system contracts",
        TxKind.NON_VM,
        tx.getTxKind());
  }

  /**
   * Verify the request carries ACCOUNT_CREATE_CONTRACT as the contract type.
   */
  @Test
  public void testAccountCreateContractTypeMapping() throws Exception {
    TransactionCapsule trxCap = createAccountCreateTransaction(
        OWNER_ADDRESS, TARGET_ADDRESS, AccountType.Normal);
    TransactionContext ctx = buildContext(trxCap);

    ExecuteTransactionRequest request = remoteSPI.buildExecuteTransactionRequest(ctx);
    TronTransaction tx = request.getTransaction();

    Assert.assertEquals(
        "ContractType must be ACCOUNT_CREATE_CONTRACT",
        tron.backend.BackendOuterClass.ContractType.ACCOUNT_CREATE_CONTRACT,
        tx.getContractType());
  }

  /**
   * Verify fromAddress in the request is the owner address.
   */
  @Test
  public void testFromAddressIsOwnerAddress() throws Exception {
    TransactionCapsule trxCap = createAccountCreateTransaction(
        OWNER_ADDRESS, TARGET_ADDRESS, AccountType.Normal);
    TransactionContext ctx = buildContext(trxCap);

    ExecuteTransactionRequest request = remoteSPI.buildExecuteTransactionRequest(ctx);
    TronTransaction tx = request.getTransaction();

    Assert.assertArrayEquals(
        "from must be the owner address",
        OWNER_ADDRESS,
        tx.getFrom().toByteArray());
  }

  /**
   * Verify toAddress in the request is empty (system contract, no recipient).
   */
  @Test
  public void testToAddressIsEmpty() throws Exception {
    TransactionCapsule trxCap = createAccountCreateTransaction(
        OWNER_ADDRESS, TARGET_ADDRESS, AccountType.Normal);
    TransactionContext ctx = buildContext(trxCap);

    ExecuteTransactionRequest request = remoteSPI.buildExecuteTransactionRequest(ctx);
    TronTransaction tx = request.getTransaction();

    Assert.assertEquals(
        "toAddress must be empty for AccountCreateContract",
        0,
        tx.getTo().toByteArray().length);
  }

  /**
   * Verify data contains the full serialized AccountCreateContract proto bytes,
   * and the bytes round-trip correctly.
   */
  @Test
  public void testDataContainsSerializedContract() throws Exception {
    TransactionCapsule trxCap = createAccountCreateTransaction(
        OWNER_ADDRESS, TARGET_ADDRESS, AccountType.Normal);
    TransactionContext ctx = buildContext(trxCap);

    ExecuteTransactionRequest request = remoteSPI.buildExecuteTransactionRequest(ctx);
    TronTransaction tx = request.getTransaction();

    byte[] dataPayload = tx.getData().toByteArray();
    Assert.assertTrue("Data payload must not be empty", dataPayload.length > 0);

    // Byte-for-byte: data must be the exact serialized AccountCreateContract
    AccountCreateContract expectedContract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(OWNER_ADDRESS))
        .setAccountAddress(ByteString.copyFrom(TARGET_ADDRESS))
        .setType(AccountType.Normal)
        .build();
    Assert.assertArrayEquals(
        "data must be exact serialized AccountCreateContract bytes",
        expectedContract.toByteArray(),
        dataPayload);

    // Also verify round-trip deserialization
    AccountCreateContract deserialized = AccountCreateContract.parseFrom(dataPayload);
    Assert.assertArrayEquals(
        "Deserialized owner must match",
        OWNER_ADDRESS,
        deserialized.getOwnerAddress().toByteArray());
    Assert.assertArrayEquals(
        "Deserialized target must match",
        TARGET_ADDRESS,
        deserialized.getAccountAddress().toByteArray());
    Assert.assertEquals(
        "Deserialized type must match",
        AccountType.Normal,
        deserialized.getType());
  }

  /**
   * Verify the contractParameter (raw Any) is preserved in the request
   * so Rust can use any.is/any.unpack for compatibility.
   */
  @Test
  public void testContractParameterPreserved() throws Exception {
    TransactionCapsule trxCap = createAccountCreateTransaction(
        OWNER_ADDRESS, TARGET_ADDRESS, AccountType.Normal);
    TransactionContext ctx = buildContext(trxCap);

    ExecuteTransactionRequest request = remoteSPI.buildExecuteTransactionRequest(ctx);
    TronTransaction tx = request.getTransaction();

    // contractParameter should be the original Any
    Assert.assertTrue(
        "contractParameter must be set",
        tx.hasContractParameter());
    AccountCreateContract unpacked =
        tx.getContractParameter().unpack(AccountCreateContract.class);
    Assert.assertArrayEquals(
        "Unpacked owner from contractParameter must match",
        OWNER_ADDRESS,
        unpacked.getOwnerAddress().toByteArray());
  }

  /**
   * End-to-end: verify the complete request structure for AccountCreateContract.
   * This catches regressions in any field mapping in RemoteExecutionSPI.
   */
  @Test
  public void testAccountCreateFullRequestStructure() throws Exception {
    TransactionCapsule trxCap = createAccountCreateTransaction(
        OWNER_ADDRESS, TARGET_ADDRESS, AccountType.Normal);
    TransactionContext ctx = buildContext(trxCap);

    ExecuteTransactionRequest request = remoteSPI.buildExecuteTransactionRequest(ctx);

    // Transaction-level assertions
    TronTransaction tx = request.getTransaction();
    Assert.assertEquals(TxKind.NON_VM, tx.getTxKind());
    Assert.assertEquals(
        tron.backend.BackendOuterClass.ContractType.ACCOUNT_CREATE_CONTRACT,
        tx.getContractType());
    Assert.assertArrayEquals(OWNER_ADDRESS, tx.getFrom().toByteArray());
    Assert.assertEquals(0, tx.getTo().toByteArray().length);
    Assert.assertTrue(tx.getData().toByteArray().length > 0);

    // Context-level assertions
    ExecutionContext execCtx = request.getContext();
    Assert.assertEquals(100L, execCtx.getBlockNumber());
    Assert.assertTrue(execCtx.getBlockTimestamp() > 0);
  }
}
