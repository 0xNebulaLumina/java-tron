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
        .expectedError("insufficient fee")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate insufficient fee: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountCreateContract (0) - Invalid owner address validation
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountCreate_validateFailOwnerAddressEmpty() throws Exception {
    String newAccountAddress = generateAddress("new_account_emp1");

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(newAccountAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("validate_fail_owner_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty bytes")
        .database("account")
        .database("dynamic-properties")
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate empty owner address: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountCreate_validateFailOwnerAddressWrongPrefix() throws Exception {
    String newAccountAddress = generateAddress("new_account_pfx1");

    // Create 21-byte address with wrong prefix (0x00 instead of 0x41)
    byte[] wrongPrefixAddress = new byte[21];
    wrongPrefixAddress[0] = 0x00; // Wrong prefix
    for (int i = 1; i < 21; i++) {
      wrongPrefixAddress[i] = (byte) i;
    }

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(wrongPrefixAddress))
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(newAccountAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("validate_fail_owner_address_wrong_prefix")
        .caseCategory("validate_fail")
        .description("Fail when owner address has wrong prefix byte (not 0x41)")
        .database("account")
        .database("dynamic-properties")
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate wrong prefix owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountCreate_validateFailOwnerAddressWrongLength() throws Exception {
    String newAccountAddress = generateAddress("new_account_len1");

    // Create 20-byte address (wrong length, should be 21)
    byte[] shortAddress = new byte[20];
    shortAddress[0] = 0x41; // Correct prefix, but wrong length
    for (int i = 1; i < 20; i++) {
      shortAddress[i] = (byte) i;
    }

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(shortAddress))
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(newAccountAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("validate_fail_owner_address_wrong_length")
        .caseCategory("validate_fail")
        .description("Fail when owner address is not 21 bytes (20 bytes)")
        .database("account")
        .database("dynamic-properties")
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate wrong length owner: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountCreateContract (0) - Invalid account address validation
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountCreate_validateFailAccountAddressEmpty() throws Exception {
    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountAddress(ByteString.EMPTY)
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("validate_fail_account_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when target account address is empty bytes")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid account address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate empty account address: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountCreate_validateFailAccountAddressWrongLength() throws Exception {
    // Create 22-byte address (wrong length, should be 21)
    byte[] longAddress = new byte[22];
    longAddress[0] = 0x41; // Correct prefix, but wrong length
    for (int i = 1; i < 22; i++) {
      longAddress[i] = (byte) i;
    }

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountAddress(ByteString.copyFrom(longAddress))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("validate_fail_account_address_wrong_length")
        .caseCategory("validate_fail")
        .description("Fail when target account address is not 21 bytes (22 bytes)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid account address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate wrong length account: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountCreateContract (0) - Fee boundary conditions
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountCreate_edgeHappyBalanceEqualsFee() throws Exception {
    String boundaryOwner = generateAddress("boundary_own_eq1");
    String newAccountAddress = generateAddress("new_account_beq1");

    // Create owner with balance exactly equal to fee
    putAccount(dbManager, boundaryOwner, CREATE_ACCOUNT_FEE, "boundary_exact");

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(boundaryOwner)))
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(newAccountAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("edge_happy_balance_equals_fee")
        .caseCategory("edge")
        .description("Success when owner balance exactly equals create account fee")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(boundaryOwner)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate balance==fee: success={}", result.isSuccess());
  }

  @Test
  public void generateAccountCreate_validateFailBalanceFeeMinus1() throws Exception {
    String boundaryOwner = generateAddress("boundary_own_m11");
    String newAccountAddress = generateAddress("new_account_bm11");

    // Create owner with balance exactly fee - 1
    putAccount(dbManager, boundaryOwner, CREATE_ACCOUNT_FEE - 1, "boundary_minus1");

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(boundaryOwner)))
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(newAccountAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("validate_fail_balance_fee_minus_one")
        .caseCategory("validate_fail")
        .description("Fail when owner balance is exactly fee - 1")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(boundaryOwner)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .expectedError("insufficient fee")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate balance==fee-1: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountCreateContract (0) - Feature flag dependent execute behavior
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountCreate_edgeHappyAllowMultiSignEnabled() throws Exception {
    String multiSignOwner = generateAddress("multisign_own_01");
    String newAccountAddress = generateAddress("new_account_ms01");

    // Create owner with sufficient balance
    putAccount(dbManager, multiSignOwner, INITIAL_BALANCE, "multisign_owner");

    // Enable multi-sign and set default active operations
    dbManager.getDynamicPropertiesStore().saveAllowMultiSign(1);
    // ACTIVE_DEFAULT_OPERATIONS must be initialized for getAllowMultiSign=1 execute path
    dbManager.getDynamicPropertiesStore().saveActiveDefaultOperations(new byte[32]);

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(multiSignOwner)))
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(newAccountAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("edge_happy_allow_multi_sign_enabled")
        .caseCategory("edge")
        .description("Success with ALLOW_MULTI_SIGN=1, new account gets default owner+active permissions")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(multiSignOwner)
        .dynamicProperty("ALLOW_MULTI_SIGN", 1)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate ALLOW_MULTI_SIGN=1: success={}", result.isSuccess());
  }

  @Test
  public void generateAccountCreate_edgeHappyBlackholeOptimizationBurnsFee() throws Exception {
    String burnOwner = generateAddress("burn_owner_0001");
    String newAccountAddress = generateAddress("new_account_brn1");

    // Create owner with sufficient balance
    putAccount(dbManager, burnOwner, INITIAL_BALANCE, "burn_owner");

    // Enable blackhole optimization (fees are burned instead of credited to blackhole)
    dbManager.getDynamicPropertiesStore().saveAllowBlackHoleOptimization(1);
    // BURN_TRX_AMOUNT must be initialized for supportBlackHoleOptimization execute path
    dbManager.getDynamicPropertiesStore().saveBurnTrx(0);

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(burnOwner)))
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(newAccountAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("edge_happy_blackhole_optimization_burns_fee")
        .caseCategory("edge")
        .description("Success with ALLOW_BLACKHOLE_OPTIMIZATION=1, fee is burned (BURN_TRX_AMOUNT incremented)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(burnOwner)
        .dynamicProperty("ALLOW_BLACKHOLE_OPTIMIZATION", 1)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate ALLOW_BLACKHOLE_OPTIMIZATION=1: success={}", result.isSuccess());
  }

  @Test
  public void generateAccountCreate_edgeHappyFeeZero() throws Exception {
    String zeroFeeOwner = generateAddress("zerofee_own_001");
    String newAccountAddress = generateAddress("new_account_zf01");

    // Create owner with zero balance (should still succeed if fee is 0)
    putAccount(dbManager, zeroFeeOwner, 0, "zero_fee_owner");

    // Set create account fee to 0
    dbManager.getDynamicPropertiesStore().saveCreateNewAccountFeeInSystemContract(0);
    dbManager.getDynamicPropertiesStore().saveCreateAccountFee(0);

    AccountCreateContract contract = AccountCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(zeroFeeOwner)))
        .setAccountAddress(ByteString.copyFrom(ByteArray.fromHexString(newAccountAddress)))
        .setType(AccountType.Normal)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_CREATE_CONTRACT", 0)
        .caseName("edge_happy_fee_zero")
        .caseCategory("edge")
        .description("Success when CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT=0, zero balance owner can create")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(zeroFeeOwner)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", 0)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountCreate fee=0: success={}", result.isSuccess());
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
  public void generateAccountUpdate_validateFailInvalidNameTooLong() throws Exception {
    String updateOwner = generateAddress("update_owner_002");

    // Create owner with empty name
    putAccount(dbManager, updateOwner, INITIAL_BALANCE, "");

    // Invalid name - 201 bytes (exceeds MAX_ACCOUNT_NAME_LEN of 200)
    byte[] tooLongName = new byte[201];
    for (int i = 0; i < 201; i++) {
      tooLongName[i] = (byte) 'a';
    }

    AccountUpdateContract contract = AccountUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(updateOwner)))
        .setAccountName(ByteString.copyFrom(tooLongName))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_UPDATE_CONTRACT", 10)
        .caseName("validate_fail_invalid_name_too_long")
        .caseCategory("validate_fail")
        .description("Fail when account name exceeds 200 bytes (201 bytes)")
        .database("account")
        .database("account-index")
        .database("dynamic-properties")
        .ownerAddress(updateOwner)
        .expectedError("Invalid accountName")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountUpdate invalid name (201 bytes): validationError={}", result.getValidationError());
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
        .expectedError("This name is existed")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountUpdate duplicate name: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountUpdateContract (10) - Invalid owner address validation
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountUpdate_validateFailOwnerAddressEmpty() throws Exception {
    AccountUpdateContract contract = AccountUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setAccountName(ByteString.copyFromUtf8("valid_name_0001"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_UPDATE_CONTRACT", 10)
        .caseName("validate_fail_owner_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty bytes")
        .database("account")
        .database("account-index")
        .database("dynamic-properties")
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountUpdate empty owner address: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountUpdateContract (10) - Account name boundary conditions
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountUpdate_edgeHappyAccountNameLen200() throws Exception {
    String updateOwner = generateAddress("update_owner_200");

    // Create owner with empty name
    putAccount(dbManager, updateOwner, INITIAL_BALANCE, "");

    // Valid name - exactly 200 bytes (MAX_ACCOUNT_NAME_LEN)
    byte[] maxLengthName = new byte[200];
    for (int i = 0; i < 200; i++) {
      maxLengthName[i] = (byte) 'a';
    }

    AccountUpdateContract contract = AccountUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(updateOwner)))
        .setAccountName(ByteString.copyFrom(maxLengthName))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_UPDATE_CONTRACT", 10)
        .caseName("edge_happy_account_name_len_200")
        .caseCategory("edge")
        .description("Success when account name is exactly 200 bytes (max allowed)")
        .database("account")
        .database("account-index")
        .database("dynamic-properties")
        .ownerAddress(updateOwner)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountUpdate name=200 bytes: success={}", result.isSuccess());
  }

  // --------------------------------------------------------------------------
  // AccountUpdateContract (10) - Owner already has name + updates disabled
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountUpdate_validateFailOwnerAlreadyNamedUpdatesDisabled() throws Exception {
    String updateOwner = generateAddress("update_owner_004");

    // Create owner with an existing name (non-empty)
    AccountCapsule ownerAccount = putAccount(dbManager, updateOwner, INITIAL_BALANCE, "");
    ownerAccount.setAccountName("existing_name_x".getBytes());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Disable account name updates
    dbManager.getDynamicPropertiesStore().saveAllowUpdateAccountName(0);

    // Try to update to a different (unique) name
    AccountUpdateContract contract = AccountUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(updateOwner)))
        .setAccountName(ByteString.copyFromUtf8("new_unique_name1"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_UPDATE_CONTRACT", 10)
        .caseName("validate_fail_owner_already_named_updates_disabled")
        .caseCategory("validate_fail")
        .description("Fail when owner already has a name and ALLOW_UPDATE_ACCOUNT_NAME=0")
        .database("account")
        .database("account-index")
        .database("dynamic-properties")
        .ownerAddress(updateOwner)
        .dynamicProperty("ALLOW_UPDATE_ACCOUNT_NAME", 0)
        .expectedError("This account name is already existed")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountUpdate owner already named: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountUpdateContract (10) - Update-enabled behavior (ALLOW_UPDATE_ACCOUNT_NAME=1)
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountUpdate_happyUpdateExistingNameUpdatesEnabled() throws Exception {
    String updateOwner = generateAddress("update_owner_005");

    // Create owner with an existing name (non-empty)
    AccountCapsule ownerAccount = putAccount(dbManager, updateOwner, INITIAL_BALANCE, "");
    ownerAccount.setAccountName("old_name_12345".getBytes());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);
    dbManager.getAccountIndexStore().put(ownerAccount);

    // Enable account name updates
    dbManager.getDynamicPropertiesStore().saveAllowUpdateAccountName(1);

    // Update to a new name
    AccountUpdateContract contract = AccountUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(updateOwner)))
        .setAccountName(ByteString.copyFromUtf8("new_name_67890"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_UPDATE_CONTRACT", 10)
        .caseName("happy_update_existing_name_updates_enabled")
        .caseCategory("happy")
        .description("Success updating an existing account name when ALLOW_UPDATE_ACCOUNT_NAME=1")
        .database("account")
        .database("account-index")
        .database("dynamic-properties")
        .ownerAddress(updateOwner)
        .dynamicProperty("ALLOW_UPDATE_ACCOUNT_NAME", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountUpdate update existing name: success={}", result.isSuccess());
  }

  @Test
  public void generateAccountUpdate_edgeHappyDuplicateNameUpdatesEnabledOverwritesIndex() throws Exception {
    String accountA = generateAddress("dup_account_aaa1");
    String accountB = generateAddress("dup_account_bbb1");
    String duplicateName = "duplicate_dup_1";

    // Create account A with the name
    AccountCapsule accountACapsule = putAccount(dbManager, accountA, INITIAL_BALANCE, "");
    accountACapsule.setAccountName(duplicateName.getBytes());
    dbManager.getAccountStore().put(accountACapsule.getAddress().toByteArray(), accountACapsule);
    dbManager.getAccountIndexStore().put(accountACapsule);

    // Create account B with empty name
    putAccount(dbManager, accountB, INITIAL_BALANCE, "");

    // Enable account name updates
    dbManager.getDynamicPropertiesStore().saveAllowUpdateAccountName(1);

    // Account B sets the same name as account A
    AccountUpdateContract contract = AccountUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(accountB)))
        .setAccountName(ByteString.copyFromUtf8(duplicateName))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_UPDATE_CONTRACT", 10)
        .caseName("edge_happy_duplicate_name_updates_enabled_overwrites_index")
        .caseCategory("edge")
        .description("Success with duplicate name when ALLOW_UPDATE_ACCOUNT_NAME=1, account-index points to last writer")
        .database("account")
        .database("account-index")
        .database("dynamic-properties")
        .ownerAddress(accountB)
        .dynamicProperty("ALLOW_UPDATE_ACCOUNT_NAME", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountUpdate duplicate name overwrite: success={}", result.isSuccess());
  }
}
