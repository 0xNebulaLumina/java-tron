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
import org.tron.core.capsule.AssetIssueCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract;
import org.tron.protos.contract.AssetIssueContractOuterClass.ParticipateAssetIssueContract;
import org.tron.protos.contract.AssetIssueContractOuterClass.UnfreezeAssetContract;
import org.tron.protos.contract.AssetIssueContractOuterClass.UpdateAssetContract;

/**
 * Generates conformance test fixtures for TRC-10 Extension contracts:
 * - ParticipateAssetIssueContract (9)
 * - UnfreezeAssetContract (14)
 * - UpdateAssetContract (15)
 *
 * <p>Run with: ./gradlew :framework:test --tests "Trc10ExtensionFixtureGeneratorTest" -Dconformance.output=conformance/fixtures
 */
public class Trc10ExtensionFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(Trc10ExtensionFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String PARTICIPANT_ADDRESS;
  private static final String OTHER_ADDRESS;
  private static final long INITIAL_BALANCE = 1_000_000_000_000L; // 1M TRX
  private static final String ASSET_NAME = "TestToken";
  private static final String ASSET_ID = "1000001";
  private static final long TOTAL_SUPPLY = 1_000_000_000_000L; // 1 trillion tokens
  private static final int TRX_NUM = 1;
  private static final int NUM = 10; // 10 tokens per TRX

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049abc";
    PARTICIPANT_ADDRESS = Wallet.getAddressPreFixString() + "1111111111111111111111111111111111111111";
    OTHER_ADDRESS = Wallet.getAddressPreFixString() + "2222222222222222222222222222222222222222";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("TRC-10 Extension Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Create owner account (asset issuer)
    AccountCapsule ownerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("owner"),
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);

    // Add asset to owner
    Protocol.Account.Builder ownerBuilder = ownerAccount.getInstance().toBuilder();
    ownerBuilder.putAssetV2(ASSET_ID, TOTAL_SUPPLY / 2); // Half supply available
    ownerAccount = new AccountCapsule(ownerBuilder.build());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Create participant account
    AccountCapsule participantAccount = new AccountCapsule(
        ByteString.copyFromUtf8("participant"),
        ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(participantAccount.getAddress().toByteArray(), participantAccount);

    // Create other account
    AccountCapsule otherAccount = new AccountCapsule(
        ByteString.copyFromUtf8("other"),
        ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(otherAccount.getAddress().toByteArray(), otherAccount);

    // Enable TRC-10 features
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(1);

    // Create asset issue
    long currentTime = System.currentTimeMillis();
    AssetIssueContract assetIssue = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setName(ByteString.copyFromUtf8(ASSET_NAME))
        .setAbbr(ByteString.copyFromUtf8("TT"))
        .setTotalSupply(TOTAL_SUPPLY)
        .setTrxNum(TRX_NUM)
        .setNum(NUM)
        .setPrecision(6)
        .setStartTime(currentTime - 86400000) // Started 1 day ago
        .setEndTime(currentTime + 86400000 * 30) // Ends in 30 days
        .setDescription(ByteString.copyFromUtf8("Test Token for Conformance"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .setId(ASSET_ID)
        .build();

    AssetIssueCapsule assetIssueCapsule = new AssetIssueCapsule(assetIssue);
    dbManager.getAssetIssueV2Store().put(assetIssueCapsule.createDbV2Key(), assetIssueCapsule);

    // Set block properties
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(currentTime);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
  }

  // ==========================================================================
  // ParticipateAssetIssueContract (9) Fixtures
  // ==========================================================================

  @Test
  public void generateParticipateAssetIssue_happyPath() throws Exception {
    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(ASSET_ID))
        .setAmount(100_000_000L) // 100 TRX
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Participate in asset issuance (buy tokens with TRX)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateParticipateAssetIssue_insufficientBalance() throws Exception {
    // Create account with minimal balance
    String poorAddress = Wallet.getAddressPreFixString() + "3333333333333333333333333333333333333333";
    AccountCapsule poorAccount = new AccountCapsule(
        ByteString.copyFromUtf8("poor"),
        ByteString.copyFrom(ByteArray.fromHexString(poorAddress)),
        AccountType.Normal,
        1000L); // Minimal balance
    dbManager.getAccountStore().put(poorAccount.getAddress().toByteArray(), poorAccount);

    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(poorAddress)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(ASSET_ID))
        .setAmount(100_000_000L) // 100 TRX - more than balance
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_insufficient_balance")
        .caseCategory("validate_fail")
        .description("Fail when participant has insufficient TRX balance")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(poorAddress)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue insufficient balance: validationError={}", result.getValidationError());
  }

  @Test
  public void generateParticipateAssetIssue_assetNotFound() throws Exception {
    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8("9999999")) // Non-existent asset
        .setAmount(100_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_asset_not_found")
        .caseCategory("validate_fail")
        .description("Fail when asset does not exist")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .expectedError("not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue asset not found: validationError={}", result.getValidationError());
  }

  @Test
  public void generateParticipateAssetIssue_saleEnded() throws Exception {
    // Create an expired asset
    String expiredAssetId = "1000002";
    long currentTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    AssetIssueContract expiredAsset = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setName(ByteString.copyFromUtf8("ExpiredToken"))
        .setAbbr(ByteString.copyFromUtf8("EXP"))
        .setTotalSupply(TOTAL_SUPPLY)
        .setTrxNum(TRX_NUM)
        .setNum(NUM)
        .setPrecision(6)
        .setStartTime(currentTime - 86400000 * 60) // Started 60 days ago
        .setEndTime(currentTime - 86400000) // Ended 1 day ago
        .setDescription(ByteString.copyFromUtf8("Expired Token"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .setId(expiredAssetId)
        .build();

    AssetIssueCapsule expiredCapsule = new AssetIssueCapsule(expiredAsset);
    dbManager.getAssetIssueV2Store().put(expiredCapsule.createDbV2Key(), expiredCapsule);

    // Add tokens to other account
    AccountCapsule other = dbManager.getAccountStore().get(ByteArray.fromHexString(OTHER_ADDRESS));
    Protocol.Account.Builder otherBuilder = other.getInstance().toBuilder();
    otherBuilder.putAssetV2(expiredAssetId, TOTAL_SUPPLY / 2);
    dbManager.getAccountStore().put(other.getAddress().toByteArray(), new AccountCapsule(otherBuilder.build()));

    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(expiredAssetId))
        .setAmount(100_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_sale_ended")
        .caseCategory("validate_fail")
        .description("Fail when asset sale period has ended")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .expectedError("period")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue sale ended: validationError={}", result.getValidationError());
  }

  @Test
  public void generateParticipateAssetIssue_toSelf() throws Exception {
    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS))) // Same as owner
        .setAssetName(ByteString.copyFromUtf8(ASSET_ID))
        .setAmount(100_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_self_participate")
        .caseCategory("validate_fail")
        .description("Fail when participating in own asset")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("yourself")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue to self: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // UnfreezeAssetContract (14) Fixtures
  // ==========================================================================

  @Test
  public void generateUnfreezeAsset_happyPath() throws Exception {
    // Create asset with frozen supply
    setupAssetWithFrozenSupply();

    UnfreezeAssetContract contract = UnfreezeAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_ASSET_CONTRACT", 14)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Unfreeze expired frozen asset supply")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeAsset happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateUnfreezeAsset_noFrozenAsset() throws Exception {
    UnfreezeAssetContract contract = UnfreezeAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_ASSET_CONTRACT", 14)
        .caseName("validate_fail_no_frozen")
        .caseCategory("validate_fail")
        .description("Fail when account has no frozen assets")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("frozen")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeAsset no frozen: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUnfreezeAsset_notYetExpired() throws Exception {
    // Setup frozen supply that hasn't expired yet
    String futureAssetId = "1000003";
    long currentTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    AssetIssueContract.FrozenSupply frozenSupply = AssetIssueContract.FrozenSupply.newBuilder()
        .setFrozenAmount(100_000_000_000L)
        .setFrozenDays(365) // Frozen for 365 days
        .build();

    AssetIssueContract futureAsset = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setName(ByteString.copyFromUtf8("FutureToken"))
        .setAbbr(ByteString.copyFromUtf8("FUT"))
        .setTotalSupply(TOTAL_SUPPLY)
        .setTrxNum(TRX_NUM)
        .setNum(NUM)
        .setPrecision(6)
        .setStartTime(currentTime - 1000) // Just started
        .setEndTime(currentTime + 86400000 * 365)
        .addFrozenSupply(frozenSupply)
        .setDescription(ByteString.copyFromUtf8("Future Token"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .setId(futureAssetId)
        .build();

    AssetIssueCapsule futureCapsule = new AssetIssueCapsule(futureAsset);
    dbManager.getAssetIssueV2Store().put(futureCapsule.createDbV2Key(), futureCapsule);

    // Mark participant as having issued this asset
    AccountCapsule participant = dbManager.getAccountStore().get(ByteArray.fromHexString(PARTICIPANT_ADDRESS));
    Protocol.Account.Builder pBuilder = participant.getInstance().toBuilder();
    pBuilder.setAssetIssuedID(ByteString.copyFromUtf8(futureAssetId));
    // Add frozen supply to account
    Protocol.Account.Frozen frozen = Protocol.Account.Frozen.newBuilder()
        .setFrozenBalance(100_000_000_000L)
        .setExpireTime(currentTime + 86400000L * 365) // Far in future
        .build();
    pBuilder.addFrozenSupply(frozen);
    dbManager.getAccountStore().put(participant.getAddress().toByteArray(), new AccountCapsule(pBuilder.build()));

    UnfreezeAssetContract contract = UnfreezeAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_ASSET_CONTRACT", 14)
        .caseName("validate_fail_not_expired")
        .caseCategory("validate_fail")
        .description("Fail when frozen supply has not yet expired")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .expectedError("time")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeAsset not expired: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // UpdateAssetContract (15) Fixtures
  // ==========================================================================

  @Test
  public void generateUpdateAsset_happyPath() throws Exception {
    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setDescription(ByteString.copyFromUtf8("Updated description for test token"))
        .setUrl(ByteString.copyFromUtf8("https://updated.example.com"))
        .setNewLimit(2000)
        .setNewPublicLimit(2000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Update asset metadata (description, URL, limits)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateAsset_notAssetOwner() throws Exception {
    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setDescription(ByteString.copyFromUtf8("Trying to update asset I don't own"))
        .setUrl(ByteString.copyFromUtf8("https://malicious.com"))
        .setNewLimit(2000)
        .setNewPublicLimit(2000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_not_owner")
        .caseCategory("validate_fail")
        .description("Fail when account is not the asset owner")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .expectedError("issue")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset not owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateAsset_invalidUrl() throws Exception {
    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setDescription(ByteString.copyFromUtf8("Valid description"))
        .setUrl(ByteString.copyFromUtf8("")) // Empty URL
        .setNewLimit(2000)
        .setNewPublicLimit(2000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_invalid_url")
        .caseCategory("validate_fail")
        .description("Fail when URL is invalid")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("url")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset invalid URL: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateAsset_descriptionTooLong() throws Exception {
    // Create a very long description (> 200 bytes)
    StringBuilder longDesc = new StringBuilder();
    for (int i = 0; i < 300; i++) {
      longDesc.append("X");
    }

    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setDescription(ByteString.copyFromUtf8(longDesc.toString()))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setNewLimit(2000)
        .setNewPublicLimit(2000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_description_too_long")
        .caseCategory("validate_fail")
        .description("Fail when description exceeds maximum length")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("description")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset description too long: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Helper Methods
  // ==========================================================================

  private void setupAssetWithFrozenSupply() {
    // Mark owner as having issued asset
    AccountCapsule owner = dbManager.getAccountStore().get(ByteArray.fromHexString(OWNER_ADDRESS));
    long currentTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    Protocol.Account.Builder ownerBuilder = owner.getInstance().toBuilder();
    ownerBuilder.setAssetIssuedID(ByteString.copyFromUtf8(ASSET_ID));
    // Add expired frozen supply to account
    Protocol.Account.Frozen frozen = Protocol.Account.Frozen.newBuilder()
        .setFrozenBalance(50_000_000_000L)
        .setExpireTime(currentTime - 1000) // Already expired
        .build();
    ownerBuilder.addFrozenSupply(frozen);
    dbManager.getAccountStore().put(owner.getAddress().toByteArray(), new AccountCapsule(ownerBuilder.build()));
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
