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
import org.tron.common.parameter.CommonParameter;
import org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract;
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
  private static final String NON_EXISTENT_ACCOUNT_ADDRESS;
  private static final long INITIAL_BALANCE = 100_000_000_000L;

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049abc";
    CONTRACT_ADDRESS = Wallet.getAddressPreFixString() + "1111111111111111111111111111111111111111";
    OTHER_ADDRESS = Wallet.getAddressPreFixString() + "2222222222222222222222222222222222222222";
    // Valid address format but not present in AccountStore
    NON_EXISTENT_ACCOUNT_ADDRESS = Wallet.getAddressPreFixString() + "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
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

  @Test
  public void generateUpdateSetting_negativePercent() throws Exception {
    UpdateSettingContract contract = UpdateSettingContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setConsumeUserResourcePercent(-1) // Invalid: < 0
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("validate_fail_negative_percent")
        .caseCategory("validate_fail")
        .description("Fail when consume_user_resource_percent is negative (< 0)")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("percent not in [0, 100]")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting negative percent: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateSetting_ownerAddressEmpty() throws Exception {
    UpdateSettingContract contract = UpdateSettingContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Invalid: empty address
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setConsumeUserResourcePercent(75)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("validate_fail_owner_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when ownerAddress is empty (ByteString.EMPTY)")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting owner address empty: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateSetting_ownerAddressWrongLength() throws Exception {
    // 10 bytes instead of 21 bytes
    UpdateSettingContract contract = UpdateSettingContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(new byte[10])) // Invalid: wrong length
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setConsumeUserResourcePercent(75)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("validate_fail_owner_address_wrong_length")
        .caseCategory("validate_fail")
        .description("Fail when ownerAddress has wrong length (not 21 bytes)")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting owner address wrong length: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateSetting_ownerAccountNotExist() throws Exception {
    // Valid address format but not present in AccountStore
    UpdateSettingContract contract = UpdateSettingContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(NON_EXISTENT_ACCOUNT_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setConsumeUserResourcePercent(75)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(NON_EXISTENT_ACCOUNT_ADDRESS)
        .expectedError("does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting owner account not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateSetting_contractAddressEmpty() throws Exception {
    UpdateSettingContract contract = UpdateSettingContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.EMPTY) // Empty contract address
        .setConsumeUserResourcePercent(75)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("validate_fail_contract_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when contractAddress is empty (falls through to contract not exist)")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Contract does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting contract address empty: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateSetting_typeMismatch() throws Exception {
    // Transaction type is UpdateSettingContract but payload is AssetIssueContract
    AssetIssueContract wrongPayload = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setName(ByteString.copyFromUtf8("TestToken"))
        .setTotalSupply(1000000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateSettingContract, wrongPayload);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_SETTING_CONTRACT", 33)
        .caseName("validate_fail_type_mismatch")
        .caseCategory("validate_fail")
        .description("Fail when contract type is UpdateSettingContract but payload is different protobuf")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .expectedError("contract type error")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateSetting type mismatch: validationError={}", result.getValidationError());
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

  @Test
  public void generateUpdateEnergyLimit_forkNotEnabled() throws Exception {
    // Set fork height high so energy limit feature is not enabled
    long originalForkHeight = CommonParameter.getInstance().getBlockNumForEnergyLimit();
    CommonParameter.getInstance().setBlockNumForEnergyLimit(100); // Block 10 < 100

    try {
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
          .caseName("validate_fail_fork_not_enabled")
          .caseCategory("validate_fail")
          .description("Fail when energy limit fork is not enabled (blockNum < blockNumForEnergyLimit)")
          .database("account")
          .database("contract")
          .database("dynamic-properties")
          .ownerAddress(OWNER_ADDRESS)
          .expectedError("contract type error, unexpected type [UpdateEnergyLimitContract]")
          .build();

      FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
      log.info("UpdateEnergyLimit fork not enabled: validationError={}", result.getValidationError());
    } finally {
      // Restore original fork height
      CommonParameter.getInstance().setBlockNumForEnergyLimit(originalForkHeight);
    }
  }

  @Test
  public void generateUpdateEnergyLimit_largeLimit() throws Exception {
    // Very large but valid energy limit (Long.MAX_VALUE)
    UpdateEnergyLimitContract contract = UpdateEnergyLimitContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setOriginEnergyLimit(Long.MAX_VALUE) // Extreme but valid value
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateEnergyLimitContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ENERGY_LIMIT_CONTRACT", 45)
        .caseName("edge_large_energy_limit")
        .caseCategory("happy")
        .description("Update origin_energy_limit to Long.MAX_VALUE (extreme but valid)")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("origin_energy_limit", Long.MAX_VALUE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateEnergyLimit large limit: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateEnergyLimit_ownerAddressEmpty() throws Exception {
    UpdateEnergyLimitContract contract = UpdateEnergyLimitContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Invalid: empty address
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setOriginEnergyLimit(20_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateEnergyLimitContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ENERGY_LIMIT_CONTRACT", 45)
        .caseName("validate_fail_owner_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when ownerAddress is empty (ByteString.EMPTY)")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateEnergyLimit owner address empty: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateEnergyLimit_ownerAddressWrongLength() throws Exception {
    // 10 bytes instead of 21 bytes
    UpdateEnergyLimitContract contract = UpdateEnergyLimitContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(new byte[10])) // Invalid: wrong length
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setOriginEnergyLimit(20_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateEnergyLimitContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ENERGY_LIMIT_CONTRACT", 45)
        .caseName("validate_fail_owner_address_wrong_length")
        .caseCategory("validate_fail")
        .description("Fail when ownerAddress has wrong length (not 21 bytes)")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateEnergyLimit owner address wrong length: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateEnergyLimit_ownerAccountNotExist() throws Exception {
    // Valid address format but not present in AccountStore
    UpdateEnergyLimitContract contract = UpdateEnergyLimitContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(NON_EXISTENT_ACCOUNT_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .setOriginEnergyLimit(20_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateEnergyLimitContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ENERGY_LIMIT_CONTRACT", 45)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(NON_EXISTENT_ACCOUNT_ADDRESS)
        .expectedError("does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateEnergyLimit owner account not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateEnergyLimit_contractAddressEmpty() throws Exception {
    UpdateEnergyLimitContract contract = UpdateEnergyLimitContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.EMPTY) // Empty contract address
        .setOriginEnergyLimit(20_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateEnergyLimitContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ENERGY_LIMIT_CONTRACT", 45)
        .caseName("validate_fail_contract_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when contractAddress is empty (falls through to contract not exist)")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Contract does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateEnergyLimit contract address empty: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateEnergyLimit_typeMismatch() throws Exception {
    // Transaction type is UpdateEnergyLimitContract but payload is AssetIssueContract
    AssetIssueContract wrongPayload = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setName(ByteString.copyFromUtf8("TestToken"))
        .setTotalSupply(1000000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateEnergyLimitContract, wrongPayload);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ENERGY_LIMIT_CONTRACT", 45)
        .caseName("validate_fail_type_mismatch")
        .caseCategory("validate_fail")
        .description("Fail when contract type is UpdateEnergyLimitContract but payload is different protobuf")
        .database("account")
        .database("contract")
        .database("dynamic-properties")
        .expectedError("contract type error")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateEnergyLimit type mismatch: validationError={}", result.getValidationError());
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

  @Test
  public void generateClearABI_ownerAddressEmpty() throws Exception {
    ClearABIContract contract = ClearABIContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Invalid: empty address
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ClearABIContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_owner_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when ownerAddress is empty (ByteString.EMPTY)")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI owner address empty: validationError={}", result.getValidationError());
  }

  @Test
  public void generateClearABI_ownerAddressWrongLength() throws Exception {
    // 10 bytes instead of 21 bytes
    ClearABIContract contract = ClearABIContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(new byte[10])) // Invalid: wrong length
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ClearABIContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_owner_address_wrong_length")
        .caseCategory("validate_fail")
        .description("Fail when ownerAddress has wrong length (not 21 bytes)")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI owner address wrong length: validationError={}", result.getValidationError());
  }

  @Test
  public void generateClearABI_ownerAccountNotExist() throws Exception {
    // Valid address format but not present in AccountStore
    ClearABIContract contract = ClearABIContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(NON_EXISTENT_ACCOUNT_ADDRESS)))
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(CONTRACT_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ClearABIContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .ownerAddress(NON_EXISTENT_ACCOUNT_ADDRESS)
        .expectedError("not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI owner account not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateClearABI_contractAddressEmpty() throws Exception {
    ClearABIContract contract = ClearABIContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setContractAddress(ByteString.EMPTY) // Empty contract address
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ClearABIContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_contract_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when contractAddress is empty (falls through to contract not exists)")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Contract not exists")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI contract address empty: validationError={}", result.getValidationError());
  }

  @Test
  public void generateClearABI_typeMismatch() throws Exception {
    // Transaction type is ClearABIContract but payload is AssetIssueContract
    AssetIssueContract wrongPayload = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setName(ByteString.copyFromUtf8("TestToken"))
        .setTotalSupply(1000000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ClearABIContract, wrongPayload);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_type_mismatch")
        .caseCategory("validate_fail")
        .description("Fail when contract type is ClearABIContract but payload is different protobuf")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .expectedError("contract type error")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI type mismatch: validationError={}", result.getValidationError());
  }

  @Test
  public void generateClearABI_invalidProtobufBytes() throws Exception {
    // Manually build Any with correct type_url but invalid/corrupted value bytes
    // This covers the InvalidProtocolBufferException catch block in validate()
    Any invalidAny = Any.newBuilder()
        .setTypeUrl("type.googleapis.com/" + ClearABIContract.getDescriptor().getFullName())
        .setValue(ByteString.copyFrom(new byte[]{0x0A, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF})) // invalid varint length for owner_address
        .build();

    TransactionCapsule trxCap = createTransactionWithRawAny(
        Transaction.Contract.ContractType.ClearABIContract, invalidAny);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_invalid_protobuf_bytes")
        .caseCategory("validate_fail")
        .description("Fail when contract parameter contains invalid/truncated protobuf bytes")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .expectedError("Protocol") // InvalidProtocolBufferException message contains "Protocol"
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI invalid protobuf: validationError={}", result.getValidationError());
  }

  /**
   * Test case: invalid tag (zero)
   * Protobuf wire format: tag = 0x00 (field 0, wire type 0 - invalid)
   * Expected: InvalidProtocolBufferException with "Protocol message contained an invalid tag (zero)."
   */
  @Test
  public void generateClearABI_invalidProtobufTagZero() throws Exception {
    Any invalidAny = Any.newBuilder()
        .setTypeUrl("type.googleapis.com/" + ClearABIContract.getDescriptor().getFullName())
        .setValue(ByteString.copyFrom(new byte[]{0x00})) // tag = 0 is invalid
        .build();

    TransactionCapsule trxCap = createTransactionWithRawAny(
        Transaction.Contract.ContractType.ClearABIContract, invalidAny);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_invalid_protobuf_tag_zero")
        .caseCategory("validate_fail")
        .description("Fail when protobuf contains invalid tag (zero)")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .expectedError("invalid tag")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI invalid tag zero: validationError={}", result.getValidationError());
  }

  /**
   * Test case: invalid wire type (6)
   * Protobuf wire format: tag = (1 << 3) | 6 = 0x0E (field 1, wire type 6 - invalid)
   * Expected: InvalidProtocolBufferException with "Protocol message tag had invalid wire type."
   */
  @Test
  public void generateClearABI_invalidProtobufWireType() throws Exception {
    Any invalidAny = Any.newBuilder()
        .setTypeUrl("type.googleapis.com/" + ClearABIContract.getDescriptor().getFullName())
        .setValue(ByteString.copyFrom(new byte[]{0x0E})) // field 1, wire type 6 (invalid)
        .build();

    TransactionCapsule trxCap = createTransactionWithRawAny(
        Transaction.Contract.ContractType.ClearABIContract, invalidAny);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_invalid_protobuf_wire_type")
        .caseCategory("validate_fail")
        .description("Fail when protobuf contains invalid wire type (6)")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .expectedError("wire type")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI invalid wire type: validationError={}", result.getValidationError());
  }

  /**
   * Test case: malformed varint (too long)
   * Protobuf wire format: tag 0x0A (field 1, wire type 2) followed by 10 continuation bytes
   * Expected: InvalidProtocolBufferException with "CodedInputStream encountered a malformed varint."
   */
  @Test
  public void generateClearABI_malformedVarintLength() throws Exception {
    // 0x0A = field 1, wire type 2 (length-delimited)
    // Then 10 bytes of 0xFF (continuation bits set) = malformed varint (too long)
    byte[] malformedData = new byte[]{
        0x0A, // tag: field 1, wire type 2
        (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF,
        (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF  // 10 continuation bytes
    };

    Any invalidAny = Any.newBuilder()
        .setTypeUrl("type.googleapis.com/" + ClearABIContract.getDescriptor().getFullName())
        .setValue(ByteString.copyFrom(malformedData))
        .build();

    TransactionCapsule trxCap = createTransactionWithRawAny(
        Transaction.Contract.ContractType.ClearABIContract, invalidAny);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_malformed_varint_length")
        .caseCategory("validate_fail")
        .description("Fail when protobuf contains malformed varint (too long)")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .expectedError("malformed varint")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI malformed varint: validationError={}", result.getValidationError());
  }

  /**
   * Test case: truncated unknown length-delimited field
   * Protobuf wire format: unknown field with wire type 2 and length that exceeds remaining data
   * Expected: InvalidProtocolBufferException with truncated message
   */
  @Test
  public void generateClearABI_truncatedUnknownField() throws Exception {
    // Use unknown field 99 (not 1 or 2), wire type 2 (length-delimited)
    // Tag = (99 << 3) | 2 = 794 = varint 0xFA 0x06
    // Length = 100 (0x64), but only provide 5 bytes
    byte[] truncatedData = new byte[]{
        (byte) 0xFA, 0x06, // tag: field 99, wire type 2
        0x64,              // length: 100 bytes
        0x01, 0x02, 0x03, 0x04, 0x05  // only 5 bytes provided
    };

    Any invalidAny = Any.newBuilder()
        .setTypeUrl("type.googleapis.com/" + ClearABIContract.getDescriptor().getFullName())
        .setValue(ByteString.copyFrom(truncatedData))
        .build();

    TransactionCapsule trxCap = createTransactionWithRawAny(
        Transaction.Contract.ContractType.ClearABIContract, invalidAny);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CLEAR_ABI_CONTRACT", 48)
        .caseName("validate_fail_truncated_unknown_field")
        .caseCategory("validate_fail")
        .description("Fail when protobuf contains truncated unknown length-delimited field")
        .database("account")
        .database("contract")
        .database("abi")
        .database("dynamic-properties")
        .expectedError("Protocol") // truncated message contains "Protocol"
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ClearABI truncated unknown field: validationError={}", result.getValidationError());
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

  /**
   * Creates a transaction with a pre-built Any parameter (for testing invalid protobuf bytes).
   * This allows injecting malformed protobuf data to test error handling.
   */
  private TransactionCapsule createTransactionWithRawAny(
      Transaction.Contract.ContractType declaredType,
      Any rawAny) {
    Transaction.Contract protoContract = Transaction.Contract.newBuilder()
        .setType(declaredType)
        .setParameter(rawAny)
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
}
