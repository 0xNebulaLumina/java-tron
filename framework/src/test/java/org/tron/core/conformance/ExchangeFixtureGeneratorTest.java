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
import org.tron.core.capsule.ExchangeCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.ExchangeContract.ExchangeCreateContract;
import org.tron.protos.contract.ExchangeContract.ExchangeInjectContract;
import org.tron.protos.contract.ExchangeContract.ExchangeWithdrawContract;
import org.tron.protos.contract.ExchangeContract.ExchangeTransactionContract;

/**
 * Generates conformance test fixtures for Exchange contracts (41-44).
 *
 * <p>Run with: ./gradlew :framework:test --tests "ExchangeFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures
 */
public class ExchangeFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(ExchangeFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String OTHER_ADDRESS;
  private static final long INITIAL_BALANCE = 1_000_000_000_000L; // 1,000,000 TRX
  private static final long EXCHANGE_CREATE_FEE = 1024_000_000L; // 1024 TRX default fee
  private static final long EXCHANGE_BALANCE_LIMIT = 100_000_000_000_000_000L; // Default limit

  // TRX token symbol
  private static final byte[] TRX_TOKEN = "_".getBytes();
  // Simulated TRC-10 token IDs
  private static final byte[] TOKEN_A = "1000001".getBytes();
  private static final byte[] TOKEN_B = "1000002".getBytes();

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049abc";
    OTHER_ADDRESS = Wallet.getAddressPreFixString() + "548794500882809695a8a687866e76d4271a1abc";
  }

  @Before
  public void setup() {
    // Initialize test accounts and dynamic properties
    initializeTestData();

    // Configure fixture generator
    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Set dynamic properties FIRST - addAssetAmountV2 needs AllowSameTokenName to be set
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(1000000);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
    dbManager.getDynamicPropertiesStore().saveLatestExchangeNum(0);
    dbManager.getDynamicPropertiesStore().saveExchangeCreateFee(EXCHANGE_CREATE_FEE);
    dbManager.getDynamicPropertiesStore().saveExchangeBalanceLimit(EXCHANGE_BALANCE_LIMIT);
    // Enable allowSameTokenName for V2 exchanges - MUST be set before addAssetAmountV2
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(1);

    // Create owner account with high balance for exchange creation fee
    AccountCapsule ownerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("owner"),
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    // Add TRC-10 asset balances for testing
    ownerAccount.addAssetAmountV2(TOKEN_A, 100_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    ownerAccount.addAssetAmountV2(TOKEN_B, 100_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Create another account for permission tests
    AccountCapsule otherAccount = new AccountCapsule(
        ByteString.copyFromUtf8("other"),
        ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(otherAccount.getAddress().toByteArray(), otherAccount);
  }

  // ==========================================================================
  // ExchangeCreate (41) Fixtures
  // ==========================================================================

  @Test
  public void generateExchangeCreate_happyPath_TrxToToken() throws Exception {
    // Create exchange: TRX <-> TOKEN_A
    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFirstTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setFirstTokenBalance(10_000_000_000L) // 10,000 TRX
        .setSecondTokenId(ByteString.copyFrom(TOKEN_A))
        .setSecondTokenBalance(10_000_000_000L) // 10B TOKEN_A
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("happy_path_trx_to_token")
        .caseCategory("happy")
        .description("Create a new TRX to TRC-10 token exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("EXCHANGE_CREATE_FEE", EXCHANGE_CREATE_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate TRX-to-Token happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeCreate_happyPath_TokenToToken() throws Exception {
    // Create exchange: TOKEN_A <-> TOKEN_B
    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFirstTokenId(ByteString.copyFrom(TOKEN_A))
        .setFirstTokenBalance(5_000_000_000L)
        .setSecondTokenId(ByteString.copyFrom(TOKEN_B))
        .setSecondTokenBalance(5_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("happy_path_token_to_token")
        .caseCategory("happy")
        .description("Create a new TRC-10 token to TRC-10 token exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate Token-to-Token happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeCreate_insufficientFee() throws Exception {
    // Create account with insufficient balance for fee
    String poorAddress = Wallet.getAddressPreFixString() + "1234567890123456789012345678901234567890";
    AccountCapsule poorAccount = new AccountCapsule(
        ByteString.copyFromUtf8("poor"),
        ByteString.copyFrom(ByteArray.fromHexString(poorAddress)),
        AccountType.Normal,
        100_000_000L); // Only 100 TRX, less than 1024 TRX fee
    poorAccount.addAssetAmountV2(TOKEN_A, 10_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(poorAccount.getAddress().toByteArray(), poorAccount);

    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(poorAddress)))
        .setFirstTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setFirstTokenBalance(10_000_000L) // 10 TRX
        .setSecondTokenId(ByteString.copyFrom(TOKEN_A))
        .setSecondTokenBalance(10_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_insufficient_fee")
        .caseCategory("validate_fail")
        .description("Fail when account has insufficient TRX for exchange creation fee")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(poorAddress)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate insufficient fee: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeCreate_sameTokens() throws Exception {
    // Try to create exchange with same token on both sides
    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFirstTokenId(ByteString.copyFrom(TOKEN_A))
        .setFirstTokenBalance(10_000_000_000L)
        .setSecondTokenId(ByteString.copyFrom(TOKEN_A)) // Same token!
        .setSecondTokenBalance(10_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_same_tokens")
        .caseCategory("validate_fail")
        .description("Fail when trying to create exchange with same token on both sides")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("same")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate same tokens: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeCreate_ownerAddressEmpty() throws Exception {
    // Invalid owner address: empty
    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setFirstTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setFirstTokenBalance(10_000_000_000L)
        .setSecondTokenId(ByteString.copyFrom(TOKEN_A))
        .setSecondTokenBalance(10_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate empty owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeCreate_ownerAccountNotExist() throws Exception {
    // Valid address format but account doesn't exist
    String nonExistentAddress = Wallet.getAddressPreFixString() + "999999999999999999999999999999999999abcd";

    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
        .setFirstTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setFirstTokenBalance(10_000_000_000L)
        .setSecondTokenId(ByteString.copyFrom(TOKEN_A))
        .setSecondTokenBalance(10_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .expectedError("not exists")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate account not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeCreate_firstTokenIdNotNumber() throws Exception {
    // First token id is not a valid number (non-TRX)
    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFirstTokenId(ByteString.copyFrom("abc".getBytes()))
        .setFirstTokenBalance(10_000_000_000L)
        .setSecondTokenId(ByteString.copyFrom(TOKEN_A))
        .setSecondTokenBalance(10_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_first_token_id_not_number")
        .caseCategory("validate_fail")
        .description("Fail when first token id is not a valid number")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("first token id is not a valid number")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate first token invalid: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeCreate_secondTokenIdNotNumber() throws Exception {
    // Second token id is not a valid number (non-TRX)
    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFirstTokenId(ByteString.copyFrom(TOKEN_A))
        .setFirstTokenBalance(10_000_000_000L)
        .setSecondTokenId(ByteString.copyFrom("xyz".getBytes()))
        .setSecondTokenBalance(10_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_second_token_id_not_number")
        .caseCategory("validate_fail")
        .description("Fail when second token id is not a valid number")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("second token id is not a valid number")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate second token invalid: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeCreate_firstTokenBalanceZero() throws Exception {
    // First token balance is zero
    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFirstTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setFirstTokenBalance(0L)
        .setSecondTokenId(ByteString.copyFrom(TOKEN_A))
        .setSecondTokenBalance(10_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_first_token_balance_zero")
        .caseCategory("validate_fail")
        .description("Fail when first token balance is zero")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("token balance must greater than zero")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate first token zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeCreate_secondTokenBalanceZero() throws Exception {
    // Second token balance is zero
    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFirstTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setFirstTokenBalance(10_000_000_000L)
        .setSecondTokenId(ByteString.copyFrom(TOKEN_A))
        .setSecondTokenBalance(0L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_second_token_balance_zero")
        .caseCategory("validate_fail")
        .description("Fail when second token balance is zero")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("token balance must greater than zero")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate second token zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeCreate_balanceLimitExceededFirst() throws Exception {
    // Set a low balance limit for this test
    long lowLimit = 1_000_000_000L; // 1000 TRX
    dbManager.getDynamicPropertiesStore().saveExchangeBalanceLimit(lowLimit);

    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFirstTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setFirstTokenBalance(10_000_000_000L) // Exceeds limit
        .setSecondTokenId(ByteString.copyFrom(TOKEN_A))
        .setSecondTokenBalance(100_000_000L) // Within limit
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_balance_limit_exceeded_first")
        .caseCategory("validate_fail")
        .description("Fail when first token balance exceeds limit")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("EXCHANGE_BALANCE_LIMIT", lowLimit)
        .expectedError("token balance must less than")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate balance limit first: validationError={}", result.getValidationError());

    // Restore default limit
    dbManager.getDynamicPropertiesStore().saveExchangeBalanceLimit(EXCHANGE_BALANCE_LIMIT);
  }

  @Test
  public void generateExchangeCreate_balanceLimitExceededSecond() throws Exception {
    // Set a low balance limit for this test
    long lowLimit = 1_000_000_000L; // 1000 TRX
    dbManager.getDynamicPropertiesStore().saveExchangeBalanceLimit(lowLimit);

    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFirstTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setFirstTokenBalance(100_000_000L) // Within limit
        .setSecondTokenId(ByteString.copyFrom(TOKEN_A))
        .setSecondTokenBalance(10_000_000_000L) // Exceeds limit
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_balance_limit_exceeded_second")
        .caseCategory("validate_fail")
        .description("Fail when second token balance exceeds limit")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("EXCHANGE_BALANCE_LIMIT", lowLimit)
        .expectedError("token balance must less than")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate balance limit second: validationError={}", result.getValidationError());

    // Restore default limit
    dbManager.getDynamicPropertiesStore().saveExchangeBalanceLimit(EXCHANGE_BALANCE_LIMIT);
  }

  @Test
  public void generateExchangeCreate_trxSideUnderfunded() throws Exception {
    // Account has enough for fee but not for TRX deposit
    // Fee is 1024 TRX, TRX deposit is 500 TRX, total needed = 1524 TRX
    String underfundedAddress = Wallet.getAddressPreFixString() + "abcd123456789012345678901234567890123456";
    AccountCapsule underfundedAccount = new AccountCapsule(
        ByteString.copyFromUtf8("underfunded"),
        ByteString.copyFrom(ByteArray.fromHexString(underfundedAddress)),
        AccountType.Normal,
        EXCHANGE_CREATE_FEE + 100_000_000L); // 1024 TRX fee + 100 TRX (not enough for 500 TRX deposit)
    underfundedAccount.addAssetAmountV2(TOKEN_A, 100_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(underfundedAccount.getAddress().toByteArray(), underfundedAccount);

    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(underfundedAddress)))
        .setFirstTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setFirstTokenBalance(500_000_000_000L) // 500,000 TRX - way more than available
        .setSecondTokenId(ByteString.copyFrom(TOKEN_A))
        .setSecondTokenBalance(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_trx_side_underfunded_fee_ok")
        .caseCategory("validate_fail")
        .description("Fail when TRX balance covers fee but not deposit amount")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(underfundedAddress)
        .expectedError("balance is not enough")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate TRX underfunded: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeCreate_firstTokenBalanceNotEnough() throws Exception {
    // Account doesn't have enough of first token (TRC-10)
    String lowTokenAddress = Wallet.getAddressPreFixString() + "def0123456789012345678901234567890123456";
    AccountCapsule lowTokenAccount = new AccountCapsule(
        ByteString.copyFromUtf8("lowtoken"),
        ByteString.copyFrom(ByteArray.fromHexString(lowTokenAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);
    lowTokenAccount.addAssetAmountV2(TOKEN_A, 100L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore()); // Very little TOKEN_A
    lowTokenAccount.addAssetAmountV2(TOKEN_B, 100_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(lowTokenAccount.getAddress().toByteArray(), lowTokenAccount);

    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(lowTokenAddress)))
        .setFirstTokenId(ByteString.copyFrom(TOKEN_A))
        .setFirstTokenBalance(10_000_000_000L) // More than account has
        .setSecondTokenId(ByteString.copyFrom(TOKEN_B))
        .setSecondTokenBalance(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_first_token_balance_not_enough")
        .caseCategory("validate_fail")
        .description("Fail when first TRC-10 token balance is not enough")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(lowTokenAddress)
        .expectedError("first token balance is not enough")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate first token not enough: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeCreate_secondTokenBalanceNotEnough() throws Exception {
    // Account doesn't have enough of second token (TRC-10)
    String lowToken2Address = Wallet.getAddressPreFixString() + "ef01234567890123456789012345678901234567";
    AccountCapsule lowToken2Account = new AccountCapsule(
        ByteString.copyFromUtf8("lowtoken2"),
        ByteString.copyFrom(ByteArray.fromHexString(lowToken2Address)),
        AccountType.Normal,
        INITIAL_BALANCE);
    lowToken2Account.addAssetAmountV2(TOKEN_A, 100_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    lowToken2Account.addAssetAmountV2(TOKEN_B, 100L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore()); // Very little TOKEN_B
    dbManager.getAccountStore().put(lowToken2Account.getAddress().toByteArray(), lowToken2Account);

    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(lowToken2Address)))
        .setFirstTokenId(ByteString.copyFrom(TOKEN_A))
        .setFirstTokenBalance(1_000_000_000L)
        .setSecondTokenId(ByteString.copyFrom(TOKEN_B))
        .setSecondTokenBalance(10_000_000_000L) // More than account has
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("validate_fail_second_token_balance_not_enough")
        .caseCategory("validate_fail")
        .description("Fail when second TRC-10 token balance is not enough")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(lowToken2Address)
        .expectedError("second token balance is not enough")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate second token not enough: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // ExchangeInject (42) Fixtures
  // ==========================================================================

  @Test
  public void generateExchangeInject_happyPath() throws Exception {
    // First create an exchange
    createExchange(1, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Inject more liquidity
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(1)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(1_000_000_000L) // Inject 1000 TRX
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("happy_path_inject")
        .caseCategory("happy")
        .description("Inject additional liquidity into an existing exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("exchange_id", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeInject_notCreator() throws Exception {
    // Create an exchange owned by OWNER_ADDRESS
    createExchange(2, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // OTHER_ADDRESS tries to inject (should fail)
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setExchangeId(2)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("validate_fail_not_creator")
        .caseCategory("validate_fail")
        .description("Fail when non-creator tries to inject liquidity")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("creator")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject not creator: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeInject_nonexistentExchange() throws Exception {
    // Try to inject into non-existent exchange
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(999)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("validate_fail_nonexistent")
        .caseCategory("validate_fail")
        .description("Fail when injecting into non-existent exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject nonexistent: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeInject_tokenIdNotNumber() throws Exception {
    // Create an exchange first
    createExchange(101, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to inject with non-numeric token id
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(101)
        .setTokenId(ByteString.copyFrom("abc".getBytes()))
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("validate_fail_token_id_not_number")
        .caseCategory("validate_fail")
        .description("Fail when token id is not a valid number")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("token id is not a valid number")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject token id not number: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeInject_tokenIdNotInExchange() throws Exception {
    // Create an exchange with TRX <-> TOKEN_A
    createExchange(102, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to inject TOKEN_B (not in this exchange)
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(102)
        .setTokenId(ByteString.copyFrom(TOKEN_B))
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("validate_fail_token_id_not_in_exchange")
        .caseCategory("validate_fail")
        .description("Fail when token id is not in the exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("token id is not in exchange")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject token not in exchange: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeInject_zeroQuant() throws Exception {
    // Create an exchange
    createExchange(103, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to inject with zero quant
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(103)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(0L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("validate_fail_zero_quant")
        .caseCategory("validate_fail")
        .description("Fail when injected quant is zero")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("injected token quant must greater than zero")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject zero quant: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeInject_exchangeClosed() throws Exception {
    // Create an exchange with zero balance on one side (closed state)
    createExchange(104, TRX_TOKEN, TOKEN_A, 0L, 10_000_000_000L);

    // Try to inject into closed exchange
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(104)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("validate_fail_exchange_closed_balance_zero")
        .caseCategory("validate_fail")
        .description("Fail when exchange is closed (balance is zero)")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("the exchange has been closed")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject exchange closed: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeInject_calculatedAnotherTokenQuantZero() throws Exception {
    // Create an exchange with skewed balances (10000:1 ratio)
    createExchange(105, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 1L);

    // Inject small amount so calculated other token quant truncates to 0
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(105)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(1L) // Very small - 1 * 1 / 10_000_000_000 = 0
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("validate_fail_calculated_another_token_quant_zero")
        .caseCategory("validate_fail")
        .description("Fail when calculated proportional token quant is zero")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("the calculated token quant  must be greater than 0")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject calculated quant zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeInject_balanceLimitExceeded() throws Exception {
    // Set a low balance limit for this test
    long lowLimit = 11_000_000_000L;
    dbManager.getDynamicPropertiesStore().saveExchangeBalanceLimit(lowLimit);

    // Create an exchange near the limit
    createExchange(106, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to inject enough to exceed the limit
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(106)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(2_000_000_000L) // Would push to 12B, exceeding 11B limit
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("validate_fail_balance_limit_exceeded_post_inject")
        .caseCategory("validate_fail")
        .description("Fail when injection would exceed balance limit")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("EXCHANGE_BALANCE_LIMIT", lowLimit)
        .expectedError("token balance must less than")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject balance limit: validationError={}", result.getValidationError());

    // Restore default limit
    dbManager.getDynamicPropertiesStore().saveExchangeBalanceLimit(EXCHANGE_BALANCE_LIMIT);
  }

  @Test
  public void generateExchangeInject_tokenBalanceNotEnough() throws Exception {
    // Create a new account with limited token balance
    String limitedTokenAddress = Wallet.getAddressPreFixString() + "1111222233334444555566667777888899990000";
    AccountCapsule limitedTokenAccount = new AccountCapsule(
        ByteString.copyFromUtf8("limitedtoken"),
        ByteString.copyFrom(ByteArray.fromHexString(limitedTokenAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);
    limitedTokenAccount.addAssetAmountV2(TOKEN_A, 100L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore()); // Very little TOKEN_A
    dbManager.getAccountStore().put(limitedTokenAccount.getAddress().toByteArray(), limitedTokenAccount);

    // Create an exchange owned by this account
    createExchangeWithOwner(107, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L, limitedTokenAddress);

    // Try to inject more tokens than account has
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(limitedTokenAddress)))
        .setExchangeId(107)
        .setTokenId(ByteString.copyFrom(TOKEN_A))
        .setQuant(1_000_000_000L) // More than account has
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("validate_fail_inject_token_balance_not_enough")
        .caseCategory("validate_fail")
        .description("Fail when injected token balance is not enough")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(limitedTokenAddress)
        .expectedError("token balance is not enough")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject token balance not enough: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeInject_anotherTokenBalanceNotEnough() throws Exception {
    // Create a new account with limited other token balance
    String limitedOtherAddress = Wallet.getAddressPreFixString() + "2222333344445555666677778888999900001111";
    AccountCapsule limitedOtherAccount = new AccountCapsule(
        ByteString.copyFromUtf8("limitedother"),
        ByteString.copyFrom(ByteArray.fromHexString(limitedOtherAddress)),
        AccountType.Normal,
        10_000_000_000L); // Limited TRX (10,000 TRX)
    limitedOtherAccount.addAssetAmountV2(TOKEN_A, 100_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore()); // Plenty of TOKEN_A
    dbManager.getAccountStore().put(limitedOtherAccount.getAddress().toByteArray(), limitedOtherAccount);

    // Create an exchange owned by this account (1:1 ratio)
    createExchangeWithOwner(108, TRX_TOKEN, TOKEN_A, 1_000_000_000L, 1_000_000_000L, limitedOtherAddress);

    // Try to inject TOKEN_A - will require proportional TRX that account doesn't have
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(limitedOtherAddress)))
        .setExchangeId(108)
        .setTokenId(ByteString.copyFrom(TOKEN_A))
        .setQuant(50_000_000_000L) // Would require 50,000 TRX which account doesn't have
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("validate_fail_inject_another_token_balance_not_enough")
        .caseCategory("validate_fail")
        .description("Fail when proportional other token balance is not enough")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(limitedOtherAddress)
        .expectedError("balance is not enough")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject another token not enough: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeInject_happyPathSecondTokenSide() throws Exception {
    // Create an exchange
    createExchange(109, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Inject on the second token side (TOKEN_A instead of TRX)
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(109)
        .setTokenId(ByteString.copyFrom(TOKEN_A)) // Second token
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("happy_path_inject_second_token_side")
        .caseCategory("happy")
        .description("Inject liquidity on the second token side")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("exchange_id", 109)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject second token side happy path: success={}", result.isSuccess());
  }

  // ==========================================================================
  // ExchangeWithdraw (43) Fixtures
  // ==========================================================================

  @Test
  public void generateExchangeWithdraw_happyPath() throws Exception {
    // Create an exchange with sufficient liquidity
    createExchange(3, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Withdraw liquidity
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(3)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(1_000_000_000L) // Withdraw 1000 TRX
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("happy_path_withdraw")
        .caseCategory("happy")
        .description("Withdraw liquidity from an existing exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("exchange_id", 3)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeWithdraw_notCreator() throws Exception {
    // Create an exchange owned by OWNER_ADDRESS
    createExchange(4, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // OTHER_ADDRESS tries to withdraw (should fail)
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setExchangeId(4)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("validate_fail_not_creator")
        .caseCategory("validate_fail")
        .description("Fail when non-creator tries to withdraw liquidity")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("creator")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw not creator: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeWithdraw_insufficientBalance() throws Exception {
    // Create an exchange with some liquidity
    createExchange(5, TRX_TOKEN, TOKEN_A, 1_000_000_000L, 1_000_000_000L);

    // Try to withdraw more than available
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(5)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(10_000_000_000L) // More than available
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("validate_fail_insufficient_balance")
        .caseCategory("validate_fail")
        .description("Fail when trying to withdraw more than available balance")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw insufficient balance: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeWithdraw_nonexistentExchange() throws Exception {
    // Try to withdraw from non-existent exchange
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(9999)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("validate_fail_nonexistent_exchange")
        .caseCategory("validate_fail")
        .description("Fail when withdrawing from non-existent exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("not exists")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw nonexistent: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeWithdraw_tokenIdNotNumber() throws Exception {
    // Create an exchange
    createExchange(201, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to withdraw with non-numeric token id
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(201)
        .setTokenId(ByteString.copyFrom("abc".getBytes()))
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("validate_fail_token_id_not_number")
        .caseCategory("validate_fail")
        .description("Fail when token id is not a valid number")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("token id is not a valid number")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw token id not number: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeWithdraw_tokenNotInExchange() throws Exception {
    // Create an exchange with TRX <-> TOKEN_A
    createExchange(202, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to withdraw TOKEN_B (not in this exchange)
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(202)
        .setTokenId(ByteString.copyFrom(TOKEN_B))
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("validate_fail_token_not_in_exchange")
        .caseCategory("validate_fail")
        .description("Fail when token is not in the exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("token is not in exchange")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw token not in exchange: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeWithdraw_zeroQuant() throws Exception {
    // Create an exchange
    createExchange(203, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to withdraw with zero quant
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(203)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(0L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("validate_fail_zero_quant")
        .caseCategory("validate_fail")
        .description("Fail when withdraw quant is zero")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("withdraw token quant must greater than zero")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw zero quant: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeWithdraw_exchangeClosed() throws Exception {
    // Create an exchange with zero balance on one side (closed state)
    createExchange(204, TRX_TOKEN, TOKEN_A, 0L, 10_000_000_000L);

    // Try to withdraw from closed exchange
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(204)
        .setTokenId(ByteString.copyFrom(TOKEN_A))
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("validate_fail_exchange_closed_balance_zero")
        .caseCategory("validate_fail")
        .description("Fail when exchange is closed (balance is zero)")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("the exchange has been closed")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw exchange closed: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeWithdraw_anotherTokenQuantZero() throws Exception {
    // Create an exchange with skewed balances so withdrawal calculates to 0 for other side
    createExchange(205, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 1L);

    // Withdraw small amount so calculated other token quant truncates to 0
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(205)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(1L) // 1 * 1 / 10_000_000_000 = 0
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("validate_fail_withdraw_another_token_quant_zero")
        .caseCategory("validate_fail")
        .description("Fail when calculated proportional withdrawal is zero")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("withdraw another token quant must greater than zero")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw another quant zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeWithdraw_notPreciseEnough() throws Exception {
    // Create an exchange with 1:2 ratio for precision test
    // Example from unit test: balances (100_000_000, 200_000_000), withdraw secondTokenQuant=9991
    createExchange(206, TRX_TOKEN, TOKEN_A, 100_000_000L, 200_000_000L);

    // Withdraw an odd amount that causes precision loss
    // Withdraw TOKEN_A (second token) with amount 9991
    // anotherTokenQuant = 100_000_000 * 9991 / 200_000_000 = 4995.5 -> truncates to 4995
    // remainder = 4995.5 - 4995 = 0.5
    // 0.5 / 4995 > 0.0001 -> fails "Not precise enough"
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(206)
        .setTokenId(ByteString.copyFrom(TOKEN_A))
        .setQuant(9991L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("validate_fail_not_precise_enough")
        .caseCategory("validate_fail")
        .description("Fail due to precision guard on withdrawal")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Not precise enough")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw not precise: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeWithdraw_happyPathSecondTokenSide() throws Exception {
    // Create an exchange
    createExchange(207, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Withdraw on the second token side (TOKEN_A instead of TRX)
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(207)
        .setTokenId(ByteString.copyFrom(TOKEN_A)) // Second token
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("happy_path_withdraw_second_token_side")
        .caseCategory("happy")
        .description("Withdraw liquidity on the second token side")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("exchange_id", 207)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw second token side happy path: success={}", result.isSuccess());
  }

  // ==========================================================================
  // ExchangeTransaction (44) Fixtures
  // ==========================================================================

  @Test
  public void generateExchangeTransaction_happyPath() throws Exception {
    // Create an exchange with liquidity
    createExchange(6, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Execute a token swap
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(6)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(100_000_000L) // Sell 100 TRX
        .setExpected(1L) // Expect at least 1 token in return
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("happy_path_swap")
        .caseCategory("happy")
        .description("Execute a token swap on an exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("exchange_id", 6)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeTransaction_reverseDirection() throws Exception {
    // Create an exchange with liquidity
    createExchange(7, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Execute a reverse swap (selling TOKEN_A for TRX)
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(7)
        .setTokenId(ByteString.copyFrom(TOKEN_A)) // Selling TOKEN_A this time
        .setQuant(100_000_000L)
        .setExpected(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("happy_path_reverse_swap")
        .caseCategory("happy")
        .description("Execute a reverse direction token swap on an exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("exchange_id", 7)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction reverse direction: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeTransaction_slippageTooHigh() throws Exception {
    // Create an exchange with liquidity
    createExchange(8, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Execute a swap with unrealistic expectations
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(8)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(100_000_000L) // Sell 100 TRX
        .setExpected(1_000_000_000_000L) // Expect way more than possible
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("validate_fail_slippage")
        .caseCategory("validate_fail")
        .description("Fail when expected output exceeds AMM calculation")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("expected")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction slippage: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeTransaction_wrongToken() throws Exception {
    // Create an exchange with TRX <-> TOKEN_A
    createExchange(9, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to swap with TOKEN_B (not in this exchange)
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(9)
        .setTokenId(ByteString.copyFrom(TOKEN_B)) // Wrong token!
        .setQuant(100_000_000L)
        .setExpected(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("validate_fail_wrong_token")
        .caseCategory("validate_fail")
        .description("Fail when trying to swap with a token not in the exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("token")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction wrong token: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeTransaction_zeroQuant() throws Exception {
    // Create an exchange
    createExchange(10, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to swap with zero quantity
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(10)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(0) // Zero quantity
        .setExpected(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("validate_fail_zero_quant")
        .caseCategory("validate_fail")
        .description("Fail when trying to swap with zero quantity")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("quant")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction zero quant: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeTransaction_nonexistentExchange() throws Exception {
    // Try to trade on non-existent exchange
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(99999)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(100_000_000L)
        .setExpected(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("validate_fail_nonexistent_exchange")
        .caseCategory("validate_fail")
        .description("Fail when trading on non-existent exchange")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("not exists")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction nonexistent: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeTransaction_tokenIdNotNumber() throws Exception {
    // Create an exchange
    createExchange(301, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to trade with non-numeric token id
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(301)
        .setTokenId(ByteString.copyFrom("abc".getBytes()))
        .setQuant(100_000_000L)
        .setExpected(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("validate_fail_token_id_not_number")
        .caseCategory("validate_fail")
        .description("Fail when token id is not a valid number")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("token id is not a valid number")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction token id not number: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeTransaction_expectedZero() throws Exception {
    // Create an exchange
    createExchange(302, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to trade with zero expected
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(302)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(100_000_000L)
        .setExpected(0L) // Zero expected
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("validate_fail_expected_zero")
        .caseCategory("validate_fail")
        .description("Fail when expected output is zero")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("token expected must greater than zero")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction expected zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeTransaction_exchangeClosed() throws Exception {
    // Create an exchange with zero balance on one side (closed state)
    createExchange(303, TRX_TOKEN, TOKEN_A, 0L, 10_000_000_000L);

    // Try to trade on closed exchange
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(303)
        .setTokenId(ByteString.copyFrom(TOKEN_A))
        .setQuant(100_000_000L)
        .setExpected(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("validate_fail_exchange_closed_balance_zero")
        .caseCategory("validate_fail")
        .description("Fail when exchange is closed (balance is zero)")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("the exchange has been closed")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction exchange closed: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeTransaction_balanceLimitExceeded() throws Exception {
    // Set a low balance limit for this test
    long lowLimit = 11_000_000_000L;
    dbManager.getDynamicPropertiesStore().saveExchangeBalanceLimit(lowLimit);

    // Create an exchange near the limit
    createExchange(304, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to trade enough to push selected side over the limit
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(304)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(2_000_000_000L) // Would push TRX side to 12B, exceeding 11B limit
        .setExpected(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("validate_fail_balance_limit_exceeded_selected_side")
        .caseCategory("validate_fail")
        .description("Fail when trade would exceed balance limit")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("EXCHANGE_BALANCE_LIMIT", lowLimit)
        .expectedError("token balance must less than")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction balance limit: validationError={}", result.getValidationError());

    // Restore default limit
    dbManager.getDynamicPropertiesStore().saveExchangeBalanceLimit(EXCHANGE_BALANCE_LIMIT);
  }

  @Test
  public void generateExchangeTransaction_trxBalanceNotEnough() throws Exception {
    // Create an account with limited TRX
    String limitedTrxAddress = Wallet.getAddressPreFixString() + "3333444455556666777788889999000011112222";
    AccountCapsule limitedTrxAccount = new AccountCapsule(
        ByteString.copyFromUtf8("limitedtrx"),
        ByteString.copyFrom(ByteArray.fromHexString(limitedTrxAddress)),
        AccountType.Normal,
        10_000_000L); // Only 10 TRX
    limitedTrxAccount.addAssetAmountV2(TOKEN_A, 100_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(limitedTrxAccount.getAddress().toByteArray(), limitedTrxAccount);

    // Create an exchange
    createExchange(305, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to sell more TRX than account has
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(limitedTrxAddress)))
        .setExchangeId(305)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(100_000_000L) // 100 TRX - more than account has
        .setExpected(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("validate_fail_trx_balance_not_enough")
        .caseCategory("validate_fail")
        .description("Fail when TRX balance is not enough for trade")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(limitedTrxAddress)
        .expectedError("balance is not enough")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction TRX balance not enough: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeTransaction_tokenBalanceNotEnough() throws Exception {
    // Create an account with limited TOKEN_A
    String limitedTokenAddress = Wallet.getAddressPreFixString() + "4444555566667777888899990000111122223333";
    AccountCapsule limitedTokenAccount = new AccountCapsule(
        ByteString.copyFromUtf8("limitedtoken"),
        ByteString.copyFrom(ByteArray.fromHexString(limitedTokenAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);
    limitedTokenAccount.addAssetAmountV2(TOKEN_A, 100L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore()); // Very little TOKEN_A
    dbManager.getAccountStore().put(limitedTokenAccount.getAddress().toByteArray(), limitedTokenAccount);

    // Create an exchange
    createExchange(306, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Try to sell more TOKEN_A than account has
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(limitedTokenAddress)))
        .setExchangeId(306)
        .setTokenId(ByteString.copyFrom(TOKEN_A))
        .setQuant(100_000_000L) // More than account has
        .setExpected(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("validate_fail_token_balance_not_enough")
        .caseCategory("validate_fail")
        .description("Fail when token balance is not enough for trade")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(limitedTokenAddress)
        .expectedError("token balance is not enough")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction token balance not enough: validationError={}", result.getValidationError());
  }

  @Test
  public void generateExchangeTransaction_happyPathNonCreator() throws Exception {
    // Create an exchange owned by OWNER_ADDRESS
    createExchange(307, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Fund OTHER_ADDRESS with tokens
    AccountCapsule otherAccount = dbManager.getAccountStore().get(ByteArray.fromHexString(OTHER_ADDRESS));
    otherAccount.addAssetAmountV2(TOKEN_A, 100_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(otherAccount.getAddress().toByteArray(), otherAccount);

    // OTHER_ADDRESS trades on an exchange they didn't create (trading is permissionless)
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setExchangeId(307)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(100_000_000L) // Sell 100 TRX
        .setExpected(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("happy_path_non_creator_can_trade")
        .caseCategory("happy")
        .description("Non-creator can successfully trade on exchange (permissionless)")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .dynamicProperty("exchange_id", 307)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction non-creator happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeTransaction_happyPathStrictMathEnabled() throws Exception {
    // Enable strict math mode
    dbManager.getDynamicPropertiesStore().saveAllowStrictMath(1);

    // Create an exchange with liquidity
    createExchange(308, TRX_TOKEN, TOKEN_A, 10_000_000_000L, 10_000_000_000L);

    // Execute a token swap with strict math enabled
    // The strict math mode affects rounding behavior in AMM calculations
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(308)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(100_000_000L) // Sell 100 TRX
        .setExpected(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("happy_path_strict_math_enabled")
        .caseCategory("happy")
        .description("Execute swap with strict math mode enabled (affects AMM rounding)")
        .database("account")
        .database("exchange-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("exchange_id", 308)
        .dynamicProperty("ALLOW_STRICT_MATH", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction strict math enabled: success={}", result.isSuccess());

    // Restore default (disabled)
    dbManager.getDynamicPropertiesStore().saveAllowStrictMath(0);
  }

  // ==========================================================================
  // Helper Methods
  // ==========================================================================

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

  private void createExchange(long id, byte[] firstTokenId, byte[] secondTokenId,
                               long firstBalance, long secondBalance) {
    createExchangeWithOwner(id, firstTokenId, secondTokenId, firstBalance, secondBalance, OWNER_ADDRESS);
  }

  private void createExchangeWithOwner(long id, byte[] firstTokenId, byte[] secondTokenId,
                                        long firstBalance, long secondBalance, String ownerAddress) {
    long createTime = chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    ExchangeCapsule exchange = new ExchangeCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(ownerAddress)),
        id,
        createTime,
        firstTokenId,
        secondTokenId);
    exchange.setBalance(firstBalance, secondBalance);

    // Store in ExchangeV2Store (since allowSameTokenName=1)
    chainBaseManager.getExchangeV2Store().put(exchange.createDbKey(), exchange);
    chainBaseManager.getDynamicPropertiesStore().saveLatestExchangeNum(id);

    log.info("Created exchange {} for testing (owner={}): {} <-> {}", id,
        ownerAddress.substring(ownerAddress.length() - 8),
        new String(firstTokenId), new String(secondTokenId));
  }

  // ==========================================================================
  // ALLOW_SAME_TOKEN_NAME=0 (Legacy Mode) Fixtures
  // ==========================================================================
  // These tests use token NAMES instead of IDs, and store in ExchangeStore (v1)
  // Java updates both v1 and v2 stores when allowSameTokenName=0

  // Token NAMES for legacy mode (not numeric IDs)
  private static final byte[] TOKEN_NAME_A = "TestTokenA".getBytes();
  private static final byte[] TOKEN_NAME_B = "TestTokenB".getBytes();

  /**
   * Create exchange in legacy mode (ExchangeStore v1).
   * When allowSameTokenName=0, exchanges use token NAMES.
   */
  private void createExchangeLegacy(long id, byte[] firstTokenName, byte[] secondTokenName,
                                     long firstBalance, long secondBalance) {
    createExchangeLegacyWithOwner(id, firstTokenName, secondTokenName, firstBalance, secondBalance, OWNER_ADDRESS);
  }

  private void createExchangeLegacyWithOwner(long id, byte[] firstTokenName, byte[] secondTokenName,
                                              long firstBalance, long secondBalance, String ownerAddress) {
    long createTime = chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    ExchangeCapsule exchange = new ExchangeCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(ownerAddress)),
        id,
        createTime,
        firstTokenName,
        secondTokenName);
    exchange.setBalance(firstBalance, secondBalance);

    // Store in ExchangeStore (v1) for legacy mode
    chainBaseManager.getExchangeStore().put(exchange.createDbKey(), exchange);
    chainBaseManager.getDynamicPropertiesStore().saveLatestExchangeNum(id);

    log.info("Created LEGACY exchange {} for testing (owner={}): {} <-> {}", id,
        ownerAddress.substring(ownerAddress.length() - 8),
        new String(firstTokenName), new String(secondTokenName));
  }

  /**
   * Initialize test data for legacy mode (ALLOW_SAME_TOKEN_NAME=0).
   * Uses token NAMES in asset maps.
   */
  private void initializeTestDataLegacy() {
    // Set dynamic properties for legacy mode
    long blockTimestamp = 1000000;
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(blockTimestamp);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
    dbManager.getDynamicPropertiesStore().saveLatestExchangeNum(0);
    dbManager.getDynamicPropertiesStore().saveExchangeCreateFee(EXCHANGE_CREATE_FEE);
    dbManager.getDynamicPropertiesStore().saveExchangeBalanceLimit(EXCHANGE_BALANCE_LIMIT);
    // LEGACY MODE: allowSameTokenName=0
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(0);

    // Create AssetIssueCapsules for legacy mode tokens (required for validation)
    // In legacy mode, assets are looked up by NAME in the asset issue store
    AssetIssueContract assetA = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setName(ByteString.copyFrom(TOKEN_NAME_A))
        .setAbbr(ByteString.copyFromUtf8("TKA"))
        .setTotalSupply(100_000_000_000_000L)
        .setTrxNum(1)
        .setNum(1)
        .setPrecision(6)
        .setStartTime(blockTimestamp - 86400000)
        .setEndTime(blockTimestamp + 86400000L * 30)
        .setDescription(ByteString.copyFromUtf8("Test Token A for legacy exchange tests"))
        .setUrl(ByteString.copyFromUtf8("https://example.com/tokenA"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .setId("1000001") // Numeric ID for v2 transformation
        .build();
    AssetIssueCapsule assetCapsuleA = new AssetIssueCapsule(assetA);
    // In legacy mode, store by name key
    dbManager.getAssetIssueStore().put(assetCapsuleA.createDbKey(), assetCapsuleA);

    AssetIssueContract assetB = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setName(ByteString.copyFrom(TOKEN_NAME_B))
        .setAbbr(ByteString.copyFromUtf8("TKB"))
        .setTotalSupply(100_000_000_000_000L)
        .setTrxNum(1)
        .setNum(1)
        .setPrecision(6)
        .setStartTime(blockTimestamp - 86400000)
        .setEndTime(blockTimestamp + 86400000L * 30)
        .setDescription(ByteString.copyFromUtf8("Test Token B for legacy exchange tests"))
        .setUrl(ByteString.copyFromUtf8("https://example.com/tokenB"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .setId("1000002") // Numeric ID for v2 transformation
        .build();
    AssetIssueCapsule assetCapsuleB = new AssetIssueCapsule(assetB);
    dbManager.getAssetIssueStore().put(assetCapsuleB.createDbKey(), assetCapsuleB);

    // Create owner account with high balance
    AccountCapsule ownerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("owner"),
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    // Add TRC-10 asset balances by TOKEN NAME (not ID) for legacy mode
    // In legacy mode, assets are stored by name in account.asset map
    ownerAccount.addAssetAmount(TOKEN_NAME_A, 100_000_000_000L, false);
    ownerAccount.addAssetAmount(TOKEN_NAME_B, 100_000_000_000L, false);
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Create another account for permission tests
    AccountCapsule otherAccount = new AccountCapsule(
        ByteString.copyFromUtf8("other"),
        ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(otherAccount.getAddress().toByteArray(), otherAccount);
  }

  @Test
  public void generateExchangeInject_legacyMode_happyPath() throws Exception {
    // Reset to legacy mode
    initializeTestDataLegacy();

    // Create a legacy exchange with token NAMES
    createExchangeLegacy(1, TOKEN_NAME_A, TOKEN_NAME_B, 10_000_000_000L, 10_000_000_000L);

    // Inject using token NAME (not ID)
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(1)
        .setTokenId(ByteString.copyFrom(TOKEN_NAME_A))
        .setQuant(1_000_000_000L) // 1B of TOKEN_NAME_A
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("legacy_mode_happy_path_inject")
        .caseCategory("happy")
        .description("Inject into exchange with ALLOW_SAME_TOKEN_NAME=0 (legacy mode using token names)")
        .database("account")
        .database("exchange")        // v1 store
        .database("exchange-v2")     // also updated
        .database("asset-issue")     // required for token lookup in legacy mode
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 0)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject LEGACY mode happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeInject_legacyMode_happyPath_trxSide() throws Exception {
    // Reset to legacy mode
    initializeTestDataLegacy();

    // Create a legacy exchange: TRX <-> TOKEN_NAME_A
    createExchangeLegacy(1, TRX_TOKEN, TOKEN_NAME_A, 10_000_000_000L, 10_000_000_000L);

    // Inject TRX side
    ExchangeInjectContract contract = ExchangeInjectContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(1)
        .setTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setQuant(1_000_000_000L) // 1000 TRX
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeInjectContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_INJECT_CONTRACT", 42)
        .caseName("legacy_mode_happy_path_inject_trx_side")
        .caseCategory("happy")
        .description("Inject TRX into exchange with ALLOW_SAME_TOKEN_NAME=0 (legacy mode)")
        .database("account")
        .database("exchange")
        .database("exchange-v2")
        .database("asset-issue")     // required for token lookup in legacy mode
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 0)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeInject LEGACY mode TRX side: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeWithdraw_legacyMode_happyPath() throws Exception {
    // Reset to legacy mode
    initializeTestDataLegacy();

    // Create a legacy exchange with token NAMES
    createExchangeLegacy(1, TOKEN_NAME_A, TOKEN_NAME_B, 10_000_000_000L, 10_000_000_000L);

    // Withdraw using token NAME
    ExchangeWithdrawContract contract = ExchangeWithdrawContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(1)
        .setTokenId(ByteString.copyFrom(TOKEN_NAME_A))
        .setQuant(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeWithdrawContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_WITHDRAW_CONTRACT", 43)
        .caseName("legacy_mode_happy_path_withdraw")
        .caseCategory("happy")
        .description("Withdraw from exchange with ALLOW_SAME_TOKEN_NAME=0 (legacy mode using token names)")
        .database("account")
        .database("exchange")
        .database("exchange-v2")
        .database("asset-issue")     // required for token lookup in legacy mode
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 0)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeWithdraw LEGACY mode happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeTransaction_legacyMode_happyPath() throws Exception {
    // Reset to legacy mode
    initializeTestDataLegacy();

    // Create a legacy exchange with token NAMES
    createExchangeLegacy(1, TOKEN_NAME_A, TOKEN_NAME_B, 10_000_000_000L, 10_000_000_000L);

    // Trade using token NAME
    ExchangeTransactionContract contract = ExchangeTransactionContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setExchangeId(1)
        .setTokenId(ByteString.copyFrom(TOKEN_NAME_A))
        .setQuant(100_000_000L)
        .setExpected(1) // Minimum expected
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeTransactionContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_TRANSACTION_CONTRACT", 44)
        .caseName("legacy_mode_happy_path_transaction")
        .caseCategory("happy")
        .description("Execute exchange transaction with ALLOW_SAME_TOKEN_NAME=0 (legacy mode using token names)")
        .database("account")
        .database("exchange")
        .database("exchange-v2")
        .database("asset-issue")     // required for token lookup in legacy mode
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 0)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeTransaction LEGACY mode happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeCreate_legacyMode_happyPath() throws Exception {
    // Reset to legacy mode
    initializeTestDataLegacy();

    // Create exchange with token NAMES
    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFirstTokenId(ByteString.copyFrom(TOKEN_NAME_A))
        .setFirstTokenBalance(5_000_000_000L)
        .setSecondTokenId(ByteString.copyFrom(TOKEN_NAME_B))
        .setSecondTokenBalance(5_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("legacy_mode_happy_path_create")
        .caseCategory("happy")
        .description("Create exchange with ALLOW_SAME_TOKEN_NAME=0 (legacy mode using token names)")
        .database("account")
        .database("exchange")
        .database("exchange-v2")
        .database("asset-issue")     // required for token lookup in legacy mode
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 0)
        .dynamicProperty("EXCHANGE_CREATE_FEE", EXCHANGE_CREATE_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate LEGACY mode happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateExchangeCreate_legacyMode_trxToToken() throws Exception {
    // Reset to legacy mode
    initializeTestDataLegacy();

    // Create exchange: TRX <-> TOKEN_NAME_A
    ExchangeCreateContract contract = ExchangeCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFirstTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setFirstTokenBalance(10_000_000_000L)
        .setSecondTokenId(ByteString.copyFrom(TOKEN_NAME_A))
        .setSecondTokenBalance(10_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ExchangeCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("EXCHANGE_CREATE_CONTRACT", 41)
        .caseName("legacy_mode_trx_to_token_create")
        .caseCategory("happy")
        .description("Create TRX to token exchange with ALLOW_SAME_TOKEN_NAME=0 (legacy mode)")
        .database("account")
        .database("exchange")
        .database("exchange-v2")
        .database("asset-issue")     // required for token lookup in legacy mode
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("ALLOW_SAME_TOKEN_NAME", 0)
        .dynamicProperty("EXCHANGE_CREATE_FEE", EXCHANGE_CREATE_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ExchangeCreate LEGACY mode TRX-to-token: success={}", result.isSuccess());
  }
}
