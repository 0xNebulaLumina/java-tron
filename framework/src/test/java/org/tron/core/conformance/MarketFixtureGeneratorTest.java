package org.tron.core.conformance;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import java.io.File;
import java.util.ArrayList;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import org.junit.Before;
import org.junit.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.BaseTest;
import org.tron.common.utils.ByteArray;
import org.tron.core.Constant;
import org.tron.core.Wallet;
import org.tron.core.actuator.MarketSellAssetActuator;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.MarketAccountOrderCapsule;
import org.tron.core.capsule.MarketOrderCapsule;
import org.tron.core.capsule.MarketOrderIdListCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.config.args.Args;
import org.tron.core.capsule.utils.MarketUtils;
import org.tron.core.db.TronStoreWithRevoking;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.MarketOrder.State;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.MarketContract.MarketCancelOrderContract;
import org.tron.protos.contract.MarketContract.MarketSellAssetContract;

/**
 * Generates conformance test fixtures for Market contracts (52-53).
 *
 * <p>Run with: ./gradlew :framework:test --tests "MarketFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures
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
    clearMarketStores();

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
    // Set dynamic properties FIRST
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(1000000);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
    // Enable allowSameTokenName for V2
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(1);
    // Seed TRC-10 assets for validate() checks
    ConformanceFixtureTestSupport.putAssetIssueV2(
        dbManager, new String(TOKEN_A), OWNER_ADDRESS, "TokenA", 1_000_000_000_000L);
    ConformanceFixtureTestSupport.putAssetIssueV2(
        dbManager, new String(TOKEN_B), OWNER_ADDRESS, "TokenB", 1_000_000_000_000L);
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
  public void generateMarketSellAsset_edge_matchSingleMakerOrder() throws Exception {
    byte[] makerOrderId = createMarketOrder(1, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN,
        1_000_000_000L, 1_000_000_000L);

    // Taker: Sell TRX to buy TOKEN_A, should match the single maker order.
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
        .caseName("edge_match_single_maker_order")
        .caseCategory("edge")
        .description("Match a taker order against one maker order")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("market_pair_to_price")
        .database("market_pair_price_to_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    assertTrue(result.isSuccess());
    assertNotNull(result.getResultProto());
    assertEquals(1, result.getResultProto().getOrderDetailsCount());

    MarketOrderCapsule makerOrderCapsule = chainBaseManager.getMarketOrderStore().get(makerOrderId);
    assertEquals(State.INACTIVE, makerOrderCapsule.getSt());
  }

  @Test
  public void generateMarketSellAsset_edge_matchLoop_samePriceMultipleOrders() throws Exception {
    createMarketOrder(1, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN, 1_000_000_000L, 1_000_000_000L);
    createMarketOrder(2, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN, 1_000_000_000L, 1_000_000_000L);
    createMarketOrder(3, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN, 1_000_000_000L, 1_000_000_000L);

    // Taker matches three maker orders at the same price (inner match loop).
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(3_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(3_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);
    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("edge_match_loop_same_price_multiple_orders")
        .caseCategory("edge")
        .description("Match loop: taker consumes multiple maker orders at the same price")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("market_pair_to_price")
        .database("market_pair_price_to_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    assertTrue(result.isSuccess());
    assertNotNull(result.getResultProto());
    assertEquals(3, result.getResultProto().getOrderDetailsCount());
  }

  @Test
  public void generateMarketSellAsset_edge_maxMatchNum_atLimit() throws Exception {
    int maxMatchNum = MarketSellAssetActuator.getMAX_MATCH_NUM();
    long unit = 1_000_000L;
    for (int i = 0; i < maxMatchNum; i++) {
      createMarketOrder(i, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN, unit, unit);
    }

    // Taker matches exactly MAX_MATCH_NUM maker orders.
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(unit * maxMatchNum)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(unit * maxMatchNum)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);
    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("edge_max_match_num_at_limit")
        .caseCategory("edge")
        .description("MAX_MATCH_NUM boundary: exactly at limit should succeed")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("market_pair_to_price")
        .database("market_pair_price_to_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    assertTrue(result.isSuccess());
    assertNotNull(result.getResultProto());
    assertEquals(maxMatchNum, result.getResultProto().getOrderDetailsCount());
  }

  @Test
  public void generateMarketSellAsset_edge_maxMatchNum_exceeded() throws Exception {
    int maxMatchNum = MarketSellAssetActuator.getMAX_MATCH_NUM();
    long unit = 1_000_000L;
    for (int i = 0; i < maxMatchNum + 1; i++) {
      createMarketOrder(i, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN, unit, unit);
    }

    // Taker tries to match MAX_MATCH_NUM + 1 maker orders; execution should revert.
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(unit * (maxMatchNum + 1))
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(unit * (maxMatchNum + 1))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);
    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("edge_max_match_num_exceeded")
        .caseCategory("edge")
        .description("MAX_MATCH_NUM boundary: exceeding limit should revert")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("market_pair_to_price")
        .database("market_pair_price_to_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    assertFalse(result.isSuccess());
    assertNotNull(result.getExecutionError());
    assertTrue(result.getExecutionError().contains("Too many matches. MAX_MATCH_NUM"));
  }

  @Test
  public void generateMarketSellAsset_edge_priceQueueCleanup_removeEmptyPriceLevel() throws Exception {
    byte[] makerOrderId1 = createMarketOrder(1, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN,
        1_000_000_000L, 1_000_000_000L);
    byte[] makerOrderId2 = createMarketOrder(2, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN,
        1_000_000_000L, 2_000_000_000L);

    // Taker only matches the best price level (1:1), leaving the worse price level intact.
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
        .caseName("edge_price_queue_cleanup_remove_empty_price_level")
        .caseCategory("edge")
        .description("Price queue cleanup: delete empty price level after matching")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("market_pair_to_price")
        .database("market_pair_price_to_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    assertTrue(result.isSuccess());
    assertNotNull(result.getResultProto());
    assertEquals(1, result.getResultProto().getOrderDetailsCount());

    MarketOrderCapsule makerOrderCapsule1 = chainBaseManager.getMarketOrderStore().get(makerOrderId1);
    assertEquals(State.INACTIVE, makerOrderCapsule1.getSt());

    MarketOrderCapsule makerOrderCapsule2 = chainBaseManager.getMarketOrderStore().get(makerOrderId2);
    assertEquals(State.ACTIVE, makerOrderCapsule2.getSt());

    byte[] makerPair = MarketUtils.createPairKey(TOKEN_A, TRX_TOKEN);
    assertEquals(1L, chainBaseManager.getMarketPairToPriceStore().getPriceNum(makerPair));

    byte[] priceKey1 = MarketUtils.createPairPriceKey(TOKEN_A, TRX_TOKEN, 1_000_000_000L, 1_000_000_000L);
    byte[] priceKey2 = MarketUtils.createPairPriceKey(TOKEN_A, TRX_TOKEN, 1_000_000_000L, 2_000_000_000L);
    assertFalse(chainBaseManager.getMarketPairPriceToOrderStore().has(priceKey1));
    assertTrue(chainBaseManager.getMarketPairPriceToOrderStore().has(priceKey2));
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

  private void clearMarketStores() {
    clearStore(chainBaseManager.getMarketAccountStore());
    clearStore(chainBaseManager.getMarketOrderStore());
    clearStore(chainBaseManager.getMarketPairToPriceStore());
    clearStore(chainBaseManager.getMarketPairPriceToOrderStore());
  }

  private void clearStore(TronStoreWithRevoking<?> store) {
    List<byte[]> keys = new ArrayList<>();
    Iterator<?> iterator = store.iterator();
    while (iterator.hasNext()) {
      Object entry = iterator.next();
      if (entry instanceof Map.Entry) {
        keys.add((byte[]) ((Map.Entry<?, ?>) entry).getKey());
      }
    }
    for (byte[] key : keys) {
      store.delete(key);
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
      byte[] buyTokenId, long sellQuantity, long buyQuantity) throws Exception {

    byte[] ownerBytes = ByteArray.fromHexString(ownerAddress);
    long createTime = chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();

    // Deduct sell balance/token from owner to simulate order creation
    AccountCapsule ownerAccount = chainBaseManager.getAccountStore().get(ownerBytes);
    if (ownerAccount == null) {
      throw new IllegalStateException("Account does not exist: " + ownerAddress);
    }
    if (java.util.Arrays.equals(sellTokenId, TRX_TOKEN)) {
      ownerAccount.setBalance(ownerAccount.getBalance() - sellQuantity);
    } else if (!ownerAccount.reduceAssetAmountV2(
        sellTokenId,
        sellQuantity,
        chainBaseManager.getDynamicPropertiesStore(),
        chainBaseManager.getAssetIssueStore())) {
      throw new IllegalStateException("Insufficient token balance for sellTokenId");
    }
    chainBaseManager.getAccountStore().put(ownerBytes, ownerAccount);

    // Calculate order ID using MarketUtils.calculateOrderId (Keccak256)
    byte[] orderId = MarketUtils.calculateOrderId(ByteString.copyFrom(ownerBytes),
        sellTokenId, buyTokenId, count);

    // Create the market order
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setSellTokenId(ByteString.copyFrom(sellTokenId))
        .setSellTokenQuantity(sellQuantity)
        .setBuyTokenId(ByteString.copyFrom(buyTokenId))
        .setBuyTokenQuantity(buyQuantity)
        .build();

    MarketOrderCapsule orderCapsule = new MarketOrderCapsule(orderId, contract);
    orderCapsule.setCreateTime(createTime);

    chainBaseManager.getMarketOrderStore().put(orderId, orderCapsule);

    // Update market account order list/counters (count = active, totalCount = orderId counter)
    MarketAccountOrderCapsule accountOrderCapsule = chainBaseManager
        .getMarketAccountStore()
        .getUnchecked(ownerBytes);
    if (accountOrderCapsule == null) {
      accountOrderCapsule = new MarketAccountOrderCapsule(ByteString.copyFrom(ownerBytes));
    }
    accountOrderCapsule.addOrders(orderCapsule.getID());
    accountOrderCapsule.setCount(accountOrderCapsule.getCount() + 1);
    accountOrderCapsule.setTotalCount(Math.max(accountOrderCapsule.getTotalCount(), count + 1));
    chainBaseManager.getMarketAccountStore().put(ownerBytes, accountOrderCapsule);

    // Add order into order book (pairToPrice / pairPriceToOrder)
    byte[] pairPriceKey = MarketUtils.createPairPriceKey(
        sellTokenId, buyTokenId, sellQuantity, buyQuantity);
    MarketOrderIdListCapsule orderIdListCapsule = chainBaseManager
        .getMarketPairPriceToOrderStore()
        .getUnchecked(pairPriceKey);
    if (orderIdListCapsule == null) {
      orderIdListCapsule = new MarketOrderIdListCapsule();
      chainBaseManager.getMarketPairToPriceStore()
          .addNewPriceKey(sellTokenId, buyTokenId, chainBaseManager.getMarketPairPriceToOrderStore());
    }
    orderIdListCapsule.addOrder(orderCapsule, chainBaseManager.getMarketOrderStore());
    chainBaseManager.getMarketPairPriceToOrderStore().put(pairPriceKey, orderIdListCapsule);

    log.info("Created market order {} for testing: {} -> {}",
        ByteArray.toHexString(orderId).substring(0, 16) + "...",
        new String(sellTokenId), new String(buyTokenId));

    return orderId;
  }
}
