package org.tron.core.conformance;

import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;
import static org.tron.core.conformance.ConformanceFixtureTestSupport.DEFAULT_BLOCK_TIMESTAMP;
import static org.tron.core.conformance.ConformanceFixtureTestSupport.DEFAULT_TX_EXPIRATION;
import static org.tron.core.conformance.ConformanceFixtureTestSupport.DEFAULT_TX_TIMESTAMP;

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
 * <p>Run with: ./gradlew :framework:test --tests "Trc10ExtensionFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures
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

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("TRC-10 Extension Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Use deterministic timestamps from ConformanceFixtureTestSupport
    long blockTimestamp = DEFAULT_BLOCK_TIMESTAMP;
    long blockNumber = 10;

    // Initialize TRC-10 dynamic properties (sets allowSameTokenName=1, oneDayNetLimit, etc.)
    ConformanceFixtureTestSupport.initTrc10DynamicProps(dbManager, blockNumber, blockTimestamp);

    // Create owner account (asset issuer)
    AccountCapsule ownerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("owner"),
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);

    // Add asset to owner and mark owner as having issued this asset
    Protocol.Account.Builder ownerBuilder = ownerAccount.getInstance().toBuilder();
    ownerBuilder.putAssetV2(ASSET_ID, TOTAL_SUPPLY / 2); // Half supply available
    ownerBuilder.setAssetIssuedID(ByteString.copyFromUtf8(ASSET_ID)); // Mark as issuer
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

    // Create asset issue with deterministic timestamps
    AssetIssueContract assetIssue = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setName(ByteString.copyFromUtf8(ASSET_NAME))
        .setAbbr(ByteString.copyFromUtf8("TT"))
        .setTotalSupply(TOTAL_SUPPLY)
        .setTrxNum(TRX_NUM)
        .setNum(NUM)
        .setPrecision(6)
        .setStartTime(blockTimestamp - 86400000) // Started 1 day ago
        .setEndTime(blockTimestamp + 86400000L * 30) // Ends in 30 days
        .setDescription(ByteString.copyFromUtf8("Test Token for Conformance"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .setId(ASSET_ID)
        .build();

    AssetIssueCapsule assetIssueCapsule = new AssetIssueCapsule(assetIssue);
    dbManager.getAssetIssueV2Store().put(assetIssueCapsule.createDbV2Key(), assetIssueCapsule);
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
    assertTrue("Happy path should succeed", result.isSuccess());
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
    assertNotNull("Validate fail should have validation error", result.getValidationError());
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
    assertNotNull("Validate fail should have validation error", result.getValidationError());
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
    assertNotNull("Validate fail should have validation error", result.getValidationError());
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
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  // ==========================================================================
  // ParticipateAssetIssueContract (9) - NEW EDGE CASES
  // ==========================================================================

  @Test
  public void generateParticipateAssetIssue_ownerAccountMissing() throws Exception {
    // Use a valid address that does not exist in AccountStore
    String missingAddress = Wallet.getAddressPreFixString() + "4444444444444444444444444444444444444444";

    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(missingAddress)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(ASSET_ID))
        .setAmount(100_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_owner_account_missing")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(missingAddress)
        .expectedError("Account does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue owner missing: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateParticipateAssetIssue_toAccountMissing() throws Exception {
    // Use a valid toAddress that does not exist in AccountStore
    String missingToAddress = Wallet.getAddressPreFixString() + "5555555555555555555555555555555555555555";

    // Create an asset issued by the missing address (so toAddress check can proceed)
    String altAssetId = "1000010";
    long blockTimestamp = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    AssetIssueContract altAsset = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(missingToAddress)))
        .setName(ByteString.copyFromUtf8("AltToken"))
        .setAbbr(ByteString.copyFromUtf8("ALT"))
        .setTotalSupply(TOTAL_SUPPLY)
        .setTrxNum(TRX_NUM)
        .setNum(NUM)
        .setPrecision(6)
        .setStartTime(blockTimestamp - 86400000)
        .setEndTime(blockTimestamp + 86400000L * 30)
        .setDescription(ByteString.copyFromUtf8("Alt Token"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .setId(altAssetId)
        .build();
    AssetIssueCapsule altCapsule = new AssetIssueCapsule(altAsset);
    dbManager.getAssetIssueV2Store().put(altCapsule.createDbV2Key(), altCapsule);

    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(missingToAddress)))
        .setAssetName(ByteString.copyFromUtf8(altAssetId))
        .setAmount(100_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_to_account_missing")
        .caseCategory("validate_fail")
        .description("Fail when to account (issuer account) does not exist")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .expectedError("To account does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue to account missing: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateParticipateAssetIssue_toNotIssuer() throws Exception {
    // toAddress exists but is not the asset issuer
    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS))) // Not the issuer
        .setAssetName(ByteString.copyFromUtf8(ASSET_ID)) // Issued by OWNER_ADDRESS
        .setAmount(100_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_to_not_issuer")
        .caseCategory("validate_fail")
        .description("Fail when toAddress is not the asset issuer")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .expectedError("The asset is not issued by")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue to not issuer: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateParticipateAssetIssue_saleNotStarted() throws Exception {
    // Create an asset with start time in the future
    String futureAssetId = "1000011";
    long blockTimestamp = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    AssetIssueContract futureAsset = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setName(ByteString.copyFromUtf8("FutureToken"))
        .setAbbr(ByteString.copyFromUtf8("FUT"))
        .setTotalSupply(TOTAL_SUPPLY)
        .setTrxNum(TRX_NUM)
        .setNum(NUM)
        .setPrecision(6)
        .setStartTime(blockTimestamp + 86400000L) // Starts in 1 day (future)
        .setEndTime(blockTimestamp + 86400000L * 60)
        .setDescription(ByteString.copyFromUtf8("Future Token"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .setId(futureAssetId)
        .build();

    AssetIssueCapsule futureCapsule = new AssetIssueCapsule(futureAsset);
    dbManager.getAssetIssueV2Store().put(futureCapsule.createDbV2Key(), futureCapsule);

    // Add tokens to OTHER_ADDRESS (the issuer)
    AccountCapsule other = dbManager.getAccountStore().get(ByteArray.fromHexString(OTHER_ADDRESS));
    Protocol.Account.Builder otherBuilder = other.getInstance().toBuilder();
    otherBuilder.putAssetV2(futureAssetId, TOTAL_SUPPLY / 2);
    dbManager.getAccountStore().put(other.getAddress().toByteArray(), new AccountCapsule(otherBuilder.build()));

    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(futureAssetId))
        .setAmount(100_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_sale_not_started")
        .caseCategory("validate_fail")
        .description("Fail when asset sale has not started yet")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .expectedError("No longer valid period")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue sale not started: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateParticipateAssetIssue_amountZero() throws Exception {
    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(ASSET_ID))
        .setAmount(0L) // Zero amount
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_amount_zero")
        .caseCategory("validate_fail")
        .description("Fail when amount is zero")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .expectedError("Amount must greater than 0")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue amount zero: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateParticipateAssetIssue_amountNegative() throws Exception {
    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(ASSET_ID))
        .setAmount(-1L) // Negative amount
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_amount_negative")
        .caseCategory("validate_fail")
        .description("Fail when amount is negative")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .expectedError("Amount must greater than 0")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue amount negative: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateParticipateAssetIssue_notEnoughAsset() throws Exception {
    // Create a new asset with very limited supply held by issuer
    String limitedAssetId = "1000012";
    long blockTimestamp = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    AssetIssueContract limitedAsset = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setName(ByteString.copyFromUtf8("LimitedToken"))
        .setAbbr(ByteString.copyFromUtf8("LTD"))
        .setTotalSupply(1_000_000L)
        .setTrxNum(1)
        .setNum(100) // 100 tokens per TRX
        .setPrecision(6)
        .setStartTime(blockTimestamp - 86400000)
        .setEndTime(blockTimestamp + 86400000L * 30)
        .setDescription(ByteString.copyFromUtf8("Limited Token"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .setId(limitedAssetId)
        .build();

    AssetIssueCapsule limitedCapsule = new AssetIssueCapsule(limitedAsset);
    dbManager.getAssetIssueV2Store().put(limitedCapsule.createDbV2Key(), limitedCapsule);

    // Add only 10 tokens to OTHER_ADDRESS (the issuer)
    AccountCapsule other = dbManager.getAccountStore().get(ByteArray.fromHexString(OTHER_ADDRESS));
    Protocol.Account.Builder otherBuilder = other.getInstance().toBuilder();
    otherBuilder.putAssetV2(limitedAssetId, 10L); // Only 10 tokens
    dbManager.getAccountStore().put(other.getAddress().toByteArray(), new AccountCapsule(otherBuilder.build()));

    // Try to buy 1000 TRX worth of tokens (would require 100,000 tokens)
    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(limitedAssetId))
        .setAmount(1_000_000_000L) // 1000 TRX
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_not_enough_asset")
        .caseCategory("validate_fail")
        .description("Fail when issuer does not have enough asset balance")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .expectedError("Asset balance is not enough")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue not enough asset: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateParticipateAssetIssue_exchangeAmountZero() throws Exception {
    // Create asset with high trxNum relative to num, so small amounts round to 0
    String highRatioAssetId = "1000013";
    long blockTimestamp = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    AssetIssueContract highRatioAsset = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setName(ByteString.copyFromUtf8("HighRatioToken"))
        .setAbbr(ByteString.copyFromUtf8("HRT"))
        .setTotalSupply(1_000_000_000_000L)
        .setTrxNum(1_000_000) // Very high TRX required
        .setNum(1) // Only 1 token per 1M TRX
        .setPrecision(6)
        .setStartTime(blockTimestamp - 86400000)
        .setEndTime(blockTimestamp + 86400000L * 30)
        .setDescription(ByteString.copyFromUtf8("High Ratio Token"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .setId(highRatioAssetId)
        .build();

    AssetIssueCapsule highRatioCapsule = new AssetIssueCapsule(highRatioAsset);
    dbManager.getAssetIssueV2Store().put(highRatioCapsule.createDbV2Key(), highRatioCapsule);

    // Add tokens to OTHER_ADDRESS
    AccountCapsule other = dbManager.getAccountStore().get(ByteArray.fromHexString(OTHER_ADDRESS));
    Protocol.Account.Builder otherBuilder = other.getInstance().toBuilder();
    otherBuilder.putAssetV2(highRatioAssetId, 1_000_000L);
    dbManager.getAccountStore().put(other.getAddress().toByteArray(), new AccountCapsule(otherBuilder.build()));

    // Try to buy with tiny amount (1 SUN) - floor(1 * 1 / 1000000) = 0
    ParticipateAssetIssueContract contract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(PARTICIPANT_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(highRatioAssetId))
        .setAmount(1L) // 1 SUN - too small for exchange
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ParticipateAssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PARTICIPATE_ASSET_ISSUE_CONTRACT", 9)
        .caseName("validate_fail_exchange_amount_zero")
        .caseCategory("validate_fail")
        .description("Fail when exchange amount rounds down to zero")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(PARTICIPANT_ADDRESS)
        .expectedError("Can not process the exchange")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ParticipateAssetIssue exchange amount zero: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
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
    assertTrue("Happy path should succeed", result.isSuccess());
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
    assertNotNull("Validate fail should have validation error", result.getValidationError());
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
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  // ==========================================================================
  // UnfreezeAssetContract (14) - NEW EDGE CASES
  // ==========================================================================

  @Test
  public void generateUnfreezeAsset_notIssuedAsset() throws Exception {
    // Create an account with frozen supply but no assetIssuedID set
    String noAssetIssuerAddress = Wallet.getAddressPreFixString() + "6666666666666666666666666666666666666666";
    long currentTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    AccountCapsule noAssetAccount = new AccountCapsule(
        ByteString.copyFromUtf8("noassetissuer"),
        ByteString.copyFrom(ByteArray.fromHexString(noAssetIssuerAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    // Add frozen supply but NOT set assetIssuedID
    Protocol.Account.Builder builder = noAssetAccount.getInstance().toBuilder();
    Protocol.Account.Frozen frozen = Protocol.Account.Frozen.newBuilder()
        .setFrozenBalance(50_000_000_000L)
        .setExpireTime(currentTime - 1000) // Already expired
        .build();
    builder.addFrozenSupply(frozen);
    // Note: NOT setting assetIssuedID
    dbManager.getAccountStore().put(noAssetAccount.getAddress().toByteArray(), new AccountCapsule(builder.build()));

    UnfreezeAssetContract contract = UnfreezeAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(noAssetIssuerAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_ASSET_CONTRACT", 14)
        .caseName("validate_fail_not_issued_asset")
        .caseCategory("validate_fail")
        .description("Fail when account has not issued any asset (assetIssuedID empty)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(noAssetIssuerAddress)
        .expectedError("this account has not issued any asset")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeAsset not issued asset: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUnfreezeAsset_ownerAccountMissing() throws Exception {
    // Use a valid address that does not exist in AccountStore
    String missingAddress = Wallet.getAddressPreFixString() + "7777777777777777777777777777777777777777";

    UnfreezeAssetContract contract = UnfreezeAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(missingAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_ASSET_CONTRACT", 14)
        .caseName("validate_fail_owner_account_missing")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(missingAddress)
        .expectedError("does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeAsset owner missing: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUnfreezeAsset_invalidOwnerAddress() throws Exception {
    // Use an invalid address (wrong length)
    byte[] invalidAddress = new byte[15]; // Wrong length

    UnfreezeAssetContract contract = UnfreezeAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(invalidAddress))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_ASSET_CONTRACT", 14)
        .caseName("validate_fail_invalid_owner_address")
        .caseCategory("validate_fail")
        .description("Fail when owner address has invalid format")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(ByteArray.toHexString(invalidAddress))
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeAsset invalid address: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUnfreezeAsset_partialUnfreezeSuccess() throws Exception {
    // Create an account with multiple frozen entries: some expired, some not
    String partialUnfreezeAddress = Wallet.getAddressPreFixString() + "8888888888888888888888888888888888888888";
    String partialAssetId = "1000020";
    long currentTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    // Create asset
    AssetIssueContract partialAsset = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(partialUnfreezeAddress)))
        .setName(ByteString.copyFromUtf8("PartialToken"))
        .setAbbr(ByteString.copyFromUtf8("PRT"))
        .setTotalSupply(TOTAL_SUPPLY)
        .setTrxNum(TRX_NUM)
        .setNum(NUM)
        .setPrecision(6)
        .setStartTime(currentTime - 86400000)
        .setEndTime(currentTime + 86400000L * 30)
        .setDescription(ByteString.copyFromUtf8("Partial Unfreeze Token"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .setId(partialAssetId)
        .build();

    AssetIssueCapsule partialCapsule = new AssetIssueCapsule(partialAsset);
    dbManager.getAssetIssueV2Store().put(partialCapsule.createDbV2Key(), partialCapsule);

    // Create account with two frozen entries
    AccountCapsule partialAccount = new AccountCapsule(
        ByteString.copyFromUtf8("partialunfreeze"),
        ByteString.copyFrom(ByteArray.fromHexString(partialUnfreezeAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = partialAccount.getInstance().toBuilder();
    builder.setAssetIssuedID(ByteString.copyFromUtf8(partialAssetId));
    // Frozen entry A: expired (should unfreeze)
    Protocol.Account.Frozen frozenExpired = Protocol.Account.Frozen.newBuilder()
        .setFrozenBalance(30_000_000_000L)
        .setExpireTime(currentTime - 1000) // Already expired
        .build();
    // Frozen entry B: not expired (should remain)
    Protocol.Account.Frozen frozenFuture = Protocol.Account.Frozen.newBuilder()
        .setFrozenBalance(20_000_000_000L)
        .setExpireTime(currentTime + 86400000L * 365) // Expires in 1 year
        .build();
    builder.addFrozenSupply(frozenExpired);
    builder.addFrozenSupply(frozenFuture);
    dbManager.getAccountStore().put(partialAccount.getAddress().toByteArray(), new AccountCapsule(builder.build()));

    UnfreezeAssetContract contract = UnfreezeAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(partialUnfreezeAddress)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UnfreezeAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UNFREEZE_ASSET_CONTRACT", 14)
        .caseName("edge_partial_unfreeze_success")
        .caseCategory("edge")
        .description("Successfully unfreeze only expired entries, keeping unexpired ones")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(partialUnfreezeAddress)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UnfreezeAsset partial unfreeze: success={}", result.isSuccess());
    assertTrue("Partial unfreeze should succeed", result.isSuccess());
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
    assertTrue("Happy path should succeed", result.isSuccess());
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
    assertNotNull("Validate fail should have validation error", result.getValidationError());
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
    assertNotNull("Validate fail should have validation error", result.getValidationError());
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
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  // ==========================================================================
  // UpdateAssetContract (15) - NEW EDGE CASES
  // ==========================================================================

  @Test
  public void generateUpdateAsset_ownerAccountMissing() throws Exception {
    // Use a valid address that does not exist in AccountStore
    String missingAddress = Wallet.getAddressPreFixString() + "9999999999999999999999999999999999999999";

    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(missingAddress)))
        .setDescription(ByteString.copyFromUtf8("Valid description"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setNewLimit(2000)
        .setNewPublicLimit(2000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_owner_account_missing")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(missingAddress)
        .expectedError("Account does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset owner missing: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUpdateAsset_invalidOwnerAddress() throws Exception {
    // Use an invalid address (wrong length)
    byte[] invalidAddress = new byte[15]; // Wrong length

    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(invalidAddress))
        .setDescription(ByteString.copyFromUtf8("Valid description"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setNewLimit(2000)
        .setNewPublicLimit(2000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_invalid_owner_address")
        .caseCategory("validate_fail")
        .description("Fail when owner address has invalid format")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(ByteArray.toHexString(invalidAddress))
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset invalid address: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUpdateAsset_noAssetIssued() throws Exception {
    // Create an account without assetIssuedID set
    String noAssetAddress = Wallet.getAddressPreFixString() + "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    AccountCapsule noAssetAccount = new AccountCapsule(
        ByteString.copyFromUtf8("noasset"),
        ByteString.copyFrom(ByteArray.fromHexString(noAssetAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);
    // Note: NOT setting assetIssuedID
    dbManager.getAccountStore().put(noAssetAccount.getAddress().toByteArray(), noAssetAccount);

    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(noAssetAddress)))
        .setDescription(ByteString.copyFromUtf8("Valid description"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setNewLimit(2000)
        .setNewPublicLimit(2000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_no_asset_issued")
        .caseCategory("validate_fail")
        .description("Fail when account has not issued any asset")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(noAssetAddress)
        .expectedError("Account has not issued any asset")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset no asset issued: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUpdateAsset_assetMissingInStore() throws Exception {
    // Create an account with assetIssuedID set but no corresponding asset in store
    String orphanAssetAddress = Wallet.getAddressPreFixString() + "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    String nonExistentAssetId = "9999999";

    AccountCapsule orphanAccount = new AccountCapsule(
        ByteString.copyFromUtf8("orphan"),
        ByteString.copyFrom(ByteArray.fromHexString(orphanAssetAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);

    Protocol.Account.Builder builder = orphanAccount.getInstance().toBuilder();
    builder.setAssetIssuedID(ByteString.copyFromUtf8(nonExistentAssetId)); // Asset ID not in store
    dbManager.getAccountStore().put(orphanAccount.getAddress().toByteArray(), new AccountCapsule(builder.build()));

    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(orphanAssetAddress)))
        .setDescription(ByteString.copyFromUtf8("Valid description"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setNewLimit(2000)
        .setNewPublicLimit(2000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_asset_missing_in_store")
        .caseCategory("validate_fail")
        .description("Fail when account's assetIssuedID refers to non-existent asset")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(orphanAssetAddress)
        .expectedError("Asset is not existed in AssetIssueV2Store")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset asset missing in store: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUpdateAsset_urlTooLong() throws Exception {
    // Create a very long URL (> 256 bytes)
    StringBuilder longUrl = new StringBuilder("https://example.com/");
    for (int i = 0; i < 250; i++) {
      longUrl.append("X");
    }

    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setDescription(ByteString.copyFromUtf8("Valid description"))
        .setUrl(ByteString.copyFromUtf8(longUrl.toString()))
        .setNewLimit(2000)
        .setNewPublicLimit(2000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_url_too_long")
        .caseCategory("validate_fail")
        .description("Fail when URL exceeds maximum length (256 bytes)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid url")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset URL too long: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUpdateAsset_newLimitNegative() throws Exception {
    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setDescription(ByteString.copyFromUtf8("Valid description"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setNewLimit(-1) // Negative limit
        .setNewPublicLimit(2000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_new_limit_negative")
        .caseCategory("validate_fail")
        .description("Fail when newLimit is negative")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid FreeAssetNetLimit")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset newLimit negative: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUpdateAsset_newLimitTooLarge() throws Exception {
    // Get the oneDayNetLimit and use it as newLimit (should fail, must be < oneDayNetLimit)
    long oneDayNetLimit = dbManager.getDynamicPropertiesStore().getOneDayNetLimit();

    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setDescription(ByteString.copyFromUtf8("Valid description"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setNewLimit(oneDayNetLimit) // Equal to limit (must be < limit)
        .setNewPublicLimit(2000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_new_limit_too_large")
        .caseCategory("validate_fail")
        .description("Fail when newLimit >= oneDayNetLimit")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid FreeAssetNetLimit")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset newLimit too large: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUpdateAsset_newPublicLimitNegative() throws Exception {
    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setDescription(ByteString.copyFromUtf8("Valid description"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setNewLimit(2000)
        .setNewPublicLimit(-1) // Negative public limit
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_new_public_limit_negative")
        .caseCategory("validate_fail")
        .description("Fail when newPublicLimit is negative")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid PublicFreeAssetNetLimit")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset newPublicLimit negative: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUpdateAsset_newPublicLimitTooLarge() throws Exception {
    // Get the oneDayNetLimit and use it as newPublicLimit (should fail, must be < oneDayNetLimit)
    long oneDayNetLimit = dbManager.getDynamicPropertiesStore().getOneDayNetLimit();

    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setDescription(ByteString.copyFromUtf8("Valid description"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setNewLimit(2000)
        .setNewPublicLimit(oneDayNetLimit) // Equal to limit (must be < limit)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("validate_fail_new_public_limit_too_large")
        .caseCategory("validate_fail")
        .description("Fail when newPublicLimit >= oneDayNetLimit")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid PublicFreeAssetNetLimit")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset newPublicLimit too large: validationError={}", result.getValidationError());
    assertNotNull("Validate fail should have validation error", result.getValidationError());
  }

  @Test
  public void generateUpdateAsset_edgeLimitMaxOk() throws Exception {
    // Use oneDayNetLimit - 1 for both limits (should succeed - maximum valid value)
    long oneDayNetLimit = dbManager.getDynamicPropertiesStore().getOneDayNetLimit();

    UpdateAssetContract contract = UpdateAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setDescription(ByteString.copyFromUtf8("Valid description"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setNewLimit(oneDayNetLimit - 1) // Maximum valid value
        .setNewPublicLimit(oneDayNetLimit - 1) // Maximum valid value
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_ASSET_CONTRACT", 15)
        .caseName("edge_limit_max_ok")
        .caseCategory("edge")
        .description("Successfully update with maximum valid limit values (oneDayNetLimit - 1)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateAsset edge limit max ok: success={}", result.isSuccess());
    assertTrue("Edge case with max limits should succeed", result.isSuccess());
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
    // Use deterministic timestamps from ConformanceFixtureTestSupport
    return ConformanceFixtureTestSupport.createTransaction(type, contract,
        DEFAULT_TX_TIMESTAMP, DEFAULT_TX_EXPIRATION);
  }

  private BlockCapsule createBlockContext() {
    // Use deterministic block context from ConformanceFixtureTestSupport
    return ConformanceFixtureTestSupport.createBlockContext(dbManager, OWNER_ADDRESS);
  }
}
