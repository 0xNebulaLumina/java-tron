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
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.Account.FreezeV2;
import org.tron.protos.Protocol.Account.UnFreezeV2;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.BalanceContract.FreezeBalanceV2Contract;
import org.tron.protos.contract.BalanceContract.UnfreezeBalanceV2Contract;
import org.tron.protos.contract.Common.ResourceCode;

/**
 * Generates conformance test fixtures for V2 freeze/unfreeze contracts:
 * - FreezeBalanceV2Contract (54)
 * - UnfreezeBalanceV2Contract (55)
 *
 * <p>V2 freeze requires unfreezeDelayDays > 0.
 *
 * <p>Run with: ./gradlew :framework:test --tests "FreezeV2FixtureGeneratorTest"
 * -Dconformance.output=../conformance/fixtures --dependency-verification=off
 */
public class FreezeV2FixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(FreezeV2FixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String WITNESS_ADDRESS;
  private static final int UNFREEZE_DELAY_DAYS = 14;

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049155";
    WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "548794500882809695a8a687866e76d4271a1abc";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("FreezeV2 Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Initialize V2 dynamic properties
    initCommonDynamicPropsV2(dbManager,
        DEFAULT_BLOCK_TIMESTAMP / 1000,
        DEFAULT_BLOCK_TIMESTAMP,
        UNFREEZE_DELAY_DAYS);

    // Create owner account with sufficient balance
    putAccount(dbManager, OWNER_ADDRESS, INITIAL_BALANCE, "owner");

    // Create witness
    putAccount(dbManager, WITNESS_ADDRESS, INITIAL_BALANCE, "witness");
    putWitness(dbManager, WITNESS_ADDRESS, "https://witness.network", 10_000_000L);
  }

  // ==========================================================================
  // FreezeBalanceV2Contract (54) Fixtures
  // ==========================================================================

  @Test
  public void generateFreezeBalanceV2_happyPathBandwidth() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_own01");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_v2");

    long freezeAmount = 100 * ONE_TRX;

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("happy_path_freeze_v2_bandwidth")
        .caseCategory("happy")
        .description("V2 freeze for BANDWIDTH resource")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("freeze_amount", freezeAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 BANDWIDTH happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateFreezeBalanceV2_happyPathEnergy() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_own02");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_energy_v2");

    long freezeAmount = 50 * ONE_TRX;

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setResource(ResourceCode.ENERGY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("happy_path_freeze_v2_energy")
        .caseCategory("happy")
        .description("V2 freeze for ENERGY resource")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("freeze_amount", freezeAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 ENERGY happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateFreezeBalanceV2_validateFailFeatureNotEnabled() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_own03");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_disabled");

    // Disable V2 (set delay to 0)
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(0);

    long freezeAmount = 100 * ONE_TRX;

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("validate_fail_feature_not_enabled")
        .caseCategory("validate_fail")
        .description("Fail V2 freeze when unfreezeDelayDays = 0 (V2 disabled)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .expectedError("freeze")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 not enabled: validationError={}", result.getValidationError());

    // Restore V2 mode
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(UNFREEZE_DELAY_DAYS);
  }

  @Test
  public void generateFreezeBalanceV2_validateFailFrozenBalanceGtBalance() throws Exception {
    String poorOwner = generateAddress("freeze_v2_own04");
    putAccount(dbManager, poorOwner, ONE_TRX, "poor_freeze_v2"); // Only 1 TRX

    long freezeAmount = INITIAL_BALANCE; // Much more than balance

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(poorOwner)))
        .setFrozenBalance(freezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("validate_fail_frozen_balance_gt_balance")
        .caseCategory("validate_fail")
        .description("Fail when freeze amount exceeds account balance")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(poorOwner)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 gt balance: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // UnfreezeBalanceV2Contract (55) Fixtures
  // ==========================================================================

  @Test
  public void generateUnfreezeBalanceV2_happyPathBandwidth() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_01");

    // Create account with frozenV2 balance for BANDWIDTH
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_owner");
    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    long unfreezeAmount = 50 * ONE_TRX; // Unfreeze half

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("happy_path_unfreeze_v2_bandwidth")
        .caseCategory("happy")
        .description("V2 unfreeze BANDWIDTH (creates pending unfrozen entry)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("unfreeze_amount", unfreezeAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 BANDWIDTH happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateUnfreezeBalanceV2_validateFailNoFrozenBalance() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_02");

    // Create account with NO frozenV2 balance
    putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_no_frozen");

    long unfreezeAmount = 50 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_no_frozen_balance")
        .caseCategory("validate_fail")
        .description("Fail when there is no frozenV2 balance for the resource")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("frozen")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 no frozen: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeBalanceV2_validateFailUnfreezeBalanceTooHigh() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_03");

    // Create account with small frozenV2 balance
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_small");
    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(10 * ONE_TRX) // Only 10 TRX frozen
        .build());
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    long unfreezeAmount = 100 * ONE_TRX; // More than frozen

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_unfreeze_balance_too_high")
        .caseCategory("validate_fail")
        .description("Fail when unfreeze amount exceeds frozen amount")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 too high: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeBalanceV2_edgeSweepExpiredUnfrozenV2() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_04");

    // Create account with frozenV2 balance AND an expired unfrozenV2 entry
    AccountCapsule unfreezeAccount = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_sweep");
    Protocol.Account.Builder builder = unfreezeAccount.getInstance().toBuilder();
    // Current frozen balance
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    // Expired unfrozen entry that should be swept
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(50 * ONE_TRX)
        .setUnfreezeExpireTime(DEFAULT_BLOCK_TIMESTAMP - 1000) // Expired
        .build());
    unfreezeAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(unfreezeAccount.getAddress().toByteArray(), unfreezeAccount);

    long unfreezeAmount = 30 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("edge_sweep_expired_unfrozen_v2")
        .caseCategory("edge")
        .description("Unfreeze V2 with existing expired unfrozenV2 entry (tests withdrawExpireAmount)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .note("This case tests the sweep of expired unfrozenV2 entries during execution")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 sweep: success={}", result.isSuccess());
  }

  // ==========================================================================
  // Phase 1: FreezeBalanceV2Contract (54) - Missing Fixtures
  // ==========================================================================

  // --- Owner/address/account validation branches ---

  @Test
  public void generateFreezeBalanceV2_validateFailOwnerAddressInvalidEmpty() throws Exception {
    long freezeAmount = 100 * ONE_TRX;

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Empty address
        .setFrozenBalance(freezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner_address is empty (fails DecodeUtil.addressValid)")
        .database("account")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 invalid empty address: validationError={}", result.getValidationError());
  }

  @Test
  public void generateFreezeBalanceV2_validateFailOwnerAccountNotExist() throws Exception {
    // Valid-looking address that doesn't exist in AccountStore
    String nonExistentOwner = generateAddress("freeze_v2_nonexist");

    long freezeAmount = 100 * ONE_TRX;

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentOwner)))
        .setFrozenBalance(freezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(nonExistentOwner)
        .expectedError("not exists")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 account not exist: validationError={}", result.getValidationError());
  }

  // --- Frozen balance validation branches ---

  @Test
  public void generateFreezeBalanceV2_validateFailFrozenBalanceZero() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_zero");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_zero");

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(0) // Zero frozen balance
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("validate_fail_frozen_balance_zero")
        .caseCategory("validate_fail")
        .description("Fail when frozenBalance is 0")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .expectedError("frozenBalance must be positive")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 balance zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateFreezeBalanceV2_validateFailFrozenBalanceNegative() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_neg");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_neg");

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(-1) // Negative frozen balance
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("validate_fail_frozen_balance_negative")
        .caseCategory("validate_fail")
        .description("Fail when frozenBalance is negative")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .expectedError("frozenBalance must be positive")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 balance negative: validationError={}", result.getValidationError());
  }

  @Test
  public void generateFreezeBalanceV2_validateFailFrozenBalanceLt1Trx() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_lt1");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_lt1");

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(ONE_TRX - 1) // Less than 1 TRX
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("validate_fail_frozen_balance_lt_1_trx")
        .caseCategory("validate_fail")
        .description("Fail when frozenBalance < 1 TRX (TRX_PRECISION check)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("frozenBalance", ONE_TRX - 1)
        .expectedError("frozenBalance must be greater than or equal to 1 TRX")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 balance lt 1 TRX: validationError={}", result.getValidationError());
  }

  @Test
  public void generateFreezeBalanceV2_happyPathFrozenBalanceExact1Trx() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_1trx");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_1trx");

    long freezeAmount = ONE_TRX; // Exactly 1 TRX (minimum allowed)

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("happy_path_frozen_balance_exact_1_trx")
        .caseCategory("happy")
        .description("V2 freeze with exactly 1 TRX (minimum allowed)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("freeze_amount", freezeAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 exact 1 TRX: success={}", result.isSuccess());
  }

  @Test
  public void generateFreezeBalanceV2_happyPathFrozenBalanceEqualAccountBalance() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_all");
    long accountBalance = 50 * ONE_TRX;
    putAccount(dbManager, freezeOwner, accountBalance, "freeze_owner_all");

    // Freeze entire account balance
    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(accountBalance) // Freeze all
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("happy_path_frozen_balance_equal_account_balance")
        .caseCategory("happy")
        .description("V2 freeze with frozenBalance == accountBalance (post balance is 0)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("account_balance", accountBalance)
        .dynamicProperty("freeze_amount", accountBalance)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 equal account balance: success={}", result.isSuccess());
  }

  // --- Resource code validation / coverage ---

  @Test
  public void generateFreezeBalanceV2_happyPathTronPower() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_tp");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_tp");

    long freezeAmount = 100 * ONE_TRX;

    // Ensure ALLOW_NEW_RESOURCE_MODEL = 1 (baseline)
    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setResource(ResourceCode.TRON_POWER)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("happy_path_freeze_v2_tron_power")
        .caseCategory("happy")
        .description("V2 freeze for TRON_POWER resource (new resource model enabled)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("ALLOW_NEW_RESOURCE_MODEL", 1)
        .dynamicProperty("freeze_amount", freezeAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 TRON_POWER happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateFreezeBalanceV2_validateFailTronPowerWhenNewResourceModelOff() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_tp_off");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_tp_off");

    // Disable new resource model but keep V2 enabled
    dbManager.getDynamicPropertiesStore().saveAllowNewResourceModel(0);

    long freezeAmount = 100 * ONE_TRX;

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setResource(ResourceCode.TRON_POWER)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("validate_fail_tron_power_when_new_resource_model_off")
        .caseCategory("validate_fail")
        .description("Fail TRON_POWER freeze when ALLOW_NEW_RESOURCE_MODEL = 0")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("ALLOW_NEW_RESOURCE_MODEL", 0)
        .expectedError("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 TRON_POWER disabled: validationError={}", result.getValidationError());

    // Restore new resource model
    dbManager.getDynamicPropertiesStore().saveAllowNewResourceModel(1);
  }

  @Test
  public void generateFreezeBalanceV2_validateFailResourceUnrecognizedValue() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_bad_res");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_bad_res");

    long freezeAmount = 100 * ONE_TRX;

    // Use setResourceValue to set an invalid enum value
    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setResourceValue(999) // Invalid resource code
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("validate_fail_resource_unrecognized_value")
        .caseCategory("validate_fail")
        .description("Fail when resource code is unrecognized (999)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("ALLOW_NEW_RESOURCE_MODEL", 1)
        .expectedError("ResourceCode error")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 unrecognized resource: validationError={}", result.getValidationError());
  }

  // --- Execution semantics edge fixtures ---

  @Test
  public void generateFreezeBalanceV2_edgeFreezeBandwidthTwiceAccumulates() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_accum");

    // Create account with existing frozenV2(BANDWIDTH) balance
    AccountCapsule account = putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_accum");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(50 * ONE_TRX) // Pre-existing frozen balance
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    long freezeAmount = 30 * ONE_TRX; // Freeze additional amount

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("edge_freeze_bandwidth_twice_accumulates")
        .caseCategory("edge")
        .description("Freeze BANDWIDTH twice - accumulation semantics (50 + 30 = 80 TRX)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("pre_frozen_amount", 50 * ONE_TRX)
        .dynamicProperty("freeze_amount", freezeAmount)
        .note("Tests accumulation and totalNetWeight delta behavior")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 accumulates: success={}", result.isSuccess());
  }

  @Test
  public void generateFreezeBalanceV2_edgeFreezeAmountNotMultipleOfTrxPrecision() throws Exception {
    String freezeOwner = generateAddress("freeze_v2_odd");
    putAccount(dbManager, freezeOwner, INITIAL_BALANCE, "freeze_owner_odd");

    // Freeze N*ONE_TRX + 1 to test weight rounding (floor division by TRX_PRECISION)
    long freezeAmount = 5 * ONE_TRX + 1;

    FreezeBalanceV2Contract contract = FreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freezeOwner)))
        .setFrozenBalance(freezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.FreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("FREEZE_BALANCE_V2_CONTRACT", 54)
        .caseName("edge_freeze_amount_not_multiple_of_trx_precision")
        .caseCategory("edge")
        .description("Freeze amount not multiple of TRX_PRECISION (5 TRX + 1 SUN)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(freezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("freeze_amount", freezeAmount)
        .note("Tests weight rounding via floor division by TRX_PRECISION")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("FreezeV2 odd amount: success={}", result.isSuccess());
  }

  // ==========================================================================
  // Phase 2: UnfreezeBalanceV2Contract (55) - Missing Fixtures
  // ==========================================================================

  // --- Feature gating (V2 disabled) ---

  @Test
  public void generateUnfreezeBalanceV2_validateFailFeatureNotEnabled() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_dis");

    // Create account with frozenV2 balance
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_disabled");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    // Disable V2 (set delay to 0)
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(0);

    long unfreezeAmount = 50 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_feature_not_enabled")
        .caseCategory("validate_fail")
        .description("Fail V2 unfreeze when unfreezeDelayDays = 0 (V2 disabled)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("Not support UnfreezeV2 transaction, need to be opened by the committee")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 feature not enabled: validationError={}", result.getValidationError());

    // Restore V2 mode
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(UNFREEZE_DELAY_DAYS);
  }

  // --- Owner/address/account validation branches ---

  @Test
  public void generateUnfreezeBalanceV2_validateFailOwnerAddressInvalidEmpty() throws Exception {
    long unfreezeAmount = 50 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Empty address
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner_address is empty (fails DecodeUtil.addressValid)")
        .database("account")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 invalid empty address: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeBalanceV2_validateFailOwnerAccountNotExist() throws Exception {
    // Valid-looking address that doesn't exist in AccountStore
    String nonExistentOwner = generateAddress("unfreeze_v2_nonexist");

    long unfreezeAmount = 50 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(nonExistentOwner)
        .expectedError("does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 account not exist: validationError={}", result.getValidationError());
  }

  // --- Resource coverage gaps ---

  @Test
  public void generateUnfreezeBalanceV2_happyPathEnergy() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_energy");

    // Create account with frozenV2(ENERGY) balance
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_energy");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.ENERGY)
        .setAmount(100 * ONE_TRX)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    long unfreezeAmount = 50 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.ENERGY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("happy_path_unfreeze_v2_energy")
        .caseCategory("happy")
        .description("V2 unfreeze ENERGY resource")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("unfreeze_amount", unfreezeAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 ENERGY happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateUnfreezeBalanceV2_validateFailNoFrozenBalanceEnergy() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_no_energy");
    putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_no_energy");

    long unfreezeAmount = 50 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.ENERGY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_no_frozen_balance_energy")
        .caseCategory("validate_fail")
        .description("Fail when there is no frozenV2(ENERGY) balance")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("no frozenBalance(Energy)")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 no energy: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeBalanceV2_happyPathTronPower() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_tp");

    // Create account with frozenV2(TRON_POWER) balance
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_tp");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.TRON_POWER)
        .setAmount(100 * ONE_TRX)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    long unfreezeAmount = 50 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.TRON_POWER)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("happy_path_unfreeze_v2_tron_power")
        .caseCategory("happy")
        .description("V2 unfreeze TRON_POWER resource (new resource model enabled)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("ALLOW_NEW_RESOURCE_MODEL", 1)
        .dynamicProperty("unfreeze_amount", unfreezeAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 TRON_POWER happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateUnfreezeBalanceV2_validateFailNoFrozenBalanceTronPower() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_no_tp");
    putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_no_tp");

    long unfreezeAmount = 50 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.TRON_POWER)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_no_frozen_balance_tron_power")
        .caseCategory("validate_fail")
        .description("Fail when there is no frozenV2(TRON_POWER) balance")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("no frozenBalance(TronPower)")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 no tron power: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeBalanceV2_validateFailTronPowerWhenNewResourceModelOff() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_tp_off");

    // Create account with frozenV2(TRON_POWER) balance
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_tp_off");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.TRON_POWER)
        .setAmount(100 * ONE_TRX)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    // Disable new resource model but keep V2 enabled
    dbManager.getDynamicPropertiesStore().saveAllowNewResourceModel(0);

    long unfreezeAmount = 50 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.TRON_POWER)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_tron_power_when_new_resource_model_off")
        .caseCategory("validate_fail")
        .description("Fail TRON_POWER unfreeze when ALLOW_NEW_RESOURCE_MODEL = 0")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("ALLOW_NEW_RESOURCE_MODEL", 0)
        .expectedError("ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 TRON_POWER disabled: validationError={}", result.getValidationError());

    // Restore new resource model
    dbManager.getDynamicPropertiesStore().saveAllowNewResourceModel(1);
  }

  @Test
  public void generateUnfreezeBalanceV2_validateFailResourceUnrecognizedValue() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_bad_res");

    // Create account with frozenV2 balance
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_bad_res");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    long unfreezeAmount = 50 * ONE_TRX;

    // Use setResourceValue to set an invalid enum value
    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResourceValue(999) // Invalid resource code
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_resource_unrecognized_value")
        .caseCategory("validate_fail")
        .description("Fail when resource code is unrecognized (999)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("ALLOW_NEW_RESOURCE_MODEL", 1)
        .expectedError("ResourceCode error")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 unrecognized resource: validationError={}", result.getValidationError());
  }

  // --- Unfreeze amount boundaries ---

  @Test
  public void generateUnfreezeBalanceV2_validateFailUnfreezeBalanceZero() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_zero");

    // Create account with frozenV2 balance
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_zero");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(0) // Zero unfreeze balance
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_unfreeze_balance_zero")
        .caseCategory("validate_fail")
        .description("Fail when unfreezeBalance is 0")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("Invalid unfreeze_balance, [0] is error")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 balance zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeBalanceV2_validateFailUnfreezeBalanceNegative() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_neg");

    // Create account with frozenV2 balance
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_neg");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(-1) // Negative unfreeze balance
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_unfreeze_balance_negative")
        .caseCategory("validate_fail")
        .description("Fail when unfreezeBalance is negative")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .expectedError("Invalid unfreeze_balance, [-1] is error")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 balance negative: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeBalanceV2_happyPathUnfreezeBalanceEqualFrozenAmount() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_all");

    long frozenAmount = 100 * ONE_TRX;

    // Create account with frozenV2 balance
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_all");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(frozenAmount)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    // Unfreeze all
    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(frozenAmount) // Unfreeze all
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("happy_path_unfreeze_balance_equal_frozen_amount")
        .caseCategory("happy")
        .description("V2 unfreeze with unfreezeBalance == frozenAmount (unfreeze all)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("frozen_amount", frozenAmount)
        .dynamicProperty("unfreeze_amount", frozenAmount)
        .note("Tests whether frozenV2 entry is kept at 0 vs removed")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 equal frozen amount: success={}", result.isSuccess());
  }

  @Test
  public void generateUnfreezeBalanceV2_edgeUnfreezeAmountNotMultipleOfTrxPrecision() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_odd");

    // Create account with frozenV2 balance
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_odd");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    long unfreezeAmount = 1; // 1 SUN (smallest unit)

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount) // 1 SUN
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("edge_unfreeze_amount_not_multiple_of_trx_precision")
        .caseCategory("edge")
        .description("Unfreeze 1 SUN (not multiple of TRX_PRECISION)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("unfreeze_amount", unfreezeAmount)
        .note("Tests rounding behavior in total weight updates")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 odd amount: success={}", result.isSuccess());
  }

  // --- Unfreezing-times limit (UNFREEZE_MAX_TIMES = 32) ---

  @Test
  public void generateUnfreezeBalanceV2_validateFailUnfreezingTimesOverLimit() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_limit");

    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long futureExpireTime = now + 86400000L * 30; // 30 days in future

    // Create account with 32 pending unfrozenV2 entries
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_limit");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    // Add frozen balance for new unfreeze
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    // Add 32 pending unfrozen entries (at limit)
    for (int i = 0; i < 32; i++) {
      builder.addUnfrozenV2(UnFreezeV2.newBuilder()
          .setType(ResourceCode.BANDWIDTH)
          .setUnfreezeAmount(ONE_TRX)
          .setUnfreezeExpireTime(futureExpireTime + i * 1000) // All unexpired
          .build());
    }
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    long unfreezeAmount = ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("validate_fail_unfreezing_times_over_limit")
        .caseCategory("validate_fail")
        .description("Fail when unfreezing times >= 32 (UNFREEZE_MAX_TIMES)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("pending_unfreezes", 32)
        .expectedError("Invalid unfreeze operation, unfreezing times is over limit")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 over limit: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeBalanceV2_edgeUnfreezingTimesAt31Succeeds() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_31");

    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long futureExpireTime = now + 86400000L * 30; // 30 days in future

    // Create account with 31 pending unfrozenV2 entries (under limit)
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_31");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    // Add frozen balance for new unfreeze
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    // Add 31 pending unfrozen entries (under limit)
    for (int i = 0; i < 31; i++) {
      builder.addUnfrozenV2(UnFreezeV2.newBuilder()
          .setType(ResourceCode.BANDWIDTH)
          .setUnfreezeAmount(ONE_TRX)
          .setUnfreezeExpireTime(futureExpireTime + i * 1000) // All unexpired
          .build());
    }
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    long unfreezeAmount = ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("edge_unfreezing_times_at_31_succeeds")
        .caseCategory("edge")
        .description("Success when unfreezing times = 31 (under limit, can add one more)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("pending_unfreezes", 31)
        .dynamicProperty("unfreeze_amount", unfreezeAmount)
        .note("Boundary case: 31 pending entries, one more allowed")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 at 31: success={}", result.isSuccess());
  }

  // --- Expired sweep behavior ---

  @Test
  public void generateUnfreezeBalanceV2_edgeSweepMultipleExpiredUnfrozenV2Entries() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_multi_exp");

    // Create account with multiple expired unfrozenV2 entries
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_multi_exp");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    // Current frozen balance
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    // Multiple expired entries
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(10 * ONE_TRX)
        .setUnfreezeExpireTime(DEFAULT_BLOCK_TIMESTAMP - 2000) // Expired
        .build());
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(20 * ONE_TRX)
        .setUnfreezeExpireTime(DEFAULT_BLOCK_TIMESTAMP - 1000) // Expired
        .build());
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.ENERGY)
        .setUnfreezeAmount(15 * ONE_TRX)
        .setUnfreezeExpireTime(DEFAULT_BLOCK_TIMESTAMP - 500) // Expired (different resource)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    long unfreezeAmount = 5 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("edge_sweep_multiple_expired_unfrozen_v2_entries")
        .caseCategory("edge")
        .description("Sweep multiple expired unfrozenV2 entries (sum: 45 TRX)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("expired_entries", 3)
        .dynamicProperty("total_expired_amount", 45 * ONE_TRX)
        .note("Tests withdrawExpireAmount summing across multiple entries")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 sweep multiple: success={}", result.isSuccess());
  }

  @Test
  public void generateUnfreezeBalanceV2_edgeSweepMixedExpiredAndUnexpiredUnfrozenV2Entries() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_mixed");

    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long futureExpireTime = now + 86400000L * 30; // 30 days in future

    // Create account with mixed expired/unexpired unfrozenV2 entries
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_mixed");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    // Current frozen balance
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    // Expired entry
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(25 * ONE_TRX)
        .setUnfreezeExpireTime(DEFAULT_BLOCK_TIMESTAMP - 1000) // Expired
        .build());
    // Unexpired entry
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(30 * ONE_TRX)
        .setUnfreezeExpireTime(futureExpireTime) // Not expired
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    long unfreezeAmount = 5 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("edge_sweep_mixed_expired_and_unexpired_unfrozen_v2_entries")
        .caseCategory("edge")
        .description("Sweep mixed expired/unexpired entries (expired swept, unexpired preserved)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("expired_amount", 25 * ONE_TRX)
        .dynamicProperty("unexpired_amount", 30 * ONE_TRX)
        .note("Tests selective sweep: expired removed, unexpired kept")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 sweep mixed: success={}", result.isSuccess());
  }

  @Test
  public void generateUnfreezeBalanceV2_edgeSweepExpireTimeEqualsNow() throws Exception {
    String unfreezeOwner = generateAddress("unfreeze_v2_eq_now");

    // Create account with unfrozenV2 entry where expireTime == now
    AccountCapsule account = putAccount(dbManager, unfreezeOwner, INITIAL_BALANCE, "unfreeze_v2_eq_now");
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    // Current frozen balance
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(100 * ONE_TRX)
        .build());
    // Entry with expireTime exactly equal to block timestamp (boundary condition)
    // The sweep logic uses <= now, so this should be swept
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(40 * ONE_TRX)
        .setUnfreezeExpireTime(DEFAULT_BLOCK_TIMESTAMP) // Exactly now
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);

    long unfreezeAmount = 5 * ONE_TRX;

    UnfreezeBalanceV2Contract contract = UnfreezeBalanceV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unfreezeOwner)))
        .setUnfreezeBalance(unfreezeAmount)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeBalanceV2Contract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_BALANCE_V2_CONTRACT", 55)
        .caseName("edge_sweep_expire_time_equals_now")
        .caseCategory("edge")
        .description("Sweep entry with expireTime == now (boundary: <= now is expired)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unfreezeOwner)
        .dynamicProperty("UNFREEZE_DELAY_DAYS", UNFREEZE_DELAY_DAYS)
        .dynamicProperty("expire_time", DEFAULT_BLOCK_TIMESTAMP)
        .dynamicProperty("expired_amount", 40 * ONE_TRX)
        .note("Tests boundary condition: expireTime == now is considered expired")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeV2 expire equals now: success={}", result.isSuccess());
  }
}
