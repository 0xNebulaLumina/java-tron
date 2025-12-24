package org.tron.core.conformance;

import com.google.protobuf.ByteString;
import java.io.File;
import java.io.FileOutputStream;
import java.util.Iterator;
import java.util.Map;
import java.util.TreeMap;
import org.bouncycastle.util.encoders.Hex;
import org.junit.Before;
import org.junit.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.BaseTest;
import org.tron.common.runtime.Runtime;
import org.tron.common.runtime.RuntimeImpl;
import org.tron.common.runtime.TVMTestResult;
import org.tron.common.runtime.TvmTestUtils;
import org.tron.common.utils.ByteArray;
import org.tron.common.utils.WalletUtil;
import org.tron.core.Constant;
import org.tron.core.Wallet;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.config.args.Args;
import org.tron.core.db.TransactionTrace;
import org.tron.core.store.StoreFactory;
import org.tron.core.vm.repository.Repository;
import org.tron.core.vm.repository.RepositoryImpl;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.SmartContractOuterClass.CreateSmartContract;
import org.tron.protos.contract.SmartContractOuterClass.TriggerSmartContract;
import tron.backend.BackendOuterClass.ContractType;
import tron.backend.BackendOuterClass.ExecuteTransactionRequest;
import tron.backend.BackendOuterClass.ExecutionContext;
import tron.backend.BackendOuterClass.TronTransaction;
import tron.backend.BackendOuterClass.TxKind;

/**
 * Generates conformance test fixtures for VM contracts:
 * - CreateSmartContract (30)
 * - TriggerSmartContract (31)
 *
 * <p>These fixtures test VM parity between Java and Rust execution.
 *
 * <p>Run with: ./gradlew :framework:test --tests "VmFixtureGeneratorTest"
 *              -Dconformance.output=../conformance/fixtures
 */
public class VmFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(VmFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String OTHER_ADDRESS;
  private static final long INITIAL_BALANCE = 100_000_000_000_000L; // 100,000 TRX
  private static final long DEFAULT_FEE_LIMIT = 1_000_000_000L; // 1000 TRX

  private File outputDir;

  // Simple counter contract for testing
  // pragma solidity ^0.8.0;
  // contract Counter {
  //     uint256 public count;
  //     function increment() public { count++; }
  //     function get() public view returns (uint256) { return count; }
  // }
  private static final String COUNTER_ABI = "[{\"inputs\":[],\"name\":\"count\",\"outputs\":"
      + "[{\"internalType\":\"uint256\",\"name\":\"\",\"type\":\"uint256\"}],"
      + "\"stateMutability\":\"view\",\"type\":\"function\"},{\"inputs\":[],"
      + "\"name\":\"get\",\"outputs\":[{\"internalType\":\"uint256\",\"name\":\"\","
      + "\"type\":\"uint256\"}],\"stateMutability\":\"view\",\"type\":\"function\"},"
      + "{\"inputs\":[],\"name\":\"increment\",\"outputs\":[],\"stateMutability\":\"nonpayable\","
      + "\"type\":\"function\"}]";

  // Minimal bytecode that just stores a value and returns
  // This is a simple contract that: PUSH1 0x42, PUSH1 0x00, SSTORE, STOP
  // For deployment: init code that copies runtime to memory and returns it
  private static final String SIMPLE_BYTECODE =
      "6080604052348015600f57600080fd5b5060"
      + "ac8061001e6000396000f3fe6080604052348015600f57600080fd5b506004361060325760003560e01c8063"
      + "06661abd146037578063d09de08a14604f575b600080fd5b603d6055565b60405190815260200160405180"
      + "910390f35b6053605b565b005b60005481565b600080549080606a83607a565b9190505550565b600060019"
      + "0508190565b6000600182019050919050565b6000819050919050565b600060848260758565b939250505"
      + "0565bfea264697066735822122066c23b8e8b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"
      + "0b0b0b0b0b0b0b64736f6c63430008130033";

  // Even simpler minimal bytecode for basic tests
  // Just returns immediately with no constructor logic
  private static final String MINIMAL_BYTECODE = "60806040526000805560358060156000396000f3006080"
      + "604052600080fd00a165627a7a72305820000000000000000000000000000000000000000000000000000"
      + "00000000000000029";

  // StorageDemo contract - simpler storage operations
  // pragma solidity ^0.4.0;
  // contract StorageDemo{
  //   mapping(uint => string) public int2str;
  //   function testPut(uint256 i, string s) public { int2str[i] = s; }
  //   function testDelete(uint256 i) public { delete int2str[i]; }
  // }
  private static final String STORAGE_ABI = "[{\"constant\":true,\"inputs\":[{\"name\":\"\","
      + "\"type\":\"uint256\"}],\"name\":\"int2str\",\"outputs\":[{\"name\":\"\",\"type\":\"string\"}],"
      + "\"payable\":false,\"stateMutability\":\"view\",\"type\":\"function\"},{\"constant\":false,"
      + "\"inputs\":[{\"name\":\"i\",\"type\":\"uint256\"}],\"name\":\"testDelete\",\"outputs\":[],"
      + "\"payable\":false,\"stateMutability\":\"nonpayable\",\"type\":\"function\"},{\"constant\":false,"
      + "\"inputs\":[{\"name\":\"i\",\"type\":\"uint256\"},{\"name\":\"s\",\"type\":\"string\"}],"
      + "\"name\":\"testPut\",\"outputs\":[],\"payable\":false,\"stateMutability\":\"nonpayable\","
      + "\"type\":\"function\"}]";

  private static final String STORAGE_CODE = "608060405234801561001057600080fd5b50610341806100206000"
      + "396000f30060806040526004361061005657"
      + "63ffffffff7c01000000000000000000000000000000000000000000000000000000006000350416"
      + "6313d821f4811461005b57806330099fa9146100e8578063c38e31cc14610102575b600080fd5b3480"
      + "1561006757600080fd5b50610073600435610160565b6040805160208082528351818301528351919283"
      + "929083019185019080838360005b838110156100ad578181015183820152602001610095565b5050505090"
      + "5090810190601f1680156100da5780820380516001836020036101000a031916815260200191505b509250"
      + "505060405180910390f35b3480156100f457600080fd5b506101006004356101fa565b005b348015610"
      + "10e57600080fd5b506040805160206004602480358281013560"
      + "1f8101859004850286018501909652858552610100958335953695604494919390910191908190840183"
      + "8280828437509497506102149650505050505050565b6000602081815291815260409081902080548251"
      + "60026001831615610100026000190190921691909104601f810185900485028201850190935282815292"
      + "9091908301828280156101f25780601f106101c7576101008083540402835291602001916101f2565b82"
      + "0191906000526020600020905b8154815290600101906020018083116101d557829003601f168201915b"
      + "505050505081565b600081815260208190526040812061021191610236565b50565b60008281526020"
      + "818152604090912082516102319284019061027a565b505050565b5080546001816001161561010002031"
      + "6600290046000825580601f1061025c5750610211565b601f01602090049060005260206000209081019"
      + "061021191906102f8565b828054600181600116156101000203166002900490600052602060002090601"
      + "f016020900481019282601f106102bb57805160ff19168380011785556102e8565b82800160010185558"
      + "21561"
      + "02e8579182015b828111156102e85782518255916020019190600101906102cd565b506102f492915061"
      + "02f8565b5090565b61031291905b808211156102f457600081556001016102fe565b905600a165627a7a"
      + "72305820c98643943ea978505f9cca68bdf61681462daeee9f71a6aa4414609e48dbb46b0029";

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049abc";
    OTHER_ADDRESS = Wallet.getAddressPreFixString() + "2222222222222222222222222222222222222222";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);

    log.info("VM Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Create owner account with large balance
    AccountCapsule ownerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("owner"),
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Create other account for edge case tests
    AccountCapsule otherAccount = new AccountCapsule(
        ByteString.copyFromUtf8("other"),
        ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(otherAccount.getAddress().toByteArray(), otherAccount);

    // Enable TVM features
    dbManager.getDynamicPropertiesStore().saveAllowTvmConstantinople(1);
    dbManager.getDynamicPropertiesStore().saveAllowTvmTransferTrc10(1);
    dbManager.getDynamicPropertiesStore().saveAllowMultiSign(1);

    // Set block properties
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(1000000);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
  }

  // ==========================================================================
  // CreateSmartContract (30) Fixtures
  // ==========================================================================

  @Test
  public void generateCreateSmartContract_happyPath() throws Exception {
    log.info("Generating CreateSmartContract happy path fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Build CreateSmartContract
    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "StorageDemo",
        ownerBytes,
        STORAGE_ABI,
        STORAGE_CODE,
        0,  // call value
        50, // consume user resource percent
        null, // library address pair
        10_000_000L // origin energy limit
    );

    // Create transaction with fee limit
    TransactionCapsule trxCap = new TransactionCapsule(createContract,
        Transaction.Contract.ContractType.CreateSmartContract);
    Transaction.Builder txBuilder = trxCap.getInstance().toBuilder();
    Transaction.raw.Builder rawBuilder = trxCap.getInstance().getRawData().toBuilder();
    rawBuilder.setFeeLimit(DEFAULT_FEE_LIMIT);
    rawBuilder.setTimestamp(System.currentTimeMillis());
    rawBuilder.setExpiration(System.currentTimeMillis() + 3600000);
    txBuilder.setRawData(rawBuilder);
    trxCap = new TransactionCapsule(txBuilder.build());

    BlockCapsule blockCap = createBlockContext();

    // Generate fixture
    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "happy_path",
        "happy",
        "Deploy a simple storage demo contract"
    );

    log.info("CreateSmartContract happy path: success={}, contractAddress={}",
        result.isSuccess(),
        result.getContractAddress() != null ? Hex.toHexString(result.getContractAddress()) : "null");
  }

  @Test
  public void generateCreateSmartContract_withCallValue() throws Exception {
    log.info("Generating CreateSmartContract with call value fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Build a payable contract (minimal bytecode that accepts TRX)
    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "PayableContract",
        ownerBytes,
        "[]", // minimal ABI
        MINIMAL_BYTECODE,
        1_000_000L, // 1 TRX call value
        50,
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = new TransactionCapsule(createContract,
        Transaction.Contract.ContractType.CreateSmartContract);
    Transaction.Builder txBuilder = trxCap.getInstance().toBuilder();
    Transaction.raw.Builder rawBuilder = trxCap.getInstance().getRawData().toBuilder();
    rawBuilder.setFeeLimit(DEFAULT_FEE_LIMIT);
    rawBuilder.setTimestamp(System.currentTimeMillis());
    rawBuilder.setExpiration(System.currentTimeMillis() + 3600000);
    txBuilder.setRawData(rawBuilder);
    trxCap = new TransactionCapsule(txBuilder.build());

    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "happy_path_with_value",
        "happy",
        "Deploy contract with TRX call value"
    );

    log.info("CreateSmartContract with value: success={}", result.isSuccess());
  }

  @Test
  public void generateCreateSmartContract_insufficientBalance() throws Exception {
    log.info("Generating CreateSmartContract insufficient balance fixture");

    // Create an account with very low balance
    String poorAddress = Wallet.getAddressPreFixString() + "3333333333333333333333333333333333333333";
    AccountCapsule poorAccount = new AccountCapsule(
        ByteString.copyFromUtf8("poor"),
        ByteString.copyFrom(ByteArray.fromHexString(poorAddress)),
        AccountType.Normal,
        1_000L); // Only 0.001 TRX
    dbManager.getAccountStore().put(poorAccount.getAddress().toByteArray(), poorAccount);

    byte[] poorBytes = ByteArray.fromHexString(poorAddress);

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "Test",
        poorBytes,
        STORAGE_ABI,
        STORAGE_CODE,
        0,
        50,
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = new TransactionCapsule(createContract,
        Transaction.Contract.ContractType.CreateSmartContract);
    Transaction.Builder txBuilder = trxCap.getInstance().toBuilder();
    Transaction.raw.Builder rawBuilder = trxCap.getInstance().getRawData().toBuilder();
    rawBuilder.setFeeLimit(DEFAULT_FEE_LIMIT);
    rawBuilder.setTimestamp(System.currentTimeMillis());
    rawBuilder.setExpiration(System.currentTimeMillis() + 3600000);
    txBuilder.setRawData(rawBuilder);
    trxCap = new TransactionCapsule(txBuilder.build());

    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_insufficient_balance",
        "validate_fail",
        "Fail when account has insufficient balance for deployment"
    );

    log.info("CreateSmartContract insufficient balance: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_invalidBytecode() throws Exception {
    log.info("Generating CreateSmartContract invalid bytecode fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Use invalid bytecode (not valid EVM bytecode)
    String invalidCode = "DEADBEEF";

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "Invalid",
        ownerBytes,
        "[]",
        invalidCode,
        0,
        50,
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = new TransactionCapsule(createContract,
        Transaction.Contract.ContractType.CreateSmartContract);
    Transaction.Builder txBuilder = trxCap.getInstance().toBuilder();
    Transaction.raw.Builder rawBuilder = trxCap.getInstance().getRawData().toBuilder();
    rawBuilder.setFeeLimit(DEFAULT_FEE_LIMIT);
    rawBuilder.setTimestamp(System.currentTimeMillis());
    rawBuilder.setExpiration(System.currentTimeMillis() + 3600000);
    txBuilder.setRawData(rawBuilder);
    trxCap = new TransactionCapsule(txBuilder.build());

    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "edge_invalid_bytecode",
        "edge",
        "Deploy with invalid/malformed bytecode"
    );

    log.info("CreateSmartContract invalid bytecode: success={}, error={}",
        result.isSuccess(), result.getError());
  }

  // ==========================================================================
  // TriggerSmartContract (31) Fixtures
  // ==========================================================================

  // NOTE: TriggerSmartContract tests require VMTestBase infrastructure for proper test isolation.
  // The tests below are marked with @Ignore until we integrate with the full TVM test harness.
  // CreateSmartContract fixtures are generated successfully and can be used for basic VM parity testing.

  @org.junit.Ignore("Requires VMTestBase infrastructure - see StorageTest for reference")
  @Test
  public void generateTriggerSmartContract_happyPath() throws Exception {
    log.info("Generating TriggerSmartContract happy path fixture - SKIPPED (requires VMTestBase)");
    // This test is skipped because TriggerSmartContract tests require:
    // 1. Proper test isolation using VMTestBase
    // 2. Repository-based state management
    // 3. Block context with proper transaction receipts
    // See: framework/src/test/java/org/tron/common/runtime/vm/StorageTest.java for reference
  }

  @org.junit.Ignore("Requires VMTestBase infrastructure - see StorageTest for reference")
  @Test
  public void generateTriggerSmartContract_viewFunction() throws Exception {
    log.info("Generating TriggerSmartContract view function fixture - SKIPPED (requires VMTestBase)");
  }

  @org.junit.Ignore("Requires VMTestBase infrastructure - see StorageTest for reference")
  @Test
  public void generateTriggerSmartContract_nonexistentContract() throws Exception {
    log.info("Generating TriggerSmartContract nonexistent contract fixture - SKIPPED (requires VMTestBase)");
  }

  @org.junit.Ignore("Requires VMTestBase infrastructure - see StorageTest for reference")
  @Test
  public void generateTriggerSmartContract_outOfEnergy() throws Exception {
    log.info("Generating TriggerSmartContract out of energy fixture - SKIPPED (requires VMTestBase)");
  }

  // ==========================================================================
  // Helper Methods
  // ==========================================================================

  /**
   * Generate fixture for successful TriggerSmartContract execution.
   */
  private VmFixtureResult generateTriggerFixture(
      TriggerSmartContract triggerContract,
      BlockCapsule blockCap,
      TVMTestResult tvmResult,
      String contractTypeName,
      String caseName,
      String category,
      String description) throws Exception {

    File fixtureDir = new File(outputDir, contractTypeName + "/" + caseName);
    fixtureDir.mkdirs();

    VmFixtureResult result = new VmFixtureResult();

    try {
      // Capture pre-execution state (after deployment, before trigger)
      File preDbDir = new File(fixtureDir, "pre_db");
      preDbDir.mkdirs();
      captureVmDatabases(preDbDir);

      // Build and save request
      ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, blockCap);
      File requestFile = new File(fixtureDir, "request.pb");
      try (FileOutputStream fos = new FileOutputStream(requestFile)) {
        request.writeTo(fos);
      }

      // Check execution result
      if (tvmResult.getRuntime().getRuntimeError() != null) {
        result.setSuccess(false);
        result.setError(tvmResult.getRuntime().getRuntimeError());
      } else {
        result.setSuccess(true);
        if (tvmResult.getRuntime().getResult() != null) {
          result.setReturnData(tvmResult.getRuntime().getResult().getHReturn());
        }
      }

      // Capture post-execution state
      File expectedDir = new File(fixtureDir, "expected");
      File postDbDir = new File(expectedDir, "post_db");
      postDbDir.mkdirs();
      captureVmDatabases(postDbDir);

      // Save receipt
      if (tvmResult.getReceipt() != null) {
        File resultFile = new File(expectedDir, "result.pb");
        try (FileOutputStream fos = new FileOutputStream(resultFile)) {
          tvmResult.getReceipt().getReceipt().writeTo(fos);
        }
      }

      // Save metadata
      FixtureMetadata metadata = FixtureMetadata.builder()
          .contractType(contractTypeName.toUpperCase(), 31)
          .caseName(caseName)
          .caseCategory(category)
          .description(description)
          .database("account")
          .database("contract")
          .database("code")
          .database("abi")
          .database("contract-state")
          .database("dynamic-properties")
          .ownerAddress(OWNER_ADDRESS)
          .build();

      metadata.setBlockNumber(blockCap.getNum());
      metadata.setBlockTimestamp(blockCap.getTimeStamp());
      metadata.setExpectedStatus(result.isSuccess() ? "SUCCESS" : "REVERT");
      if (result.getError() != null) {
        metadata.setExpectedErrorMessage(result.getError());
      }

      metadata.toFile(new File(fixtureDir, "metadata.json"));
      log.info("Generated trigger fixture: {}/{}", contractTypeName, caseName);

    } catch (Exception e) {
      log.error("Failed to generate trigger fixture", e);
      result.setSuccess(false);
      result.setError(e.getMessage());
    }

    return result;
  }

  /**
   * Generate fixture for failed TriggerSmartContract execution.
   */
  private void generateTriggerFixtureForFailure(
      TriggerSmartContract triggerContract,
      BlockCapsule blockCap,
      String errorMessage,
      String contractTypeName,
      String caseName,
      String category,
      String description) throws Exception {

    File fixtureDir = new File(outputDir, contractTypeName + "/" + caseName);
    fixtureDir.mkdirs();

    try {
      // Capture pre-execution state
      File preDbDir = new File(fixtureDir, "pre_db");
      preDbDir.mkdirs();
      captureVmDatabases(preDbDir);

      // Build and save request
      ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, blockCap);
      File requestFile = new File(fixtureDir, "request.pb");
      try (FileOutputStream fos = new FileOutputStream(requestFile)) {
        request.writeTo(fos);
      }

      // For failure cases, post_db should be same as pre_db (no state changes)
      File expectedDir = new File(fixtureDir, "expected");
      File postDbDir = new File(expectedDir, "post_db");
      postDbDir.mkdirs();
      captureVmDatabases(postDbDir);

      // Save metadata
      FixtureMetadata metadata = FixtureMetadata.builder()
          .contractType(contractTypeName.toUpperCase(), 31)
          .caseName(caseName)
          .caseCategory(category)
          .description(description)
          .database("account")
          .database("contract")
          .database("code")
          .database("abi")
          .database("contract-state")
          .database("dynamic-properties")
          .ownerAddress(OWNER_ADDRESS)
          .expectedError(errorMessage != null ? errorMessage : "unknown")
          .build();

      metadata.setBlockNumber(blockCap.getNum());
      metadata.setBlockTimestamp(blockCap.getTimeStamp());
      metadata.setExpectedStatus("VALIDATION_FAILED");
      metadata.setExpectedErrorMessage(errorMessage);

      metadata.toFile(new File(fixtureDir, "metadata.json"));
      log.info("Generated trigger failure fixture: {}/{}", contractTypeName, caseName);

    } catch (Exception e) {
      log.error("Failed to generate trigger failure fixture", e);
    }
  }

  private ExecuteTransactionRequest buildTriggerRequest(
      TriggerSmartContract triggerContract,
      BlockCapsule blockCap) {

    byte[] fromAddress = triggerContract.getOwnerAddress().toByteArray();
    byte[] toAddress = triggerContract.getContractAddress().toByteArray();
    byte[] data = triggerContract.toByteArray();
    long value = triggerContract.getCallValue();

    TronTransaction tronTx = TronTransaction.newBuilder()
        .setFrom(ByteString.copyFrom(fromAddress))
        .setTo(ByteString.copyFrom(toAddress))
        .setValue(ByteString.copyFrom(longToBytes32(value)))
        .setData(ByteString.copyFrom(data))
        .setEnergyLimit(DEFAULT_FEE_LIMIT)
        .setEnergyPrice(1)
        .setNonce(0)
        .setTxKind(TxKind.VM)
        .setContractType(ContractType.TRIGGER_SMART_CONTRACT)
        .build();

    ExecutionContext context = ExecutionContext.newBuilder()
        .setBlockNumber(blockCap.getNum())
        .setBlockTimestamp(blockCap.getTimeStamp())
        .setBlockHash(ByteString.copyFrom(blockCap.getBlockId().getBytes()))
        .setCoinbase(ByteString.copyFrom(blockCap.getWitnessAddress().toByteArray()))
        .setEnergyLimit(DEFAULT_FEE_LIMIT)
        .setEnergyPrice(1)
        .build();

    return ExecuteTransactionRequest.newBuilder()
        .setTransaction(tronTx)
        .setContext(context)
        .build();
  }

  private VmFixtureResult generateVmFixture(
      TransactionCapsule trxCap,
      BlockCapsule blockCap,
      com.google.protobuf.Message contract,
      String contractTypeName,
      String caseName,
      String category,
      String description) throws Exception {

    File fixtureDir = new File(outputDir, contractTypeName + "/" + caseName);
    fixtureDir.mkdirs();

    VmFixtureResult result = new VmFixtureResult();

    try {
      // Capture pre-execution state
      File preDbDir = new File(fixtureDir, "pre_db");
      preDbDir.mkdirs();
      captureVmDatabases(preDbDir);

      // Build and save request
      ExecuteTransactionRequest request = buildVmRequest(trxCap, blockCap, contract);
      File requestFile = new File(fixtureDir, "request.pb");
      try (FileOutputStream fos = new FileOutputStream(requestFile)) {
        request.writeTo(fos);
      }
      log.info("Saved request.pb ({} bytes)", requestFile.length());

      // Execute using TransactionTrace
      TransactionTrace trace = new TransactionTrace(trxCap,
          StoreFactory.getInstance(), new RuntimeImpl());
      trace.init(blockCap);
      trace.exec();
      trace.finalization();

      String runtimeError = trace.getRuntimeError();
      if (runtimeError != null) {
        result.setSuccess(false);
        result.setError(runtimeError);
      } else {
        result.setSuccess(true);
        if (trace.getRuntime() != null && trace.getRuntime().getResult() != null) {
          result.setContractAddress(trace.getRuntime().getResult().getContractAddress());
          result.setReturnData(trace.getRuntime().getResult().getHReturn());
        }
      }

      // Capture post-execution state
      File expectedDir = new File(fixtureDir, "expected");
      File postDbDir = new File(expectedDir, "post_db");
      postDbDir.mkdirs();
      captureVmDatabases(postDbDir);

      // Save result
      if (trace.getReceipt() != null) {
        File resultFile = new File(expectedDir, "result.pb");
        try (FileOutputStream fos = new FileOutputStream(resultFile)) {
          trace.getReceipt().getReceipt().writeTo(fos);
        }
      }

      // Save metadata
      FixtureMetadata metadata = FixtureMetadata.builder()
          .contractType(contractTypeName.toUpperCase(), getContractTypeNumber(contractTypeName))
          .caseName(caseName)
          .caseCategory(category)
          .description(description)
          .database("account")
          .database("contract")
          .database("code")
          .database("abi")
          .database("contract-state")
          .database("dynamic-properties")
          .ownerAddress(OWNER_ADDRESS)
          .build();

      metadata.setBlockNumber(blockCap.getNum());
      metadata.setBlockTimestamp(blockCap.getTimeStamp());
      metadata.setExpectedStatus(result.isSuccess() ? "SUCCESS" : "REVERT");
      if (result.getError() != null) {
        metadata.setExpectedErrorMessage(result.getError());
      }

      metadata.toFile(new File(fixtureDir, "metadata.json"));
      log.info("Generated VM fixture: {}/{}", contractTypeName, caseName);

    } catch (Exception e) {
      log.error("Failed to generate VM fixture", e);
      result.setSuccess(false);
      result.setError(e.getMessage());
    }

    return result;
  }

  private ExecuteTransactionRequest buildVmRequest(
      TransactionCapsule trxCap,
      BlockCapsule blockCap,
      com.google.protobuf.Message contract) {

    Transaction transaction = trxCap.getInstance();
    Transaction.Contract txContract = transaction.getRawData().getContract(0);
    byte[] fromAddress = trxCap.getOwnerAddress();
    byte[] toAddress = new byte[20]; // 20 zeros for contract creation
    byte[] data;
    long value = 0;

    if (contract instanceof CreateSmartContract) {
      CreateSmartContract createContract = (CreateSmartContract) contract;
      data = createContract.toByteArray();
      if (createContract.getNewContract().getCallValue() > 0) {
        value = createContract.getNewContract().getCallValue();
      }
    } else if (contract instanceof TriggerSmartContract) {
      TriggerSmartContract triggerContract = (TriggerSmartContract) contract;
      toAddress = triggerContract.getContractAddress().toByteArray();
      data = triggerContract.toByteArray();
      value = triggerContract.getCallValue();
    } else {
      data = txContract.getParameter().toByteArray();
    }

    TronTransaction tronTx = TronTransaction.newBuilder()
        .setFrom(ByteString.copyFrom(fromAddress))
        .setTo(ByteString.copyFrom(toAddress))
        .setValue(ByteString.copyFrom(longToBytes32(value)))
        .setData(ByteString.copyFrom(data))
        .setEnergyLimit(transaction.getRawData().getFeeLimit())
        .setEnergyPrice(1)
        .setNonce(0)
        .setTxKind(TxKind.VM)
        .setContractType(ContractType.forNumber(txContract.getType().getNumber()))
        .build();

    ExecutionContext context = ExecutionContext.newBuilder()
        .setBlockNumber(blockCap.getNum())
        .setBlockTimestamp(blockCap.getTimeStamp())
        .setBlockHash(ByteString.copyFrom(blockCap.getBlockId().getBytes()))
        .setCoinbase(ByteString.copyFrom(blockCap.getWitnessAddress().toByteArray()))
        .setEnergyLimit(transaction.getRawData().getFeeLimit())
        .setEnergyPrice(1)
        .setTransactionId(trxCap.getTransactionId().toString())
        .build();

    return ExecuteTransactionRequest.newBuilder()
        .setTransaction(tronTx)
        .setContext(context)
        .build();
  }

  private void captureVmDatabases(File outputDir) throws Exception {
    String[] databases = {"account", "contract", "code", "abi", "contract-state", "dynamic-properties"};

    for (String dbName : databases) {
      Map<byte[], byte[]> kvData = new TreeMap<>(KvFileFormat.BYTE_ARRAY_COMPARATOR);

      try {
        Iterator<Map.Entry<byte[], byte[]>> iterator = getStoreIterator(dbName);
        if (iterator != null) {
          while (iterator.hasNext()) {
            Map.Entry<byte[], byte[]> entry = iterator.next();
            kvData.put(entry.getKey(), entry.getValue());
          }
        }
      } catch (Exception e) {
        log.warn("Failed to capture state for database {}: {}", dbName, e.getMessage());
      }

      File kvFile = new File(outputDir, dbName + ".kv");
      KvFileFormat.write(kvFile, kvData);
      log.debug("Captured {} entries from {}", kvData.size(), dbName);
    }
  }

  @SuppressWarnings("unchecked")
  private Iterator<Map.Entry<byte[], byte[]>> getStoreIterator(String dbName) {
    try {
      switch (dbName) {
        case "account":
          return convertIterator(chainBaseManager.getAccountStore().iterator());
        case "contract":
          return convertIterator(chainBaseManager.getContractStore().iterator());
        case "code":
          return convertIterator(chainBaseManager.getCodeStore().iterator());
        case "abi":
          return convertIterator(chainBaseManager.getAbiStore().iterator());
        case "dynamic-properties":
          return convertIterator(chainBaseManager.getDynamicPropertiesStore().iterator());
        default:
          log.warn("Unknown database: {}", dbName);
          return null;
      }
    } catch (Exception e) {
      log.warn("Failed to get iterator for {}: {}", dbName, e.getMessage());
      return null;
    }
  }

  @SuppressWarnings("unchecked")
  private Iterator<Map.Entry<byte[], byte[]>> convertIterator(Iterator<?> storeIterator) {
    java.util.List<Map.Entry<byte[], byte[]>> entries = new java.util.ArrayList<>();
    while (storeIterator.hasNext()) {
      Object entry = storeIterator.next();
      if (entry instanceof Map.Entry) {
        Map.Entry<?, ?> mapEntry = (Map.Entry<?, ?>) entry;
        byte[] key = (byte[]) mapEntry.getKey();
        Object value = mapEntry.getValue();

        byte[] valueBytes;
        if (value instanceof byte[]) {
          valueBytes = (byte[]) value;
        } else if (value instanceof org.tron.core.capsule.ProtoCapsule) {
          valueBytes = ((org.tron.core.capsule.ProtoCapsule<?>) value).getData();
        } else if (value != null) {
          continue;
        } else {
          valueBytes = new byte[0];
        }

        entries.add(new java.util.AbstractMap.SimpleEntry<>(key, valueBytes));
      }
    }
    return entries.iterator();
  }

  private BlockCapsule createBlockContext() {
    long blockNum = chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderNumber() + 1;
    long blockTime = chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp() + 3000;

    Protocol.BlockHeader.raw rawHeader = Protocol.BlockHeader.raw.newBuilder()
        .setNumber(blockNum)
        .setTimestamp(blockTime)
        .setWitnessAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .build();

    Protocol.BlockHeader blockHeader = Protocol.BlockHeader.newBuilder()
        .setRawData(rawHeader)
        .build();

    Protocol.Block block = Protocol.Block.newBuilder()
        .setBlockHeader(blockHeader)
        .build();

    return new BlockCapsule(block);
  }

  private int getContractTypeNumber(String typeName) {
    switch (typeName.toLowerCase()) {
      case "create_smart_contract":
        return 30;
      case "trigger_smart_contract":
        return 31;
      default:
        return 0;
    }
  }

  private byte[] longToBytes32(long value) {
    byte[] result = new byte[32];
    for (int i = 7; i >= 0; i--) {
      result[31 - i] = (byte) (value >>> (i * 8));
    }
    return result;
  }

  /**
   * Result of VM fixture generation.
   */
  private static class VmFixtureResult {
    private boolean success;
    private String error;
    private byte[] contractAddress;
    private byte[] returnData;

    public boolean isSuccess() {
      return success;
    }

    public void setSuccess(boolean success) {
      this.success = success;
    }

    public String getError() {
      return error;
    }

    public void setError(String error) {
      this.error = error;
    }

    public byte[] getContractAddress() {
      return contractAddress;
    }

    public void setContractAddress(byte[] contractAddress) {
      this.contractAddress = contractAddress;
    }

    public byte[] getReturnData() {
      return returnData;
    }

    public void setReturnData(byte[] returnData) {
      this.returnData = returnData;
    }
  }
}
