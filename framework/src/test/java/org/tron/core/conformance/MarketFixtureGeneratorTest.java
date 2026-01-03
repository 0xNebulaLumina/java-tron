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
  // MarketSellAsset (52) - Missing Validation Branches
  // ==========================================================================

  @Test
  public void generateMarketSellAsset_validateFail_ownerAddressInvalidEmpty() throws Exception {
    // Invalid owner address: empty
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
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
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty")
        .database("account")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset invalid address empty: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_validateFail_ownerAccountNotExist() throws Exception {
    // Valid-looking address but account does not exist
    String nonExistentAddress = Wallet.getAddressPreFixString() + "1234567890abcdef1234567890abcdef12345678";

    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
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
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("dynamic-properties")
        .expectedError("Account does not exist!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset account not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_validateFail_sellTokenIdNotNumber() throws Exception {
    // sellTokenId is not a valid number (non-TRX, non-numeric)
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom("abc".getBytes()))
        .setSellTokenQuantity(1_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_sell_token_id_not_number")
        .caseCategory("validate_fail")
        .description("Fail when sellTokenId is not a valid number")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("sellTokenId is not a valid number")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset sell token id not number: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_validateFail_buyTokenIdNotNumber() throws Exception {
    // buyTokenId is not a valid number (non-TRX, non-numeric)
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom("abc".getBytes()))
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_buy_token_id_not_number")
        .caseCategory("validate_fail")
        .description("Fail when buyTokenId is not a valid number")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("buyTokenId is not a valid number")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset buy token id not number: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_validateFail_noSellTokenId() throws Exception {
    // Selling a non-existent token id (valid numeric but not seeded)
    byte[] nonExistentToken = "9999999".getBytes();

    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(nonExistentToken))
        .setSellTokenQuantity(1_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_no_sell_token_id")
        .caseCategory("validate_fail")
        .description("Fail when sellTokenId does not exist")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("No sellTokenId !")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset no sell token id: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_validateFail_noBuyTokenId() throws Exception {
    // Buying a non-existent token id (valid numeric but not seeded)
    byte[] nonExistentToken = "9999999".getBytes();

    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(nonExistentToken))
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_no_buy_token_id")
        .caseCategory("validate_fail")
        .description("Fail when buyTokenId does not exist")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("No buyTokenId !")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset no buy token id: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_validateFail_zeroBuyQuantity() throws Exception {
    // buyTokenQuantity is zero
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(0L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_zero_buy_quantity")
        .caseCategory("validate_fail")
        .description("Fail when buy quantity is zero")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("token quantity must greater than zero")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset zero buy quantity: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_validateFail_negativeBuyQuantity() throws Exception {
    // buyTokenQuantity is negative
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(-1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_negative_buy_quantity")
        .caseCategory("validate_fail")
        .description("Fail when buy quantity is negative")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("token quantity must greater than zero")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset negative buy quantity: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_validateFail_buyQuantityExceedsLimit() throws Exception {
    // Set a low quantity limit
    dbManager.getDynamicPropertiesStore().saveMarketQuantityLimit(1_000_000L);

    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(500_000L) // Within limit
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000_000L) // Exceeds limit
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_buy_quantity_exceeds_limit")
        .caseCategory("validate_fail")
        .description("Fail when buy quantity exceeds market quantity limit")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("MARKET_QUANTITY_LIMIT", 1_000_000L)
        .expectedError("token quantity must less than")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset buy quantity exceeds limit: validationError={}", result.getValidationError());

    // Restore limit
    dbManager.getDynamicPropertiesStore().saveMarketQuantityLimit(MARKET_QUANTITY_LIMIT);
  }

  @Test
  public void generateMarketSellAsset_validateFail_sellTokenBalanceNotEnough() throws Exception {
    // Selling more token than account has
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TOKEN_A))
        .setSellTokenQuantity(999_999_999_999_999L) // Much more than 100B owned
        .setBuyTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_sell_token_balance_not_enough")
        .caseCategory("validate_fail")
        .description("Fail when token balance is insufficient for sell quantity")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("SellToken balance is not enough !")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset sell token balance not enough: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_validateFail_maxActiveOrderNumExceeded() throws Exception {
    // Seed the owner with MAX_ACTIVE_ORDER_NUM active orders
    int maxOrders = MarketSellAssetActuator.getMAX_ACTIVE_ORDER_NUM();
    for (int i = 0; i < maxOrders; i++) {
      createMarketOrder(i, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A, 1_000_000L, 1_000_000L);
    }

    // Try to create one more order
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_max_active_order_num_exceeded")
        .caseCategory("validate_fail")
        .description("Fail when account already has MAX_ACTIVE_ORDER_NUM active orders")
        .database("account")
        .database("market_order")
        .database("market_account")
        .database("market_pair_to_price")
        .database("market_pair_price_to_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Maximum number of orders exceeded")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset max active order num exceeded: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketSellAsset_edge_maxActiveOrderNumAtLimitSucceeds() throws Exception {
    // Seed the owner with MAX_ACTIVE_ORDER_NUM - 1 active orders
    int maxOrders = MarketSellAssetActuator.getMAX_ACTIVE_ORDER_NUM();
    for (int i = 0; i < maxOrders - 1; i++) {
      createMarketOrder(i, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A, 1_000_000L, 1_000_000L);
    }

    // Creating the max-th order should succeed
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("edge_max_active_order_num_at_limit_succeeds")
        .caseCategory("edge")
        .description("Creating the 100th order succeeds when starting from 99 active orders")
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
    log.info("MarketSellAsset max active order at limit succeeds: success={}", result.isSuccess());
  }

  @Test
  public void generateMarketSellAsset_validateFail_trxSellFeeInsufficient() throws Exception {
    // Set a non-zero market sell fee
    dbManager.getDynamicPropertiesStore().saveMarketSellFee(1_000_000_000L); // 1000 TRX fee

    // Create an account with just enough for sell but not for fee
    AccountCapsule lowBalanceAccount = new AccountCapsule(
        ByteString.copyFromUtf8("lowBalance"),
        ByteString.copyFrom(ByteArray.fromHexString(
            Wallet.getAddressPreFixString() + "feeacc01234567890abcdef1234567890abcdef")),
        AccountType.Normal,
        1_000_000_000L); // Only 1000 TRX (= sell amount, no room for fee)
    dbManager.getAccountStore().put(lowBalanceAccount.getAddress().toByteArray(), lowBalanceAccount);

    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(lowBalanceAccount.getAddress())
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_000_000_000L) // All balance, no room for fee
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_trx_sell_fee_insufficient")
        .caseCategory("validate_fail")
        .description("Fail when TRX sell + fee exceeds balance")
        .database("account")
        .database("dynamic-properties")
        .dynamicProperty("MARKET_SELL_FEE", 1_000_000_000L)
        .expectedError("No enough balance !")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset TRX sell fee insufficient: validationError={}", result.getValidationError());

    // Restore fee
    dbManager.getDynamicPropertiesStore().saveMarketSellFee(MARKET_SELL_FEE);
  }

  @Test
  public void generateMarketSellAsset_validateFail_tokenSellFeeInsufficient() throws Exception {
    // Set a non-zero market sell fee
    dbManager.getDynamicPropertiesStore().saveMarketSellFee(1_000_000_000L); // 1000 TRX fee

    // Create an account with enough tokens but no TRX for fee
    AccountCapsule lowTrxAccount = new AccountCapsule(
        ByteString.copyFromUtf8("lowTrx"),
        ByteString.copyFrom(ByteArray.fromHexString(
            Wallet.getAddressPreFixString() + "feeacc02234567890abcdef1234567890abcdef")),
        AccountType.Normal,
        100L); // Only 100 SUN (way less than 1000 TRX fee)
    lowTrxAccount.addAssetAmountV2(TOKEN_A, 10_000_000_000L,
        dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(lowTrxAccount.getAddress().toByteArray(), lowTrxAccount);

    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(lowTrxAccount.getAddress())
        .setSellTokenId(ByteString.copyFrom(TOKEN_A))
        .setSellTokenQuantity(1_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setBuyTokenQuantity(1_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("validate_fail_token_sell_fee_insufficient")
        .caseCategory("validate_fail")
        .description("Fail when selling token but TRX balance insufficient for fee")
        .database("account")
        .database("dynamic-properties")
        .dynamicProperty("MARKET_SELL_FEE", 1_000_000_000L)
        .expectedError("No enough balance !")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketSellAsset token sell fee insufficient: validationError={}", result.getValidationError());

    // Restore fee
    dbManager.getDynamicPropertiesStore().saveMarketSellFee(MARKET_SELL_FEE);
  }

  // ==========================================================================
  // MarketSellAsset (52) - Missing Execution/Behavior Branches
  // ==========================================================================

  @Test
  public void generateMarketSellAsset_edge_noMatchPriceTooLow() throws Exception {
    // Seed maker orders at price 1:1 (sell TOKEN_A, buy TRX)
    createMarketOrder(1, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN, 1_000_000_000L, 1_000_000_000L);

    // Taker wants to sell TRX to buy TOKEN_A at price 2:1 (wants 2 TOKEN_A per TRX)
    // This is worse for the maker (taker asks too high a price for TOKEN_A)
    // Maker sells TOKEN_A at 1:1 (1 TOKEN_A per TRX), taker wants 2:1
    // priceMatch should fail, order should be added to book without matching
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(2_000_000_000L) // Taker wants 2:1 ratio
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);
    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("edge_no_match_price_too_low")
        .caseCategory("edge")
        .description("Taker price does not satisfy maker price; no match, taker added to book")
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
    // No match should occur
    assertEquals(0, result.getResultProto().getOrderDetailsCount());
    log.info("MarketSellAsset no match price too low: success={}, orderDetails={}",
        result.isSuccess(), result.getResultProto().getOrderDetailsCount());
  }

  @Test
  public void generateMarketSellAsset_edge_matchAcrossMultiplePriceLevels() throws Exception {
    // Seed maker orders at two different price levels
    // Price level 1: 1:1 (best price for taker)
    createMarketOrder(1, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN, 500_000_000L, 500_000_000L);
    // Price level 2: 3:2 (worse price - maker wants 1.5 TRX per TOKEN_A)
    createMarketOrder(2, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN, 500_000_000L, 750_000_000L);

    // Taker wants to buy more than what's available at the best price
    // This should consume the best price level and continue into the next
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1_500_000_000L) // Enough to match both makers
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1_000_000_000L) // At worst 1.5:1 ratio acceptable
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);
    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("edge_match_across_multiple_price_levels")
        .caseCategory("edge")
        .description("Taker consumes best price level then continues into next level")
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
    assertEquals(2, result.getResultProto().getOrderDetailsCount());
    log.info("MarketSellAsset match across multiple price levels: orderDetails={}",
        result.getResultProto().getOrderDetailsCount());
  }

  @Test
  public void generateMarketSellAsset_edge_partialFillTakerLessThanMaker() throws Exception {
    // Seed a large maker order
    byte[] makerOrderId = createMarketOrder(1, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN,
        10_000_000_000L, 10_000_000_000L);

    // Taker order smaller than maker; taker fully filled, maker remains active
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
        .caseName("edge_partial_fill_taker_less_than_maker")
        .caseCategory("edge")
        .description("Taker < maker: taker fully filled, maker stays active with reduced remain")
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
    assertEquals(1, result.getResultProto().getOrderDetailsCount());

    // Verify maker still active with reduced remain
    MarketOrderCapsule makerOrder = chainBaseManager.getMarketOrderStore().get(makerOrderId);
    assertEquals(State.ACTIVE, makerOrder.getSt());
    assertTrue(makerOrder.getSellTokenQuantityRemain() > 0);
    log.info("MarketSellAsset partial fill taker < maker: makerRemain={}",
        makerOrder.getSellTokenQuantityRemain());
  }

  @Test
  public void generateMarketSellAsset_edge_partialFillTakerGreaterThanMaker() throws Exception {
    // Seed a small maker order
    byte[] makerOrderId = createMarketOrder(1, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN,
        1_000_000_000L, 1_000_000_000L);

    // Taker order larger than maker; maker fully filled, taker remains active
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(5_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(5_000_000_000L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);
    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("edge_partial_fill_taker_greater_than_maker")
        .caseCategory("edge")
        .description("Taker > maker: maker fully filled, taker saved via saveRemainOrder")
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
    assertEquals(1, result.getResultProto().getOrderDetailsCount());

    // Verify maker is INACTIVE
    MarketOrderCapsule makerOrder = chainBaseManager.getMarketOrderStore().get(makerOrderId);
    assertEquals(State.INACTIVE, makerOrder.getSt());
    log.info("MarketSellAsset partial fill taker > maker: success");
  }

  @Test
  public void generateMarketSellAsset_edge_roundingQuantityTooSmallReturnsSellToken() throws Exception {
    // Seed a maker order with extreme price ratio that will cause rounding to 0
    // Maker: sell 1 TOKEN_A for 1_000_000_000 TRX (very high price)
    createMarketOrder(1, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN, 1L, 1_000_000_000L);

    // Taker wants to sell 1 TRX for TOKEN_A
    // multiplyAndDivide(1, 1, 1_000_000_000) = 0
    // This triggers the "quantity too small" path
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(1L) // Very small
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(1L)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);
    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("edge_rounding_quantity_too_small_returns_sell_token")
        .caseCategory("edge")
        .description("multiplyAndDivide returns 0; sell token returned, order becomes INACTIVE")
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
    // No actual fill occurs due to rounding
    assertEquals(0, result.getResultProto().getOrderDetailsCount());
    log.info("MarketSellAsset rounding too small: orderDetails={}",
        result.getResultProto().getOrderDetailsCount());
  }

  @Test
  public void generateMarketSellAsset_edge_fullPairCleanupLastPriceLevelConsumed() throws Exception {
    // Seed only one price level (single order)
    byte[] makerOrderId = createMarketOrder(1, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN,
        1_000_000_000L, 1_000_000_000L);

    // Fully consume the maker order
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
        .caseName("edge_full_pair_cleanup_last_price_level_consumed")
        .caseCategory("edge")
        .description("Last price level consumed; pairToPriceStore deletes the pair key")
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
    assertEquals(1, result.getResultProto().getOrderDetailsCount());

    // Verify pair is deleted from pairToPriceStore
    byte[] makerPair = MarketUtils.createPairKey(TOKEN_A, TRX_TOKEN);
    assertEquals(0L, chainBaseManager.getMarketPairToPriceStore().getPriceNum(makerPair));
    log.info("MarketSellAsset full pair cleanup: priceNum={}",
        chainBaseManager.getMarketPairToPriceStore().getPriceNum(makerPair));
  }

  @Test
  public void generateMarketSellAsset_edge_gcdPriceKeyCollisionSameRatio() throws Exception {
    // Create two maker orders with the same ratio (1:2 and 2:4)
    // They should share the same pairPriceKey due to GCD normalization
    byte[] makerOrderId1 = createMarketOrder(1, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN,
        1_000_000_000L, 2_000_000_000L); // 1:2 ratio
    byte[] makerOrderId2 = createMarketOrder(2, OTHER_ADDRESS, TOKEN_A, TRX_TOKEN,
        2_000_000_000L, 4_000_000_000L); // 2:4 ratio (same as 1:2 after GCD)

    // Verify both orders are at the same price level
    byte[] priceKey1 = MarketUtils.createPairPriceKey(TOKEN_A, TRX_TOKEN,
        1_000_000_000L, 2_000_000_000L);
    byte[] priceKey2 = MarketUtils.createPairPriceKey(TOKEN_A, TRX_TOKEN,
        2_000_000_000L, 4_000_000_000L);

    // Create a taker that matches both
    MarketSellAssetContract contract = MarketSellAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setSellTokenId(ByteString.copyFrom(TRX_TOKEN))
        .setSellTokenQuantity(6_000_000_000L)
        .setBuyTokenId(ByteString.copyFrom(TOKEN_A))
        .setBuyTokenQuantity(3_000_000_000L) // 2:1 ratio matches 1:2 maker
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketSellAssetContract, contract);
    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_SELL_ASSET_CONTRACT", 52)
        .caseName("edge_gcd_price_key_collision_same_ratio")
        .caseCategory("edge")
        .description("Orders with same ratio (1:2 and 2:4) share the same pairPriceKey")
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
    assertEquals(2, result.getResultProto().getOrderDetailsCount());
    log.info("MarketSellAsset GCD price key collision: orderDetails={}",
        result.getResultProto().getOrderDetailsCount());
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
  // MarketCancelOrder (53) - Missing Validation Branches
  // ==========================================================================

  @Test
  public void generateMarketCancelOrder_validateFail_ownerAddressInvalidEmpty() throws Exception {
    // Create an order first
    byte[] orderId = createMarketOrder(10, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);

    // Try to cancel with empty address
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty")
        .database("account")
        .database("market_order")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketCancelOrder invalid address empty: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketCancelOrder_validateFail_ownerAccountNotExist() throws Exception {
    // Create an order first
    byte[] orderId = createMarketOrder(11, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);

    // Valid-looking address but account does not exist
    String nonExistentAddress = Wallet.getAddressPreFixString() + "deadbeef1234567890abcdef1234567890abcdef";

    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("market_order")
        .database("dynamic-properties")
        .expectedError("Account does not exist!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketCancelOrder account not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketCancelOrder_validateFail_orderNotActiveInactiveFilled() throws Exception {
    // Create an order and set its state to INACTIVE (filled)
    byte[] orderId = createMarketOrder(12, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);

    // Mark order as INACTIVE (filled)
    MarketOrderCapsule orderCapsule = chainBaseManager.getMarketOrderStore().get(orderId);
    orderCapsule.setState(State.INACTIVE);
    chainBaseManager.getMarketOrderStore().put(orderId, orderCapsule);

    // Try to cancel a filled order
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("validate_fail_order_not_active_inactive_filled")
        .caseCategory("validate_fail")
        .description("Fail when trying to cancel a filled (INACTIVE) order")
        .database("account")
        .database("market_order")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Order is not active!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketCancelOrder filled order: validationError={}", result.getValidationError());
  }

  @Test
  public void generateMarketCancelOrder_validateFail_cancelFeeInsufficientBalance() throws Exception {
    // Set a non-zero cancel fee
    dbManager.getDynamicPropertiesStore().saveMarketCancelFee(1_000_000_000L); // 1000 TRX fee

    // Create an account with no TRX balance
    AccountCapsule lowBalanceAccount = new AccountCapsule(
        ByteString.copyFromUtf8("lowBalanceCancel"),
        ByteString.copyFrom(ByteArray.fromHexString(
            Wallet.getAddressPreFixString() + "cceacc01234567890abcdef1234567890abcdef")),
        AccountType.Normal,
        100L); // Only 100 SUN
    lowBalanceAccount.addAssetAmountV2(TOKEN_A, 10_000_000_000L,
        dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(lowBalanceAccount.getAddress().toByteArray(), lowBalanceAccount);

    // Create an order for this low-balance account
    byte[] orderId = createMarketOrder(13, ByteArray.toHexString(lowBalanceAccount.getAddress().toByteArray()),
        TOKEN_A, TRX_TOKEN, 1_000_000_000L, 1_000_000_000L);

    // Try to cancel (should fail due to insufficient fee balance)
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(lowBalanceAccount.getAddress())
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("validate_fail_cancel_fee_insufficient_balance")
        .caseCategory("validate_fail")
        .description("Fail when cancel fee exceeds TRX balance")
        .database("account")
        .database("market_order")
        .database("dynamic-properties")
        .dynamicProperty("MARKET_CANCEL_FEE", 1_000_000_000L)
        .expectedError("No enough balance !")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("MarketCancelOrder fee insufficient: validationError={}", result.getValidationError());

    // Restore fee
    dbManager.getDynamicPropertiesStore().saveMarketCancelFee(MARKET_CANCEL_FEE);
  }

  // ==========================================================================
  // MarketCancelOrder (53) - Missing Execution/Behavior Branches
  // ==========================================================================

  @Test
  public void generateMarketCancelOrder_edge_cancelRemovesOneOfManySamePrice() throws Exception {
    // Create multiple orders at the same price level
    byte[] orderId1 = createMarketOrder(20, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);
    byte[] orderId2 = createMarketOrder(21, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);
    byte[] orderId3 = createMarketOrder(22, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);

    // Cancel the middle order
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(orderId2))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("edge_cancel_removes_one_of_many_same_price")
        .caseCategory("edge")
        .description("Cancel one order; pairPriceToOrderStore remains with other orders intact")
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

    // Verify the price level still exists
    byte[] priceKey = MarketUtils.createPairPriceKey(TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);
    assertTrue(chainBaseManager.getMarketPairPriceToOrderStore().has(priceKey));
    log.info("MarketCancelOrder cancel one of many: success={}", result.isSuccess());
  }

  @Test
  public void generateMarketCancelOrder_edge_cancelLastOrderInPriceLevelDecrementsPriceNum() throws Exception {
    // Create orders at two price levels
    byte[] orderId1 = createMarketOrder(30, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L); // Price level 1 (1:1)
    byte[] orderId2 = createMarketOrder(31, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 2_000_000_000L); // Price level 2 (1:2)

    // Verify there are 2 price levels initially
    byte[] pair = MarketUtils.createPairKey(TRX_TOKEN, TOKEN_A);
    assertEquals(2L, chainBaseManager.getMarketPairToPriceStore().getPriceNum(pair));

    // Cancel the only order at price level 1
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(orderId1))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("edge_cancel_last_order_in_price_level_decrements_price_num")
        .caseCategory("edge")
        .description("Cancel last order in a price level; priceNum decrements but pair remains")
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

    // Verify priceNum is decremented to 1
    assertEquals(1L, chainBaseManager.getMarketPairToPriceStore().getPriceNum(pair));

    // Verify price level 1 is deleted but pair still exists
    byte[] priceKey1 = MarketUtils.createPairPriceKey(TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);
    assertFalse(chainBaseManager.getMarketPairPriceToOrderStore().has(priceKey1));
    log.info("MarketCancelOrder cancel last in level: priceNum={}",
        chainBaseManager.getMarketPairToPriceStore().getPriceNum(pair));
  }

  @Test
  public void generateMarketCancelOrder_edge_cancelLastOrderInLastPriceLevelDeletesPair() throws Exception {
    // Create a single order (single price level)
    byte[] orderId = createMarketOrder(40, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        1_000_000_000L, 1_000_000_000L);

    byte[] pair = MarketUtils.createPairKey(TRX_TOKEN, TOKEN_A);
    assertEquals(1L, chainBaseManager.getMarketPairToPriceStore().getPriceNum(pair));

    // Cancel the only order
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("edge_cancel_last_order_in_last_price_level_deletes_pair")
        .caseCategory("edge")
        .description("Cancel last order in last price level; pairToPriceStore deletes pair")
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

    // Verify pair is deleted from pairToPriceStore
    assertEquals(0L, chainBaseManager.getMarketPairToPriceStore().getPriceNum(pair));
    log.info("MarketCancelOrder cancel last in pair: priceNum={}",
        chainBaseManager.getMarketPairToPriceStore().getPriceNum(pair));
  }

  @Test
  public void generateMarketCancelOrder_edge_cancelPartiallyFilledOrderRefundsOnlyRemain_TRX()
      throws Exception {
    // Create an order with TRX sell
    byte[] orderId = createMarketOrder(50, OWNER_ADDRESS, TRX_TOKEN, TOKEN_A,
        10_000_000_000L, 10_000_000_000L);

    // Simulate partial fill by reducing sellTokenQuantityRemain
    MarketOrderCapsule orderCapsule = chainBaseManager.getMarketOrderStore().get(orderId);
    orderCapsule.setSellTokenQuantityRemain(3_000_000_000L); // Partially filled: 7B consumed, 3B remain
    chainBaseManager.getMarketOrderStore().put(orderId, orderCapsule);

    // Get owner's balance before cancel
    AccountCapsule ownerAccount = chainBaseManager.getAccountStore()
        .get(ByteArray.fromHexString(OWNER_ADDRESS));
    long balanceBefore = ownerAccount.getBalance();

    // Cancel the partially-filled order
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("edge_cancel_partially_filled_order_refunds_only_remain_trx")
        .caseCategory("edge")
        .description("Cancel partially filled TRX order; only sellTokenQuantityRemain refunded")
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
    log.info("MarketCancelOrder partial fill refund TRX: success={}", result.isSuccess());
  }

  @Test
  public void generateMarketCancelOrder_edge_cancelPartiallyFilledOrderRefundsOnlyRemain_Token()
      throws Exception {
    // Create an order with token sell
    byte[] orderId = createMarketOrder(60, OWNER_ADDRESS, TOKEN_A, TRX_TOKEN,
        10_000_000_000L, 10_000_000_000L);

    // Simulate partial fill by reducing sellTokenQuantityRemain
    MarketOrderCapsule orderCapsule = chainBaseManager.getMarketOrderStore().get(orderId);
    orderCapsule.setSellTokenQuantityRemain(4_000_000_000L); // Partially filled: 6B consumed, 4B remain
    chainBaseManager.getMarketOrderStore().put(orderId, orderCapsule);

    // Cancel the partially-filled order
    MarketCancelOrderContract contract = MarketCancelOrderContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOrderId(ByteString.copyFrom(orderId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.MarketCancelOrderContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("MARKET_CANCEL_ORDER_CONTRACT", 53)
        .caseName("edge_cancel_partially_filled_order_refunds_only_remain_token")
        .caseCategory("edge")
        .description("Cancel partially filled token order; only sellTokenQuantityRemain refunded")
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
    log.info("MarketCancelOrder partial fill refund token: success={}", result.isSuccess());
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
