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
}
