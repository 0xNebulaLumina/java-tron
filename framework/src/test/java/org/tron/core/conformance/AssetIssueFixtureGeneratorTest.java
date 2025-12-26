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
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract;

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
    long startTime = DEFAULT_BLOCK_TIMESTAMP + 1000;
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

    long startTime = DEFAULT_BLOCK_TIMESTAMP + 1000;
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

    long startTime = DEFAULT_BLOCK_TIMESTAMP + 1000;
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

    long startTime = DEFAULT_BLOCK_TIMESTAMP + 1000;
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

    long startTime = DEFAULT_BLOCK_TIMESTAMP + 1000;
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
}
