package org.tron.core.conformance;

import static org.tron.core.config.Parameter.ChainConstant.BLOCK_PRODUCED_INTERVAL;
import static org.tron.core.config.Parameter.ChainConstant.DELEGATE_PERIOD;

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

  // --- Phase 1 Missing Fixtures: WithdrawExpireUnfreezeContract ---

  @Test
  public void generateWithdrawExpireUnfreeze_featureNotEnabled() throws Exception {
    // Disable unfreeze delay feature
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(0);

    WithdrawExpireUnfreezeContract contract = WithdrawExpireUnfreezeContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawExpireUnfreezeContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT", 56)
        .caseName("validate_fail_feature_not_enabled")
        .caseCategory("validate_fail")
        .description("Fail when unfreeze delay feature is not enabled (unfreezeDelayDays == 0)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Not support WithdrawExpireUnfreeze transaction, need to be opened by the committee")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawExpireUnfreeze feature disabled: validationError={}", result.getValidationError());

    // Re-enable for other tests
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(14);
  }

  @Test
  public void generateWithdrawExpireUnfreeze_ownerAddressInvalidEmpty() throws Exception {
    WithdrawExpireUnfreezeContract contract = WithdrawExpireUnfreezeContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawExpireUnfreezeContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT", 56)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty/invalid")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress("")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawExpireUnfreeze invalid owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWithdrawExpireUnfreeze_ownerAccountNotExist() throws Exception {
    String nonExistentAddress = Wallet.getAddressPreFixString() + "9999999999999999999999999999999999999999";

    WithdrawExpireUnfreezeContract contract = WithdrawExpireUnfreezeContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawExpireUnfreezeContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT", 56)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(nonExistentAddress)
        .expectedError("not exists")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawExpireUnfreeze account not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWithdrawExpireUnfreeze_mixedExpiredAndUnexpired() throws Exception {
    // Test: some entries expired, some not - should succeed and withdraw only expired
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long expiredTime = now - 1000;
    long futureTime = now + 86400000; // +1 day
    String mixedAddress = Wallet.getAddressPreFixString() + "7777777777777777777777777777777777777777";

    AccountCapsule mixedAccount = new AccountCapsule(
        ByteString.copyFromUtf8("mixed_withdraw"),
        ByteString.copyFrom(ByteArray.fromHexString(mixedAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = mixedAccount.getInstance().toBuilder();
    // Expired entry (should be withdrawn)
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(5_000_000_000L)
        .setUnfreezeExpireTime(expiredTime)
        .build());
    // Not expired entry (should remain)
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.ENERGY)
        .setUnfreezeAmount(3_000_000_000L)
        .setUnfreezeExpireTime(futureTime)
        .build());
    mixedAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(mixedAccount.getAddress().toByteArray(), mixedAccount);

    WithdrawExpireUnfreezeContract contract = WithdrawExpireUnfreezeContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(mixedAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawExpireUnfreezeContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT", 56)
        .caseName("edge_mixed_expired_and_unexpired_entries")
        .caseCategory("happy")
        .description("Withdraw only expired entries, keep unexpired entries in unfrozenV2 list")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(mixedAddress)
        .dynamicProperty("withdraw_expire_amount", 5_000_000_000L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawExpireUnfreeze mixed: success={}", result.isSuccess());
  }

  @Test
  public void generateWithdrawExpireUnfreeze_expireTimeEqualsNow() throws Exception {
    // Test boundary: expireTime == now should be treated as expired (<= now) and withdrawn
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    String boundaryAddress = Wallet.getAddressPreFixString() + "8888888888888888888888888888888888888888";

    AccountCapsule boundaryAccount = new AccountCapsule(
        ByteString.copyFromUtf8("boundary"),
        ByteString.copyFrom(ByteArray.fromHexString(boundaryAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = boundaryAccount.getInstance().toBuilder();
    // Entry with expireTime exactly at 'now'
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(2_000_000_000L)
        .setUnfreezeExpireTime(now) // exactly now
        .build());
    boundaryAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(boundaryAccount.getAddress().toByteArray(), boundaryAccount);

    WithdrawExpireUnfreezeContract contract = WithdrawExpireUnfreezeContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(boundaryAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawExpireUnfreezeContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT", 56)
        .caseName("edge_expire_time_equals_now_is_withdrawable")
        .caseCategory("happy")
        .description("Entry with expireTime == now is treated as expired and withdrawable (<= now)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(boundaryAddress)
        .dynamicProperty("withdraw_expire_amount", 2_000_000_000L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawExpireUnfreeze boundary (now): success={}", result.isSuccess());
  }

  @Test
  public void generateWithdrawExpireUnfreeze_balanceOverflow() throws Exception {
    // Test overflow protection: LongMath.checkedAdd(balance, withdrawAmount) throws ArithmeticException
    long expiredTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp() - 1000;
    String overflowAddress = Wallet.getAddressPreFixString() + "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    AccountCapsule overflowAccount = new AccountCapsule(
        ByteString.copyFromUtf8("overflow"),
        ByteString.copyFrom(ByteArray.fromHexString(overflowAddress)),
        AccountType.Normal,
        Long.MAX_VALUE - 1000); // Near max value

    Protocol.Account.Builder builder = overflowAccount.getInstance().toBuilder();
    // Expired entry that will overflow when added to balance
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(10000L) // Adding this to near-max balance will overflow
        .setUnfreezeExpireTime(expiredTime)
        .build());
    overflowAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(overflowAccount.getAddress().toByteArray(), overflowAccount);

    WithdrawExpireUnfreezeContract contract = WithdrawExpireUnfreezeContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(overflowAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawExpireUnfreezeContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT", 56)
        .caseName("validate_fail_balance_overflow_on_withdraw")
        .caseCategory("validate_fail")
        .description("Fail when withdrawing would cause balance overflow (ArithmeticException)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(overflowAddress)
        .expectedError("overflow")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawExpireUnfreeze overflow: validationError={}", result.getValidationError());
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

  // --- Phase 2 Missing Fixtures: DelegateResourceContract ---

  @Test
  public void generateDelegateResource_featureDisabledSupportDR() throws Exception {
    // Disable delegate resource feature
    dbManager.getDynamicPropertiesStore().saveAllowDelegateResource(0);

    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_delegate_disabled_supportDR")
        .caseCategory("validate_fail")
        .description("Fail when supportDR() is false (ALLOW_DELEGATE_RESOURCE = 0)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("No support for resource delegate")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource supportDR disabled: validationError={}", result.getValidationError());

    // Re-enable for other tests
    dbManager.getDynamicPropertiesStore().saveAllowDelegateResource(1);
  }

  @Test
  public void generateDelegateResource_unfreezeDelayDisabled() throws Exception {
    // Disable unfreeze delay feature (keep allowDelegateResource enabled)
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(0);

    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_unfreeze_delay_disabled")
        .caseCategory("validate_fail")
        .description("Fail when supportUnfreezeDelay() is false (unfreezeDelayDays == 0)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Not support Delegate resource transaction, need to be opened by the committee")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource unfreeze delay disabled: validationError={}", result.getValidationError());

    // Re-enable for other tests
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(14);
  }

  @Test
  public void generateDelegateResource_ownerAddressInvalidEmpty() throws Exception {
    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty/invalid")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress("")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource invalid owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateDelegateResource_ownerAccountNotExist() throws Exception {
    String nonExistentAddress = Wallet.getAddressPreFixString() + "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(nonExistentAddress)
        .expectedError("not exists")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource owner not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateDelegateResource_delegateBalanceLessThan1TRX() throws Exception {
    long ONE_TRX = 1_000_000L;

    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(ONE_TRX - 1) // Less than 1 TRX
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_delegate_balance_lt_1_trx")
        .caseCategory("validate_fail")
        .description("Fail when delegateBalance < 1 TRX")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("delegateBalance must be greater than or equal to 1 TRX")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource < 1 TRX: validationError={}", result.getValidationError());
  }

  @Test
  public void generateDelegateResource_delegateBalanceExact1TRX() throws Exception {
    long ONE_TRX = 1_000_000L;

    // Need an owner with exactly 1 TRX frozen
    String exact1TrxAddress = Wallet.getAddressPreFixString() + "cccccccccccccccccccccccccccccccccccccccc";
    AccountCapsule exact1TrxAccount = new AccountCapsule(
        ByteString.copyFromUtf8("exact1trx"),
        ByteString.copyFrom(ByteArray.fromHexString(exact1TrxAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = exact1TrxAccount.getInstance().toBuilder();
    builder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(ONE_TRX)
        .build());
    exact1TrxAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(exact1TrxAccount.getAddress().toByteArray(), exact1TrxAccount);

    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(exact1TrxAddress)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(ONE_TRX) // Exactly 1 TRX
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("happy_path_delegate_balance_exact_1_trx")
        .caseCategory("happy")
        .description("Succeed when delegateBalance is exactly 1 TRX (boundary)")
        .database("account")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .database("dynamic-properties")
        .ownerAddress(exact1TrxAddress)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource exact 1 TRX: success={}", result.isSuccess());
  }

  @Test
  public void generateDelegateResource_resourceUnrecognizedValue() throws Exception {
    // Use setResourceValue(999) to set an unrecognized resource code
    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L)
        .setResourceValue(999) // Unrecognized resource code
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_resource_unrecognized_value")
        .caseCategory("validate_fail")
        .description("Fail when resource code is unrecognized (not BANDWIDTH or ENERGY)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource unrecognized resource: validationError={}", result.getValidationError());
  }

  @Test
  public void generateDelegateResource_receiverAddressInvalidEmpty() throws Exception {
    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.EMPTY)
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_receiver_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when receiver address is empty/invalid")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid receiverAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource invalid receiver: validationError={}", result.getValidationError());
  }

  @Test
  public void generateDelegateResource_receiverAccountNotExist() throws Exception {
    String nonExistentReceiver = Wallet.getAddressPreFixString() + "dddddddddddddddddddddddddddddddddddddddd";

    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentReceiver)))
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_receiver_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when receiver account does not exist")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("not exists")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource receiver not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateDelegateResource_receiverIsContractAccount() throws Exception {
    // Create a contract account
    String contractAddress = Wallet.getAddressPreFixString() + "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    AccountCapsule contractAccount = new AccountCapsule(
        ByteString.copyFromUtf8("contract"),
        ByteString.copyFrom(ByteArray.fromHexString(contractAddress)),
        AccountType.Contract,  // Contract account type
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(contractAccount.getAddress().toByteArray(), contractAccount);

    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(contractAddress)))
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_receiver_is_contract_account")
        .caseCategory("validate_fail")
        .description("Fail when receiver is a contract account")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Do not allow delegate resources to contract addresses")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource to contract: validationError={}", result.getValidationError());
  }

  @Test
  public void generateDelegateResource_lockPeriodNegative() throws Exception {
    // Enable maxDelegateLockPeriod feature
    long maxLockPeriod = DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL + 10000;
    dbManager.getDynamicPropertiesStore().saveMaxDelegateLockPeriod(maxLockPeriod);

    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .setLock(true)
        .setLockPeriod(-1) // Negative lock period
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_lock_period_negative")
        .caseCategory("validate_fail")
        .description("Fail when lock period is negative")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("The lock period of delegate resource cannot be less than 0")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource negative lock period: validationError={}", result.getValidationError());

    // Reset max lock period
    dbManager.getDynamicPropertiesStore().saveMaxDelegateLockPeriod(0);
  }

  @Test
  public void generateDelegateResource_lockPeriodExceedsMax() throws Exception {
    // Enable maxDelegateLockPeriod feature
    long maxLockPeriod = DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL + 10000;
    dbManager.getDynamicPropertiesStore().saveMaxDelegateLockPeriod(maxLockPeriod);

    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .setLock(true)
        .setLockPeriod(maxLockPeriod + 1) // Exceeds max
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_lock_period_exceeds_max")
        .caseCategory("validate_fail")
        .description("Fail when lock period exceeds max delegate lock period")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("cannot exceed " + maxLockPeriod)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource lock period exceeds max: validationError={}", result.getValidationError());

    // Reset max lock period
    dbManager.getDynamicPropertiesStore().saveMaxDelegateLockPeriod(0);
  }

  @Test
  public void generateDelegateResource_lockPeriodLessThanRemainingPreviousLock() throws Exception {
    // Enable maxDelegateLockPeriod feature
    long maxLockPeriod = DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL + 10000;
    dbManager.getDynamicPropertiesStore().saveMaxDelegateLockPeriod(maxLockPeriod);

    // Create an existing locked delegation with long remaining time
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long existingExpireTime = now + 86400000L * 30; // 30 days from now

    byte[] lockKey = DelegatedResourceCapsule.createDbKeyV2(
        ByteArray.fromHexString(OWNER_ADDRESS),
        ByteArray.fromHexString(RECEIVER_ADDRESS),
        true); // lock=true

    DelegatedResourceCapsule existingLockedDelegation = new DelegatedResourceCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)));
    existingLockedDelegation.setFrozenBalanceForBandwidth(1_000_000_000L, existingExpireTime);
    dbManager.getDelegatedResourceStore().put(lockKey, existingLockedDelegation);

    // Try to delegate with shorter lock period than remaining time
    long shortLockPeriod = 86400L; // ~1 day in blocks (much less than 30 days)

    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(1_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .setLock(true)
        .setLockPeriod(shortLockPeriod)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("validate_fail_lock_period_less_than_remaining_previous_lock")
        .caseCategory("validate_fail")
        .description("Fail when new lock period is less than remaining time of previous lock")
        .database("account")
        .database("DelegatedResource")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("lock period for BANDWIDTH this time cannot be less than the remaining time")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource lock period less than remaining: validationError={}", result.getValidationError());

    // Cleanup
    dbManager.getDelegatedResourceStore().delete(lockKey);
    dbManager.getDynamicPropertiesStore().saveMaxDelegateLockPeriod(0);
  }

  @Test
  public void generateDelegateResource_lockPeriodZeroDefaults() throws Exception {
    // Enable maxDelegateLockPeriod feature
    long maxLockPeriod = DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL + 10000;
    dbManager.getDynamicPropertiesStore().saveMaxDelegateLockPeriod(maxLockPeriod);

    DelegateResourceContract contract = DelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(10_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .setLock(true)
        .setLockPeriod(0) // Zero defaults to DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.DelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("DELEGATE_RESOURCE_CONTRACT", 57)
        .caseName("edge_lock_period_zero_defaults")
        .caseCategory("happy")
        .description("Succeed when lockPeriod=0 with supportMaxDelegateLockPeriod enabled (uses default)")
        .database("account")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("DelegateResource lock period zero defaults: success={}", result.isSuccess());

    // Reset max lock period
    dbManager.getDynamicPropertiesStore().saveMaxDelegateLockPeriod(0);
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

  // --- Phase 3 Missing Fixtures: UnDelegateResourceContract ---

  @Test
  public void generateUnDelegateResource_featureDisabledSupportDR() throws Exception {
    // Disable delegate resource feature
    dbManager.getDynamicPropertiesStore().saveAllowDelegateResource(0);

    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(5_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("validate_fail_undelegate_disabled_supportDR")
        .caseCategory("validate_fail")
        .description("Fail when supportDR() is false (ALLOW_DELEGATE_RESOURCE = 0)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("No support for resource delegate")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource supportDR disabled: validationError={}", result.getValidationError());

    // Re-enable for other tests
    dbManager.getDynamicPropertiesStore().saveAllowDelegateResource(1);
  }

  @Test
  public void generateUnDelegateResource_unfreezeDelayDisabled() throws Exception {
    // Disable unfreeze delay feature
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(0);

    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(5_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("validate_fail_unfreeze_delay_disabled")
        .caseCategory("validate_fail")
        .description("Fail when supportUnfreezeDelay() is false (unfreezeDelayDays == 0)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Not support unDelegate resource transaction, need to be opened by the committee")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource unfreeze delay disabled: validationError={}", result.getValidationError());

    // Re-enable for other tests
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(14);
  }

  @Test
  public void generateUnDelegateResource_ownerAddressInvalidEmpty() throws Exception {
    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(5_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty/invalid")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress("")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource invalid owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnDelegateResource_receiverAddressInvalidEmpty() throws Exception {
    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.EMPTY)
        .setBalance(5_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("validate_fail_receiver_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when receiver address is empty/invalid")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid receiverAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource invalid receiver: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnDelegateResource_receiverEqualsOwner() throws Exception {
    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS))) // Same as owner
        .setBalance(5_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("validate_fail_receiver_equals_owner")
        .caseCategory("validate_fail")
        .description("Fail when receiver address equals owner address")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("receiverAddress must not be the same as ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource receiver equals owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnDelegateResource_unDelegateBalanceZero() throws Exception {
    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(0) // Zero balance
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("validate_fail_unDelegate_balance_zero")
        .caseCategory("validate_fail")
        .description("Fail when unDelegateBalance is zero")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("unDelegateBalance must be more than 0 TRX")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource zero balance: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnDelegateResource_resourceUnrecognizedValue() throws Exception {
    // Create a delegation first
    createDelegation(OWNER_ADDRESS, RECEIVER_ADDRESS, ResourceCode.BANDWIDTH, 10_000_000_000L);

    // Use setResourceValue(999) to set an unrecognized resource code
    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setBalance(5_000_000_000L)
        .setResourceValue(999) // Unrecognized resource code
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("validate_fail_resource_unrecognized_value")
        .caseCategory("validate_fail")
        .description("Fail when resource code is unrecognized")
        .database("account")
        .database("DelegatedResource")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource unrecognized resource: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnDelegateResource_onlyLockedDelegationNotExpired() throws Exception {
    // Create ONLY a locked delegation with expireTime >= now (not available for undelegate)
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long futureExpireTime = now + 86400000L; // 1 day from now

    String lockedOwner = Wallet.getAddressPreFixString() + "f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1";
    String lockedReceiver = Wallet.getAddressPreFixString() + "f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2";

    // Create accounts
    AccountCapsule lockedOwnerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("locked_owner"),
        ByteString.copyFrom(ByteArray.fromHexString(lockedOwner)),
        AccountType.Normal,
        INITIAL_BALANCE);
    Protocol.Account.Builder ownerBuilder = lockedOwnerAccount.getInstance().toBuilder();
    ownerBuilder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(FROZEN_BALANCE)
        .build());
    lockedOwnerAccount = new AccountCapsule(ownerBuilder.build());
    dbManager.getAccountStore().put(lockedOwnerAccount.getAddress().toByteArray(), lockedOwnerAccount);

    AccountCapsule lockedReceiverAccount = new AccountCapsule(
        ByteString.copyFromUtf8("locked_receiver"),
        ByteString.copyFrom(ByteArray.fromHexString(lockedReceiver)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(lockedReceiverAccount.getAddress().toByteArray(), lockedReceiverAccount);

    // Create ONLY locked delegation (no unlock delegation)
    byte[] lockKey = DelegatedResourceCapsule.createDbKeyV2(
        ByteArray.fromHexString(lockedOwner),
        ByteArray.fromHexString(lockedReceiver),
        true); // lock=true

    DelegatedResourceCapsule lockedDelegation = new DelegatedResourceCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(lockedOwner)),
        ByteString.copyFrom(ByteArray.fromHexString(lockedReceiver)));
    lockedDelegation.setFrozenBalanceForBandwidth(10_000_000_000L, futureExpireTime);
    dbManager.getDelegatedResourceStore().put(lockKey, lockedDelegation);

    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(lockedOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(lockedReceiver)))
        .setBalance(5_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("validate_fail_only_locked_delegation_not_expired")
        .caseCategory("validate_fail")
        .description("Fail when delegation exists only in lock record and is not expired (expireTime >= now)")
        .database("account")
        .database("DelegatedResource")
        .database("dynamic-properties")
        .ownerAddress(lockedOwner)
        .expectedError("insufficient delegatedFrozenBalance(BANDWIDTH)")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource only locked not expired: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnDelegateResource_lockedExpireTimeEqualsNow() throws Exception {
    // Create ONLY a locked delegation with expireTime == now (still locked, strict < now for unlock)
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    String boundaryOwner = Wallet.getAddressPreFixString() + "f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3";
    String boundaryReceiver = Wallet.getAddressPreFixString() + "f4f4f4f4f4f4f4f4f4f4f4f4f4f4f4f4f4f4f4f4";

    // Create accounts
    AccountCapsule boundaryOwnerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("boundary_owner"),
        ByteString.copyFrom(ByteArray.fromHexString(boundaryOwner)),
        AccountType.Normal,
        INITIAL_BALANCE);
    Protocol.Account.Builder ownerBuilder = boundaryOwnerAccount.getInstance().toBuilder();
    ownerBuilder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(FROZEN_BALANCE)
        .build());
    boundaryOwnerAccount = new AccountCapsule(ownerBuilder.build());
    dbManager.getAccountStore().put(boundaryOwnerAccount.getAddress().toByteArray(), boundaryOwnerAccount);

    AccountCapsule boundaryReceiverAccount = new AccountCapsule(
        ByteString.copyFromUtf8("boundary_receiver"),
        ByteString.copyFrom(ByteArray.fromHexString(boundaryReceiver)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(boundaryReceiverAccount.getAddress().toByteArray(), boundaryReceiverAccount);

    // Create ONLY locked delegation with expireTime == now
    byte[] lockKey = DelegatedResourceCapsule.createDbKeyV2(
        ByteArray.fromHexString(boundaryOwner),
        ByteArray.fromHexString(boundaryReceiver),
        true); // lock=true

    DelegatedResourceCapsule lockedDelegation = new DelegatedResourceCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(boundaryOwner)),
        ByteString.copyFrom(ByteArray.fromHexString(boundaryReceiver)));
    lockedDelegation.setFrozenBalanceForBandwidth(10_000_000_000L, now); // exactly now
    dbManager.getDelegatedResourceStore().put(lockKey, lockedDelegation);

    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(boundaryOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(boundaryReceiver)))
        .setBalance(5_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("validate_fail_locked_expire_time_equals_now")
        .caseCategory("validate_fail")
        .description("Fail when locked delegation has expireTime == now (still locked, unlock requires < now)")
        .database("account")
        .database("DelegatedResource")
        .database("dynamic-properties")
        .ownerAddress(boundaryOwner)
        .expectedError("insufficient delegatedFrozenBalance(BANDWIDTH)")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource locked expire time equals now: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnDelegateResource_fullUndelegateDeletesStoreAndIndex() throws Exception {
    // Create a delegation with index
    String fullOwner = Wallet.getAddressPreFixString() + "f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5";
    String fullReceiver = Wallet.getAddressPreFixString() + "f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6";

    // Create accounts
    AccountCapsule fullOwnerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("full_owner"),
        ByteString.copyFrom(ByteArray.fromHexString(fullOwner)),
        AccountType.Normal,
        INITIAL_BALANCE);
    Protocol.Account.Builder ownerBuilder = fullOwnerAccount.getInstance().toBuilder();
    ownerBuilder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(FROZEN_BALANCE)
        .build());
    fullOwnerAccount = new AccountCapsule(ownerBuilder.build());
    dbManager.getAccountStore().put(fullOwnerAccount.getAddress().toByteArray(), fullOwnerAccount);

    AccountCapsule fullReceiverAccount = new AccountCapsule(
        ByteString.copyFromUtf8("full_receiver"),
        ByteString.copyFrom(ByteArray.fromHexString(fullReceiver)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(fullReceiverAccount.getAddress().toByteArray(), fullReceiverAccount);

    // Create delegation via helper (which creates store entry and updates accounts)
    createDelegation(fullOwner, fullReceiver, ResourceCode.BANDWIDTH, 10_000_000_000L);

    // Also seed the DelegatedResourceAccountIndex
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    dbManager.getDelegatedResourceAccountIndexStore().delegateV2(
        ByteArray.fromHexString(fullOwner),
        ByteArray.fromHexString(fullReceiver),
        now);

    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(fullOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(fullReceiver)))
        .setBalance(10_000_000_000L) // Full amount
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("happy_path_full_undelegate_deletes_store_and_index")
        .caseCategory("happy")
        .description("Full undelegate removes DelegatedResourceStore entry and updates DelegatedResourceAccountIndex")
        .database("account")
        .database("DelegatedResource")
        .database("DelegatedResourceAccountIndex")
        .database("dynamic-properties")
        .ownerAddress(fullOwner)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource full undelegate: success={}", result.isSuccess());
  }

  @Test
  public void generateUnDelegateResource_receiverAccountMissing() throws Exception {
    // Create delegation but delete receiver account (TVM contract suicide scenario)
    String suicideOwner = Wallet.getAddressPreFixString() + "f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7";
    String suicideReceiver = Wallet.getAddressPreFixString() + "f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8";

    // Create owner account
    AccountCapsule suicideOwnerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("suicide_owner"),
        ByteString.copyFrom(ByteArray.fromHexString(suicideOwner)),
        AccountType.Normal,
        INITIAL_BALANCE);
    Protocol.Account.Builder ownerBuilder = suicideOwnerAccount.getInstance().toBuilder();
    ownerBuilder.addFrozenV2(FreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setAmount(FROZEN_BALANCE)
        .build());
    // Add delegated balance tracking
    ownerBuilder.setDelegatedFrozenV2BalanceForBandwidth(10_000_000_000L);
    suicideOwnerAccount = new AccountCapsule(ownerBuilder.build());
    dbManager.getAccountStore().put(suicideOwnerAccount.getAddress().toByteArray(), suicideOwnerAccount);

    // Create delegation store entry (but no receiver account)
    byte[] unlockKey = DelegatedResourceCapsule.createDbKeyV2(
        ByteArray.fromHexString(suicideOwner),
        ByteArray.fromHexString(suicideReceiver),
        false); // unlock

    DelegatedResourceCapsule delegation = new DelegatedResourceCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(suicideOwner)),
        ByteString.copyFrom(ByteArray.fromHexString(suicideReceiver)));
    delegation.setFrozenBalanceForBandwidth(10_000_000_000L, 0);
    dbManager.getDelegatedResourceStore().put(unlockKey, delegation);

    // Note: receiver account is NOT created (simulating TVM contract suicide)

    UnDelegateResourceContract contract = UnDelegateResourceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(suicideOwner)))
        .setReceiverAddress(ByteString.copyFrom(ByteArray.fromHexString(suicideReceiver)))
        .setBalance(5_000_000_000L)
        .setResource(ResourceCode.BANDWIDTH)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnDelegateResourceContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNDELEGATE_RESOURCE_CONTRACT", 58)
        .caseName("happy_path_receiver_account_missing")
        .caseCategory("happy")
        .description("Succeed when receiver account is missing (TVM contract suicide scenario)")
        .database("account")
        .database("DelegatedResource")
        .database("dynamic-properties")
        .ownerAddress(suicideOwner)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnDelegateResource receiver missing: success={}", result.isSuccess());
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

  // --- Phase 4 Missing Fixtures: CancelAllUnfreezeV2Contract ---

  @Test
  public void generateCancelAllUnfreezeV2_unfreezeDelayDisabled() throws Exception {
    // Disable unfreeze delay (even if ALLOW_CANCEL_ALL_UNFREEZE_V2 = 1)
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(0);

    String testAddress = Wallet.getAddressPreFixString() + "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
    long futureTime = System.currentTimeMillis() + 86400000;

    AccountCapsule testAccount = new AccountCapsule(
        ByteString.copyFromUtf8("test_cancel"),
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
        .caseName("validate_fail_unfreeze_delay_disabled")
        .caseCategory("validate_fail")
        .description("Fail when unfreezeDelayDays=0 even if ALLOW_CANCEL_ALL_UNFREEZE_V2=1")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(testAddress)
        .expectedError("Not support CancelAllUnfreezeV2 transaction, need to be opened by the committee")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 unfreeze delay disabled: validationError={}", result.getValidationError());

    // Re-enable for other tests
    dbManager.getDynamicPropertiesStore().saveUnfreezeDelayDays(14);
  }

  @Test
  public void generateCancelAllUnfreezeV2_ownerAddressInvalidEmpty() throws Exception {
    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty/invalid")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress("")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 invalid owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateCancelAllUnfreezeV2_ownerAccountNotExist() throws Exception {
    String nonExistentAddress = Wallet.getAddressPreFixString() + "a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2";

    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(nonExistentAddress)
        .expectedError("not exists")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 account not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateCancelAllUnfreezeV2_tronPowerUnexpiredRefreezes() throws Exception {
    // Test TRON_POWER unexpired entries: should refreeze and update tronPowerWeight
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long futureTime = now + 86400000; // +1 day
    String tronPowerAddress = Wallet.getAddressPreFixString() + "a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3";

    AccountCapsule tronPowerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("tron_power"),
        ByteString.copyFrom(ByteArray.fromHexString(tronPowerAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = tronPowerAccount.getInstance().toBuilder();
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.TRON_POWER)
        .setUnfreezeAmount(10_000_000_000L) // 10K TRX
        .setUnfreezeExpireTime(futureTime)
        .build());
    tronPowerAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(tronPowerAccount.getAddress().toByteArray(), tronPowerAccount);

    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(tronPowerAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("happy_path_tron_power_unexpired_refreezes")
        .caseCategory("happy")
        .description("Unexpired TRON_POWER entry is re-frozen, updating frozenForTronPowerV2 and totalTronPowerWeight")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(tronPowerAddress)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 TRON_POWER unexpired: success={}", result.isSuccess());
  }

  @Test
  public void generateCancelAllUnfreezeV2_tronPowerExpiredWithdraws() throws Exception {
    // Test TRON_POWER expired entries: should be withdrawn, not re-frozen
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long pastTime = now - 1000;
    String tronPowerAddress = Wallet.getAddressPreFixString() + "a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4";

    AccountCapsule tronPowerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("tron_power_expired"),
        ByteString.copyFrom(ByteArray.fromHexString(tronPowerAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = tronPowerAccount.getInstance().toBuilder();
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.TRON_POWER)
        .setUnfreezeAmount(10_000_000_000L) // 10K TRX
        .setUnfreezeExpireTime(pastTime)
        .build());
    tronPowerAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(tronPowerAccount.getAddress().toByteArray(), tronPowerAccount);

    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(tronPowerAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("happy_path_tron_power_expired_withdraws")
        .caseCategory("happy")
        .description("Expired TRON_POWER entry contributes to withdrawExpireAmount, not re-frozen")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(tronPowerAddress)
        .dynamicProperty("withdraw_expire_amount", 10_000_000_000L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 TRON_POWER expired: success={}", result.isSuccess());
  }

  @Test
  public void generateCancelAllUnfreezeV2_expireTimeEqualsNowTreatedAsExpired() throws Exception {
    // Test boundary: expireTime == now is treated as expired (<= now) and goes to withdraw
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    String boundaryAddress = Wallet.getAddressPreFixString() + "a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5";

    AccountCapsule boundaryAccount = new AccountCapsule(
        ByteString.copyFromUtf8("boundary_cancel"),
        ByteString.copyFrom(ByteArray.fromHexString(boundaryAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = boundaryAccount.getInstance().toBuilder();
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(5_000_000_000L)
        .setUnfreezeExpireTime(now) // exactly now
        .build());
    boundaryAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(boundaryAccount.getAddress().toByteArray(), boundaryAccount);

    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(boundaryAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("edge_expire_time_equals_now_treated_as_expired")
        .caseCategory("happy")
        .description("Entry with expireTime == now is treated as expired (withdrawn, not re-frozen)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(boundaryAddress)
        .dynamicProperty("withdraw_expire_amount", 5_000_000_000L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 boundary (now): success={}", result.isSuccess());
  }

  @Test
  public void generateCancelAllUnfreezeV2_allEntriesExpiredWithdrawOnly() throws Exception {
    // All entries are expired: should behave like "withdraw only" with cancel amounts = 0
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long pastTime = now - 1000;
    String allExpiredAddress = Wallet.getAddressPreFixString() + "a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6";

    AccountCapsule allExpiredAccount = new AccountCapsule(
        ByteString.copyFromUtf8("all_expired"),
        ByteString.copyFrom(ByteArray.fromHexString(allExpiredAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = allExpiredAccount.getInstance().toBuilder();
    // Multiple expired entries across different resources
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(3_000_000_000L)
        .setUnfreezeExpireTime(pastTime)
        .build());
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.ENERGY)
        .setUnfreezeAmount(2_000_000_000L)
        .setUnfreezeExpireTime(pastTime)
        .build());
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.TRON_POWER)
        .setUnfreezeAmount(1_000_000_000L)
        .setUnfreezeExpireTime(pastTime)
        .build());
    allExpiredAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(allExpiredAccount.getAddress().toByteArray(), allExpiredAccount);

    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(allExpiredAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("edge_all_entries_expired_withdraw_only")
        .caseCategory("happy")
        .description("All entries expired: withdraw-only behavior with cancel amounts = 0")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(allExpiredAddress)
        .dynamicProperty("withdraw_expire_amount", 6_000_000_000L) // 3B + 2B + 1B
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 all expired: success={}", result.isSuccess());
  }

  @Test
  public void generateCancelAllUnfreezeV2_multipleEntriesSameResourceSums() throws Exception {
    // Multiple unexpired entries of the same resource: amounts should sum
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long futureTime = now + 86400000;
    String multiSameAddress = Wallet.getAddressPreFixString() + "a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7";

    AccountCapsule multiSameAccount = new AccountCapsule(
        ByteString.copyFromUtf8("multi_same"),
        ByteString.copyFrom(ByteArray.fromHexString(multiSameAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = multiSameAccount.getInstance().toBuilder();
    // Multiple unexpired BANDWIDTH entries
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(3_000_000_000L)
        .setUnfreezeExpireTime(futureTime)
        .build());
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(2_000_000_000L)
        .setUnfreezeExpireTime(futureTime + 1000)
        .build());
    multiSameAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(multiSameAccount.getAddress().toByteArray(), multiSameAccount);

    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(multiSameAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("edge_multiple_entries_same_resource_sums")
        .caseCategory("happy")
        .description("Multiple unexpired entries of same resource: amounts sum for refreeze")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(multiSameAddress)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 multiple same resource: success={}", result.isSuccess());
  }

  @Test
  public void generateCancelAllUnfreezeV2_amountNotMultipleOfTrxPrecision() throws Exception {
    // Test rounding: unexpired amount not a multiple of TRX_PRECISION (1_000_000)
    // Weight calculation: amount / TRX_PRECISION uses floor division
    long now = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    long futureTime = now + 86400000;
    String roundingAddress = Wallet.getAddressPreFixString() + "a8a8a8a8a8a8a8a8a8a8a8a8a8a8a8a8a8a8a8a8";

    AccountCapsule roundingAccount = new AccountCapsule(
        ByteString.copyFromUtf8("rounding"),
        ByteString.copyFrom(ByteArray.fromHexString(roundingAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = roundingAccount.getInstance().toBuilder();
    // Amount that is not a multiple of TRX_PRECISION (1M SUN = 1 TRX)
    // 1_500_001 SUN = 1.500001 TRX, weight = floor(1_500_001 / 1_000_000) = 1
    builder.addUnfrozenV2(UnFreezeV2.newBuilder()
        .setType(ResourceCode.BANDWIDTH)
        .setUnfreezeAmount(1_500_001L) // 1.500001 TRX
        .setUnfreezeExpireTime(futureTime)
        .build());
    roundingAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(roundingAccount.getAddress().toByteArray(), roundingAccount);

    CancelAllUnfreezeV2Contract contract = CancelAllUnfreezeV2Contract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(roundingAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.CancelAllUnfreezeV2Contract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("CANCEL_ALL_UNFREEZE_V2_CONTRACT", 59)
        .caseName("edge_amount_not_multiple_of_trx_precision_rounding")
        .caseCategory("happy")
        .description("Non-TRX-multiple amount to pin floor division rounding in weight updates")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(roundingAddress)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("CancelAllUnfreezeV2 rounding: success={}", result.isSuccess());
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
