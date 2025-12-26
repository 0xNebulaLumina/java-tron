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
import org.tron.core.capsule.AssetIssueCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.AssetIssueContractOuterClass.TransferAssetContract;
import org.tron.protos.contract.BalanceContract.TransferContract;

/**
 * Generates conformance test fixtures for transfer contracts:
 * - TransferContract (1)
 * - TransferAssetContract (2)
 *
 * <p>Run with: ./gradlew :framework:test --tests "TransferFixtureGeneratorTest"
 * -Dconformance.output=../conformance/fixtures --dependency-verification=off
 */
public class TransferFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(TransferFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String RECEIVER_ADDRESS;
  private static final String WITNESS_ADDRESS;
  private static final String TOKEN_ID = "1000001";
  private static final long CREATE_ACCOUNT_FEE = ONE_TRX;

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049151";
    RECEIVER_ADDRESS = Wallet.getAddressPreFixString() + "1111111111111111111111111111111111111111";
    WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "548794500882809695a8a687866e76d4271a1abc";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("Transfer Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Initialize dynamic properties for TRC-10
    initTrc10DynamicProps(dbManager,
        DEFAULT_BLOCK_TIMESTAMP / 1000,
        DEFAULT_BLOCK_TIMESTAMP);

    // Set create account fee for new recipient path
    dbManager.getDynamicPropertiesStore().saveCreateNewAccountFeeInSystemContract(CREATE_ACCOUNT_FEE);

    // Create owner account with sufficient TRX and TRC-10 tokens
    AccountCapsule ownerAccount = putAccount(dbManager, OWNER_ADDRESS, INITIAL_BALANCE, "owner");
    ownerAccount.addAssetAmountV2(TOKEN_ID.getBytes(), 1_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Create receiver account
    putAccount(dbManager, RECEIVER_ADDRESS, INITIAL_BALANCE, "receiver");

    // Create witness
    putAccount(dbManager, WITNESS_ADDRESS, INITIAL_BALANCE, "witness");
    putWitness(dbManager, WITNESS_ADDRESS, "https://witness.network", 10_000_000L);

    // Create TRC-10 asset
    putAssetIssueV2(dbManager, TOKEN_ID, OWNER_ADDRESS, "TestToken", 1_000_000_000_000L);
  }

  // ==========================================================================
  // TransferContract (1) Fixtures
  // ==========================================================================

  @Test
  public void generateTransfer_happyPathExistingRecipient() throws Exception {
    long amount = 10 * ONE_TRX;

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("happy_path_existing_recipient")
        .caseCategory("happy")
        .description("Normal TRX transfer to an existing account")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("transfer_amount", amount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer happy path existing: success={}", result.isSuccess());
  }

  @Test
  public void generateTransfer_happyPathCreatesRecipient() throws Exception {
    String newRecipient = generateAddress("new_recipient_01");
    long amount = 10 * ONE_TRX;

    // Ensure new recipient doesn't exist
    // (not created in setup)

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(newRecipient)))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("happy_path_creates_recipient")
        .caseCategory("happy")
        .description("TRX transfer that creates the recipient account (pays create-account-fee)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("transfer_amount", amount)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer creates recipient: success={}", result.isSuccess());
  }

  @Test
  public void generateTransfer_validateFailToSelf() throws Exception {
    long amount = 10 * ONE_TRX;

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS))) // Self
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_to_self")
        .caseCategory("validate_fail")
        .description("Fail when transferring to self")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("self")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer to self: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransfer_validateFailAmountZero() throws Exception {
    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAmount(0) // Zero amount
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_amount_zero")
        .caseCategory("validate_fail")
        .description("Fail when transfer amount is zero")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("amount")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer amount zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransfer_validateFailInsufficientBalance() throws Exception {
    String poorOwner = generateAddress("poor_owner_0001");
    putAccount(dbManager, poorOwner, ONE_TRX, "poor");

    // Try to transfer more than balance
    long amount = INITIAL_BALANCE + ONE_TRX;

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(poorOwner)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_insufficient_balance")
        .caseCategory("validate_fail")
        .description("Fail when owner has insufficient balance")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(poorOwner)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer insufficient: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // TransferAssetContract (2) Fixtures
  // ==========================================================================

  @Test
  public void generateTransferAsset_happyPathExistingRecipient() throws Exception {
    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("happy_path_transfer_asset_existing_recipient")
        .caseCategory("happy")
        .description("Transfer TRC-10 asset to existing account")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("token_id", TOKEN_ID)
        .dynamicProperty("amount", amount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset happy path existing: success={}", result.isSuccess());
  }

  @Test
  public void generateTransferAsset_happyPathCreatesRecipient() throws Exception {
    String newRecipient = generateAddress("new_asset_recv01");
    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(newRecipient)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("happy_path_transfer_asset_creates_recipient")
        .caseCategory("happy")
        .description("Transfer TRC-10 asset that creates recipient account")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("token_id", TOKEN_ID)
        .dynamicProperty("amount", amount)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset creates recipient: success={}", result.isSuccess());
  }

  @Test
  public void generateTransferAsset_validateFailAssetNotFound() throws Exception {
    String nonExistentToken = "9999999";
    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(nonExistentToken))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_asset_not_found")
        .caseCategory("validate_fail")
        .description("Fail when token ID does not exist in asset-issue-v2")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("asset")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset asset not found: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailInsufficientAssetBalance() throws Exception {
    String poorAssetOwner = generateAddress("poor_asset_own1");
    putAccount(dbManager, poorAssetOwner, INITIAL_BALANCE, "poor_asset");
    // Don't add any tokens to this account

    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(poorAssetOwner)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_insufficient_asset_balance")
        .caseCategory("validate_fail")
        .description("Fail when owner has insufficient asset balance")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(poorAssetOwner)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset insufficient asset: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailToSelf() throws Exception {
    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS))) // Self
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_to_self")
        .caseCategory("validate_fail")
        .description("Fail when transferring asset to self")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("self")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset to self: validationError={}", result.getValidationError());
  }
}
