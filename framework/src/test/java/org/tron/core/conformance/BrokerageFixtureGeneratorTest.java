package org.tron.core.conformance;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import java.io.File;
import org.junit.Before;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.junit.Test;
import org.tron.common.BaseTest;
import org.tron.common.utils.ByteArray;
import org.tron.core.Constant;
import org.tron.core.Wallet;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.capsule.WitnessCapsule;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.StorageContract.UpdateBrokerageContract;

/**
 * Generates conformance test fixtures for UpdateBrokerageContract (49).
 *
 * <p>Run with: ./gradlew :framework:test --tests "BrokerageFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures
 */
public class BrokerageFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(BrokerageFixtureGeneratorTest.class);
  private static final String WITNESS_ADDRESS;
  private static final String NON_WITNESS_ADDRESS;
  private static final long INITIAL_BALANCE = 100_000_000_000L;

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049abc";
    NON_WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "1111111111111111111111111111111111111111";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("Brokerage Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Create witness account
    AccountCapsule witnessAccount = new AccountCapsule(
        ByteString.copyFromUtf8("witness"),
        ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(witnessAccount.getAddress().toByteArray(), witnessAccount);

    // Create witness
    WitnessCapsule witness = new WitnessCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)),
        10_000_000L,
        "https://witness.network");
    dbManager.getWitnessStore().put(witness.getAddress().toByteArray(), witness);

    // Create non-witness account
    AccountCapsule nonWitnessAccount = new AccountCapsule(
        ByteString.copyFromUtf8("non_witness"),
        ByteString.copyFrom(ByteArray.fromHexString(NON_WITNESS_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(nonWitnessAccount.getAddress().toByteArray(), nonWitnessAccount);

    // Enable change delegation feature
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(1);

    // Set block properties
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(1000000);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
  }

  // ==========================================================================
  // UpdateBrokerageContract (49) Fixtures
  // ==========================================================================

  @Test
  public void generateUpdateBrokerage_happyPath() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(20) // 20% brokerage
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Update brokerage rate for a witness to 20%")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .dynamicProperty("brokerage", 20)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateBrokerage_zeroPercent() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(0) // 0% - witness takes no commission
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("happy_path_zero")
        .caseCategory("happy")
        .description("Set brokerage to 0% (witness takes no commission)")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage zero: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateBrokerage_100Percent() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(100) // 100% - witness takes all commission
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("happy_path_100")
        .caseCategory("happy")
        .description("Set brokerage to 100% (witness takes all commission)")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage 100: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateBrokerage_notWitness() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(NON_WITNESS_ADDRESS)))
        .setBrokerage(20)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_not_witness")
        .caseCategory("validate_fail")
        .description("Fail when owner is not a witness/SR")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(NON_WITNESS_ADDRESS)
        .expectedError("witness")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage not witness: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateBrokerage_negativePercent() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(-1) // Invalid: negative
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_negative")
        .caseCategory("validate_fail")
        .description("Fail when brokerage is negative")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("brokerage")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage negative: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateBrokerage_over100Percent() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(101) // Invalid: > 100
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_over_100")
        .caseCategory("validate_fail")
        .description("Fail when brokerage is greater than 100")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("brokerage")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage over 100: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateBrokerage_changeDelegationDisabled() throws Exception {
    // Disable change delegation
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(0);

    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(20)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_disabled")
        .caseCategory("validate_fail")
        .description("Fail when change delegation feature is not enabled")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("delegation")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage disabled: validationError={}", result.getValidationError());

    // Re-enable for other tests
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(1);
  }

  @Test
  public void generateUpdateBrokerage_accountNotExist() throws Exception {
    String nonExistentAddress = Wallet.getAddressPreFixString() + "9999999999999999999999999999999999999999";

    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
        .setBrokerage(20)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(nonExistentAddress)
        .expectedError("exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage account not exist: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Helper Methods
  // ==========================================================================

  private TransactionCapsule createTransaction(Transaction.Contract.ContractType type,
                                                com.google.protobuf.Message contract) {
    Transaction.Contract protoContract = Transaction.Contract.newBuilder()
        .setType(type)
        .setParameter(Any.pack(contract))
        .build();

    Transaction transaction = Transaction.newBuilder()
        .setRawData(Transaction.raw.newBuilder()
            .addContract(protoContract)
            .setTimestamp(System.currentTimeMillis())
            .setExpiration(System.currentTimeMillis() + 3600000)
            .build())
        .build();

    return new TransactionCapsule(transaction);
  }

  private BlockCapsule createBlockContext() {
    long blockNum = chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderNumber() + 1;
    long blockTime = chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp() + 3000;

    Protocol.BlockHeader.raw rawHeader = Protocol.BlockHeader.raw.newBuilder()
        .setNumber(blockNum)
        .setTimestamp(blockTime)
        .setWitnessAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .build();

    Protocol.BlockHeader blockHeader = Protocol.BlockHeader.newBuilder()
        .setRawData(rawHeader)
        .build();

    Protocol.Block block = Protocol.Block.newBuilder()
        .setBlockHeader(blockHeader)
        .build();

    return new BlockCapsule(block);
  }
}
