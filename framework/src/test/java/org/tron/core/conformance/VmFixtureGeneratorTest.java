package org.tron.core.conformance;

import static org.tron.core.conformance.ConformanceFixtureTestSupport.DEFAULT_BLOCK_TIMESTAMP;
import static org.tron.core.conformance.ConformanceFixtureTestSupport.DEFAULT_TX_EXPIRATION;
import static org.tron.core.conformance.ConformanceFixtureTestSupport.DEFAULT_TX_TIMESTAMP;
import static org.tron.core.conformance.ConformanceFixtureTestSupport.INITIAL_BALANCE;

import com.google.protobuf.ByteString;
import java.io.File;
import java.io.FileOutputStream;
import java.util.Iterator;
import java.util.Map;
import java.util.SortedMap;
import java.util.TreeMap;
import org.bouncycastle.util.encoders.Hex;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.BaseTest;
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
import org.tron.core.exception.ContractValidateException;
import org.tron.core.exception.VMIllegalException;
import org.tron.core.store.StoreFactory;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.SmartContractOuterClass.CreateSmartContract;
import org.tron.protos.contract.SmartContractOuterClass.SmartContract;
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
  private static final long DEFAULT_FEE_LIMIT = 1_000_000_000L; // 1000 TRX
  private static final long DEFAULT_MAX_FEE_LIMIT = 10_000_000_000L; // 10,000 TRX

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

    // Enable TVM features (VM creation enabled by default)
    dbManager.getDynamicPropertiesStore().saveAllowCreationOfContracts(1);
    dbManager.getDynamicPropertiesStore().saveAllowTvmConstantinople(1);
    dbManager.getDynamicPropertiesStore().saveAllowTvmTransferTrc10(1);
    dbManager.getDynamicPropertiesStore().saveAllowMultiSign(1);
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(1);

    // Set maxFeeLimit to a known value for deterministic feeLimit validation
    dbManager.getDynamicPropertiesStore().saveMaxFeeLimit(DEFAULT_MAX_FEE_LIMIT);

    // Set deterministic block properties (from ConformanceFixtureTestSupport)
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(DEFAULT_BLOCK_TIMESTAMP);
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

    // Create transaction with fee limit and deterministic timestamps
    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    // Generate fixture
    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "happy_path",
        "happy",
        "Deploy a simple storage demo contract"
    );

    Assert.assertTrue("happy_path should succeed", result.isSuccess());
    Assert.assertNotNull("Contract address should be set", result.getContractAddress());
    log.info("CreateSmartContract happy path: success={}, contractAddress={}",
        result.isSuccess(),
        Hex.toHexString(result.getContractAddress()));
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

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "happy_path_with_value",
        "happy",
        "Deploy contract with TRX call value"
    );

    Assert.assertTrue("happy_path_with_value should succeed", result.isSuccess());
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
        2_000L, // make validation fail deterministically: callValue > account balance
        50,
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_insufficient_balance",
        "validate_fail",
        "Fail when account has insufficient balance for deployment"
    );

    Assert.assertFalse("validate_fail_insufficient_balance should fail", result.isSuccess());
    Assert.assertNotNull("Error message should be set", result.getError());
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

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
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
  // Phase 1: VALIDATION_FAILED fixtures
  // ==========================================================================

  @Test
  public void generateCreateSmartContract_vmDisabled() throws Exception {
    log.info("Generating CreateSmartContract VM disabled fixture");

    // Temporarily disable VM
    dbManager.getDynamicPropertiesStore().saveAllowCreationOfContracts(0);

    try {
      byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

      CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
          "Test",
          ownerBytes,
          STORAGE_ABI,
          STORAGE_CODE,
          0,
          50,
          null,
          10_000_000L
      );

      TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
      BlockCapsule blockCap = createBlockContext();

      VmFixtureResult result = generateVmFixture(
          trxCap, blockCap, createContract,
          "create_smart_contract",
          "validate_fail_vm_disabled",
          "validate_fail",
          "Fail when VM/contract creation is disabled"
      );

      Assert.assertFalse("validate_fail_vm_disabled should fail", result.isSuccess());
      Assert.assertTrue("Error should mention VM being off",
          result.getError() != null && result.getError().contains("vm work is off"));
      log.info("CreateSmartContract VM disabled: error={}", result.getError());
    } finally {
      // Re-enable VM for other tests
      dbManager.getDynamicPropertiesStore().saveAllowCreationOfContracts(1);
    }
  }

  @Test
  public void generateCreateSmartContract_ownerOriginMismatch() throws Exception {
    log.info("Generating CreateSmartContract owner/origin mismatch fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);
    byte[] otherBytes = ByteArray.fromHexString(OTHER_ADDRESS);

    // Build CreateSmartContract manually with different owner and origin addresses
    SmartContract.Builder builder = SmartContract.newBuilder();
    builder.setName("MismatchContract");
    builder.setOriginAddress(ByteString.copyFrom(otherBytes)); // origin = OTHER
    builder.setAbi(SmartContract.ABI.getDefaultInstance());
    builder.setConsumeUserResourcePercent(50);
    builder.setOriginEnergyLimit(10_000_000L);
    builder.setBytecode(ByteString.copyFrom(Hex.decode(MINIMAL_BYTECODE)));

    CreateSmartContract createContract = CreateSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes)) // owner = OWNER
        .setNewContract(builder.build())
        .build();

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_owner_origin_mismatch",
        "validate_fail",
        "Fail when ownerAddress != originAddress"
    );

    Assert.assertFalse("validate_fail_owner_origin_mismatch should fail", result.isSuccess());
    Assert.assertTrue("Error should mention owner/origin mismatch",
        result.getError() != null && result.getError().contains("OwnerAddress is not equals OriginAddress"));
    log.info("CreateSmartContract owner/origin mismatch: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_nameTooLong() throws Exception {
    log.info("Generating CreateSmartContract name too long fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Create a name with 33 ASCII bytes (exceeds 32 limit)
    String longName = "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567"; // 33 chars

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        longName,
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        50,
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_contract_name_too_long",
        "validate_fail",
        "Fail when contract name exceeds 32 bytes"
    );

    Assert.assertFalse("validate_fail_contract_name_too_long should fail", result.isSuccess());
    Assert.assertTrue("Error should mention name length",
        result.getError() != null && result.getError().contains("contractName's length cannot be greater than 32"));
    log.info("CreateSmartContract name too long: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_nameLen32Ok() throws Exception {
    log.info("Generating CreateSmartContract name exactly 32 bytes fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Create a name with exactly 32 ASCII bytes (at the boundary)
    String exactName = "ABCDEFGHIJKLMNOPQRSTUVWXYZ123456"; // 32 chars

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        exactName,
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        50,
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "edge_contract_name_len_32_ok",
        "edge",
        "Success when contract name is exactly 32 bytes"
    );

    Assert.assertTrue("edge_contract_name_len_32_ok should succeed", result.isSuccess());
    log.info("CreateSmartContract name 32 bytes: success={}", result.isSuccess());
  }

  @Test
  public void generateCreateSmartContract_percentNegative() throws Exception {
    log.info("Generating CreateSmartContract percent negative fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Build CreateSmartContract with negative percent (must build manually)
    SmartContract.Builder builder = SmartContract.newBuilder();
    builder.setName("NegPercent");
    builder.setOriginAddress(ByteString.copyFrom(ownerBytes));
    builder.setAbi(SmartContract.ABI.getDefaultInstance());
    builder.setConsumeUserResourcePercent(-1); // Invalid: negative
    builder.setOriginEnergyLimit(10_000_000L);
    builder.setBytecode(ByteString.copyFrom(Hex.decode(MINIMAL_BYTECODE)));

    CreateSmartContract createContract = CreateSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setNewContract(builder.build())
        .build();

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_percent_negative",
        "validate_fail",
        "Fail when consumeUserResourcePercent < 0"
    );

    Assert.assertFalse("validate_fail_percent_negative should fail", result.isSuccess());
    Assert.assertTrue("Error should mention percent range",
        result.getError() != null && result.getError().contains("percent must be >= 0 and <= 100"));
    log.info("CreateSmartContract percent negative: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_percentGt100() throws Exception {
    log.info("Generating CreateSmartContract percent > 100 fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Build CreateSmartContract with percent > 100 (must build manually)
    SmartContract.Builder builder = SmartContract.newBuilder();
    builder.setName("HighPercent");
    builder.setOriginAddress(ByteString.copyFrom(ownerBytes));
    builder.setAbi(SmartContract.ABI.getDefaultInstance());
    builder.setConsumeUserResourcePercent(101); // Invalid: > 100
    builder.setOriginEnergyLimit(10_000_000L);
    builder.setBytecode(ByteString.copyFrom(Hex.decode(MINIMAL_BYTECODE)));

    CreateSmartContract createContract = CreateSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setNewContract(builder.build())
        .build();

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_percent_gt_100",
        "validate_fail",
        "Fail when consumeUserResourcePercent > 100"
    );

    Assert.assertFalse("validate_fail_percent_gt_100 should fail", result.isSuccess());
    Assert.assertTrue("Error should mention percent range",
        result.getError() != null && result.getError().contains("percent must be >= 0 and <= 100"));
    log.info("CreateSmartContract percent > 100: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_percent0Ok() throws Exception {
    log.info("Generating CreateSmartContract percent = 0 fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "Percent0",
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        0, // Boundary: percent = 0
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "edge_percent_0_ok",
        "edge",
        "Success when consumeUserResourcePercent = 0"
    );

    Assert.assertTrue("edge_percent_0_ok should succeed", result.isSuccess());
    log.info("CreateSmartContract percent 0: success={}", result.isSuccess());
  }

  @Test
  public void generateCreateSmartContract_percent100Ok() throws Exception {
    log.info("Generating CreateSmartContract percent = 100 fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "Percent100",
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        100, // Boundary: percent = 100
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "edge_percent_100_ok",
        "edge",
        "Success when consumeUserResourcePercent = 100"
    );

    Assert.assertTrue("edge_percent_100_ok should succeed", result.isSuccess());
    log.info("CreateSmartContract percent 100: success={}", result.isSuccess());
  }

  @Test
  public void generateCreateSmartContract_feeLimitNegative() throws Exception {
    log.info("Generating CreateSmartContract feeLimit negative fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "NegFeeLimit",
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        50,
        null,
        10_000_000L
    );

    // Create transaction with negative feeLimit
    TransactionCapsule trxCap = createTransactionCapsule(createContract, -1L);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_fee_limit_negative",
        "validate_fail",
        "Fail when feeLimit < 0"
    );

    Assert.assertFalse("validate_fail_fee_limit_negative should fail", result.isSuccess());
    Assert.assertTrue("Error should mention feeLimit range",
        result.getError() != null && result.getError().contains("feeLimit must be >= 0"));
    log.info("CreateSmartContract feeLimit negative: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_feeLimitAboveMax() throws Exception {
    log.info("Generating CreateSmartContract feeLimit above max fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "HighFeeLimit",
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        50,
        null,
        10_000_000L
    );

    // Create transaction with feeLimit > maxFeeLimit
    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_MAX_FEE_LIMIT + 1);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_fee_limit_above_max",
        "validate_fail",
        "Fail when feeLimit > maxFeeLimit"
    );

    Assert.assertFalse("validate_fail_fee_limit_above_max should fail", result.isSuccess());
    Assert.assertTrue("Error should mention feeLimit range",
        result.getError() != null && result.getError().contains("feeLimit must be >= 0"));
    log.info("CreateSmartContract feeLimit above max: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_contractAddressAlreadyExists() throws Exception {
    log.info("Generating CreateSmartContract contract address already exists fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "Collision",
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        50,
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);

    // Pre-create an account at the derived contract address
    byte[] contractAddress = WalletUtil.generateContractAddress(trxCap.getInstance());
    AccountCapsule existingAccount = new AccountCapsule(
        ByteString.copyFromUtf8("existing"),
        ByteString.copyFrom(contractAddress),
        AccountType.Normal,
        1_000_000L);
    dbManager.getAccountStore().put(existingAccount.getAddress().toByteArray(), existingAccount);

    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_contract_address_already_exists",
        "validate_fail",
        "Fail when contract address already exists"
    );

    Assert.assertFalse("validate_fail_contract_address_already_exists should fail", result.isSuccess());
    Assert.assertTrue("Error should mention existing contract address",
        result.getError() != null && result.getError().contains("Trying to create a contract with existing contract address"));
    log.info("CreateSmartContract contract address exists: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_tokenIdTooSmall() throws Exception {
    log.info("Generating CreateSmartContract tokenId too small fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Build CreateSmartContract with tokenId = 1_000_000 (boundary: must be > 1_000_000)
    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "TokenSmall",
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        50,
        null,
        10_000_000L,
        1L, // tokenValue > 0
        1_000_000L // tokenId = MIN_TOKEN_ID (invalid: must be > 1_000_000)
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_token_id_too_small",
        "validate_fail",
        "Fail when tokenId <= 1_000_000 and tokenId != 0"
    );

    Assert.assertFalse("validate_fail_token_id_too_small should fail", result.isSuccess());
    Assert.assertTrue("Error should mention tokenId",
        result.getError() != null && result.getError().contains("tokenId must be > 1000000"));
    log.info("CreateSmartContract tokenId too small: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_tokenValuePositiveTokenIdZero() throws Exception {
    log.info("Generating CreateSmartContract tokenValue > 0 with tokenId = 0 fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Build CreateSmartContract with tokenValue > 0 but tokenId = 0
    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "TokenZeroId",
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        50,
        null,
        10_000_000L,
        100L, // tokenValue > 0
        0L // tokenId = 0 (invalid when tokenValue > 0)
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_token_value_positive_token_id_zero",
        "validate_fail",
        "Fail when tokenValue > 0 and tokenId = 0"
    );

    Assert.assertFalse("validate_fail_token_value_positive_token_id_zero should fail", result.isSuccess());
    Assert.assertTrue("Error should mention invalid tokenValue/tokenId",
        result.getError() != null && result.getError().contains("invalid arguments with tokenValue"));
    log.info("CreateSmartContract tokenValue > 0, tokenId = 0: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_tokenAssetMissing() throws Exception {
    log.info("Generating CreateSmartContract token asset missing fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Use a valid tokenId that doesn't exist in the asset store
    long nonExistentTokenId = 1_000_001L;

    // Build CreateSmartContract with non-existent token
    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "TokenMissing",
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        50,
        null,
        10_000_000L,
        100L, // tokenValue > 0
        nonExistentTokenId // tokenId points to non-existent asset
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_token_asset_missing",
        "validate_fail",
        "Fail when tokenId points to non-existent TRC-10 asset"
    );

    Assert.assertFalse("validate_fail_token_asset_missing should fail", result.isSuccess());
    Assert.assertTrue("Error should mention missing asset",
        result.getError() != null && result.getError().contains("No asset"));
    log.info("CreateSmartContract token asset missing: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_tokenBalanceInsufficient() throws Exception {
    log.info("Generating CreateSmartContract token balance insufficient fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);
    long tokenId = 1_000_002L;

    // Create the TRC-10 asset in the store
    ConformanceFixtureTestSupport.putAssetIssueV2(
        dbManager,
        String.valueOf(tokenId),
        OWNER_ADDRESS,
        "TestToken",
        1_000_000_000L
    );

    // Owner has no token balance (balance not added to account)

    // Build CreateSmartContract with token transfer that owner doesn't have
    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "TokenInsuff",
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        50,
        null,
        10_000_000L,
        100L, // tokenValue > 0, owner has 0
        tokenId
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_token_balance_insufficient",
        "validate_fail",
        "Fail when owner has insufficient TRC-10 token balance"
    );

    Assert.assertFalse("validate_fail_token_balance_insufficient should fail", result.isSuccess());
    Assert.assertTrue("Error should mention insufficient asset balance",
        result.getError() != null &&
            (result.getError().contains("assetBalance must greater than 0")
                || result.getError().contains("assetBalance is not sufficient")));
    log.info("CreateSmartContract token balance insufficient: error={}", result.getError());
  }

  // ==========================================================================
  // Phase 2: Execution / runtime parity fixtures
  // ==========================================================================

  @Test
  public void generateCreateSmartContract_constructorRevert() throws Exception {
    log.info("Generating CreateSmartContract constructor REVERT fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Bytecode that immediately reverts: PUSH1 0x00 PUSH1 0x00 REVERT
    // 0x6000 6000 fd
    String revertBytecode = "60006000fd";

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "Reverter",
        ownerBytes,
        "[]",
        revertBytecode,
        0,
        50,
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "edge_constructor_revert",
        "edge",
        "Constructor executes REVERT opcode"
    );

    // REVERT in constructor should result in runtime error
    Assert.assertFalse("edge_constructor_revert should fail", result.isSuccess());
    log.info("CreateSmartContract constructor revert: success={}, error={}",
        result.isSuccess(), result.getError());
  }

  @Test
  public void generateCreateSmartContract_outOfEnergy() throws Exception {
    log.info("Generating CreateSmartContract out of energy fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Bytecode with infinite loop: JUMPDEST PUSH1 0x00 JUMP
    // label: JUMPDEST (0x5b), PUSH1 0x00 (0x6000), JUMP (0x56)
    // This will run until energy is exhausted
    String infiniteLoopBytecode = "5b600056";

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "InfiniteLoop",
        ownerBytes,
        "[]",
        infiniteLoopBytecode,
        0,
        50,
        null,
        10_000_000L
    );

    // Use a very low feeLimit to trigger OOG quickly
    TransactionCapsule trxCap = createTransactionCapsule(createContract, 1_000L);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "edge_constructor_out_of_energy",
        "edge",
        "Constructor runs out of energy (OOG)"
    );

    // OOG should result in runtime error
    Assert.assertFalse("edge_constructor_out_of_energy should fail", result.isSuccess());
    log.info("CreateSmartContract out of energy: success={}, error={}",
        result.isSuccess(), result.getError());
  }

  @Test
  public void generateCreateSmartContract_nameMultibyteOver32Bytes() throws Exception {
    log.info("Generating CreateSmartContract multibyte name over 32 bytes fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Create a name with fewer than 32 visible chars but > 32 bytes in UTF-8
    // Chinese characters are 3 bytes each in UTF-8
    // 11 Chinese chars = 33 bytes > 32 byte limit
    String multibyteNname = "中文合约名称测试一二三"; // 11 chars, 33 bytes

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        multibyteNname,
        ownerBytes,
        "[]",
        MINIMAL_BYTECODE,
        0,
        50,
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "validate_fail_contract_name_multibyte_over_32_bytes",
        "validate_fail",
        "Fail when contract name has < 32 chars but > 32 bytes (UTF-8)"
    );

    Assert.assertFalse("validate_fail_contract_name_multibyte_over_32_bytes should fail", result.isSuccess());
    Assert.assertTrue("Error should mention name length",
        result.getError() != null && result.getError().contains("contractName's length cannot be greater than 32"));
    log.info("CreateSmartContract multibyte name: error={}", result.getError());
  }

  @Test
  public void generateCreateSmartContract_notEnoughEnergyToSaveCode() throws Exception {
    log.info("Generating CreateSmartContract not enough energy to save code fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Init code that returns a non-trivial runtime code (32 bytes of zeros)
    // PUSH32 0x00...00 (32 zeros), PUSH1 0x00, MSTORE, PUSH1 0x20, PUSH1 0x00, RETURN
    // This returns 32 bytes of runtime code
    // Save cost = 32 * 200 = 6400 energy
    // We set feeLimit just enough for init but not for saving
    String initCodeReturns32Bytes =
        "7F0000000000000000000000000000000000000000000000000000000000000000" // PUSH32 zeros
        + "6000" // PUSH1 0x00
        + "52"   // MSTORE
        + "6020" // PUSH1 0x20 (32 bytes)
        + "6000" // PUSH1 0x00
        + "F3";  // RETURN

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "SaveCodeFail",
        ownerBytes,
        "[]",
        initCodeReturns32Bytes,
        0,
        50,
        null,
        10_000_000L
    );

    // Use a feeLimit that allows init to run but not save 32 bytes (6400 energy needed)
    // Init code costs: ~200 (PUSH32) + 3 (PUSH1) + 3 (MSTORE base) + memory expansion + 3 (PUSH1) + 3 (PUSH1) + 0 (RETURN)
    // Set feeLimit to around 1000 which should be enough for init but not for 6400 save cost
    TransactionCapsule trxCap = createTransactionCapsule(createContract, 5000L);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "edge_not_enough_energy_to_save_code",
        "edge",
        "Not enough energy to save returned runtime code"
    );

    // This should fail with "save just created contract code" error
    Assert.assertFalse("edge_not_enough_energy_to_save_code should fail", result.isSuccess());
    log.info("CreateSmartContract not enough energy to save code: success={}, error={}",
        result.isSuccess(), result.getError());
  }

  @Test
  public void generateCreateSmartContract_londonInvalidCodePrefix0xEF() throws Exception {
    log.info("Generating CreateSmartContract London invalid code prefix 0xEF fixture");

    // Enable TVM London for this test
    dbManager.getDynamicPropertiesStore().saveAllowTvmLondon(1);

    try {
      byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

      // Init code that returns runtime code starting with 0xEF
      // PUSH1 0xEF, PUSH1 0x00, MSTORE8, PUSH1 0x01, PUSH1 0x00, RETURN
      // This returns 1 byte: 0xEF (invalid under London rules)
      String initCodeReturnsEF =
          "60EF" // PUSH1 0xEF
          + "6000" // PUSH1 0x00
          + "53"   // MSTORE8 (store single byte)
          + "6001" // PUSH1 0x01 (return 1 byte)
          + "6000" // PUSH1 0x00
          + "F3";  // RETURN

      CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
          "EFPrefix",
          ownerBytes,
          "[]",
          initCodeReturnsEF,
          0,
          50,
          null,
          10_000_000L
      );

      TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
      BlockCapsule blockCap = createBlockContext();

      VmFixtureResult result = generateVmFixture(
          trxCap, blockCap, createContract,
          "create_smart_contract",
          "edge_london_invalid_code_prefix_0xef",
          "edge",
          "Runtime code starting with 0xEF rejected under London rules"
      );

      Assert.assertFalse("edge_london_invalid_code_prefix_0xef should fail", result.isSuccess());
      Assert.assertTrue("Error should mention invalid code 0xef",
          result.getError() != null && result.getError().contains("0xef"));
      log.info("CreateSmartContract London 0xEF: success={}, error={}",
          result.isSuccess(), result.getError());
    } finally {
      // Disable TVM London for other tests
      dbManager.getDynamicPropertiesStore().saveAllowTvmLondon(0);
    }
  }

  @Test
  public void generateCreateSmartContract_emptyRuntimeCodeSuccess() throws Exception {
    log.info("Generating CreateSmartContract empty runtime code success fixture");

    byte[] ownerBytes = ByteArray.fromHexString(OWNER_ADDRESS);

    // Init code that returns empty runtime code: PUSH1 0x00, PUSH1 0x00, RETURN
    // Returns 0 bytes (empty runtime)
    String initCodeReturnsEmpty =
        "6000" // PUSH1 0x00 (size = 0)
        + "6000" // PUSH1 0x00 (offset = 0)
        + "F3";  // RETURN

    CreateSmartContract createContract = TvmTestUtils.buildCreateSmartContract(
        "EmptyRuntime",
        ownerBytes,
        "[]",
        initCodeReturnsEmpty,
        0,
        50,
        null,
        10_000_000L
    );

    TransactionCapsule trxCap = createTransactionCapsule(createContract, DEFAULT_FEE_LIMIT);
    BlockCapsule blockCap = createBlockContext();

    VmFixtureResult result = generateVmFixture(
        trxCap, blockCap, createContract,
        "create_smart_contract",
        "edge_empty_runtime_code_success",
        "edge",
        "Success with empty runtime code (RETURN(0,0))"
    );

    // Empty runtime code should still result in SUCCESS (contract exists but has no code)
    Assert.assertTrue("edge_empty_runtime_code_success should succeed", result.isSuccess());
    log.info("CreateSmartContract empty runtime: success={}", result.isSuccess());
  }

  // ==========================================================================
  // TriggerSmartContract (31) Fixtures
  // ==========================================================================

  // NOTE: TriggerSmartContract tests are implemented in VmTriggerFixtureGeneratorTest.java
  // which extends VMTestBase for proper test isolation and TVM infrastructure.
  // See: VmTriggerFixtureGeneratorTest for:
  //   - generateTriggerSmartContract_happyPath
  //   - generateTriggerSmartContract_storageWrite
  //   - generateTriggerSmartContract_viewFunction
  //   - generateTriggerSmartContract_deleteStorage
  //   - generateTriggerSmartContract_nonexistentContract
  //   - generateTriggerSmartContract_outOfEnergy

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
    TransactionTrace trace = new TransactionTrace(
        trxCap, StoreFactory.getInstance(), new RuntimeImpl());
    trace.init(blockCap);

    String validationError = null;
    try {
      trace.exec();
      trace.setResult();
      trace.finalization();
    } catch (ContractValidateException | VMIllegalException e) {
      validationError = e.getMessage();
    }

    if (validationError != null) {
      result.setSuccess(false);
      result.setError(validationError);
    } else {
      String runtimeError = trace.getRuntimeError();
      if (runtimeError != null && !runtimeError.isEmpty()) {
        result.setSuccess(false);
        result.setError(runtimeError);
      } else {
        result.setSuccess(true);
      }
      if (trace.getRuntime() != null && trace.getRuntime().getResult() != null) {
        result.setContractAddress(trace.getRuntime().getResult().getContractAddress());
        result.setReturnData(trace.getRuntime().getResult().getHReturn());
      }
    }

    // Capture post-execution state (validation failures should have no changes)
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt only when execution reached trace.exec()
    if (validationError == null && trace.getReceipt() != null) {
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
    if (validationError != null) {
      metadata.setExpectedStatus("VALIDATION_FAILED");
      metadata.setExpectedErrorMessage(validationError);
    } else {
      metadata.setExpectedStatus(result.isSuccess() ? "SUCCESS" : "REVERT");
      if (result.getError() != null) {
        metadata.setExpectedErrorMessage(result.getError());
      }
    }

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated VM fixture: {}/{} (status={})",
        contractTypeName, caseName, metadata.getExpectedStatus());

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
      SortedMap<byte[], byte[]> kvData = new TreeMap<>(KvFileFormat.BYTE_ARRAY_COMPARATOR);

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
        case "contract-state":
          return convertIterator(chainBaseManager.getContractStateStore().iterator());
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

  /**
   * Create a TransactionCapsule with deterministic timestamps for reproducible fixtures.
   */
  private TransactionCapsule createTransactionCapsule(CreateSmartContract createContract, long feeLimit) {
    TransactionCapsule trxCap = new TransactionCapsule(createContract,
        Transaction.Contract.ContractType.CreateSmartContract);
    Transaction.Builder txBuilder = trxCap.getInstance().toBuilder();
    Transaction.raw.Builder rawBuilder = trxCap.getInstance().getRawData().toBuilder();
    rawBuilder.setFeeLimit(feeLimit);
    // Use deterministic timestamps from ConformanceFixtureTestSupport
    rawBuilder.setTimestamp(DEFAULT_TX_TIMESTAMP);
    rawBuilder.setExpiration(DEFAULT_TX_EXPIRATION);
    txBuilder.setRawData(rawBuilder);
    if (txBuilder.getRetCount() == 0) {
      txBuilder.addRet(Transaction.Result.newBuilder().build());
    }
    return new TransactionCapsule(txBuilder.build());
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
