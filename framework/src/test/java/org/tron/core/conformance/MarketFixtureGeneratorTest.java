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
import org.tron.core.capsule.MarketAccountOrderCapsule;
import org.tron.core.capsule.MarketOrderCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.MarketOrder;
import org.tron.protos.Protocol.MarketOrder.State;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.MarketContract.MarketSellAssetContract;
import org.tron.protos.contract.MarketContract.MarketCancelOrderContract;

/**
 * Generates conformance test fixtures for Market contracts (52-53).
 *
 * <p>Run with: ./gradlew :framework:test --tests "MarketFixtureGeneratorTest" -Dconformance.output=conformance/fixtures
 */
public class MarketFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(MarketFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String OTHER_ADDRESS;
  private static final long INITIAL_BALANCE = 1_000_000_000_000L; // 1,000,000 TRX
  private static final long MARKET_SELL_FEE = 0L; // Default no fee
  private static final long MARKET_CANCEL_FEE = 0L; // Default no fee
  private static final long MARKET_QUANTITY_LIMIT = 1_000_000_000_000_000L; // Default limit

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
    // Set dynamic properties FIRST
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(1000000);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
    // Enable allowSameTokenName for V2
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(1);
    // Enable market transactions
    dbManager.getDynamicPropertiesStore().saveAllowMarketTransaction(1);
    // Set market fees and limits
    dbManager.getDynamicPropertiesStore().saveMarketSellFee(MARKET_SELL_FEE);
    dbManager.getDynamicPropertiesStore().saveMarketCancelFee(MARKET_CANCEL_FEE);
    dbManager.getDynamicPropertiesStore().saveMarketQuantityLimit(MARKET_QUANTITY_LIMIT);

    // Create owner account with high balance
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
    otherAccount.addAssetAmountV2(TOKEN_A, 10_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(otherAccount.getAddress().toByteArray(), otherAccount);
  }

  // ==========================================================================
  // MarketSellAsset (52) Fixtures
  // ==========================================================================

  @Test
  public void generateMarketSellAsset_happyPath_TrxToToken() throws Exception {
    // Create market order: Sell TRX to buy TOKEN_A
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_000_000_000L) // 1000 TRX
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000_000L) // Expect at least 1000 TOKEN_A
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("happy_path_trx_to_token")
        .caseCategory("happy")
        .description("Create a market sell order: TRX -> TRC-10 token")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("market_pair_to_price")
        .database("market_pair_price_to_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset TRX-to-Token happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateMarketSellAsset_happyPath_TokenToTrx() throws Exception {
    // Create market order: Sell TOKEN_A to buy TRX
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TOKEN_A))
        .setSellTokenQuantity(1_000_000_000L) // 1000 TOKEN_A
        .setBuyTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setBuyTokenQuantity(1_000_000_000L) // Expect at least 1000 TRX
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("happy_path_token_to_trx")
        .caseCategory("happy")
        .description("Create a market sell order: TRC-10 token -> TRX")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("market_pair_to_price")
        .database("market_pair_price_to_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset Token-to-TRX happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateMarketSellAsset_happyPath_TokenToToken() throws Exception {
    // Create market order: Sell TOKEN_A to buy TOKEN_B
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TOKEN_A))
        .setSellTokenQuantity(500_000_000L) // 500M TOKEN_A
        .setBuyTokenId(ByteString.copyFrom(TOKEN_B))
        .setBuyTokenQuantity(500_000_000L) // Expect at least 500M TOKEN_B
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("happy_path_token_to_token")
        .caseCategory("happy")
        .description("Create a market sell order: TRC-10 token -> TRC-10 token")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("market_pair_to_price")
        .database("market_pair_price_to_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset Token-to-Token happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateMarketSellAsset_marketDisabled() throws Exception {
    // Disable market transactions
    dbManager.getDynamicPropertiesStore().saveAllowMarketTransaction(0);

    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_market_disabled")
        .caseCategory("validate_fail")
        .description("Fail when market transactions are disabled")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("market")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset market disabled: validationError={}", result.getValidationError());

    // Re-enable for other tests
    dbManager.getDynamicPropertiesStore().saveAllowMarketTransaction(1);
  }

  @Test
  public void generateMarketSellAsset_sameTokens() throws Exception {
    // Try to sell TOKEN_A for TOKEN_A (should fail)
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TOKEN_A))
        .setSellTokenQuantity(1_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A)) // Same token!
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_same_tokens")
        .caseCategory("validate_fail")
        .description("Fail when sell and buy tokens are the same")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("same")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset same tokens: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_insufficientBalance() throws Exception {
    // Try to sell more TRX than owner has
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(10_000_000_000_000L) // More than initial balance
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_insufficient_balance")
        .caseCategory("validate_fail")
        .description("Fail when owner has insufficient balance for sell token")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset insufficient balance: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_quantityExceedsLimit() throws Exception {
    // Set a low quantity limit
    dbManager.getDynamicPropertiesStore().saveMarketQuantityLimit(1_000_000L);

    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_000_000_000L) // Exceeds limit
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_quantity_exceeds_limit")
        .caseCategory("validate_fail")
        .description("Fail when sell quantity exceeds market quantity limit")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("MARKET_QUANTITY_LIMIT", 1_000_000L)
        .expectedError("limit")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset quantity exceeds limit: validationError={}", result.getValidationError());

    // Restore limit
    dbManager.getDynamicPropertiesStore().saveMarketQuantityLimit(MARKET_QUANTITY_LIMIT);
  }

  @Test
  public void generateMarketSellAsset_zeroQuantity() throws Exception {
    // Try to sell zero tokens
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(0L) // Zero quantity
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_zero_sell_quantity")
        .caseCategory("validate_fail")
        .description("Fail when sell quantity is zero")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("quantity")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset zero quantity: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // MarketCancelOrder (53) Fixtures
  // ==========================================================================

  @Test
  public void generateMarketCancelOrder_happyPath() throws Exception {
    // First create an order to cancel
    byte[] orderId = createMarketOrder(1, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);

    // Cancel the order
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("happy_path_cancel")
        .caseCategory("happy")
        .description("Cancel an existing market order")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("market_pair_to_price")
        .database("market_pair_price_to_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketCancelOrder happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateMarketCancelOrder_happyPath_TokenOrder() throws Exception {
    // Create a token-to-token order to cancel
    byte[] orderId = createMarketOrder(2, OWNER_ADDRESS, TOKEN_A, TOKEN_B,
        500_000_000L, 500_000_000L);

    // Cancel the order
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("happy_path_cancel_token_order")
        .caseCategory("happy")
        .description("Cancel a token-to-token market order")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("market_pair_to_price")
        .database("market_pair_price_to_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketCancelOrder token order: success={}", result.isSuccess());
  }

  @Test
  public void generateMarketCancelOrder_notOwner() throws Exception {
    // Create an order owned by OWNER_ADDRESS
    byte[] orderId = createMarketOrder(3, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);

    // OTHER_ADDRESS tries to cancel (should fail)
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OTHER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("validate_fail_not_owner")
        .caseCategory("validate_fail")
        .description("Fail when non-owner tries to cancel an order")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("dynamic-properties")
        .ownerAddress(OTHER_ADDRESS)
        .expectedError("owner")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketCancelOrder not owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketCancelOrder_nonexistent() throws Exception {
    // Try to cancel a non-existent order
    byte[] fakeOrderId = new byte[32];
    System.arraycopy("nonexistent_order_id_12345678901".getBytes(), 0, fakeOrderId, 0, 32);

    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(fakeOrderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("validate_fail_nonexistent")
        .caseCategory("validate_fail")
        .description("Fail when trying to cancel non-existent order")
        .database("account")
        .database("market_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketCancelOrder nonexistent: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketCancelOrder_alreadyCanceled() throws Exception {
    // Create an order and set its state to CANCELED
    byte[] orderId = createMarketOrder(4, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);

    // Mark order as already canceled
    MarketOrderCapsule orderCapsule = chainBaseManager.getMarketOrderStore().get(orderId);
    orderCapsule.setState(State.CANCELED);
    chainBaseManager.getMarketOrderStore().put(orderId, orderCapsule);

    // Try to cancel again
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("validate_fail_already_canceled")
        .caseCategory("validate_fail")
        .description("Fail when trying to cancel an already canceled order")
        .database("account")
        .database("market_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("cancel")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketCancelOrder already canceled: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketCancelOrder_marketDisabled() throws Exception {
    // Create an order first while market is enabled
    byte[] orderId = createMarketOrder(5, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);

    // Disable market transactions
    dbManager.getDynamicPropertiesStore().saveAllowMarketTransaction(0);

    // Try to cancel (should fail)
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("validate_fail_market_disabled")
        .caseCategory("validate_fail")
        .description("Fail when market transactions are disabled")
        .database("account")
        .database("market_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("market")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketCancelOrder market disabled: validationError={}", result.getValidationError());

    // Re-enable for other tests
    dbManager.getDynamicPropertiesStore().saveAllowMarketTransaction(1);
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

  /**
   * Creates a market order for testing.
   *
   * @param count the order counter (used for unique ID generation)
   * @param ownerAddress the order owner address
   * @param sellTokenId the token to sell
   * @param buyTokenId the token to buy
   * @param sellQuantity sell token quantity
   * @param buyQuantity expected buy token quantity
   * @return the order ID
   */
  private byte[] createMarketOrder(long count, String ownerAddress, byte[] sellTokenId,
      byte[] buyTokenId, long sellQuantity, long buyQuantity) {

    byte[] ownerBytes = ByteArray.fromHexString(ownerAddress);
    long createTime = chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    // Calculate order ID using Keccak256 hash (matching Java implementation)
    byte[] orderId = calculateOrderId(ownerBytes, sellTokenId, buyTokenId, count);

    // Create the market order
    MarketOrder order = MarketOrder.newBuilder()
        .setOrderId(ByteString.copyFrom(orderId))
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setCreateTime(createTime)
        .setSellTokenId(ByteString.copyFrom(sellTokenId))
        .setSellTokenQuantity(sellQuantity)
        .setSellTokenQuantityRemain(sellQuantity)
        .setBuyTokenId(ByteString.copyFrom(buyTokenId))
        .setBuyTokenQuantity(buyQuantity)
        .setState(State.ACTIVE)
        .build();

    MarketOrderCapsule orderCapsule = new MarketOrderCapsule(order);
    chainBaseManager.getMarketOrderStore().put(orderId, orderCapsule);

    // Update market account order count
    byte[] accountKey = ownerBytes;
    MarketAccountOrderCapsule accountOrder;
    try {
      if (chainBaseManager.getMarketAccountStore().has(accountKey)) {
        accountOrder = chainBaseManager.getMarketAccountStore().get(accountKey);
      } else {
        accountOrder = new MarketAccountOrderCapsule(ByteString.copyFrom(ownerBytes));
      }
    } catch (Exception e) {
      accountOrder = new MarketAccountOrderCapsule(ByteString.copyFrom(ownerBytes));
    }
    accountOrder.setCount(count);
    chainBaseManager.getMarketAccountStore().put(accountKey, accountOrder);

    log.info("Created market order {} for testing: {} -> {}",
        ByteArray.toHexString(orderId).substring(0, 16) + "...",
        new String(sellTokenId), new String(buyTokenId));

    return orderId;
  }

  /**
   * Calculate order ID using Keccak256 hash (matching MarketUtils.calculateOrderId).
   */
  private byte[] calculateOrderId(byte[] owner, byte[] sellTokenId, byte[] buyTokenId, long count) {
    // Combine: owner (21 bytes) + sellTokenId + buyTokenId + count (8 bytes)
    byte[] countBytes = new byte[8];
    for (int i = 7; i >= 0; i--) {
      countBytes[i] = (byte) (count & 0xFF);
      count >>= 8;
    }

    int len = owner.length + sellTokenId.length + buyTokenId.length + 8;
    byte[] data = new byte[len];
    int offset = 0;
    System.arraycopy(owner, 0, data, offset, owner.length);
    offset += owner.length;
    System.arraycopy(sellTokenId, 0, data, offset, sellTokenId.length);
    offset += sellTokenId.length;
    System.arraycopy(buyTokenId, 0, data, offset, buyTokenId.length);
    offset += buyTokenId.length;
    System.arraycopy(countBytes, 0, data, offset, 8);

    // Use SHA3-256 (Keccak256)
    return org.tron.common.crypto.Hash.sha3(data);
  }
}
