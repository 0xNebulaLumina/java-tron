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
import org.tron.core.capsule.ContractCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.SmartContractOuterClass.ClearABIContract;
import org.tron.protos.contract.SmartContractOuterClass.SmartContract;
import org.tron.protos.contract.SmartContractOuterClass.SmartContract.ABI;
import org.tron.protos.contract.SmartContractOuterClass.UpdateEnergyLimitContract;
import org.tron.protos.contract.SmartContractOuterClass.UpdateSettingContract;

/**
 * Generates conformance test fixtures for Smart Contract metadata contracts:
 * - UpdateSettingContract (33)
 * - UpdateEnergyLimitContract (45)
 * - ClearABIContract (48)
 *
 * <p>Run with: ./gradlew :framework:test --tests "ContractMetadataFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures
 */
public class ContractMetadataFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(ContractMetadataFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String CONTRACT_ADDRESS;
  private static final String OTHER_ADDRESS;
  private static final long INITIAL_BALANCE = 100_000_000_000L;

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049abc";
    CONTRACT_ADDRESS = Wallet.getAddressPreFixString() + "1111111111111111111111111111111111111111";
    OTHER_ADDRESS = Wallet.getAddressPreFixString() + "2222222222222222222222222222222222222222";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("Contract Metadata Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Create owner account
    AccountCapsule ownerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("owner"),
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Create other account (for unauthorized tests)
    AccountCapsule otherAccount = new AccountCapsule(
        ByteString.copyFromUtf8("other"),
        ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(otherAccount.getAddress().toByteArray(), otherAccount);

    // Create a smart contract
    SmartContract smartContract = SmartContract.newBuilder()
        .setOriginAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setConsumeUserResourcePercent(50)
        .setOriginEnergyLimit(10_000_000L)
        .setAbi(ABI.newBuilder()
            .addEntrys(ABI.Entry.newBuilder()
                .setName("transfer")
                .setType(ABI.Entry.EntryType.Function)
                .build())
            .build())
        .build();

    ContractCapsule contractCapsule = new ContractCapsule(smartContract);
    dbManager.getContractStore().put(ByteArray.fromHexString(CONTRACT_ADDRESS), contractCapsule);

    // Store ABI
    chainBaseManager.getAbiStore().put(ByteArray.fromHexString(CONTRACT_ADDRESS),
        new org.tron.core.capsule.AbiCapsule(smartContract.getAbi().toByteArray()));

    // Enable TVM Constantinople
    dbManager.getDynamicPropertiesStore().saveAllowTvmConstantinople(1);

    // Set block properties
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(1000000);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
  }

  // ==========================================================================
  // UpdateSettingContract (33) Fixtures
  // ==========================================================================

  @Test
  public void generateUpdateSetting_happyPath() throws Exception {
    UpdateSettingContract contract = UpdateSettingContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setConsumeUserResourcePercent(75)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Update consume_user_resource_percent for a smart contract")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("consume_user_resource_percent", 75)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateSetting_setToZero() throws Exception {
    UpdateSettingContract contract = UpdateSettingContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setConsumeUserResourcePercent(0)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("happy_path_zero")
        .caseCategory("happy")
        .description("Set consume_user_resource_percent to 0 (contract pays all)")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting set to zero: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateSetting_setTo100() throws Exception {
    UpdateSettingContract contract = UpdateSettingContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setConsumeUserResourcePercent(100)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("happy_path_100")
        .caseCategory("happy")
        .description("Set consume_user_resource_percent to 100 (user pays all)")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting set to 100: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateSetting_notOwner() throws Exception {
    UpdateSettingContract contract = UpdateSettingContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setConsumeUserResourcePercent(75)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("validate_fail_not_owner")
        .caseCategory("validate_fail")
        .description("Fail when caller is not the contract owner (origin_address)")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("owner")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting not owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateSetting_contractNotExist() throws Exception {
    String nonExistentContract = Wallet.getAddressPreFixString() + "9999999999999999999999999999999999999999";

    UpdateSettingContract contract = UpdateSettingContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentContract)))
        .setConsumeUserResourcePercent(75)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("validate_fail_contract_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when target contract does not exist")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting contract not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateSetting_invalidPercent() throws Exception {
    UpdateSettingContract contract = UpdateSettingContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setConsumeUserResourcePercent(101) // Invalid: > 100
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("validate_fail_invalid_percent")
        .caseCategory("validate_fail")
        .description("Fail when consume_user_resource_percent > 100")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("percent")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting invalid percent: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // UpdateEnergyLimitContract (45) Fixtures
  // ==========================================================================

  @Test
  public void generateUpdateEnergyLimit_happyPath() throws Exception {
    UpdateEnergyLimitContract contract = UpdateEnergyLimitContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setOriginEnergyLimit(20_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateEnergyLimitContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ENERGY_LIMIT_CONTRACT", 45)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Update origin_energy_limit for a smart contract")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("origin_energy_limit", 20_000_000L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateEnergyLimit happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateEnergyLimit_notOwner() throws Exception {
    UpdateEnergyLimitContract contract = UpdateEnergyLimitContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setOriginEnergyLimit(20_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateEnergyLimitContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ENERGY_LIMIT_CONTRACT", 45)
        .caseName("validate_fail_not_owner")
        .caseCategory("validate_fail")
        .description("Fail when caller is not the contract owner")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("owner")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateEnergyLimit not owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateEnergyLimit_contractNotExist() throws Exception {
    String nonExistentContract = Wallet.getAddressPreFixString() + "9999999999999999999999999999999999999999";

    UpdateEnergyLimitContract contract = UpdateEnergyLimitContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentContract)))
        .setOriginEnergyLimit(20_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateEnergyLimitContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ENERGY_LIMIT_CONTRACT", 45)
        .caseName("validate_fail_contract_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when target contract does not exist")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateEnergyLimit contract not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateEnergyLimit_zeroLimit() throws Exception {
    UpdateEnergyLimitContract contract = UpdateEnergyLimitContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setOriginEnergyLimit(0) // Invalid: must be > 0
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateEnergyLimitContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ENERGY_LIMIT_CONTRACT", 45)
        .caseName("validate_fail_zero_limit")
        .caseCategory("validate_fail")
        .description("Fail when origin_energy_limit is 0")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("energy")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateEnergyLimit zero limit: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateEnergyLimit_negativeLimit() throws Exception {
    UpdateEnergyLimitContract contract = UpdateEnergyLimitContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setOriginEnergyLimit(-1) // Invalid: must be > 0
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateEnergyLimitContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ENERGY_LIMIT_CONTRACT", 45)
        .caseName("validate_fail_negative_limit")
        .caseCategory("validate_fail")
        .description("Fail when origin_energy_limit is negative")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("energy")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateEnergyLimit negative limit: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // ClearABIContract (48) Fixtures
  // ==========================================================================

  @Test
  public void generateClearABI_happyPath() throws Exception {
    ClearABIContract contract = ClearABIContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ClearABIContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Clear ABI for a smart contract")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateClearABI_notOwner() throws Exception {
    ClearABIContract contract = ClearABIContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ClearABIContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_not_owner")
        .caseCategory("validate_fail")
        .description("Fail when caller is not the contract owner")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("owner")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI not owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateClearABI_contractNotExist() throws Exception {
    String nonExistentContract = Wallet.getAddressPreFixString() + "9999999999999999999999999999999999999999";

    ClearABIContract contract = ClearABIContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentContract)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ClearABIContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_contract_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when target contract does not exist")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI contract not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateClearABI_constantinopleNotEnabled() throws Exception {
    // Disable Constantinople
    dbManager.getDynamicPropertiesStore().saveAllowTvmConstantinople(0);

    ClearABIContract contract = ClearABIContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ClearABIContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_constantinople_disabled")
        .caseCategory("validate_fail")
        .description("Fail when TVM Constantinople is not enabled")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Constantinople")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI Constantinople disabled: validationError={}", result.getValidationError());

    // Re-enable Constantinople for other tests
    dbManager.getDynamicPropertiesStore().saveAllowTvmConstantinople(1);
  }

  @Test
  public void generateClearABI_alreadyCleared() throws Exception {
    // Create a contract without ABI
    String contractNoAbi = Wallet.getAddressPreFixString() + "3333333333333333333333333333333333333333";
    SmartContract smartContract = SmartContract.newBuilder()
        .setOriginAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(contractNoAbi)))
        .setConsumeUserResourcePercent(50)
        .setOriginEnergyLimit(10_000_000L)
        // No ABI set
        .build();

    ContractCapsule contractCapsule = new ContractCapsule(smartContract);
    dbManager.getContractStore().put(ByteArray.fromHexString(contractNoAbi), contractCapsule);

    ClearABIContract contract = ClearABIContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(contractNoAbi)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ClearABIContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("happy_path_no_abi")
        .caseCategory("happy")
        .description("Clear ABI on a contract that has no ABI (idempotent operation)")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI already cleared: success={}", result.isSuccess());
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
}
