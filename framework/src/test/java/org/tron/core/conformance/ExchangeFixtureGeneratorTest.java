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
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.ExchangeCapsule;
import org.tron.core.capsule.TransactionCapsule;
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
 * <p>Run with: ./gradlew :framework:test --tests "ExchangeFixtureGeneratorTest" -Dconformance.output=conformance/fixtures
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
    String outputPath = System.getProperty("conformance.output", "conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
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

    // Set dynamic properties
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(1000000);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
    dbManager.getDynamicPropertiesStore().saveLatestExchangeNum(0);
    dbManager.getDynamicPropertiesStore().saveExchangeCreateFee(EXCHANGE_CREATE_FEE);
    dbManager.getDynamicPropertiesStore().saveExchangeBalanceLimit(EXCHANGE_BALANCE_LIMIT);
    // Enable allowSameTokenName for V2 exchanges
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(1);
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
    long createTime = chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    ExchangeCapsule exchange = new ExchangeCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        id,
        createTime,
        firstTokenId,
        secondTokenId);
    exchange.setBalance(firstBalance, secondBalance);

    // Store in ExchangeV2Store (since allowSameTokenName=1)
    chainBaseManager.getExchangeV2Store().put(exchange.createDbKey(), exchange);
    chainBaseManager.getDynamicPropertiesStore().saveLatestExchangeNum(id);

    log.info("Created exchange {} for testing: {} <-> {}", id,
        new String(firstTokenId), new String(secondTokenId));
  }
}
