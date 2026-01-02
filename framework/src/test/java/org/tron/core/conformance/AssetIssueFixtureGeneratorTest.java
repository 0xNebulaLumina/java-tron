package org.tron.core.conformance;

import static org.tron.core.conformance.ConformanceFixtureTestSupport.*;

import com.google.protobuf.ByteString;
import java.io.File;
import java.util.Arrays;
import org.junit.Before;
import org.junit.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
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
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract;
import org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract.FrozenSupply;

/**
 * Generates conformance test fixtures for asset issuance:
 * - AssetIssueContract (6)
 *
 * <p>Run with: ./gradlew :framework:test --tests "AssetIssueFixtureGeneratorTest"
 * -Dconformance.output=../conformance/fixtures --dependency-verification=off
 */
public class AssetIssueFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(AssetIssueFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String WITNESS_ADDRESS;
  private static final long ASSET_ISSUE_FEE = 1024 * ONE_TRX; // 1024 TRX

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049153";
    WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "548794500882809695a8a687866e76d4271a1abc";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("AssetIssue Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Initialize dynamic properties for TRC-10 issuance
    initTrc10DynamicProps(dbManager,
        DEFAULT_BLOCK_TIMESTAMP / 1000,
        DEFAULT_BLOCK_TIMESTAMP);

    // Create owner account with sufficient balance for asset issuance fee
    putAccount(dbManager, OWNER_ADDRESS, INITIAL_BALANCE, "owner");

    // Create witness
    putAccount(dbManager, WITNESS_ADDRESS, INITIAL_BALANCE, "witness");
    putWitness(dbManager, WITNESS_ADDRESS, "https://witness.network", 10_000_000L);
  }

  // ==========================================================================
  // AssetIssueContract (6) Fixtures
  // ==========================================================================

  @Test
  public void generateAssetIssue_happyPathIssueAssetV2() throws Exception {
    String issuerAddress = generateAddress("asset_issuer_01");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "asset_issuer");

    // Asset start time must be > latestBlockHeaderTimestamp
    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000 * 30; // 30 days

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("TestToken"))
        .setAbbr(ByteString.copyFromUtf8("TT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Test token for conformance"))
        .setUrl(ByteString.copyFromUtf8("https://test-token.network"))
        .setFreeAssetNetLimit(10000)
        .setPublicFreeAssetNetLimit(10000)
        // No frozen supply for simplicity
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("happy_path_issue_asset_v2")
        .caseCategory("happy")
        .description("Issue a new TRC-10 asset with allowSameTokenName=1 (V2 mode)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 1)
        .dynamicProperty("ASSET_ISSUE_FEE", ASSET_ISSUE_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateAssetIssue_validateFailStartTimeBeforeHead() throws Exception {
    String issuerAddress = generateAddress("asset_issuer_02");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "asset_issuer2");

    // Start time in the past (before head block timestamp)
    long startTime = DEFAULT_BLOCK_TIMESTAMP - 1000;
    long endTime = DEFAULT_BLOCK_TIMESTAMP + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("PastToken"))
        .setAbbr(ByteString.copyFromUtf8("PT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with past start time"))
        .setUrl(ByteString.copyFromUtf8("https://past-token.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_start_time_before_head")
        .caseCategory("validate_fail")
        .description("Fail when start_time is before latestBlockHeaderTimestamp")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("time")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue past start: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailTotalSupplyZero() throws Exception {
    String issuerAddress = generateAddress("asset_issuer_03");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "asset_issuer3");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("ZeroToken"))
        .setAbbr(ByteString.copyFromUtf8("ZT"))
        .setTotalSupply(0) // Zero total supply
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with zero supply"))
        .setUrl(ByteString.copyFromUtf8("https://zero-token.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_total_supply_zero")
        .caseCategory("validate_fail")
        .description("Fail when total_supply is zero")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("supply")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue zero supply: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailOwnerAlreadyIssued() throws Exception {
    String issuerAddress = generateAddress("asset_issuer_04");

    // Create account that has already issued an asset
    AccountCapsule issuerAccount = putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "asset_issuer4");
    Protocol.Account.Builder builder = issuerAccount.getInstance().toBuilder();
    builder.setAssetIssuedName(ByteString.copyFromUtf8("ExistingToken"));
    issuerAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(issuerAccount.getAddress().toByteArray(), issuerAccount);

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("SecondToken"))
        .setAbbr(ByteString.copyFromUtf8("ST"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Second token attempt"))
        .setUrl(ByteString.copyFromUtf8("https://second-token.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_owner_already_issued")
        .caseCategory("validate_fail")
        .description("Fail when owner has already issued an asset")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("asset")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue already issued: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailInsufficientBalance() throws Exception {
    String poorIssuer = generateAddress("poor_issuer_001");
    putAccount(dbManager, poorIssuer, ONE_TRX, "poor_issuer"); // Only 1 TRX

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(poorIssuer)))
        .setName(ByteString.copyFromUtf8("PoorToken"))
        .setAbbr(ByteString.copyFromUtf8("PO"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token from poor account"))
        .setUrl(ByteString.copyFromUtf8("https://poor-token.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_insufficient_balance")
        .caseCategory("validate_fail")
        .description("Fail when owner has insufficient balance for asset issuance fee")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(poorIssuer)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue insufficient: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailInvalidName() throws Exception {
    String issuerAddress = generateAddress("asset_issuer_05");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "asset_issuer5");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    // Invalid name - "trx" is reserved
    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("trx"))
        .setAbbr(ByteString.copyFromUtf8("TRX"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Invalid TRX name"))
        .setUrl(ByteString.copyFromUtf8("https://trx-token.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_invalid_name_trx")
        .caseCategory("validate_fail")
        .description("Fail when asset name is 'trx' (reserved)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("name")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue invalid name: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Phase 1: Owner / Address / Account Branches
  // ==========================================================================

  @Test
  public void generateAssetIssue_validateFailOwnerAddressInvalidEmpty() throws Exception {
    // Empty owner address
    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Empty address
        .setName(ByteString.copyFromUtf8("EmptyOwnerToken"))
        .setAbbr(ByteString.copyFromUtf8("EOT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with empty owner"))
        .setUrl(ByteString.copyFromUtf8("https://empty-owner.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner_address is empty (fails DecodeUtil.addressValid)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue empty owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailOwnerAccountNotExists() throws Exception {
    // Valid address format but account not in store
    String nonExistentAddress = generateAddress("non_existent_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
        .setName(ByteString.copyFromUtf8("NoAccountToken"))
        .setAbbr(ByteString.copyFromUtf8("NAT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token from non-existent account"))
        .setUrl(ByteString.copyFromUtf8("https://no-account.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_owner_account_not_exists")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(nonExistentAddress)
        .expectedError("Account not exists")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue account not exists: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_happyPathBalanceEqualsFee() throws Exception {
    String exactBalanceIssuer = generateAddress("exact_balance_issuer");
    // Balance exactly equals ASSET_ISSUE_FEE
    putAccount(dbManager, exactBalanceIssuer, ASSET_ISSUE_FEE, "exact_balance_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000 * 30;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(exactBalanceIssuer)))
        .setName(ByteString.copyFromUtf8("ExactBalanceToken"))
        .setAbbr(ByteString.copyFromUtf8("EBT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with exact fee balance"))
        .setUrl(ByteString.copyFromUtf8("https://exact-balance.network"))
        .setFreeAssetNetLimit(10000)
        .setPublicFreeAssetNetLimit(10000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("happy_path_balance_equals_fee")
        .caseCategory("happy")
        .description("Success when owner balance exactly equals ASSET_ISSUE_FEE")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(exactBalanceIssuer)
        .dynamicProperty("ASSET_ISSUE_FEE", ASSET_ISSUE_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue exact balance: success={}", result.isSuccess());
  }

  @Test
  public void generateAssetIssue_validateFailBalanceFeeMinus1() throws Exception {
    String insufficientIssuer = generateAddress("insufficient_issuer");
    // Balance is ASSET_ISSUE_FEE - 1
    putAccount(dbManager, insufficientIssuer, ASSET_ISSUE_FEE - 1, "insufficient_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(insufficientIssuer)))
        .setName(ByteString.copyFromUtf8("ShortBalanceToken"))
        .setAbbr(ByteString.copyFromUtf8("SBT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with balance 1 SUN short"))
        .setUrl(ByteString.copyFromUtf8("https://short-balance.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_balance_fee_minus_1")
        .caseCategory("validate_fail")
        .description("Fail when owner balance is ASSET_ISSUE_FEE - 1")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(insufficientIssuer)
        .expectedError("No enough balance for fee!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue balance minus 1: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Phase 2: Asset Name / Abbreviation Validation
  // ==========================================================================

  @Test
  public void generateAssetIssue_validateFailAssetNameEmpty() throws Exception {
    String issuerAddress = generateAddress("name_empty_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "name_empty_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.EMPTY) // Empty name
        .setAbbr(ByteString.copyFromUtf8("ENT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with empty name"))
        .setUrl(ByteString.copyFromUtf8("https://empty-name.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_asset_name_empty")
        .caseCategory("validate_fail")
        .description("Fail when asset name is empty")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Invalid assetName")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue empty name: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailAssetNameTooLong33() throws Exception {
    String issuerAddress = generateAddress("name_long_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "name_long_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    // 33 byte name (max is 32)
    byte[] longName = new byte[33];
    Arrays.fill(longName, (byte) 'A'); // All 'A's (0x41, in valid range)

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFrom(longName))
        .setAbbr(ByteString.copyFromUtf8("LNT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with 33 byte name"))
        .setUrl(ByteString.copyFromUtf8("https://long-name.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_asset_name_too_long_33")
        .caseCategory("validate_fail")
        .description("Fail when asset name length is 33 bytes (max is 32)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Invalid assetName")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue name too long: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailAssetNameContainsSpace() throws Exception {
    String issuerAddress = generateAddress("name_space_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "name_space_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("Token Name")) // Contains space (0x20)
        .setAbbr(ByteString.copyFromUtf8("TN"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with space in name"))
        .setUrl(ByteString.copyFromUtf8("https://space-name.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_asset_name_contains_space")
        .caseCategory("validate_fail")
        .description("Fail when asset name contains space (0x20, below valid range 0x21-0x7E)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Invalid assetName")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue name with space: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailAssetNameNonAscii() throws Exception {
    String issuerAddress = generateAddress("name_nonascii_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "name_nonascii_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    // Name with byte > 0x7E
    byte[] nonAsciiName = new byte[] {'T', 'o', 'k', 'e', 'n', (byte) 0x80};

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFrom(nonAsciiName))
        .setAbbr(ByteString.copyFromUtf8("NAT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with non-ASCII byte"))
        .setUrl(ByteString.copyFromUtf8("https://nonascii.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_asset_name_non_ascii")
        .caseCategory("validate_fail")
        .description("Fail when asset name contains byte > 0x7E")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Invalid assetName")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue non-ASCII name: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailAssetNameReservedTRXUppercase() throws Exception {
    String issuerAddress = generateAddress("name_trx_upper_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "name_trx_upper_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("TRX")) // Uppercase reserved name
        .setAbbr(ByteString.copyFromUtf8("TRX"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with uppercase TRX name"))
        .setUrl(ByteString.copyFromUtf8("https://trx-upper.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_asset_name_reserved_trx_uppercase")
        .caseCategory("validate_fail")
        .description("Fail when asset name is 'TRX' (case-insensitive reserved check)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 1)
        .expectedError("assetName can't be trx")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue TRX uppercase: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_happyPathAbbrEmpty() throws Exception {
    String issuerAddress = generateAddress("abbr_empty_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "abbr_empty_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000 * 30;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("NoAbbrToken"))
        // No abbr set - empty is allowed
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with no abbreviation"))
        .setUrl(ByteString.copyFromUtf8("https://no-abbr.network"))
        .setFreeAssetNetLimit(10000)
        .setPublicFreeAssetNetLimit(10000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("happy_path_abbr_empty")
        .caseCategory("happy")
        .description("Success when abbreviation is empty (abbr is optional)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue empty abbr: success={}", result.isSuccess());
  }

  @Test
  public void generateAssetIssue_validateFailAbbrInvalidContainsSpace() throws Exception {
    String issuerAddress = generateAddress("abbr_space_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "abbr_space_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("SpaceAbbrToken"))
        .setAbbr(ByteString.copyFromUtf8("S T")) // Contains space
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with space in abbr"))
        .setUrl(ByteString.copyFromUtf8("https://space-abbr.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_abbr_invalid_contains_space")
        .caseCategory("validate_fail")
        .description("Fail when abbreviation contains space")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Invalid abbreviation for token")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue abbr with space: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailAbbrTooLong33() throws Exception {
    String issuerAddress = generateAddress("abbr_long_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "abbr_long_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    // 33 byte abbr (max for validAssetName is 32)
    byte[] longAbbr = new byte[33];
    Arrays.fill(longAbbr, (byte) 'A');

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("LongAbbrToken"))
        .setAbbr(ByteString.copyFrom(longAbbr))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with 33 byte abbr"))
        .setUrl(ByteString.copyFromUtf8("https://long-abbr.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_abbr_too_long_33")
        .caseCategory("validate_fail")
        .description("Fail when abbreviation length is 33 bytes (max is 32)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Invalid abbreviation for token")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue abbr too long: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Phase 3: URL and Description Branches
  // ==========================================================================

  @Test
  public void generateAssetIssue_validateFailUrlEmpty() throws Exception {
    String issuerAddress = generateAddress("url_empty_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "url_empty_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("NoUrlToken"))
        .setAbbr(ByteString.copyFromUtf8("NUT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with empty URL"))
        .setUrl(ByteString.EMPTY) // Empty URL
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_url_empty")
        .caseCategory("validate_fail")
        .description("Fail when URL is empty (URL is required)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Invalid url")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue empty URL: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailUrlTooLong257() throws Exception {
    String issuerAddress = generateAddress("url_long_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "url_long_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    // 257 byte URL (max is 256)
    StringBuilder urlBuilder = new StringBuilder("https://");
    for (int i = 0; i < 249; i++) { // 8 (https://) + 249 = 257
      urlBuilder.append('a');
    }

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("LongUrlToken"))
        .setAbbr(ByteString.copyFromUtf8("LUT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with 257 byte URL"))
        .setUrl(ByteString.copyFromUtf8(urlBuilder.toString()))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_url_too_long_257")
        .caseCategory("validate_fail")
        .description("Fail when URL length is 257 bytes (max is 256)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Invalid url")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue URL too long: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_happyPathDescriptionEmpty() throws Exception {
    String issuerAddress = generateAddress("desc_empty_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "desc_empty_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000 * 30;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("NoDescToken"))
        .setAbbr(ByteString.copyFromUtf8("NDT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        // No description - empty is allowed
        .setUrl(ByteString.copyFromUtf8("https://no-desc.network"))
        .setFreeAssetNetLimit(10000)
        .setPublicFreeAssetNetLimit(10000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("happy_path_description_empty")
        .caseCategory("happy")
        .description("Success when description is empty (description allows empty)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue empty description: success={}", result.isSuccess());
  }

  @Test
  public void generateAssetIssue_validateFailDescriptionTooLong201() throws Exception {
    String issuerAddress = generateAddress("desc_long_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "desc_long_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    // 201 byte description (max is 200)
    StringBuilder descBuilder = new StringBuilder();
    for (int i = 0; i < 201; i++) {
      descBuilder.append('a');
    }

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("LongDescToken"))
        .setAbbr(ByteString.copyFromUtf8("LDT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8(descBuilder.toString()))
        .setUrl(ByteString.copyFromUtf8("https://long-desc.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_description_too_long_201")
        .caseCategory("validate_fail")
        .description("Fail when description length is 201 bytes (max is 200)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Invalid description")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue description too long: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Phase 4: Time Field Branches
  // ==========================================================================

  @Test
  public void generateAssetIssue_validateFailStartTimeZero() throws Exception {
    String issuerAddress = generateAddress("start_zero_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "start_zero_issuer");

    long endTime = DEFAULT_BLOCK_TIMESTAMP + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("ZeroStartToken"))
        .setAbbr(ByteString.copyFromUtf8("ZST"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(0) // Zero start time
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with zero start time"))
        .setUrl(ByteString.copyFromUtf8("https://zero-start.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_start_time_zero")
        .caseCategory("validate_fail")
        .description("Fail when start_time is 0")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Start time should be not empty")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue zero start time: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailEndTimeZero() throws Exception {
    String issuerAddress = generateAddress("end_zero_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "end_zero_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("ZeroEndToken"))
        .setAbbr(ByteString.copyFromUtf8("ZET"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(0) // Zero end time
        .setDescription(ByteString.copyFromUtf8("Token with zero end time"))
        .setUrl(ByteString.copyFromUtf8("https://zero-end.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_end_time_zero")
        .caseCategory("validate_fail")
        .description("Fail when end_time is 0")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("End time should be not empty")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue zero end time: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailEndTimeEqualsStartTime() throws Exception {
    String issuerAddress = generateAddress("end_eq_start_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "end_eq_start_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("EqualTimeToken"))
        .setAbbr(ByteString.copyFromUtf8("ETT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(startTime) // End == Start
        .setDescription(ByteString.copyFromUtf8("Token with end equal to start"))
        .setUrl(ByteString.copyFromUtf8("https://equal-time.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_end_time_equals_start_time")
        .caseCategory("validate_fail")
        .description("Fail when end_time equals start_time")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("End time should be greater than start time")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue end equals start: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailEndTimeBeforeStartTime() throws Exception {
    String issuerAddress = generateAddress("end_before_start_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "end_before_start_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("ReversedTimeToken"))
        .setAbbr(ByteString.copyFromUtf8("RTT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(startTime - 1) // End < Start
        .setDescription(ByteString.copyFromUtf8("Token with end before start"))
        .setUrl(ByteString.copyFromUtf8("https://reversed-time.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_end_time_before_start_time")
        .caseCategory("validate_fail")
        .description("Fail when end_time < start_time")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("End time should be greater than start time")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue end before start: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailStartTimeEqualsHeadBlockTime() throws Exception {
    String issuerAddress = generateAddress("start_eq_head_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "start_eq_head_issuer");

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);
    // Get the updated head block timestamp after createBlockContext
    long headBlockTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("HeadTimeToken"))
        .setAbbr(ByteString.copyFromUtf8("HTT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(headBlockTime) // Exactly head block time
        .setEndTime(headBlockTime + 86400000)
        .setDescription(ByteString.copyFromUtf8("Token with start equal to head block time"))
        .setUrl(ByteString.copyFromUtf8("https://head-time.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_start_time_equals_head_block_time")
        .caseCategory("validate_fail")
        .description("Fail when start_time equals latestBlockHeaderTimestamp")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Start time should be greater than HeadBlockTime")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue start equals head: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_happyPathStartTimeJustAfterHeadBlockTime() throws Exception {
    String issuerAddress = generateAddress("start_after_head_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "start_after_head_issuer");

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);
    // Get the updated head block timestamp after createBlockContext
    long headBlockTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("JustAfterToken"))
        .setAbbr(ByteString.copyFromUtf8("JAT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(headBlockTime + 1) // Just 1ms after head block time
        .setEndTime(headBlockTime + 86400000)
        .setDescription(ByteString.copyFromUtf8("Token with start just after head"))
        .setUrl(ByteString.copyFromUtf8("https://just-after.network"))
        .setFreeAssetNetLimit(10000)
        .setPublicFreeAssetNetLimit(10000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("happy_path_start_time_just_after_head_block_time")
        .caseCategory("happy")
        .description("Success when start_time = headBlockTime + 1")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue start just after head: success={}", result.isSuccess());
  }

  // ==========================================================================
  // Phase 5: Numeric Fields / Flags
  // ==========================================================================

  @Test
  public void generateAssetIssue_validateFailTrxNumZero() throws Exception {
    String issuerAddress = generateAddress("trx_num_zero_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "trx_num_zero_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("ZeroTrxNumToken"))
        .setAbbr(ByteString.copyFromUtf8("ZTN"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(0) // Zero trx_num
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with zero trx_num"))
        .setUrl(ByteString.copyFromUtf8("https://zero-trxnum.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_trx_num_zero")
        .caseCategory("validate_fail")
        .description("Fail when trx_num is 0")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("TrxNum must greater than 0!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue zero trx_num: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailNumZero() throws Exception {
    String issuerAddress = generateAddress("num_zero_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "num_zero_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("ZeroNumToken"))
        .setAbbr(ByteString.copyFromUtf8("ZNT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(0) // Zero num
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with zero num"))
        .setUrl(ByteString.copyFromUtf8("https://zero-num.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_num_zero")
        .caseCategory("validate_fail")
        .description("Fail when num is 0")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Num must greater than 0!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue zero num: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailPrecisionHigh7() throws Exception {
    String issuerAddress = generateAddress("precision_high_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "precision_high_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("HighPrecisionToken"))
        .setAbbr(ByteString.copyFromUtf8("HPT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(7) // Precision > 6
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with precision 7"))
        .setUrl(ByteString.copyFromUtf8("https://high-precision.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_precision_high_7")
        .caseCategory("validate_fail")
        .description("Fail when precision is 7 (max is 6)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 1)
        .expectedError("precision cannot exceed 6")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue precision too high: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailPrecisionNegative1() throws Exception {
    String issuerAddress = generateAddress("precision_neg_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "precision_neg_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("NegPrecisionToken"))
        .setAbbr(ByteString.copyFromUtf8("NPT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(-1) // Negative precision
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with precision -1"))
        .setUrl(ByteString.copyFromUtf8("https://neg-precision.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_precision_negative_1")
        .caseCategory("validate_fail")
        .description("Fail when precision is -1 (negative)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 1)
        .expectedError("precision cannot exceed 6")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue negative precision: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_happyPathPrecisionZero() throws Exception {
    String issuerAddress = generateAddress("precision_zero_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "precision_zero_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000 * 30;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("ZeroPrecisionToken"))
        .setAbbr(ByteString.copyFromUtf8("ZPT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(0) // Zero precision - allowed
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with precision 0"))
        .setUrl(ByteString.copyFromUtf8("https://zero-precision.network"))
        .setFreeAssetNetLimit(10000)
        .setPublicFreeAssetNetLimit(10000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("happy_path_precision_zero")
        .caseCategory("happy")
        .description("Success when precision is 0 (boundary-happy)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue zero precision: success={}", result.isSuccess());
  }

  @Test
  public void generateAssetIssue_validateFailPublicFreeAssetNetUsageNonZero() throws Exception {
    String issuerAddress = generateAddress("pub_net_usage_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "pub_net_usage_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("PubNetUsageToken"))
        .setAbbr(ByteString.copyFromUtf8("PNU"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with non-zero public_free_asset_net_usage"))
        .setUrl(ByteString.copyFromUtf8("https://pub-net-usage.network"))
        .setPublicFreeAssetNetUsage(1) // Non-zero
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_public_free_asset_net_usage_non_zero")
        .caseCategory("validate_fail")
        .description("Fail when public_free_asset_net_usage is non-zero")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("PublicFreeAssetNetUsage must be 0!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue non-zero pub_net_usage: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailFreeAssetNetLimitNegative() throws Exception {
    String issuerAddress = generateAddress("free_net_neg_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "free_net_neg_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("NegFreeNetToken"))
        .setAbbr(ByteString.copyFromUtf8("NFN"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with negative free_asset_net_limit"))
        .setUrl(ByteString.copyFromUtf8("https://neg-free-net.network"))
        .setFreeAssetNetLimit(-1) // Negative
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_free_asset_net_limit_negative")
        .caseCategory("validate_fail")
        .description("Fail when free_asset_net_limit is negative")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Invalid FreeAssetNetLimit")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue negative free_net_limit: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailFreeAssetNetLimitEqualOneDayNetLimit() throws Exception {
    String issuerAddress = generateAddress("free_net_eq_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "free_net_eq_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;
    long oneDayNetLimit = 300_000_000L; // Default value

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("MaxFreeNetToken"))
        .setAbbr(ByteString.copyFromUtf8("MFN"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with free_asset_net_limit equal to oneDayNetLimit"))
        .setUrl(ByteString.copyFromUtf8("https://max-free-net.network"))
        .setFreeAssetNetLimit(oneDayNetLimit) // Equal to oneDayNetLimit
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_free_asset_net_limit_equal_one_day_net_limit")
        .caseCategory("validate_fail")
        .description("Fail when free_asset_net_limit equals oneDayNetLimit")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ONE_DAY_NET_LIMIT", oneDayNetLimit)
        .expectedError("Invalid FreeAssetNetLimit")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue free_net_limit equals max: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailPublicFreeAssetNetLimitNegative() throws Exception {
    String issuerAddress = generateAddress("pub_net_neg_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "pub_net_neg_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("NegPubNetToken"))
        .setAbbr(ByteString.copyFromUtf8("NPN"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with negative public_free_asset_net_limit"))
        .setUrl(ByteString.copyFromUtf8("https://neg-pub-net.network"))
        .setPublicFreeAssetNetLimit(-1) // Negative
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_public_free_asset_net_limit_negative")
        .caseCategory("validate_fail")
        .description("Fail when public_free_asset_net_limit is negative")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Invalid PublicFreeAssetNetLimit")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue negative pub_net_limit: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailPublicFreeAssetNetLimitEqualOneDayNetLimit() throws Exception {
    String issuerAddress = generateAddress("pub_net_eq_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "pub_net_eq_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;
    long oneDayNetLimit = 300_000_000L;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("MaxPubNetToken"))
        .setAbbr(ByteString.copyFromUtf8("MPN"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with public_free_asset_net_limit equal to oneDayNetLimit"))
        .setUrl(ByteString.copyFromUtf8("https://max-pub-net.network"))
        .setPublicFreeAssetNetLimit(oneDayNetLimit) // Equal to oneDayNetLimit
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_public_free_asset_net_limit_equal_one_day_net_limit")
        .caseCategory("validate_fail")
        .description("Fail when public_free_asset_net_limit equals oneDayNetLimit")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ONE_DAY_NET_LIMIT", oneDayNetLimit)
        .expectedError("Invalid PublicFreeAssetNetLimit")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue pub_net_limit equals max: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Phase 6: Frozen Supply List
  // ==========================================================================

  @Test
  public void generateAssetIssue_happyPathWithValidFrozenSupply() throws Exception {
    String issuerAddress = generateAddress("frozen_supply_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "frozen_supply_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000 * 30;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("FrozenSupplyToken"))
        .setAbbr(ByteString.copyFromUtf8("FST"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with frozen supply"))
        .setUrl(ByteString.copyFromUtf8("https://frozen-supply.network"))
        .setFreeAssetNetLimit(10000)
        .setPublicFreeAssetNetLimit(10000)
        .addFrozenSupply(FrozenSupply.newBuilder()
            .setFrozenAmount(100_000_000L)
            .setFrozenDays(30) // Within min/max range
            .build())
        .addFrozenSupply(FrozenSupply.newBuilder()
            .setFrozenAmount(200_000_000L)
            .setFrozenDays(60)
            .build())
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("happy_path_with_valid_frozen_supply")
        .caseCategory("happy")
        .description("Success with valid frozen supply entries")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 1)
        .dynamicProperty("MIN_FROZEN_SUPPLY_TIME", 1)
        .dynamicProperty("MAX_FROZEN_SUPPLY_TIME", 3652)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue with frozen supply: success={}", result.isSuccess());
  }

  @Test
  public void generateAssetIssue_validateFailFrozenSupplyListTooLong() throws Exception {
    String issuerAddress = generateAddress("frozen_list_long_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "frozen_list_long_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract.Builder contractBuilder = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("TooManyFrozenToken"))
        .setAbbr(ByteString.copyFromUtf8("TMF"))
        .setTotalSupply(100_000_000_000L) // Large supply to accommodate many frozen entries
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with too many frozen supply entries"))
        .setUrl(ByteString.copyFromUtf8("https://too-many-frozen.network"));

    // Add maxFrozenSupplyNumber + 1 entries (11 entries, max is 10)
    for (int i = 0; i < 11; i++) {
      contractBuilder.addFrozenSupply(FrozenSupply.newBuilder()
          .setFrozenAmount(1_000_000L)
          .setFrozenDays(30)
          .build());
    }

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contractBuilder.build());

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_frozen_supply_list_too_long")
        .caseCategory("validate_fail")
        .description("Fail when frozen supply list has more than maxFrozenSupplyNumber entries")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("MAX_FROZEN_SUPPLY_NUMBER", 10)
        .expectedError("Frozen supply list length is too long")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue frozen list too long: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailFrozenAmountZero() throws Exception {
    String issuerAddress = generateAddress("frozen_amt_zero_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "frozen_amt_zero_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("ZeroFrozenToken"))
        .setAbbr(ByteString.copyFromUtf8("ZFT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with zero frozen amount"))
        .setUrl(ByteString.copyFromUtf8("https://zero-frozen.network"))
        .addFrozenSupply(FrozenSupply.newBuilder()
            .setFrozenAmount(0) // Zero amount
            .setFrozenDays(30)
            .build())
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_frozen_amount_zero")
        .caseCategory("validate_fail")
        .description("Fail when frozen_amount is 0")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Frozen supply must be greater than 0!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue zero frozen amount: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailFrozenAmountExceedsTotalSupply() throws Exception {
    String issuerAddress = generateAddress("frozen_exceed_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "frozen_exceed_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("ExceedFrozenToken"))
        .setAbbr(ByteString.copyFromUtf8("EFT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with frozen amount exceeding total"))
        .setUrl(ByteString.copyFromUtf8("https://exceed-frozen.network"))
        .addFrozenSupply(FrozenSupply.newBuilder()
            .setFrozenAmount(1_000_000_001L) // Exceeds total supply
            .setFrozenDays(30)
            .build())
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_frozen_amount_exceeds_total_supply")
        .caseCategory("validate_fail")
        .description("Fail when frozen_amount exceeds total_supply")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Frozen supply cannot exceed total supply")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue frozen exceeds total: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailFrozenAmountSumExceedsTotalSupply() throws Exception {
    String issuerAddress = generateAddress("frozen_sum_exceed_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "frozen_sum_exceed_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("SumExceedToken"))
        .setAbbr(ByteString.copyFromUtf8("SET"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with frozen sum exceeding total"))
        .setUrl(ByteString.copyFromUtf8("https://sum-exceed.network"))
        .addFrozenSupply(FrozenSupply.newBuilder()
            .setFrozenAmount(600_000_000L)
            .setFrozenDays(30)
            .build())
        .addFrozenSupply(FrozenSupply.newBuilder()
            .setFrozenAmount(500_000_000L) // Sum = 1.1B > total 1B
            .setFrozenDays(60)
            .build())
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_frozen_amount_sum_exceeds_total_supply")
        .caseCategory("validate_fail")
        .description("Fail when cumulative frozen amounts exceed total_supply")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .expectedError("Frozen supply cannot exceed total supply")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue frozen sum exceeds total: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailFrozenDaysBelowMin() throws Exception {
    String issuerAddress = generateAddress("frozen_days_low_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "frozen_days_low_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("LowDaysToken"))
        .setAbbr(ByteString.copyFromUtf8("LDT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with frozen days below min"))
        .setUrl(ByteString.copyFromUtf8("https://low-days.network"))
        .addFrozenSupply(FrozenSupply.newBuilder()
            .setFrozenAmount(100_000_000L)
            .setFrozenDays(0) // Below min (1)
            .build())
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_frozen_days_below_min")
        .caseCategory("validate_fail")
        .description("Fail when frozen_days is below minFrozenSupplyTime")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("MIN_FROZEN_SUPPLY_TIME", 1)
        .expectedError("frozenDuration must be less than")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue frozen days below min: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAssetIssue_validateFailFrozenDaysAboveMax() throws Exception {
    String issuerAddress = generateAddress("frozen_days_high_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "frozen_days_high_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("HighDaysToken"))
        .setAbbr(ByteString.copyFromUtf8("HDT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with frozen days above max"))
        .setUrl(ByteString.copyFromUtf8("https://high-days.network"))
        .addFrozenSupply(FrozenSupply.newBuilder()
            .setFrozenAmount(100_000_000L)
            .setFrozenDays(3653) // Above max (3652)
            .build())
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_frozen_days_above_max")
        .caseCategory("validate_fail")
        .description("Fail when frozen_days is above maxFrozenSupplyTime")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("MAX_FROZEN_SUPPLY_TIME", 3652)
        .expectedError("frozenDuration must be less than")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue frozen days above max: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Phase 7: V1 Mode Differences (ALLOW_SAME_TOKEN_NAME=0)
  // ==========================================================================

  @Test
  public void generateAssetIssue_happyPathIssueAssetV1() throws Exception {
    // Reset to V1 mode
    initCommonDynamicPropsV1(dbManager,
        DEFAULT_BLOCK_TIMESTAMP / 1000,
        DEFAULT_BLOCK_TIMESTAMP);
    // Set TRC-10 specific props for V1
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(0); // V1 mode
    dbManager.getDynamicPropertiesStore().saveAssetIssueFee(ASSET_ISSUE_FEE);
    dbManager.getDynamicPropertiesStore().saveTokenIdNum(1000000);
    dbManager.getDynamicPropertiesStore().saveMaxFrozenSupplyNumber(10);
    dbManager.getDynamicPropertiesStore().saveOneDayNetLimit(300_000_000L);
    dbManager.getDynamicPropertiesStore().saveMinFrozenSupplyTime(1);
    dbManager.getDynamicPropertiesStore().saveMaxFrozenSupplyTime(3652);

    String issuerAddress = generateAddress("v1_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "v1_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000 * 30;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("V1Token"))
        .setAbbr(ByteString.copyFromUtf8("V1T"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6) // Will be forced to 0 in V2 store
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("V1 mode token issuance"))
        .setUrl(ByteString.copyFromUtf8("https://v1-token.network"))
        .setFreeAssetNetLimit(10000)
        .setPublicFreeAssetNetLimit(10000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("happy_path_issue_asset_v1")
        .caseCategory("happy")
        .description("V1 mode issuance: writes to both asset-issue and asset-issue-v2, v2 precision forced to 0")
        .database("account")
        .database("asset-issue")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 0)
        .dynamicProperty("ASSET_ISSUE_FEE", ASSET_ISSUE_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue V1 mode: success={}", result.isSuccess());
  }

  @Test
  public void generateAssetIssue_validateFailTokenExistsV1() throws Exception {
    // Reset to V1 mode
    initCommonDynamicPropsV1(dbManager,
        DEFAULT_BLOCK_TIMESTAMP / 1000,
        DEFAULT_BLOCK_TIMESTAMP);
    // Set TRC-10 specific props for V1
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(0); // V1 mode
    dbManager.getDynamicPropertiesStore().saveAssetIssueFee(ASSET_ISSUE_FEE);
    dbManager.getDynamicPropertiesStore().saveTokenIdNum(1000000);
    dbManager.getDynamicPropertiesStore().saveMaxFrozenSupplyNumber(10);
    dbManager.getDynamicPropertiesStore().saveOneDayNetLimit(300_000_000L);
    dbManager.getDynamicPropertiesStore().saveMinFrozenSupplyTime(1);
    dbManager.getDynamicPropertiesStore().saveMaxFrozenSupplyTime(3652);

    String existingOwner = generateAddress("existing_token_owner");
    String newIssuer = generateAddress("new_v1_issuer");

    // Seed an existing token with the same name in asset-issue store
    AssetIssueContract existingToken = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(existingOwner)))
        .setName(ByteString.copyFromUtf8("DuplicateName"))
        .setAbbr(ByteString.copyFromUtf8("DUP"))
        .setId("1000001")
        .setTotalSupply(1_000_000_000L)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(DEFAULT_BLOCK_TIMESTAMP - 1000)
        .setEndTime(DEFAULT_BLOCK_TIMESTAMP + 86400000 * 365)
        .setDescription(ByteString.copyFromUtf8("Existing token"))
        .setUrl(ByteString.copyFromUtf8("https://existing.network"))
        .build();
    AssetIssueCapsule existingCapsule = new AssetIssueCapsule(existingToken);
    dbManager.getAssetIssueStore().put(existingCapsule.createDbKey(), existingCapsule);

    putAccount(dbManager, newIssuer, INITIAL_BALANCE, "new_v1_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(newIssuer)))
        .setName(ByteString.copyFromUtf8("DuplicateName")) // Same name as existing
        .setAbbr(ByteString.copyFromUtf8("DUP"))
        .setTotalSupply(500_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Duplicate token name attempt"))
        .setUrl(ByteString.copyFromUtf8("https://duplicate.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("validate_fail_token_exists_v1")
        .caseCategory("validate_fail")
        .description("V1 mode: fail when token name already exists in asset-issue store")
        .database("account")
        .database("asset-issue")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(newIssuer)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 0)
        .expectedError("Token exists")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue V1 token exists: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Phase 8: Fee Sink (Burn vs Blackhole)
  // ==========================================================================

  @Test
  public void generateAssetIssue_happyPathFeeBurnEnabled() throws Exception {
    // Reset dynamic properties with blackhole optimization enabled (burn mode)
    initTrc10DynamicProps(dbManager,
        DEFAULT_BLOCK_TIMESTAMP / 1000,
        DEFAULT_BLOCK_TIMESTAMP);
    dbManager.getDynamicPropertiesStore().saveAllowBlackHoleOptimization(1); // Enable burn

    String issuerAddress = generateAddress("fee_burn_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "fee_burn_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000 * 30;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("BurnFeeToken"))
        .setAbbr(ByteString.copyFromUtf8("BFT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with fee burn enabled"))
        .setUrl(ByteString.copyFromUtf8("https://burn-fee.network"))
        .setFreeAssetNetLimit(10000)
        .setPublicFreeAssetNetLimit(10000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("happy_path_fee_burn_enabled")
        .caseCategory("happy")
        .description("Fee burn mode: fee is burned via dynamicStore.burnTrx() instead of crediting blackhole")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 1)
        .dynamicProperty("ALLOW_BLACK_HOLE_OPTIMIZATION", 1)
        .dynamicProperty("ASSET_ISSUE_FEE", ASSET_ISSUE_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue fee burn enabled: success={}", result.isSuccess());
  }

  @Test
  public void generateAssetIssue_happyPathFeeToBlackholeDisabled() throws Exception {
    // Reset dynamic properties with blackhole optimization disabled (credit blackhole)
    initTrc10DynamicProps(dbManager,
        DEFAULT_BLOCK_TIMESTAMP / 1000,
        DEFAULT_BLOCK_TIMESTAMP);
    dbManager.getDynamicPropertiesStore().saveAllowBlackHoleOptimization(0); // Disable burn

    // Ensure blackhole account exists to receive the fee
    String blackholeAddress = ByteArray.toHexString(
        dbManager.getAccountStore().getBlackhole().getAddress().toByteArray());
    // The blackhole account should already exist from test setup, but let's ensure it has balance tracking
    if (dbManager.getAccountStore().get(dbManager.getAccountStore().getBlackhole().getAddress().toByteArray()) == null) {
      putAccount(dbManager, blackholeAddress, 0, "blackhole");
    }

    String issuerAddress = generateAddress("fee_blackhole_issuer");
    putAccount(dbManager, issuerAddress, INITIAL_BALANCE, "fee_blackhole_issuer");

    long startTime = DEFAULT_BLOCK_TIMESTAMP + DEFAULT_BLOCK_INTERVAL_MS + 1000;
    long endTime = startTime + 86400000 * 30;

    AssetIssueContract contract = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(issuerAddress)))
        .setName(ByteString.copyFromUtf8("BlackholeFeeToken"))
        .setAbbr(ByteString.copyFromUtf8("BHT"))
        .setTotalSupply(1_000_000_000L)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Token with fee to blackhole"))
        .setUrl(ByteString.copyFromUtf8("https://blackhole-fee.network"))
        .setFreeAssetNetLimit(10000)
        .setPublicFreeAssetNetLimit(10000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AssetIssueContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ASSET_ISSUE_CONTRACT", 6)
        .caseName("happy_path_fee_to_blackhole_disabled")
        .caseCategory("happy")
        .description("Blackhole mode: fee is credited to blackhole account instead of being burned")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(issuerAddress)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 1)
        .dynamicProperty("ALLOW_BLACK_HOLE_OPTIMIZATION", 0)
        .dynamicProperty("ASSET_ISSUE_FEE", ASSET_ISSUE_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AssetIssue fee to blackhole: success={}", result.isSuccess());
  }
}
