package org.tron.core.conformance;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import java.io.File;
import java.util.ArrayList;
import java.util.List;
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
import org.tron.core.capsule.DelegatedResourceCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.Account.UnFreezeV2;
import org.tron.protos.Protocol.Account.FreezeV2;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.BalanceContract.CancelAllUnfreezeV2Contract;
import org.tron.protos.contract.BalanceContract.DelegateResourceContract;
import org.tron.protos.contract.BalanceContract.UnDelegateResourceContract;
import org.tron.protos.contract.BalanceContract.WithdrawExpireUnfreezeContract;
import org.tron.protos.contract.Common.ResourceCode;

/**
 * Generates conformance test fixtures for Resource/Delegation contracts:
 * - WithdrawExpireUnfreezeContract (56)
 * - DelegateResourceContract (57)
 * - UnDelegateResourceContract (58)
 * - CancelAllUnfreezeV2Contract (59)
 *
 * <p>Run with: ./gradlew :framework:test --tests "ResourceDelegationFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures
 */
public class ResourceDelegationFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(ResourceDelegationFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String RECEIVER_ADDRESS;
  private static final String OTHER_ADDRESS;
  private static final long INITIAL_BALANCE = 1_000_000_000_000L; // 1M TRX
  private static final long FROZEN_BALANCE = 100_000_000_000L; // 100K TRX frozen

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049abc";
    RECEIVER_ADDRESS = Wallet.getAddressPreFixString() + "1111111111111111111111111111111111111111";
    OTHER_ADDRESS = Wallet.getAddressPreFixString() + "2222222222222222222222222222222222222222";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("Resource/Delegation Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Create owner account with frozen balance
    AccountCapsule ownerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("owner"),
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);

    // Add frozen V2 balance for BANDWIDTH
    Protocol.Account.Builder ownerBuilder = ownerAccount.getInstance().toBuilder();
    ownerBuilder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(FROZEN_BALANCE)
        .build());
    ownerBuilder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.ENERGY)
        .setAmount(FROZEN_BALANCE)
        .build());
    ownerAccount = new AccountCapsule(ownerBuilder.build());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Create receiver account
    AccountCapsule receiverAccount = new AccountCapsule(
        ByteString.copyFromUtf8("receiver"),
        ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(receiverAccount.getAddress().toByteArray(), receiverAccount);

    // Create other account
    AccountCapsule otherAccount = new AccountCapsule(
        ByteString.copyFromUtf8("other"),
        ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(otherAccount.getAddress().toByteArray(), otherAccount);

    // Enable required features
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(14);
    dbManager.getDynamicPropertiesStore().saveAllowDelegateResource(1);
    dbManager.getDynamicPropertiesStore().saveAllowNewResourceModel(1);
    dbManager.getDynamicPropertiesStore().saveAllowCancelAllUnfreezeV2(1);

    // Set total weights
    dbManager.getDynamicPropertiesStore().saveTotalNetWeight(1_000_000_000L);
    dbManager.getDynamicPropertiesStore().saveTotalEnergyWeight(1_000_000_000L);
    dbManager.getDynamicPropertiesStore().saveTotalTronPowerWeight(1_000_000_000L);

    // Set block properties
    long currentTime = System.currentTimeMillis();
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(currentTime);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
  }

  // ==========================================================================
  // WithdrawExpireUnfreezeContract (56) Fixtures
  // ==========================================================================

  @Test
  public void generateWithdrawExpireUnfreeze_happyPath() throws Exception {
    // Add expired unfrozen balance to owner
    long expiredTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp() - 1000;
    addUnfrozenV2ToOwner(ResourceCode.BANDWIDTH, 10_000_000_000L, expiredTime);

    WithdrawExpireUnfreezeContract contract = WithdrawExpireUnfreezeContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawExpireUnfreezeContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT", 56)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Withdraw expired unfrozen balance")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("withdraw_expire_amount", 10_000_000_000L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawExpireUnfreeze happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateWithdrawExpireUnfreeze_nothingToWithdraw() throws Exception {
    // Owner has no unfrozen balance
    WithdrawExpireUnfreezeContract contract = WithdrawExpireUnfreezeContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawExpireUnfreezeContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT", 56)
        .caseName("validate_fail_nothing_to_withdraw")
        .caseCategory("validate_fail")
        .description("Fail when there is no expired unfrozen balance to withdraw")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("expire")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawExpireUnfreeze nothing to withdraw: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWithdrawExpireUnfreeze_notYetExpired() throws Exception {
    // Add not-yet-expired unfrozen balance
    long futureTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp() + 86400000; // +1 day
    addUnfrozenV2ToAccount(OTHER_ADDRESS, ResourceCode.BANDWIDTH, 10_000_000_000L, futureTime);

    WithdrawExpireUnfreezeContract contract = WithdrawExpireUnfreezeContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawExpireUnfreezeContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT", 56)
        .caseName("validate_fail_not_expired")
        .caseCategory("validate_fail")
        .description("Fail when unfrozen balance has not yet expired")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("expire")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawExpireUnfreeze not expired: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWithdrawExpireUnfreeze_multipleResources() throws Exception {
    // Add expired unfrozen balance for multiple resource types
    long expiredTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp() - 1000;
    String multiAddress = Wallet.getAddressPreFixString() + "3333333333333333333333333333333333333333";

    // Create account with multiple expired unfrozen balances
    AccountCapsule multiAccount = new AccountCapsule(
        ByteString.copyFromUtf8("multi"),
        ByteString.copyFrom(ByteArray.fromHexString(multiAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = multiAccount.getInstance().toBuilder();
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(5_000_000_000L)
        .setUnfreezeExpireTime(expiredTime)
        .build());
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.ENERGY)
        .setUnfreezeAmount(3_000_000_000L)
        .setUnfreezeExpireTime(expiredTime)
        .build());
    multiAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(multiAccount.getAddress().toByteArray(), multiAccount);

    WithdrawExpireUnfreezeContract contract = WithdrawExpireUnfreezeContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(multiAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawExpireUnfreezeContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT", 56)
        .caseName("happy_path_multiple")
        .caseCategory("happy")
        .description("Withdraw multiple expired unfrozen balances (BANDWIDTH + ENERGY)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(multiAddress)
        .dynamicProperty("withdraw_expire_amount", 8_000_000_000L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawExpireUnfreeze multiple: success={}", result.isSuccess());
  }

  // ==========================================================================
  // DelegateResourceContract (57) Fixtures
  // ==========================================================================

  @Test
  public void generateDelegateResource_happyPath() throws Exception {
    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L) // 10K TRX
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("happy_path_bandwidth")
        .caseCategory("happy")
        .description("Delegate BANDWIDTH resource to another account")
        .database("account")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource BANDWIDTH: success={}", result.isSuccess());
  }

  @Test
  public void generateDelegateResource_energy() throws Exception {
    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.ENERGY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("happy_path_energy")
        .caseCategory("happy")
        .description("Delegate ENERGY resource to another account")
        .database("account")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource ENERGY: success={}", result.isSuccess());
  }

  @Test
  public void generateDelegateResource_withLock() throws Exception {
    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .setLock(true)
        .setLockPeriod(86400) // 1 day lock
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("happy_path_with_lock")
        .caseCategory("happy")
        .description("Delegate resource with lock period")
        .database("account")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource with lock: success={}", result.isSuccess());
  }

  @Test
  public void generateDelegateResource_insufficientFrozen() throws Exception {
    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L) // Other has no frozen balance
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_insufficient_frozen")
        .caseCategory("validate_fail")
        .description("Fail when owner has insufficient frozen balance")
        .database("account")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("frozen")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource insufficient: validationError={}", result.getValidationError());
  }

  @Test
  public void generateDelegateResource_toSelf() throws Exception {
    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS))) // Self
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_self_delegate")
        .caseCategory("validate_fail")
        .description("Fail when trying to delegate to self")
        .database("account")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("self")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource to self: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // UnDelegateResourceContract (58) Fixtures
  // ==========================================================================

  @Test
  public void generateUnDelegateResource_happyPath() throws Exception {
    // First create a delegation
    createDelegation(OWNER_ADDRESS, RECEIVER_ADDRESS, ResourceCode.BANDWIDTH, 10_000_000_000L);

    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(5_000_000_000L) // Undelegate half
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Undelegate resource from another account")
        .database("account")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateUnDelegateResource_noDelegation() throws Exception {
    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(5_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("validate_fail_no_delegation")
        .caseCategory("validate_fail")
        .description("Fail when there is no delegation to undelegate")
        .database("account")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("delegat")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource no delegation: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnDelegateResource_exceedsAmount() throws Exception {
    // Create a small delegation
    createDelegation(OTHER_ADDRESS, RECEIVER_ADDRESS, ResourceCode.BANDWIDTH, 1_000_000_000L);

    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L) // More than delegated
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("validate_fail_exceeds_amount")
        .caseCategory("validate_fail")
        .description("Fail when trying to undelegate more than delegated")
        .database("account")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource exceeds: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // CancelAllUnfreezeV2Contract (59) Fixtures
  // ==========================================================================

  @Test
  public void generateCancelAllUnfreezeV2_happyPath() throws Exception {
    // Add unfrozen balance (not yet expired) to cancel
    long futureTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp() + 86400000;
    String cancelAddress = Wallet.getAddressPreFixString() + "4444444444444444444444444444444444444444";

    AccountCapsule cancelAccount = new AccountCapsule(
        ByteString.copyFromUtf8("cancel"),
        ByteString.copyFrom(ByteArray.fromHexString(cancelAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = cancelAccount.getInstance().toBuilder();
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(5_000_000_000L)
        .setUnfreezeExpireTime(futureTime)
        .build());
    cancelAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(cancelAccount.getAddress().toByteArray(), cancelAccount);

    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(cancelAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Cancel all pending unfreeze operations")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(cancelAddress)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateCancelAllUnfreezeV2_mixedExpiry() throws Exception {
    // Add both expired and unexpired unfrozen balance
    long pastTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp() - 1000;
    long futureTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp() + 86400000;
    String mixedAddress = Wallet.getAddressPreFixString() + "5555555555555555555555555555555555555555";

    AccountCapsule mixedAccount = new AccountCapsule(
        ByteString.copyFromUtf8("mixed"),
        ByteString.copyFrom(ByteArray.fromHexString(mixedAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = mixedAccount.getInstance().toBuilder();
    // Expired
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(3_000_000_000L)
        .setUnfreezeExpireTime(pastTime)
        .build());
    // Not expired
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.ENERGY)
        .setUnfreezeAmount(5_000_000_000L)
        .setUnfreezeExpireTime(futureTime)
        .build());
    mixedAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(mixedAccount.getAddress().toByteArray(), mixedAccount);

    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(mixedAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("happy_path_mixed")
        .caseCategory("happy")
        .description("Cancel with mixed expired/unexpired entries (expired goes to withdraw, unexpired re-freezes)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(mixedAddress)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 mixed: success={}", result.isSuccess());
  }

  @Test
  public void generateCancelAllUnfreezeV2_nothingToCancel() throws Exception {
    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("validate_fail_nothing_to_cancel")
        .caseCategory("validate_fail")
        .description("Fail when there are no pending unfreeze operations to cancel")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("unfreeze")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 nothing to cancel: validationError={}", result.getValidationError());
  }

  @Test
  public void generateCancelAllUnfreezeV2_featureDisabled() throws Exception {
    // Disable cancel all unfreeze v2
    dbManager.getDynamicPropertiesStore().saveAllowCancelAllUnfreezeV2(0);

    String testAddress = Wallet.getAddressPreFixString() + "6666666666666666666666666666666666666666";
    long futureTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp() + 86400000;

    AccountCapsule testAccount = new AccountCapsule(
        ByteString.copyFromUtf8("test"),
        ByteString.copyFrom(ByteArray.fromHexString(testAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = testAccount.getInstance().toBuilder();
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(5_000_000_000L)
        .setUnfreezeExpireTime(futureTime)
        .build());
    testAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(testAccount.getAddress().toByteArray(), testAccount);

    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(testAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("validate_fail_disabled")
        .caseCategory("validate_fail")
        .description("Fail when CancelAllUnfreezeV2 feature is not enabled")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(testAddress)
        .expectedError("support")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 disabled: validationError={}", result.getValidationError());

    // Re-enable for other tests
    dbManager.getDynamicPropertiesStore().saveAllowCancelAllUnfreezeV2(1);
  }

  // ==========================================================================
  // Helper Methods
  // ==========================================================================

  private void addUnfrozenV2ToOwner(ResourceCode resource, long amount, long expireTime) {
    AccountCapsule account = dbManager.getAccountStore()
        .get(ByteArray.fromHexString(OWNER_ADDRESS));
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(resource)
        .setUnfreezeAmount(amount)
        .setUnfreezeExpireTime(expireTime)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);
  }

  private void addUnfrozenV2ToAccount(String address, ResourceCode resource, long amount, long expireTime) {
    AccountCapsule account = dbManager.getAccountStore()
        .get(ByteArray.fromHexString(address));
    Protocol.Account.Builder builder = account.getInstance().toBuilder();
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(resource)
        .setUnfreezeAmount(amount)
        .setUnfreezeExpireTime(expireTime)
        .build());
    account = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);
  }

  private void createDelegation(String from, String to, ResourceCode resource, long amount) {
    byte[] key = DelegatedResourceCapsule.createDbKeyV2(
        ByteArray.fromHexString(from),
        ByteArray.fromHexString(to),
        false);

    DelegatedResourceCapsule delegation = new DelegatedResourceCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(from)),
        ByteString.copyFrom(ByteArray.fromHexString(to)));

    if (resource == ResourceCode.BANDWIDTH) {
      delegation.setFrozenBalanceForBandwidth(amount, 0);
    } else if (resource == ResourceCode.ENERGY) {
      delegation.setFrozenBalanceForEnergy(amount, 0);
    }

    dbManager.getDelegatedResourceStore().put(key, delegation);

    // Update owner's delegated balance
    AccountCapsule owner = dbManager.getAccountStore()
        .get(ByteArray.fromHexString(from));
    if (owner != null) {
      if (resource == ResourceCode.BANDWIDTH) {
        owner.addDelegatedFrozenV2BalanceForBandwidth(amount);
      } else {
        owner.addDelegatedFrozenV2BalanceForEnergy(amount);
      }
      dbManager.getAccountStore().put(owner.getAddress().toByteArray(), owner);
    }

    // Update receiver's acquired balance
    AccountCapsule receiver = dbManager.getAccountStore()
        .get(ByteArray.fromHexString(to));
    if (receiver != null) {
      if (resource == ResourceCode.BANDWIDTH) {
        receiver.addAcquiredDelegatedFrozenV2BalanceForBandwidth(amount);
      } else {
        receiver.addAcquiredDelegatedFrozenV2BalanceForEnergy(amount);
      }
      dbManager.getAccountStore().put(receiver.getAddress().toByteArray(), receiver);
    }
  }

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
