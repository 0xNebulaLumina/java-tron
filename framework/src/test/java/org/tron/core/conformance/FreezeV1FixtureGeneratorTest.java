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
}
