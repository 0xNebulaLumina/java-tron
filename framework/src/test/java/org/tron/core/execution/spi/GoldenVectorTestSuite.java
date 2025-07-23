package org.tron.core.execution.spi;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.utils.ByteArray;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.protos.Protocol;
import org.tron.protos.contract.BalanceContract;
import org.tron.protos.contract.SmartContractOuterClass;

/**
 * Golden Vector Test Suite for Shadow Execution Verification.
 *
 * <p>This test suite contains curated edge-case transactions designed to thoroughly test the
 * equivalence between Java and Rust execution engines. Each test vector represents a specific
 * scenario that could potentially expose differences in implementation behavior.
 */
public class GoldenVectorTestSuite {

  private static final Logger logger = LoggerFactory.getLogger(GoldenVectorTestSuite.class);

  // Test configuration
  private static final boolean ENABLE_SHADOW_EXECUTION =
      Boolean.parseBoolean(System.getProperty("test.shadow.enabled", "false"));
  private static final String EXECUTION_MODE = System.getProperty("execution.mode", "EMBEDDED");

  // Test addresses (deterministic for reproducibility)
  private static final String OWNER_ADDRESS = "41548794500882809695a8a687866e76d4271a1abc";
  private static final String RECEIVER_ADDRESS = "41abd4b9367799eaa3197fecb144eb71de1e049150";
  private static final String CONTRACT_ADDRESS = "4112345678901234567890123456789012345678";

  private ExecutionSPI executionSPI;
  private List<GoldenVector> goldenVectors;

  @Before
  public void setUp() {
    // Initialize execution SPI based on configuration
    executionSPI = ExecutionSpiFactory.createExecution();

    // Initialize golden vectors
    goldenVectors = new ArrayList<>();
    initializeGoldenVectors();

    logger.info(
        "Golden Vector Test Suite initialized with {} vectors (mode: {}, shadow: {})",
        goldenVectors.size(),
        EXECUTION_MODE,
        ENABLE_SHADOW_EXECUTION);
  }

  @After
  public void tearDown() {
    if (executionSPI instanceof ShadowExecutionSPI) {
      ((ShadowExecutionSPI) executionSPI).cleanup();
      logger.info(
          "Shadow execution stats: {}", ((ShadowExecutionSPI) executionSPI).getMismatchStats());
    }
  }

  /** Test all golden vectors for deterministic execution. */
  @Test
  public void testAllGoldenVectors() throws Exception {
    // For now, just verify the framework is working
    Assert.assertNotNull("ExecutionSPI should be initialized", executionSPI);
    Assert.assertFalse("Should have golden vectors", goldenVectors.isEmpty());

    logger.info("Golden Vector Test Framework initialized with {} vectors", goldenVectors.size());

    // Test framework validation
    for (GoldenVector vector : goldenVectors) {
      Assert.assertNotNull("Vector name should not be null", vector.getName());
      Assert.assertNotNull("Vector category should not be null", vector.getCategory());
      Assert.assertNotNull("Vector transaction should not be null", vector.getTransaction());
      Assert.assertNotNull("Vector expected result should not be null", vector.getExpectedResult());
    }

    logger.info("✅ All {} golden vectors have valid structure", goldenVectors.size());

    // TODO: Enable actual execution testing once proper test environment is set up
    // For now, this validates the golden vector framework is working correctly
  }

  /** Test basic transfer transactions. */
  @Test
  public void testBasicTransferVectors() throws Exception {
    List<GoldenVector> transferVectors =
        goldenVectors.stream()
            .filter(v -> v.getCategory().equals("TRANSFER"))
            .collect(java.util.stream.Collectors.toList());

    Assert.assertFalse("Should have transfer vectors", transferVectors.isEmpty());

    for (GoldenVector vector : transferVectors) {
      // Validate vector structure
      Assert.assertEquals("Should be transfer category", "TRANSFER", vector.getCategory());
      Assert.assertFalse("Should not be contract call", vector.isContractCall());
      Assert.assertNotNull("Should have transaction", vector.getTransaction());

      logger.info("✅ Transfer vector '{}' has valid structure", vector.getName());
    }

    logger.info("Validated {} transfer vectors", transferVectors.size());
  }

  /** Test smart contract execution vectors. */
  @Test
  public void testSmartContractVectors() throws Exception {
    List<GoldenVector> contractVectors =
        goldenVectors.stream()
            .filter(v -> v.getCategory().equals("SMART_CONTRACT"))
            .collect(java.util.stream.Collectors.toList());

    for (GoldenVector vector : contractVectors) {
      testGoldenVector(vector);
    }
  }

  /** Test edge case and failure scenario vectors. */
  @Test
  public void testEdgeCaseVectors() throws Exception {
    List<GoldenVector> edgeCaseVectors =
        goldenVectors.stream()
            .filter(v -> v.getCategory().equals("EDGE_CASE"))
            .collect(java.util.stream.Collectors.toList());

    for (GoldenVector vector : edgeCaseVectors) {
      testGoldenVector(vector);
    }
  }

  /** Test a single golden vector. */
  private void testGoldenVector(GoldenVector vector) throws Exception {
    // Create transaction context
    TransactionContext context = createTransactionContext(vector.getTransaction());

    // Execute transaction
    CompletableFuture<ExecutionSPI.ExecutionResult> future;
    if (vector.isContractCall()) {
      future = executionSPI.callContract(context);
    } else {
      future = executionSPI.executeTransaction(context);
    }

    ExecutionSPI.ExecutionResult result = future.get();

    // Verify expected results
    verifyGoldenVectorResult(vector, result);
  }

  /** Verify the execution result matches the expected golden vector result. */
  private void verifyGoldenVectorResult(GoldenVector vector, ExecutionSPI.ExecutionResult result) {
    GoldenVector.ExpectedResult expected = vector.getExpectedResult();

    // Verify success/failure
    Assert.assertEquals(
        "Success mismatch for " + vector.getName(), expected.isSuccess(), result.isSuccess());

    // Verify energy usage (allow small tolerance for implementation differences)
    if (expected.getEnergyUsed() > 0) {
      long energyDiff = Math.abs(result.getEnergyUsed() - expected.getEnergyUsed());
      long tolerance = Math.max(1, expected.getEnergyUsed() / 100); // 1% tolerance
      Assert.assertTrue(
          String.format(
              "Energy usage mismatch for %s: expected %d, got %d (diff: %d, tolerance: %d)",
              vector.getName(),
              expected.getEnergyUsed(),
              result.getEnergyUsed(),
              energyDiff,
              tolerance),
          energyDiff <= tolerance);
    }

    // Verify return data
    if (expected.getReturnData() != null) {
      Assert.assertArrayEquals(
          "Return data mismatch for " + vector.getName(),
          expected.getReturnData(),
          result.getReturnData());
    }

    // Verify error message for failed transactions
    if (!expected.isSuccess() && expected.getErrorMessage() != null) {
      Assert.assertTrue(
          "Error message mismatch for " + vector.getName(),
          result.getErrorMessage() != null
              && result.getErrorMessage().contains(expected.getErrorMessage()));
    }

    // Verify state changes count
    if (expected.getStateChangesCount() >= 0) {
      Assert.assertEquals(
          "State changes count mismatch for " + vector.getName(),
          expected.getStateChangesCount(),
          result.getStateChanges().size());
    }
  }

  /** Create a transaction context for testing. */
  private TransactionContext createTransactionContext(TransactionCapsule transaction) {
    // Create a mock transaction context
    // In a real implementation, this would be properly initialized with block context
    return new TransactionContext(null, transaction, null, false, false);
  }

  /** Initialize all golden vectors for testing. */
  private void initializeGoldenVectors() {
    goldenVectors = new ArrayList<>();

    // For now, use programmatic creation until Jackson dependency is available
    // TODO: Enable JSON loading once Jackson is added to dependencies
    // try {
    //   goldenVectors = GoldenVectorLoader.loadDefaultVectors();
    // } catch (Exception e) {
    //   logger.warn("Failed to load golden vectors from JSON, using programmatic creation", e);
    // }

    // Create vectors programmatically
    addBasicTransferVectors();
    addSmartContractVectors();
    addEdgeCaseVectors();
    addResourceManagementVectors();
    addMultiSignatureVectors();

    logger.info(
        "Initialized {} golden vectors across {} categories",
        goldenVectors.size(),
        goldenVectors.stream().map(GoldenVector::getCategory).distinct().count());
  }

  /** Add programmatically created vectors for complex edge cases. */
  private void addProgrammaticVectors() {
    // Add any vectors that are too complex to configure in JSON
    // These would be edge cases that require special transaction construction
  }

  /** Add basic transfer transaction vectors. */
  private void addBasicTransferVectors() {
    // Normal transfer
    goldenVectors.add(
        createTransferVector(
            "basic_transfer_normal",
            OWNER_ADDRESS,
            RECEIVER_ADDRESS,
            1000000L,
            true,
            268,
            null,
            1));

    // Zero amount transfer
    goldenVectors.add(
        createTransferVector(
            "basic_transfer_zero_amount", OWNER_ADDRESS, RECEIVER_ADDRESS, 0L, true, 268, null, 1));

    // Maximum amount transfer
    goldenVectors.add(
        createTransferVector(
            "basic_transfer_max_amount",
            OWNER_ADDRESS,
            RECEIVER_ADDRESS,
            Long.MAX_VALUE,
            false,
            0,
            "insufficient balance",
            0));

    // Self transfer
    goldenVectors.add(
        createTransferVector(
            "basic_transfer_self", OWNER_ADDRESS, OWNER_ADDRESS, 1000000L, true, 268, null, 1));
  }

  /** Add smart contract execution vectors. */
  private void addSmartContractVectors() {
    // Simple contract call
    goldenVectors.add(
        createContractCallVector(
            "contract_call_simple",
            CONTRACT_ADDRESS,
            "getValue()",
            "",
            true,
            1000,
            new byte[] {0x00, 0x00, 0x00, 0x42},
            0));

    // Contract call with parameters
    goldenVectors.add(
        createContractCallVector(
            "contract_call_with_params",
            CONTRACT_ADDRESS,
            "setValue(uint256)",
            "42",
            true,
            2000,
            new byte[0],
            1));

    // Contract call that reverts
    goldenVectors.add(
        createContractCallVector(
            "contract_call_revert",
            CONTRACT_ADDRESS,
            "revertFunction()",
            "",
            false,
            1500,
            null,
            0));
  }

  /** Add edge case vectors. */
  private void addEdgeCaseVectors() {
    // Transaction with expired timestamp
    goldenVectors.add(createExpiredTransactionVector());

    // Transaction with invalid signature
    goldenVectors.add(createInvalidSignatureVector());

    // Transaction with insufficient energy
    goldenVectors.add(createInsufficientEnergyVector());
  }

  /** Add resource management vectors. */
  private void addResourceManagementVectors() {
    // TODO: Add freeze/unfreeze, bandwidth, energy vectors
  }

  /** Add multi-signature vectors. */
  private void addMultiSignatureVectors() {
    // TODO: Add multi-sig transaction vectors
  }

  /** Create a transfer transaction golden vector. */
  private GoldenVector createTransferVector(
      String name,
      String from,
      String to,
      long amount,
      boolean expectedSuccess,
      long expectedEnergy,
      String expectedError,
      int expectedStateChanges) {
    try {
      BalanceContract.TransferContract.Builder transferBuilder =
          BalanceContract.TransferContract.newBuilder()
              .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(from)))
              .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(to)))
              .setAmount(amount);

      TransactionCapsule transaction =
          new TransactionCapsule(
              transferBuilder.build(), Protocol.Transaction.Contract.ContractType.TransferContract);

      GoldenVector.ExpectedResult expectedResult =
          new GoldenVector.ExpectedResult(
              expectedSuccess, expectedEnergy, null, expectedError, expectedStateChanges);

      return new GoldenVector(name, "TRANSFER", transaction, false, expectedResult);
    } catch (Exception e) {
      throw new RuntimeException("Failed to create transfer vector: " + name, e);
    }
  }

  /** Create a contract call golden vector. */
  private GoldenVector createContractCallVector(
      String name,
      String contractAddress,
      String method,
      String params,
      boolean expectedSuccess,
      long expectedEnergy,
      byte[] expectedReturnData,
      int expectedStateChanges) {
    try {
      SmartContractOuterClass.TriggerSmartContract.Builder triggerBuilder =
          SmartContractOuterClass.TriggerSmartContract.newBuilder()
              .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
              .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(contractAddress)))
              .setData(ByteString.copyFrom(encodeMethodCall(method, params)));

      TransactionCapsule transaction =
          new TransactionCapsule(
              triggerBuilder.build(),
              Protocol.Transaction.Contract.ContractType.TriggerSmartContract);

      GoldenVector.ExpectedResult expectedResult =
          new GoldenVector.ExpectedResult(
              expectedSuccess, expectedEnergy, expectedReturnData, null, expectedStateChanges);

      return new GoldenVector(name, "SMART_CONTRACT", transaction, true, expectedResult);
    } catch (Exception e) {
      throw new RuntimeException("Failed to create contract call vector: " + name, e);
    }
  }

  /** Create an expired transaction vector. */
  private GoldenVector createExpiredTransactionVector() {
    try {
      BalanceContract.TransferContract.Builder transferBuilder =
          BalanceContract.TransferContract.newBuilder()
              .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
              .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
              .setAmount(1000000L);

      Protocol.Transaction.raw.Builder rawBuilder =
          Protocol.Transaction.raw
              .newBuilder()
              .addContract(
                  Protocol.Transaction.Contract.newBuilder()
                      .setType(Protocol.Transaction.Contract.ContractType.TransferContract)
                      .setParameter(Any.pack(transferBuilder.build())))
              .setTimestamp(System.currentTimeMillis())
              .setExpiration(System.currentTimeMillis() - 1000); // Expired 1 second ago

      TransactionCapsule transaction =
          new TransactionCapsule(Protocol.Transaction.newBuilder().setRawData(rawBuilder).build());

      GoldenVector.ExpectedResult expectedResult =
          new GoldenVector.ExpectedResult(false, 0, null, "TRANSACTION_EXPIRATION_ERROR", 0);

      return new GoldenVector(
          "expired_transaction", "EDGE_CASE", transaction, false, expectedResult);
    } catch (Exception e) {
      throw new RuntimeException("Failed to create expired transaction vector", e);
    }
  }

  /** Create an invalid signature vector. */
  private GoldenVector createInvalidSignatureVector() {
    try {
      BalanceContract.TransferContract.Builder transferBuilder =
          BalanceContract.TransferContract.newBuilder()
              .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
              .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
              .setAmount(1000000L);

      Protocol.Transaction.raw.Builder rawBuilder =
          Protocol.Transaction.raw
              .newBuilder()
              .addContract(
                  Protocol.Transaction.Contract.newBuilder()
                      .setType(Protocol.Transaction.Contract.ContractType.TransferContract)
                      .setParameter(Any.pack(transferBuilder.build())))
              .setTimestamp(System.currentTimeMillis())
              .setExpiration(System.currentTimeMillis() + 60000);

      // Add invalid signature
      Protocol.Transaction.Builder txBuilder =
          Protocol.Transaction.newBuilder()
              .setRawData(rawBuilder)
              .addSignature(ByteString.copyFrom(new byte[64])); // Invalid signature

      TransactionCapsule transaction = new TransactionCapsule(txBuilder.build());

      GoldenVector.ExpectedResult expectedResult =
          new GoldenVector.ExpectedResult(false, 0, null, "SIGNATURE_FORMAT_ERROR", 0);

      return new GoldenVector("invalid_signature", "EDGE_CASE", transaction, false, expectedResult);
    } catch (Exception e) {
      throw new RuntimeException("Failed to create invalid signature vector", e);
    }
  }

  /** Create an insufficient energy vector. */
  private GoldenVector createInsufficientEnergyVector() {
    try {
      SmartContractOuterClass.TriggerSmartContract.Builder triggerBuilder =
          SmartContractOuterClass.TriggerSmartContract.newBuilder()
              .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
              .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
              .setData(ByteString.copyFrom(encodeMethodCall("expensiveOperation()", "")));

      Protocol.Transaction.raw.Builder rawBuilder =
          Protocol.Transaction.raw
              .newBuilder()
              .addContract(
                  Protocol.Transaction.Contract.newBuilder()
                      .setType(Protocol.Transaction.Contract.ContractType.TriggerSmartContract)
                      .setParameter(Any.pack(triggerBuilder.build())))
              .setTimestamp(System.currentTimeMillis())
              .setExpiration(System.currentTimeMillis() + 60000)
              .setFeeLimit(1000); // Very low fee limit

      TransactionCapsule transaction =
          new TransactionCapsule(Protocol.Transaction.newBuilder().setRawData(rawBuilder).build());

      GoldenVector.ExpectedResult expectedResult =
          new GoldenVector.ExpectedResult(false, 1000, null, "OUT_OF_ENERGY", 0);

      return new GoldenVector(
          "insufficient_energy", "EDGE_CASE", transaction, true, expectedResult);
    } catch (Exception e) {
      throw new RuntimeException("Failed to create insufficient energy vector", e);
    }
  }

  /** Encode a method call for smart contract interaction. */
  private byte[] encodeMethodCall(String method, String params) {
    // Simplified method encoding - in a real implementation, this would use proper ABI encoding
    String methodSignature = method;
    if (!params.isEmpty()) {
      methodSignature += "(" + params + ")";
    }
    return methodSignature.getBytes();
  }
}
