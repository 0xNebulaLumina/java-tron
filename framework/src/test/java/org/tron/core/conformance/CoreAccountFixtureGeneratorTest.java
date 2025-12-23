package org.tron.core.conformance;

import static org.tron.core.conformance.ConformanceFixtureTestSupport.*;

import com.google.protobuf.ByteString;
import java.io.File;
import org.junit.Before;
import org.junit.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.BaseTest;
import org.tron.common.utils.ByteArray;
import org.tron.core.Constant;
import org.tron.core.Wallet;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.AccountContract.AccountCreateContract;
import org.tron.protos.contract.AccountContract.AccountUpdateContract;

/**
 * Generates conformance test fixtures for core account contracts:
 * - AccountCreateContract (0)
 * - AccountUpdateContract (10)
 *
 * <p>Run with: ./gradlew :framework:test --tests "CoreAccountFixtureGeneratorTest"
 * -Dconformance.output=../conformance/fixtures --dependency-verification=off
 */
public class CoreAccountFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(CoreAccountFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String WITNESS_ADDRESS;
  private static final long CREATE_ACCOUNT_FEE = ONE_TRX; // 1 TRX

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049150";
    WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "548794500882809695a8a687866e76d4271a1abc";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("CoreAccount Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Initialize dynamic properties
    initCommonDynamicPropsV1(dbManager,
        DEFAULT_BLOCK_TIMESTAMP / 1000, // block number
        DEFAULT_BLOCK_TIMESTAMP);       // block timestamp

    // Set create account fee
    dbManager.getDynamicPropertiesStore().saveCreateNewAccountFeeInSystemContract(CREATE_ACCOUNT_FEE);
    dbManager.getDynamicPropertiesStore().saveCreateAccountFee(CREATE_ACCOUNT_FEE);

    // Create owner account with sufficient balance
    putAccount(dbManager, OWNER_ADDRESS, INITIAL_BALANCE, "owner");

    // Create witness account and witness entry
    putAccount(dbManager, WITNESS_ADDRESS, INITIAL_BALANCE, "witness");
    putWitness(dbManager, WITNESS_ADDRESS, "https://witness.network", 10_000_000L);
  }

  // ==========================================================================
  // AccountCreateContract (0) Fixtures
  // ==========================================================================

  @Test
  public void generateAccountCreate_happyPath() throws Exception {
    String newAccountAddress = generateAddress("new_account_happy");

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(newAccountAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("happy_path_create_account")
        .caseCategory("happy")
        .description("Create a new account when owner has sufficient balance and target absent")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateAccountCreate_validateFailOwnerMissing() throws Exception {
    String nonExistentOwner = generateAddress("missing_owner_0001");
    String newAccountAddress = generateAddress("new_account_miss");

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentOwner)))
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(newAccountAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("validate_fail_owner_missing")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(nonExistentOwner)
        .expectedError("exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate owner missing: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountCreate_validateFailAccountExists() throws Exception {
    String existingAddress = generateAddress("existing_acc_001");

    // Pre-create the target account
    putAccount(dbManager, existingAddress, ONE_TRX, "existing");

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(existingAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("validate_fail_account_exists")
        .caseCategory("validate_fail")
        .description("Fail when target account already exists")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate account exists: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountCreate_validateFailInsufficientFee() throws Exception {
    String lowBalanceOwner = generateAddress("low_balance_own1");
    String newAccountAddress = generateAddress("new_account_fee1");

    // Create owner with insufficient balance
    putAccount(dbManager, lowBalanceOwner, CREATE_ACCOUNT_FEE / 2, "low_balance");

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(lowBalanceOwner)))
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(newAccountAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("validate_fail_insufficient_fee")
        .caseCategory("validate_fail")
        .description("Fail when owner balance is less than create account fee")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(lowBalanceOwner)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate insufficient fee: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // AccountUpdateContract (10) Fixtures
  // ==========================================================================

  @Test
  public void generateAccountUpdate_happyPathSetNameFirstTime() throws Exception {
    String updateOwner = generateAddress("update_owner_001");
    String accountName = "my_account_name";

    // Create owner with empty name
    putAccount(dbManager, updateOwner, INITIAL_BALANCE, "");

    // Ensure account update name is disabled (can only set once)
    dbManager.getDynamicPropertiesStore().saveAllowUpdateAccountName(0);

    AccountUpdateContract contract = AccountUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(updateOwner)))
        .setAccountName(ByteString.copyFromUtf8(accountName))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_UPDATE_CONTRACT", 10)
        .caseName("happy_path_set_name_first_time")
        .caseCategory("happy")
        .description("Set account name for the first time (allowUpdateAccountName=0)")
        .database("account")
        .database("account-index")
        .database("dynamic-properties")
        .ownerAddress(updateOwner)
        .dynamicProperty("ALLOW_UPDATE_ACCOUNT_NAME", 0)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountUpdate happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateAccountUpdate_validateFailInvalidName() throws Exception {
    String updateOwner = generateAddress("update_owner_002");

    // Create owner with empty name
    putAccount(dbManager, updateOwner, INITIAL_BALANCE, "");

    // Invalid name - empty
    AccountUpdateContract contract = AccountUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(updateOwner)))
        .setAccountName(ByteString.EMPTY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_UPDATE_CONTRACT", 10)
        .caseName("validate_fail_invalid_name")
        .caseCategory("validate_fail")
        .description("Fail when account name is empty or invalid")
        .database("account")
        .database("account-index")
        .database("dynamic-properties")
        .ownerAddress(updateOwner)
        .expectedError("name")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountUpdate invalid name: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountUpdate_validateFailAccountMissing() throws Exception {
    String nonExistentOwner = generateAddress("missing_upd_own1");

    AccountUpdateContract contract = AccountUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentOwner)))
        .setAccountName(ByteString.copyFromUtf8("valid_name_1234"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_UPDATE_CONTRACT", 10)
        .caseName("validate_fail_account_missing")
        .caseCategory("validate_fail")
        .description("Fail when account does not exist")
        .database("account")
        .database("account-index")
        .database("dynamic-properties")
        .ownerAddress(nonExistentOwner)
        .expectedError("exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountUpdate account missing: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountUpdate_validateFailDuplicateNameUpdatesDisabled() throws Exception {
    String updateOwner = generateAddress("update_owner_003");
    String existingNameOwner = generateAddress("existing_name_01");
    String accountName = "duplicate_name1";

    // Create owner with empty name
    putAccount(dbManager, updateOwner, INITIAL_BALANCE, "");

    // Create another account with the same name
    AccountCapsule existingAccount = putAccount(dbManager, existingNameOwner, INITIAL_BALANCE, "");
    existingAccount.setAccountName(accountName.getBytes());
    dbManager.getAccountStore().put(existingAccount.getAddress().toByteArray(), existingAccount);
    // Add to account-index
    dbManager.getAccountIndexStore().put(existingAccount);

    // Disable account name updates
    dbManager.getDynamicPropertiesStore().saveAllowUpdateAccountName(0);

    AccountUpdateContract contract = AccountUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(updateOwner)))
        .setAccountName(ByteString.copyFromUtf8(accountName))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_UPDATE_CONTRACT", 10)
        .caseName("validate_fail_duplicate_name_updates_disabled")
        .caseCategory("validate_fail")
        .description("Fail when name is already taken and updates are disabled")
        .database("account")
        .database("account-index")
        .database("dynamic-properties")
        .ownerAddress(updateOwner)
        .expectedError("exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountUpdate duplicate name: validationError={}", result.getValidationError());
  }
}
