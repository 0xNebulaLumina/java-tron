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
import org.tron.core.capsule.DelegatedResourceAccountIndexCapsule;
import org.tron.core.capsule.DelegatedResourceCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.store.DelegationStore;
import org.tron.protos.Protocol.AccountType;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.Account.Frozen;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.Protocol.Account.AccountResource;
import org.tron.protos.contract.BalanceContract.FreezeBalanceContract;
import org.tron.protos.contract.BalanceContract.UnfreezeBalanceContract;
import org.tron.protos.contract.Common.ResourceCode;

/**
 * Generates conformance test fixtures for V1 freeze/unfreeze contracts:
 * - FreezeBalanceContract (11) - V1 Freeze
 * - UnfreezeBalanceContract (12) - V1 Unfreeze
 *
 * <p>V1 freeze requires unfreezeDelayDays = 0 (V2 disabled).
 *
 * <p>Run with: ./gradlew :framework:test --tests "FreezeV1FixtureGeneratorTest"
 * -Dconformance.output=../conformance/fixtures --dependency-verification=off
 */
public class FreezeV1FixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(FreezeV1FixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String WITNESS_ADDRESS;
  private static final long MIN_FREEZE_DURATION = 3; // 3 days

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049154";
    WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "548794500882809695a8a687866e76d4271a1abc";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("FreezeV1 Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Initialize V1 dynamic properties (unfreezeDelayDays = 0)
    initCommonDynamicPropsV1(dbManager,
        DEFAULT_BLOCK_TIMESTAMP / 1000,
        DEFAULT_BLOCK_TIMESTAMP);

    // Ensure V1 is enabled (V2 disabled)
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(0);

    // Disable new resource model (no TRON_POWER)
    dbManager.getDynamicPropertiesStore().saveAllowNewResourceModel(0);

    // No delegation
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(0);

    // Create owner account with sufficient balance
    putAccount(dbManager, OWNER_ADDRESS, INITIAL_BALANCE, "owner");

    // Create witness
    putAccount(dbManager, WITNESS_ADDRESS, INITIAL_BALANCE, "witness");
    putWitness(dbManager, WITNESS_ADDRESS, "https://witness.network", 10_000_000L);
  }

  // ==========================================================================
  // FreezeBalanceContract (11) - V1 Fixtures
  // ==========================================================================

  @Test
  public void generateFreezeBalanceV1_happyPathBandwidth() throws Exception {
    String freezeOwner = generateAddress("freeze_v1_own01");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner");

    long freezeAmount = 100 * ONE_TRX;

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("happy_path_freeze_bandwidth_v1")
        .caseCategory("happy")
        .description("V1 freeze for BANDWIDTH resource")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", 0)
        .dynamicProperty("freeze_amount", freezeAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 BANDWIDTH happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateFreezeBalanceV1_happyPathEnergy() throws Exception {
    String freezeOwner = generateAddress("freeze_v1_own02");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_energy");

    long freezeAmount = 50 * ONE_TRX;

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.ENERGY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("happy_path_freeze_energy_v1")
        .caseCategory("happy")
        .description("V1 freeze for ENERGY resource")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", 0)
        .dynamicProperty("freeze_amount", freezeAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 ENERGY happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateFreezeBalanceV1_validateFailV2Open() throws Exception {
    String freezeOwner = generateAddress("freeze_v1_own03");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_v2");

    // Enable V2 to make V1 fail
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(14);

    long freezeAmount = 100 * ONE_TRX;

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_v1_closed_when_v2_open")
        .caseCategory("validate_fail")
        .description("Fail V1 freeze when unfreezeDelayDays > 0 (V2 enabled)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .expectedError("freeze")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 V2 open: validationError={}", result.getValidationError());

    // Restore V1 mode for other tests
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(0);
  }

  @Test
  public void generateFreezeBalanceV1_validateFailFrozenBalanceLt1Trx() throws Exception {
    String freezeOwner = generateAddress("freeze_v1_own04");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_lt1");

    long freezeAmount = ONE_TRX / 2; // Less than 1 TRX

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_frozen_balance_lt_1_trx")
        .caseCategory("validate_fail")
        .description("Fail when freeze amount is less than 1 TRX")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 lt 1 TRX: validationError={}", result.getValidationError());
  }

  @Test
  public void generateFreezeBalanceV1_validateFailFrozenBalanceGtBalance() throws Exception {
    String poorOwner = generateAddress("freeze_v1_own05");
    putAccount(dbManager, poorOwner, ONE_TRX, "poor_freeze_owner"); // Only 1 TRX

    long freezeAmount = INITIAL_BALANCE; // Much more than balance

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(poorOwner)))
        .setFrozenBalance(freezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_frozen_balance_gt_balance")
        .caseCategory("validate_fail")
        .description("Fail when freeze amount exceeds account balance")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(poorOwner)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 gt balance: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // UnfreezeBalanceContract (12) - V1 Fixtures
  // ==========================================================================

  @Test
  public void generateUnfreezeBalanceV1_happyPathBandwidth() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_01");

    // Create account with expired frozen balance
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_owner");
    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    // Add expired frozen entry (expireTime <= latestBlockHeaderTimestamp)
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX)
        .setExpireTime(DEFAULT_BLOCK_TIMESTAMP - 1000) // Expired
        .build());
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("happy_path_unfreeze_bandwidth_v1")
        .caseCategory("happy")
        .description("V1 unfreeze expired BANDWIDTH frozen balance")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", 0)
        .dynamicProperty("CHANGE_DELEGATION", 0)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 BANDWIDTH happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateUnfreezeBalanceV1_validateFailNotExpired() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_02");

    // Create account with NOT expired frozen balance
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_not_expired");
    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    // Add NOT expired frozen entry
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX)
        .setExpireTime(DEFAULT_BLOCK_TIMESTAMP + 86400000) // Future - not expired
        .build());
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_not_expired")
        .caseCategory("validate_fail")
        .description("Fail when frozen balance has not expired yet")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("expire")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 not expired: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeBalanceV1_validateFailNoFrozenBalance() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_03");

    // Create account with NO frozen balance
    putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_no_frozen");

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_no_frozen_balance")
        .caseCategory("validate_fail")
        .description("Fail when there is no frozen balance to unfreeze")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("frozen")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 no frozen: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Phase 1: FreezeBalanceContract (11) - Missing Fixtures (delegation OFF)
  // ==========================================================================

  // --- Owner/address/account branches ---

  @Test
  public void generateFreezeBalanceV1_validateFailOwnerAddressInvalidEmpty() throws Exception {
    // Empty owner address - should fail with "Invalid address"
    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setFrozenBalance(100 * ONE_TRX)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty")
        .database("account")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 empty owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateFreezeBalanceV1_validateFailOwnerAccountNotExist() throws Exception {
    // Use a valid-looking address that's not in AccountStore
    String nonExistentOwner = generateAddress("non_exist_freeze_01");
    // Note: we do NOT put this address in the account store

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentOwner)))
        .setFrozenBalance(100 * ONE_TRX)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("account")
        .database("dynamic-properties")
        .expectedError("not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 account not exist: validationError={}", result.getValidationError());
  }

  // --- Frozen balance branches ---

  @Test
  public void generateFreezeBalanceV1_validateFailFrozenBalanceZero() throws Exception {
    String freezeOwner = generateAddress("freeze_v1_zero_bal");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_zero");

    // frozenBalance = 0, should fail with "frozenBalance must be positive"
    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(0)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_frozen_balance_zero")
        .caseCategory("validate_fail")
        .description("Fail when frozenBalance is zero")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .expectedError("frozenBalance must be positive")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 zero balance: validationError={}", result.getValidationError());
  }

  @Test
  public void generateFreezeBalanceV1_happyPathFrozenBalanceExact1Trx() throws Exception {
    String freezeOwner = generateAddress("freeze_v1_1trx");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_1trx");

    // Minimum allowed: exactly 1 TRX
    long freezeAmount = ONE_TRX;

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("happy_path_frozen_balance_exact_1_trx")
        .caseCategory("happy")
        .description("Freeze exactly 1 TRX (minimum allowed)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("freeze_amount", freezeAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 exact 1 TRX: success={}", result.isSuccess());
  }

  @Test
  public void generateFreezeBalanceV1_happyPathFrozenBalanceEqualAccountBalance() throws Exception {
    String freezeOwner = generateAddress("freeze_v1_all_bal");
    long accountBalance = 50 * ONE_TRX;
    putAccount(dbManager, freezeOwner, accountBalance, "freeze_owner_all");

    // Freeze entire balance
    long freezeAmount = accountBalance;

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("happy_path_frozen_balance_equal_account_balance")
        .caseCategory("happy")
        .description("Freeze entire account balance (post balance should be 0)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("freeze_amount", freezeAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 entire balance: success={}", result.isSuccess());
  }

  // --- Frozen duration branches (requires checkFrozenTime=1) ---
  // Note: checkFrozenTime defaults to 1 in Args.java, min/max frozen time both default to 3 days

  @Test
  public void generateFreezeBalanceV1_validateFailFrozenDurationTooShort() throws Exception {
    String freezeOwner = generateAddress("freeze_v1_dur_short");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_dur_short");

    // Set min/max frozen time explicitly
    dbManager.getDynamicPropertiesStore().saveMinFrozenTime(3);
    dbManager.getDynamicPropertiesStore().saveMaxFrozenTime(3);

    // Duration too short (less than min)
    long frozenDuration = 2; // Less than 3

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(100 * ONE_TRX)
        .setFrozenDuration(frozenDuration)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_frozen_duration_too_short")
        .caseCategory("validate_fail")
        .description("Fail when frozenDuration is less than minFrozenTime")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("frozenDuration", frozenDuration)
        .dynamicProperty("minFrozenTime", 3)
        .dynamicProperty("maxFrozenTime", 3)
        .expectedError("frozenDuration must be less than")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 duration too short: validationError={}", result.getValidationError());
  }

  @Test
  public void generateFreezeBalanceV1_validateFailFrozenDurationTooLong() throws Exception {
    String freezeOwner = generateAddress("freeze_v1_dur_long");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_dur_long");

    // Set min/max frozen time explicitly
    dbManager.getDynamicPropertiesStore().saveMinFrozenTime(3);
    dbManager.getDynamicPropertiesStore().saveMaxFrozenTime(3);

    // Duration too long (more than max)
    long frozenDuration = 4; // More than 3

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(100 * ONE_TRX)
        .setFrozenDuration(frozenDuration)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_frozen_duration_too_long")
        .caseCategory("validate_fail")
        .description("Fail when frozenDuration exceeds maxFrozenTime")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("frozenDuration", frozenDuration)
        .dynamicProperty("minFrozenTime", 3)
        .dynamicProperty("maxFrozenTime", 3)
        .expectedError("frozenDuration must be less than")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 duration too long: validationError={}", result.getValidationError());
  }

  // --- Pre-state guard (frozenCount must be 0 or 1) ---

  @Test
  public void generateFreezeBalanceV1_validateFailFrozenCountNot0Or1() throws Exception {
    String freezeOwner = generateAddress("freeze_v1_cnt_bad");

    // Create account with 2 frozen entries (illegal state)
    AccountCapsule account = putAccount(dbManager, freezeOwner, INITIAL_BALANCE,
        "freeze_owner_cnt_bad");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    // Add two frozen entries
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(50 * ONE_TRX)
        .setExpireTime(DEFAULT_BLOCK_TIMESTAMP + 86400000)
        .build());
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(30 * ONE_TRX)
        .setExpireTime(DEFAULT_BLOCK_TIMESTAMP + 86400000)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(10 * ONE_TRX)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_frozen_count_not_0_or_1")
        .caseCategory("validate_fail")
        .description("Fail when account has 2 frozen entries (invalid state)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .expectedError("frozenCount must be 0 or 1")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 frozenCount bad: validationError={}", result.getValidationError());
  }

  // --- Resource code validation ---

  @Test
  public void generateFreezeBalanceV1_validateFailResourceTronPowerWhenNewResourceModelOff()
      throws Exception {
    String freezeOwner = generateAddress("freeze_v1_tp_off");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_tp");

    // Ensure ALLOW_NEW_RESOURCE_MODEL = 0 (already set in initializeTestData)
    dbManager.getDynamicPropertiesStore().saveAllowNewResourceModel(0);

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(100 * ONE_TRX)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.TRON_POWER)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_resource_tron_power_when_new_resource_model_off")
        .caseCategory("validate_fail")
        .description("Fail TRON_POWER resource when allowNewResourceModel=0")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .expectedError("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 TRON_POWER off: validationError={}", result.getValidationError());
  }

  // --- Receiver set while delegation is OFF (edge case) ---

  @Test
  public void generateFreezeBalanceV1_edgeReceiverAddressIgnoredWhenDelegationOff()
      throws Exception {
    String freezeOwner = generateAddress("freeze_v1_rcv_off");
    String receiverAddr = generateAddress("freeze_v1_rcv_tgt");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_rcv");
    putAccount(dbManager, receiverAddr, INITIAL_BALANCE, "receiver_ignored");

    // Ensure delegation is OFF
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(0);

    long freezeAmount = 100 * ONE_TRX;

    // Set receiverAddress even though delegation is OFF
    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(receiverAddr)))
        .setFrozenBalance(freezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("edge_receiver_address_ignored_when_delegation_off")
        .caseCategory("edge")
        .description("Receiver address is ignored when delegation is OFF (self-freeze)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("CHANGE_DELEGATION", 0)
        .dynamicProperty("receiver_address_set_but_ignored", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 receiver ignored: success={}", result.isSuccess());
  }

  // --- Multi-freeze execution semantics ---

  @Test
  public void generateFreezeBalanceV1_edgeFreezeBandwidthTwiceAccumulates() throws Exception {
    String freezeOwner = generateAddress("freeze_v1_multi");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_multi");

    long firstFreezeAmount = 50 * ONE_TRX;

    // First freeze
    FreezeBalanceContract firstContract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(firstFreezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule firstTrxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, firstContract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata firstMetadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("edge_freeze_bandwidth_first_of_two")
        .caseCategory("edge")
        .description("First freeze in multi-freeze sequence")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("freeze_amount", firstFreezeAmount)
        .build();

    FixtureGenerator.FixtureResult firstResult = generator.generate(firstTrxCap, blockCap,
        firstMetadata);
    log.info("FreezeV1 multi first: success={}", firstResult.isSuccess());

    // Second freeze (accumulates on top of first)
    long secondFreezeAmount = 30 * ONE_TRX;

    FreezeBalanceContract secondContract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(secondFreezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule secondTrxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, secondContract);

    BlockCapsule blockCap2 = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata secondMetadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("edge_freeze_bandwidth_twice_accumulates")
        .caseCategory("edge")
        .description("Second freeze accumulates on existing frozen balance")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("first_freeze_amount", firstFreezeAmount)
        .dynamicProperty("second_freeze_amount", secondFreezeAmount)
        .dynamicProperty("expected_total_frozen", firstFreezeAmount + secondFreezeAmount)
        .build();

    FixtureGenerator.FixtureResult secondResult = generator.generate(secondTrxCap, blockCap2,
        secondMetadata);
    log.info("FreezeV1 multi second: success={}", secondResult.isSuccess());
  }

  // ==========================================================================
  // Phase 3: UnfreezeBalanceContract (12) - Missing Fixtures (delegation OFF)
  // ==========================================================================

  // --- Owner/address/account branches ---

  @Test
  public void generateUnfreezeBalanceV1_validateFailOwnerAddressInvalidEmpty() throws Exception {
    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 empty owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeBalanceV1_validateFailOwnerAccountNotExist() throws Exception {
    String nonExistentOwner = generateAddress("non_exist_unfreeze_01");
    // Note: we do NOT put this address in the account store

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentOwner)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .expectedError("does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 account not exist: validationError={}", result.getValidationError());
  }

  // --- BANDWIDTH expiration boundary ---

  @Test
  public void generateUnfreezeBalanceV1_edgeExpireTimeEqualsNowIsUnfreezable() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_edge_exp");

    // Create account with frozen balance where expireTime == latestBlockHeaderTimestamp
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_edge_expire");
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX)
        .setExpireTime(now) // Exactly equals now (boundary)
        .build());
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("edge_expire_time_equals_now_is_unfreezable_bandwidth")
        .caseCategory("edge")
        .description("Unfreeze succeeds when expireTime == now (boundary condition, uses <=)")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 expire==now: success={}", result.isSuccess());
  }

  // --- Multiple frozen entries edge cases ---

  @Test
  public void generateUnfreezeBalanceV1_edgePartialUnfreezeOneExpiredOneNot() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_partial");

    // Create account with two frozen entries: one expired, one not
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_partial");
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    long expiredAmount = 50 * ONE_TRX;
    long notExpiredAmount = 30 * ONE_TRX;

    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    // First entry: expired
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(expiredAmount)
        .setExpireTime(now - 1000) // Expired
        .build());
    // Second entry: not expired
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(notExpiredAmount)
        .setExpireTime(now + 86400000) // Future
        .build());
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("edge_partial_unfreeze_one_expired_one_not")
        .caseCategory("edge")
        .description("Partial unfreeze: only expired entry is unfrozen, not-expired remains")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("expired_amount", expiredAmount)
        .dynamicProperty("not_expired_amount", notExpiredAmount)
        .dynamicProperty("expected_unfreeze_amount", expiredAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 partial: success={}", result.isSuccess());
  }

  @Test
  public void generateUnfreezeBalanceV1_edgeMultipleExpiredEntriesUnfreezeSum() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_multi_exp");

    // Create account with two expired frozen entries
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_multi_exp");
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    long firstAmount = 40 * ONE_TRX;
    long secondAmount = 60 * ONE_TRX;

    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    // First expired entry
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(firstAmount)
        .setExpireTime(now - 2000) // Expired
        .build());
    // Second expired entry
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(secondAmount)
        .setExpireTime(now - 1000) // Also expired
        .build());
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("edge_multiple_expired_entries_unfreeze_sum")
        .caseCategory("edge")
        .description("Multiple expired entries: unfreeze amount equals the sum")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("first_expired_amount", firstAmount)
        .dynamicProperty("second_expired_amount", secondAmount)
        .dynamicProperty("expected_unfreeze_amount", firstAmount + secondAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 multi expired: success={}", result.isSuccess());
  }

  // --- ENERGY resource coverage ---

  @Test
  public void generateUnfreezeBalanceV1_happyPathUnfreezeEnergyV1() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_energy_ok");

    // Create account with expired frozen ENERGY balance
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_energy_ok");
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    AccountResource.Builder resourceBuilder = builder.getAccountResourceBuilder();
    resourceBuilder.setFrozenBalanceForEnergy(Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX)
        .setExpireTime(now - 1000) // Expired
        .build());
    builder.setAccountResource(resourceBuilder);
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.ENERGY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("happy_path_unfreeze_energy_v1")
        .caseCategory("happy")
        .description("V1 unfreeze expired ENERGY frozen balance")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 ENERGY happy: success={}", result.isSuccess());
  }

  @Test
  public void generateUnfreezeBalanceV1_validateFailUnfreezeEnergyNotExpired() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_energy_ne");

    // Create account with NOT expired frozen ENERGY balance
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_energy_not_exp");
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    AccountResource.Builder resourceBuilder = builder.getAccountResourceBuilder();
    resourceBuilder.setFrozenBalanceForEnergy(Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX)
        .setExpireTime(now + 86400000) // Future
        .build());
    builder.setAccountResource(resourceBuilder);
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.ENERGY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_unfreeze_energy_not_expired")
        .caseCategory("validate_fail")
        .description("Fail when ENERGY frozen balance has not expired yet")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("It's not time to unfreeze(Energy).")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 ENERGY not expired: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeBalanceV1_validateFailUnfreezeEnergyNoFrozen() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_energy_nf");

    // Create account with NO frozen ENERGY balance
    putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_energy_no_frozen");

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.ENERGY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_unfreeze_energy_no_frozen")
        .caseCategory("validate_fail")
        .description("Fail when there is no frozen ENERGY balance")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("no frozenBalance(Energy)")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 ENERGY no frozen: validationError={}", result.getValidationError());
  }

  // --- Invalid resource code ---

  @Test
  public void generateUnfreezeBalanceV1_validateFailUnfreezeTronPowerWhenNewResourceModelOff()
      throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_tp_off");
    putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_tp_off");

    // Ensure ALLOW_NEW_RESOURCE_MODEL = 0
    dbManager.getDynamicPropertiesStore().saveAllowNewResourceModel(0);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.TRON_POWER)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_unfreeze_tron_power_when_new_resource_model_off")
        .caseCategory("validate_fail")
        .description("Fail TRON_POWER resource when allowNewResourceModel=0")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 TRON_POWER off: validationError={}", result.getValidationError());
  }

  // --- Receiver set while delegation is OFF (edge: ignored by Java-tron) ---

  @Test
  public void generateUnfreezeBalanceV1_edgeReceiverAddressIgnoredWhenDelegationOff()
      throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_rcv_off");
    String receiverAddr = generateAddress("unfreeze_v1_rcv_tgt");

    // Create account with expired frozen balance
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_rcv_off");
    putAccount(dbManager, receiverAddr, INITIAL_BALANCE, "receiver_ignored");

    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX)
        .setExpireTime(now - 1000) // Expired
        .build());
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    // Ensure delegation is OFF
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(0);

    // Set receiverAddress even though delegation is OFF
    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(receiverAddr)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("edge_receiver_address_ignored_when_delegation_off")
        .caseCategory("edge")
        .description("Receiver address is ignored when delegation is OFF (self-unfreeze)")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("CHANGE_DELEGATION", 0)
        .dynamicProperty("receiver_address_set_but_ignored", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 receiver ignored: success={}", result.isSuccess());
  }

  // --- V2-open compatibility (important cross-impl behavior) ---

  @Test
  public void generateUnfreezeBalanceV1_edgeUnfreezeV1SucceedsWhenV2Open() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v1_v2open");

    // Create account with expired V1 frozen balance
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_v2_compat");
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX)
        .setExpireTime(now - 1000) // Expired legacy V1 entry
        .build());
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    // Enable V2 (unfreezeDelayDays > 0)
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(14);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("edge_unfreeze_v1_succeeds_when_v2_open")
        .caseCategory("edge")
        .description("V1 unfreeze succeeds even when V2 is open (legacy frozen entry)")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", 14)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 with V2 open: success={}", result.isSuccess());

    // Restore V1 mode for other tests
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(0);
  }

  // ==========================================================================
  // Phase 2: FreezeBalanceContract (11) - Delegation-Enabled Fixtures
  // ==========================================================================

  /**
   * Helper to enable delegation mode for Phase 2 and Phase 4 tests.
   */
  private void enableDelegationMode() {
    // Enable delegation
    dbManager.getDynamicPropertiesStore().saveAllowDelegateResource(1);
    // Optionally enable change delegation for reward delegation side effects
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(1);
  }

  /**
   * Helper to restore delegation-off mode after tests.
   */
  private void disableDelegationMode() {
    dbManager.getDynamicPropertiesStore().saveAllowDelegateResource(0);
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(0);
  }

  // --- Phase 2: Happy delegation paths ---

  @Test
  public void generateFreezeBalanceV1_happyPathDelegateFreezeBandwidth() throws Exception {
    enableDelegationMode();

    String freezeOwner = generateAddress("freeze_del_bw_own");
    String receiverAddr = generateAddress("freeze_del_bw_rcv");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_delegator_bw");
    putAccount(dbManager, receiverAddr, INITIAL_BALANCE, "freeze_receiver_bw");

    long freezeAmount = 100 * ONE_TRX;

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(receiverAddr)))
        .setFrozenBalance(freezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("happy_path_delegate_freeze_bandwidth")
        .caseCategory("happy")
        .description("Delegated freeze for BANDWIDTH resource")
        .database("account")
        .database("dynamic-properties")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .ownerAddress(freezeOwner)
        .dynamicProperty("ALLOW_DELEGATE_RESOURCE", 1)
        .dynamicProperty("freeze_amount", freezeAmount)
        .dynamicProperty("receiver_address", receiverAddr)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 delegate BANDWIDTH: success={}", result.isSuccess());

    disableDelegationMode();
  }

  @Test
  public void generateFreezeBalanceV1_happyPathDelegateFreezeEnergy() throws Exception {
    enableDelegationMode();

    String freezeOwner = generateAddress("freeze_del_en_own");
    String receiverAddr = generateAddress("freeze_del_en_rcv");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_delegator_en");
    putAccount(dbManager, receiverAddr, INITIAL_BALANCE, "freeze_receiver_en");

    long freezeAmount = 80 * ONE_TRX;

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(receiverAddr)))
        .setFrozenBalance(freezeAmount)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.ENERGY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("happy_path_delegate_freeze_energy")
        .caseCategory("happy")
        .description("Delegated freeze for ENERGY resource")
        .database("account")
        .database("dynamic-properties")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .ownerAddress(freezeOwner)
        .dynamicProperty("ALLOW_DELEGATE_RESOURCE", 1)
        .dynamicProperty("freeze_amount", freezeAmount)
        .dynamicProperty("receiver_address", receiverAddr)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 delegate ENERGY: success={}", result.isSuccess());

    disableDelegationMode();
  }

  // --- Phase 2: Delegation validation failures ---

  @Test
  public void generateFreezeBalanceV1_validateFailReceiverSameAsOwner() throws Exception {
    enableDelegationMode();

    String freezeOwner = generateAddress("freeze_del_same");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_same_owner");

    // Receiver is the same as owner
    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(100 * ONE_TRX)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_receiver_same_as_owner")
        .caseCategory("validate_fail")
        .description("Fail when receiver address is the same as owner address")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .expectedError("receiverAddress must not be the same as ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 receiver=owner: validationError={}", result.getValidationError());

    disableDelegationMode();
  }

  @Test
  public void generateFreezeBalanceV1_validateFailReceiverInvalidAddress() throws Exception {
    enableDelegationMode();

    String freezeOwner = generateAddress("freeze_del_inv_rcv");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_invalid_rcv");

    // Invalid receiver address (wrong length)
    byte[] invalidReceiver = new byte[10]; // Invalid length

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(invalidReceiver))
        .setFrozenBalance(100 * ONE_TRX)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_receiver_invalid_address")
        .caseCategory("validate_fail")
        .description("Fail when receiver address is invalid")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .expectedError("Invalid receiverAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 invalid receiver: validationError={}", result.getValidationError());

    disableDelegationMode();
  }

  @Test
  public void generateFreezeBalanceV1_validateFailReceiverAccountNotExist() throws Exception {
    enableDelegationMode();

    String freezeOwner = generateAddress("freeze_del_no_rcv");
    String nonExistentReceiver = generateAddress("freeze_del_no_rcv_tgt");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_no_rcv_owner");
    // Note: receiver account is NOT created

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentReceiver)))
        .setFrozenBalance(100 * ONE_TRX)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_receiver_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when receiver account does not exist")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .expectedError("not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 receiver not exist: validationError={}", result.getValidationError());

    disableDelegationMode();
  }

  @Test
  public void generateFreezeBalanceV1_validateFailDelegateToContractAddress() throws Exception {
    enableDelegationMode();
    // Enable TVM Constantinople for contract address check
    dbManager.getDynamicPropertiesStore().saveAllowTvmConstantinople(1);

    String freezeOwner = generateAddress("freeze_del_contract");
    String contractAddr = generateAddress("freeze_del_contract_rcv");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_contract_owner");

    // Create a contract account
    AccountCapsule contractAccount = new AccountCapsule(
        ByteString.copyFromUtf8("contract_receiver"),
        ByteString.copyFrom(ByteArray.fromHexString(contractAddr)),
        AccountType.Contract,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(contractAccount.getAddress().toByteArray(), contractAccount);

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(contractAddr)))
        .setFrozenBalance(100 * ONE_TRX)
        .setFrozenDuration(MIN_FREEZE_DURATION)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_CONTRACT", 11)
        .caseName("validate_fail_delegate_to_contract_address")
        .caseCategory("validate_fail")
        .description("Fail when delegating resources to a contract address")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("ALLOW_TVM_CONSTANTINOPLE", 1)
        .expectedError("Do not allow delegate resources to contract addresses")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV1 delegate to contract: validationError={}", result.getValidationError());

    // Restore settings
    dbManager.getDynamicPropertiesStore().saveAllowTvmConstantinople(0);
    disableDelegationMode();
  }

  // ==========================================================================
  // Phase 4: UnfreezeBalanceContract (12) - Delegation-Enabled Fixtures
  // ==========================================================================

  /**
   * Helper to seed a DelegatedResource entry for unfreeze tests.
   */
  private void seedDelegatedResource(String ownerAddr, String receiverAddr,
      long bandwidthBalance, long bandwidthExpireTime,
      long energyBalance, long energyExpireTime) {

    byte[] ownerBytes = ByteArray.fromHexString(ownerAddr);
    byte[] receiverBytes = ByteArray.fromHexString(receiverAddr);
    byte[] key = DelegatedResourceCapsule.createDbKey(ownerBytes, receiverBytes);

    DelegatedResourceCapsule delegatedResource = new DelegatedResourceCapsule(
        ByteString.copyFrom(ownerBytes),
        ByteString.copyFrom(receiverBytes));

    if (bandwidthBalance > 0) {
      delegatedResource.setFrozenBalanceForBandwidth(bandwidthBalance, bandwidthExpireTime);
    }
    if (energyBalance > 0) {
      delegatedResource.setFrozenBalanceForEnergy(energyBalance, energyExpireTime);
    }

    dbManager.getDelegatedResourceStore().put(key, delegatedResource);

    // Also update the account index
    DelegatedResourceAccountIndexCapsule ownerIndex =
        dbManager.getDelegatedResourceAccountIndexStore().get(ownerBytes);
    if (ownerIndex == null) {
      ownerIndex = new DelegatedResourceAccountIndexCapsule(ByteString.copyFrom(ownerBytes));
    }
    if (!ownerIndex.getToAccountsList().contains(ByteString.copyFrom(receiverBytes))) {
      ownerIndex.addToAccount(ByteString.copyFrom(receiverBytes));
    }
    dbManager.getDelegatedResourceAccountIndexStore().put(ownerBytes, ownerIndex);

    DelegatedResourceAccountIndexCapsule receiverIndex =
        dbManager.getDelegatedResourceAccountIndexStore().get(receiverBytes);
    if (receiverIndex == null) {
      receiverIndex = new DelegatedResourceAccountIndexCapsule(ByteString.copyFrom(receiverBytes));
    }
    if (!receiverIndex.getFromAccountsList().contains(ByteString.copyFrom(ownerBytes))) {
      receiverIndex.addFromAccount(ByteString.copyFrom(ownerBytes));
    }
    dbManager.getDelegatedResourceAccountIndexStore().put(receiverBytes, receiverIndex);
  }

  // --- Phase 4: Delegated unfreeze happy paths ---

  @Test
  public void generateUnfreezeBalanceV1_happyPathUnfreezeDelegatedBandwidth() throws Exception {
    enableDelegationMode();

    String unfreezeOwner = generateAddress("unfreeze_del_bw_own");
    String receiverAddr = generateAddress("unfreeze_del_bw_rcv");

    // Create accounts
    AccountCapsule ownerAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_del_bw_owner");
    AccountCapsule receiverAccount = putAccount(dbManager, receiverAddr, INITIAL_BALANCE,
        "unfreeze_del_bw_receiver");

    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long delegatedAmount = 100 * ONE_TRX;

    // Set up owner's delegated frozen balance
    Protocol.Account.Builder ownerBuilder = ownerAccount.getInstance().toBuilder();
    ownerBuilder.setDelegatedFrozenBalanceForBandwidth(delegatedAmount);
    ownerAccount = new AccountCapsule(ownerBuilder.build());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Set up receiver's acquired delegated frozen balance
    Protocol.Account.Builder receiverBuilder = receiverAccount.getInstance().toBuilder();
    receiverBuilder.setAcquiredDelegatedFrozenBalanceForBandwidth(delegatedAmount);
    receiverAccount = new AccountCapsule(receiverBuilder.build());
    dbManager.getAccountStore().put(receiverAccount.getAddress().toByteArray(), receiverAccount);

    // Seed the delegated resource (expired)
    seedDelegatedResource(unfreezeOwner, receiverAddr,
        delegatedAmount, now - 1000, // BANDWIDTH expired
        0, 0); // No ENERGY

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(receiverAddr)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("happy_path_unfreeze_delegated_bandwidth")
        .caseCategory("happy")
        .description("Delegated unfreeze for expired BANDWIDTH delegation")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("ALLOW_DELEGATE_RESOURCE", 1)
        .dynamicProperty("delegated_amount", delegatedAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 delegated BANDWIDTH: success={}", result.isSuccess());

    disableDelegationMode();
  }

  @Test
  public void generateUnfreezeBalanceV1_happyPathUnfreezeDelegatedEnergy() throws Exception {
    enableDelegationMode();

    String unfreezeOwner = generateAddress("unfreeze_del_en_own");
    String receiverAddr = generateAddress("unfreeze_del_en_rcv");

    // Create accounts
    AccountCapsule ownerAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_del_en_owner");
    AccountCapsule receiverAccount = putAccount(dbManager, receiverAddr, INITIAL_BALANCE,
        "unfreeze_del_en_receiver");

    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long delegatedAmount = 80 * ONE_TRX;

    // Set up owner's delegated frozen balance for energy
    Protocol.Account.Builder ownerBuilder = ownerAccount.getInstance().toBuilder();
    Protocol.Account.AccountResource.Builder ownerResourceBuilder =
        ownerBuilder.getAccountResource().toBuilder();
    ownerResourceBuilder.setDelegatedFrozenBalanceForEnergy(delegatedAmount);
    ownerBuilder.setAccountResource(ownerResourceBuilder.build());
    ownerAccount = new AccountCapsule(ownerBuilder.build());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Set up receiver's acquired delegated frozen balance for energy
    Protocol.Account.Builder receiverBuilder = receiverAccount.getInstance().toBuilder();
    Protocol.Account.AccountResource.Builder receiverResourceBuilder =
        receiverBuilder.getAccountResource().toBuilder();
    receiverResourceBuilder.setAcquiredDelegatedFrozenBalanceForEnergy(delegatedAmount);
    receiverBuilder.setAccountResource(receiverResourceBuilder.build());
    receiverAccount = new AccountCapsule(receiverBuilder.build());
    dbManager.getAccountStore().put(receiverAccount.getAddress().toByteArray(), receiverAccount);

    // Seed the delegated resource (expired)
    seedDelegatedResource(unfreezeOwner, receiverAddr,
        0, 0, // No BANDWIDTH
        delegatedAmount, now - 1000); // ENERGY expired

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(receiverAddr)))
        .setResource(ResourceCode.ENERGY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("happy_path_unfreeze_delegated_energy")
        .caseCategory("happy")
        .description("Delegated unfreeze for expired ENERGY delegation")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("ALLOW_DELEGATE_RESOURCE", 1)
        .dynamicProperty("delegated_amount", delegatedAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 delegated ENERGY: success={}", result.isSuccess());

    disableDelegationMode();
  }

  // --- Phase 4: Delegated unfreeze validation failures ---

  @Test
  public void generateUnfreezeBalanceV1_validateFailDelegatedReceiverSameAsOwner()
      throws Exception {
    enableDelegationMode();

    String unfreezeOwner = generateAddress("unfreeze_del_same");
    putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_same_owner");

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_delegated_receiver_same_as_owner")
        .caseCategory("validate_fail")
        .description("Fail when receiver address is the same as owner for delegated unfreeze")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("receiverAddress must not be the same as ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 del receiver=owner: validationError={}", result.getValidationError());

    disableDelegationMode();
  }

  @Test
  public void generateUnfreezeBalanceV1_validateFailDelegatedReceiverInvalidAddress()
      throws Exception {
    enableDelegationMode();

    String unfreezeOwner = generateAddress("unfreeze_del_inv");
    putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_invalid_rcv");

    byte[] invalidReceiver = new byte[10]; // Invalid length

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(invalidReceiver))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_delegated_receiver_invalid_address")
        .caseCategory("validate_fail")
        .description("Fail when receiver address is invalid for delegated unfreeze")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("Invalid receiverAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 del invalid receiver: validationError={}", result.getValidationError());

    disableDelegationMode();
  }

  @Test
  public void generateUnfreezeBalanceV1_validateFailDelegatedResourceNotExist() throws Exception {
    enableDelegationMode();

    String unfreezeOwner = generateAddress("unfreeze_del_no_res");
    String receiverAddr = generateAddress("unfreeze_del_no_res_rcv");
    putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_no_del_owner");
    putAccount(dbManager, receiverAddr, INITIAL_BALANCE, "unfreeze_no_del_rcv");
    // Note: No DelegatedResource entry is seeded

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(receiverAddr)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_delegated_resource_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when delegated resource entry does not exist")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("delegated Resource does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 del not exist: validationError={}", result.getValidationError());

    disableDelegationMode();
  }

  @Test
  public void generateUnfreezeBalanceV1_validateFailNoDelegatedFrozenBalance() throws Exception {
    enableDelegationMode();

    String unfreezeOwner = generateAddress("unfreeze_del_no_bal");
    String receiverAddr = generateAddress("unfreeze_del_no_bal_rcv");
    putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_no_bal_owner");
    putAccount(dbManager, receiverAddr, INITIAL_BALANCE, "unfreeze_no_bal_rcv");

    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    // Seed delegated resource with 0 bandwidth balance (but has energy)
    seedDelegatedResource(unfreezeOwner, receiverAddr,
        0, 0, // BANDWIDTH is 0
        100 * ONE_TRX, now - 1000); // Has ENERGY

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(receiverAddr)))
        .setResource(ResourceCode.BANDWIDTH) // Try to unfreeze BANDWIDTH which is 0
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_no_delegated_frozen_balance_bandwidth")
        .caseCategory("validate_fail")
        .description("Fail when delegated frozen balance for BANDWIDTH is 0")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .database("DelegatedResource")
        .ownerAddress(unfreezeOwner)
        .expectedError("no delegatedFrozenBalance(BANDWIDTH)")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 del no BANDWIDTH: validationError={}", result.getValidationError());

    disableDelegationMode();
  }

  @Test
  public void generateUnfreezeBalanceV1_validateFailNoDelegatedFrozenBalanceEnergy()
      throws Exception {
    enableDelegationMode();

    String unfreezeOwner = generateAddress("unfreeze_del_no_en");
    String receiverAddr = generateAddress("unfreeze_del_no_en_rcv");
    putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_no_en_owner");
    putAccount(dbManager, receiverAddr, INITIAL_BALANCE, "unfreeze_no_en_rcv");

    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    // Seed delegated resource with 0 energy balance (but has bandwidth)
    seedDelegatedResource(unfreezeOwner, receiverAddr,
        100 * ONE_TRX, now - 1000, // Has BANDWIDTH
        0, 0); // ENERGY is 0

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(receiverAddr)))
        .setResource(ResourceCode.ENERGY) // Try to unfreeze ENERGY which is 0
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_no_delegated_frozen_balance_energy")
        .caseCategory("validate_fail")
        .description("Fail when delegated frozen balance for ENERGY is 0")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .database("DelegatedResource")
        .ownerAddress(unfreezeOwner)
        .expectedError("no delegateFrozenBalance(Energy)")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 del no ENERGY: validationError={}", result.getValidationError());

    disableDelegationMode();
  }

  @Test
  public void generateUnfreezeBalanceV1_validateFailDelegatedNotExpired() throws Exception {
    enableDelegationMode();

    String unfreezeOwner = generateAddress("unfreeze_del_not_exp");
    String receiverAddr = generateAddress("unfreeze_del_not_exp_rcv");

    // Create accounts
    AccountCapsule ownerAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_notexp_owner");
    AccountCapsule receiverAccount = putAccount(dbManager, receiverAddr, INITIAL_BALANCE,
        "unfreeze_notexp_rcv");

    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long delegatedAmount = 100 * ONE_TRX;

    // Set up owner's delegated frozen balance
    Protocol.Account.Builder ownerBuilder = ownerAccount.getInstance().toBuilder();
    ownerBuilder.setDelegatedFrozenBalanceForBandwidth(delegatedAmount);
    ownerAccount = new AccountCapsule(ownerBuilder.build());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Set up receiver's acquired delegated frozen balance
    Protocol.Account.Builder receiverBuilder = receiverAccount.getInstance().toBuilder();
    receiverBuilder.setAcquiredDelegatedFrozenBalanceForBandwidth(delegatedAmount);
    receiverAccount = new AccountCapsule(receiverBuilder.build());
    dbManager.getAccountStore().put(receiverAccount.getAddress().toByteArray(), receiverAccount);

    // Seed delegated resource (NOT expired - future expireTime)
    seedDelegatedResource(unfreezeOwner, receiverAddr,
        delegatedAmount, now + 86400000, // BANDWIDTH NOT expired (future)
        0, 0);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(receiverAddr)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("validate_fail_delegated_not_expired")
        .caseCategory("validate_fail")
        .description("Fail when delegated resource has not expired yet")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .database("DelegatedResource")
        .ownerAddress(unfreezeOwner)
        .expectedError("It's not time to unfreeze.")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 del not expired: validationError={}", result.getValidationError());

    disableDelegationMode();
  }

  // ==========================================================================
  // Phase 5: UnfreezeBalanceContract (12) - Parity Feature Fixtures
  //          (withdrawReward, weight clamping, delegate optimization)
  // ==========================================================================

  /**
   * Fixture: unfreeze with CHANGE_DELEGATION=1 triggers withdrawReward(),
   * which should add delegation reward to account.allowance.
   *
   * <p>Setup: owner has votes for a witness, delegation store has reward data
   * for the owner's voting cycle, so withdrawReward produces a non-zero reward.
   */
  @Test
  public void generateUnfreezeBalanceV1_edgeWithdrawRewardUpdatesAllowance() throws Exception {
    // Enable delegation for withdrawReward path
    enableDelegationMode();
    dbManager.getDynamicPropertiesStore().saveAllowNewReward(1);

    String unfreezeOwner = generateAddress("unfreeze_v1_wr_ok");
    String witnessAddr = WITNESS_ADDRESS;

    // Create owner account with expired frozen balance AND votes
    AccountCapsule ownerAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_wr_owner");
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    Protocol.Account.Builder builder = ownerAccount.getInstance().toBuilder();
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX)
        .setExpireTime(now - 1000) // Expired
        .build());
    // Add a vote for the witness
    builder.addVotes(Protocol.Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(witnessAddr)))
        .setVoteCount(100)
        .build());
    ownerAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Set up delegation store: cycles and rewards
    byte[] ownerBytes = ByteArray.fromHexString(unfreezeOwner);
    byte[] witnessBytes = ByteArray.fromHexString(witnessAddr);
    DelegationStore delegationStore = dbManager.getDelegationStore();

    // Current cycle = 10, begin = 5, end = 6
    dbManager.getDynamicPropertiesStore().saveCurrentCycleNumber(10);
    delegationStore.setBeginCycle(ownerBytes, 5);
    delegationStore.setEndCycle(ownerBytes, 6);

    // Seed accountVote snapshot at cycle 5 (the "latest cycle" path in withdrawReward)
    delegationStore.setAccountVote(5, ownerBytes, ownerAccount);

    // Seed reward and vote count for cycle 5 so computeReward returns non-zero
    long witnessReward = 1000 * ONE_TRX; // Total reward for the witness in cycle 5
    long witnessVoteCount = 1000; // Total votes for the witness in cycle 5
    delegationStore.addReward(5, witnessBytes, witnessReward);
    delegationStore.setWitnessVote(5, witnessBytes, witnessVoteCount);

    // Set NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE to Long.MAX_VALUE (use old algorithm)
    // to use the simple per-cycle reward calculation
    dbManager.getDynamicPropertiesStore().put(
        "NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE".getBytes(),
        new org.tron.core.capsule.BytesCapsule(ByteArray.fromLong(Long.MAX_VALUE)));

    // Also set total net weight > 0 for weight delta
    dbManager.getDynamicPropertiesStore().saveTotalNetWeight(100);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("edge_withdraw_reward_updates_allowance")
        .caseCategory("edge")
        .description("Unfreeze with CHANGE_DELEGATION=1 triggers withdrawReward, "
            + "adding delegation reward to account.allowance")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .database("delegation")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("CHANGE_DELEGATION", 1)
        .dynamicProperty("ALLOW_NEW_REWARD", 1)
        .dynamicProperty("current_cycle", 10)
        .dynamicProperty("begin_cycle", 5)
        .dynamicProperty("end_cycle", 6)
        .dynamicProperty("witness_reward_cycle_5", witnessReward)
        .dynamicProperty("witness_vote_count_cycle_5", witnessVoteCount)
        .dynamicProperty("owner_vote_count", 100)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 withdrawReward: success={}", result.isSuccess());

    // Restore
    dbManager.getDynamicPropertiesStore().saveAllowNewReward(0);
    disableDelegationMode();
  }

  /**
   * Fixture: unfreeze with ALLOW_NEW_REWARD=1 exercises weight clamping.
   *
   * <p>When total weight would go negative after subtracting the unfrozen amount,
   * Java clamps to max(0, newValue). This fixture sets totalNetWeight to a small
   * value so that the unfreeze amount exceeds it.
   */
  @Test
  public void generateUnfreezeBalanceV1_edgeWeightClampingWithAllowNewReward() throws Exception {
    dbManager.getDynamicPropertiesStore().saveAllowNewReward(1);

    String unfreezeOwner = generateAddress("unfreeze_v1_wclamp");

    // Create account with expired frozen balance larger than total net weight
    AccountCapsule ownerAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_wclamp_owner");
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    long freezeAmount = 200 * ONE_TRX;

    Protocol.Account.Builder builder = ownerAccount.getInstance().toBuilder();
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(freezeAmount)
        .setExpireTime(now - 1000) // Expired
        .build());
    ownerAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Set totalNetWeight to a small value (less than freezeAmount / ONE_TRX)
    // so after subtracting the unfreeze weight delta, it would go negative
    // and should be clamped to 0
    dbManager.getDynamicPropertiesStore().saveTotalNetWeight(50); // 50 < 200

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("edge_weight_clamping_with_allow_new_reward")
        .caseCategory("edge")
        .description("Unfreeze with ALLOW_NEW_REWARD=1 clamps totalNetWeight to max(0, newValue) "
            + "when unfreeze amount exceeds current weight")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("ALLOW_NEW_REWARD", 1)
        .dynamicProperty("total_net_weight_before", 50)
        .dynamicProperty("freeze_amount", freezeAmount)
        .dynamicProperty("expected_total_net_weight_after", 0) // clamped to 0
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 weight clamping: success={}", result.isSuccess());

    // Restore
    dbManager.getDynamicPropertiesStore().saveAllowNewReward(0);
    dbManager.getDynamicPropertiesStore().saveTotalNetWeight(0);
  }

  /**
   * Fixture: delegated unfreeze with ALLOW_DELEGATE_OPTIMIZATION=1.
   *
   * <p>When this flag is enabled, Java calls convert(owner) + convert(receiver)
   * to migrate legacy blob-style DelegatedResourceAccountIndex entries to
   * prefixed keys, then calls unDelegate(owner, receiver) to delete the
   * prefixed key entries.
   */
  @Test
  public void generateUnfreezeBalanceV1_edgeDelegatedUnfreezeWithOptimization() throws Exception {
    enableDelegationMode();
    dbManager.getDynamicPropertiesStore().saveAllowDelegateOptimization(1);

    String unfreezeOwner = generateAddress("unfreeze_del_opt_own");
    String receiverAddr = generateAddress("unfreeze_del_opt_rcv");

    // Create accounts
    AccountCapsule ownerAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE,
        "unfreeze_opt_owner");
    AccountCapsule receiverAccount = putAccount(dbManager, receiverAddr, INITIAL_BALANCE,
        "unfreeze_opt_receiver");

    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long delegatedAmount = 100 * ONE_TRX;

    // Set up owner's delegated frozen balance for bandwidth
    Protocol.Account.Builder ownerBuilder = ownerAccount.getInstance().toBuilder();
    ownerBuilder.setDelegatedFrozenBalanceForBandwidth(delegatedAmount);
    ownerAccount = new AccountCapsule(ownerBuilder.build());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Set up receiver's acquired delegated frozen balance
    Protocol.Account.Builder receiverBuilder = receiverAccount.getInstance().toBuilder();
    receiverBuilder.setAcquiredDelegatedFrozenBalanceForBandwidth(delegatedAmount);
    receiverAccount = new AccountCapsule(receiverBuilder.build());
    dbManager.getAccountStore().put(receiverAccount.getAddress().toByteArray(), receiverAccount);

    // Seed the delegated resource (expired) - legacy V1 format
    seedDelegatedResource(unfreezeOwner, receiverAddr,
        delegatedAmount, now - 1000, // BANDWIDTH expired
        0, 0); // No ENERGY

    // Set total net weight so unfreeze doesn't go negative
    dbManager.getDynamicPropertiesStore().saveTotalNetWeight(delegatedAmount / ONE_TRX);

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(receiverAddr)))
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_CONTRACT", 12)
        .caseName("edge_delegated_unfreeze_with_optimization")
        .caseCategory("edge")
        .description("Delegated unfreeze with ALLOW_DELEGATE_OPTIMIZATION=1 converts legacy "
            + "index to prefixed keys, then deletes them via unDelegate")
        .database("account")
        .database("votes")
        .database("dynamic-properties")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("ALLOW_DELEGATE_RESOURCE", 1)
        .dynamicProperty("ALLOW_DELEGATE_OPTIMIZATION", 1)
        .dynamicProperty("delegated_amount", delegatedAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV1 delegate optimization: success={}", result.isSuccess());

    // Restore
    dbManager.getDynamicPropertiesStore().saveAllowDelegateOptimization(0);
    disableDelegationMode();
  }
}
