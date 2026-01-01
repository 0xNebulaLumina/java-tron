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
}
