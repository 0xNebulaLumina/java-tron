package org.tron.core.conformance;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import com.google.protobuf.Message;
import org.tron.common.utils.ByteArray;
import org.tron.core.Wallet;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.AssetIssueCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.capsule.WitnessCapsule;
import org.tron.core.db.Manager;
import org.tron.core.store.DynamicPropertiesStore;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract;

/**
 * Shared test support utilities for conformance fixture generators.
 *
 * <p>This class provides helper methods to reduce duplication across generator tests:
 * - Transaction creation with deterministic timestamps
 * - Block context creation
 * - Account/Witness/Asset seeding
 * - Common dynamic property initialization
 */
public final class ConformanceFixtureTestSupport {

  // Default fixed timestamps for deterministic fixture generation
  public static final long DEFAULT_BLOCK_TIMESTAMP = 1700000000000L; // 2023-11-14 22:13:20 UTC
  public static final long DEFAULT_TX_TIMESTAMP = 1700000000000L;
  public static final long DEFAULT_TX_EXPIRATION = DEFAULT_TX_TIMESTAMP + 3600000L; // +1 hour
  public static final long DEFAULT_BLOCK_INTERVAL_MS = 3000L;
  public static final long DEFAULT_TX_FEE_LIMIT = 1_000_000_000L; // 1000 TRX
  private static final ByteString ZERO_BLOCK_HASH = ByteString.copyFrom(new byte[32]);

  // Common balance values
  public static final long ONE_TRX = 1_000_000L;
  public static final long HUNDRED_TRX = 100_000_000L;
  public static final long THOUSAND_TRX = 1_000_000_000L;
  public static final long INITIAL_BALANCE = 300_000_000_000L; // 300K TRX

  private ConformanceFixtureTestSupport() {
    // Utility class
  }

  /**
   * Create a TransactionCapsule with deterministic timestamps.
   *
   * @param type Contract type
   * @param contract Protobuf contract message
   * @return TransactionCapsule ready for fixture generation
   */
  public static TransactionCapsule createTransaction(
      Transaction.Contract.ContractType type,
      Message contract) {
    return createTransaction(type, contract, DEFAULT_TX_TIMESTAMP, DEFAULT_TX_EXPIRATION);
  }

  /**
   * Create a TransactionCapsule with specified timestamps.
   *
   * @param type Contract type
   * @param contract Protobuf contract message
   * @param timestampMs Transaction timestamp in milliseconds
   * @param expirationMs Transaction expiration in milliseconds
   * @return TransactionCapsule ready for fixture generation
   */
  public static TransactionCapsule createTransaction(
      Transaction.Contract.ContractType type,
      Message contract,
      long timestampMs,
      long expirationMs) {
    return createTransaction(type, contract, timestampMs, expirationMs, DEFAULT_TX_FEE_LIMIT);
  }

  public static TransactionCapsule createTransaction(
      Transaction.Contract.ContractType type,
      Message contract,
      long timestampMs,
      long expirationMs,
      long feeLimit) {

    long refBlockNum = Math.max(0, timestampMs / 1000);
    byte[] refBlockNumBytes = ByteArray.fromLong(refBlockNum);
    ByteString refBlockBytes = ByteString.copyFrom(ByteArray.subArray(refBlockNumBytes, 6, 8));
    ByteString refBlockHash = ByteString.copyFrom(ByteArray.fromLong(timestampMs));

    Transaction.Contract protoContract = Transaction.Contract.newBuilder()
        .setType(type)
        .setParameter(Any.pack(contract))
        .build();

    Transaction transaction = Transaction.newBuilder()
        .setRawData(Transaction.raw.newBuilder()
            .addContract(protoContract)
            .setFeeLimit(feeLimit)
            .setRefBlockNum(refBlockNum)
            .setRefBlockBytes(refBlockBytes)
            .setRefBlockHash(refBlockHash)
            .setTimestamp(timestampMs)
            .setExpiration(expirationMs)
            .build())
        .build();

    return new TransactionCapsule(transaction);
  }

  /**
   * Create a BlockCapsule for fixture generation.
   *
   * @param blockNumber Block number
   * @param blockTimestamp Block timestamp in milliseconds
   * @param witnessHexAddress Witness address in hex format (with or without 0x41 prefix)
   * @return BlockCapsule ready for fixture generation
   */
  public static BlockCapsule createBlockContext(
      long blockNumber,
      long blockTimestamp,
      String witnessHexAddress) {
    return createBlockContext(blockNumber, blockTimestamp, witnessHexAddress, ZERO_BLOCK_HASH);
  }

  public static BlockCapsule createBlockContext(
      long blockNumber,
      long blockTimestamp,
      String witnessHexAddress,
      ByteString parentHash) {

    Protocol.BlockHeader.raw rawHeader = Protocol.BlockHeader.raw.newBuilder()
        .setNumber(blockNumber)
        .setParentHash(parentHash)
        .setTimestamp(blockTimestamp)
        .setWitnessAddress(ByteString.copyFrom(ByteArray.fromHexString(witnessHexAddress)))
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
   * Create a BlockCapsule using dynamic properties from the manager.
   *
   * @param dbManager Database manager
   * @param witnessHexAddress Witness address in hex format
   * @return BlockCapsule ready for fixture generation
   */
  public static BlockCapsule createBlockContext(Manager dbManager, String witnessHexAddress) {
    DynamicPropertiesStore dynamicStore = dbManager.getDynamicPropertiesStore();
    long blockNum = dynamicStore.getLatestBlockHeaderNumber() + 1;
    long blockTime = dynamicStore.getLatestBlockHeaderTimestamp() + DEFAULT_BLOCK_INTERVAL_MS;
    ByteString parentHash;
    try {
      parentHash = dynamicStore.getLatestBlockHeaderHash().getByteString();
    } catch (IllegalArgumentException e) {
      parentHash = ZERO_BLOCK_HASH;
    }
    BlockCapsule blockCap = createBlockContext(blockNum, blockTime, witnessHexAddress, parentHash);

    // Embedded actuators read time/height/hash from dynamic props; keep them aligned with the
    // "next block" context that the remote backend executes against.
    dynamicStore.saveLatestBlockHeaderNumber(blockNum);
    dynamicStore.saveLatestBlockHeaderTimestamp(blockTime);
    dynamicStore.saveLatestBlockHeaderHash(blockCap.getBlockId().getByteString());

    return blockCap;
  }

  /**
   * Create and store an account.
   *
   * @param dbManager Database manager
   * @param hexAddress Account address in hex format
   * @param balanceSun Account balance in SUN
   * @return The created AccountCapsule
   */
  public static AccountCapsule putAccount(
      Manager dbManager,
      String hexAddress,
      long balanceSun) {
    return putAccount(dbManager, hexAddress, balanceSun, "account");
  }

  /**
   * Create and store an account with a custom name.
   *
   * @param dbManager Database manager
   * @param hexAddress Account address in hex format
   * @param balanceSun Account balance in SUN
   * @param accountName Account name
   * @return The created AccountCapsule
   */
  public static AccountCapsule putAccount(
      Manager dbManager,
      String hexAddress,
      long balanceSun,
      String accountName) {

    AccountCapsule account = new AccountCapsule(
        ByteString.copyFromUtf8(accountName),
        ByteString.copyFrom(ByteArray.fromHexString(hexAddress)),
        AccountType.Normal,
        balanceSun);

    dbManager.getAccountStore().put(account.getAddress().toByteArray(), account);
    return account;
  }

  /**
   * Create and store a witness.
   *
   * @param dbManager Database manager
   * @param hexAddress Witness address in hex format
   * @param url Witness URL
   * @param voteCount Initial vote count
   * @return The created WitnessCapsule
   */
  public static WitnessCapsule putWitness(
      Manager dbManager,
      String hexAddress,
      String url,
      long voteCount) {

    WitnessCapsule witness = new WitnessCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(hexAddress)),
        voteCount,
        url);

    dbManager.getWitnessStore().put(witness.getAddress().toByteArray(), witness);
    return witness;
  }

  /**
   * Create and store a TRC-10 asset (V2 format, allowSameTokenName=1).
   *
   * @param dbManager Database manager
   * @param tokenId Token ID as string (e.g., "1000001")
   * @param ownerHexAddress Asset owner address in hex format
   * @param name Asset name
   * @param totalSupply Total supply
   * @return The created AssetIssueCapsule
   */
  public static AssetIssueCapsule putAssetIssueV2(
      Manager dbManager,
      String tokenId,
      String ownerHexAddress,
      String name,
      long totalSupply) {

    long nowMs;
    try {
      nowMs = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    } catch (IllegalArgumentException e) {
      nowMs = DEFAULT_BLOCK_TIMESTAMP;
    }
    long startTime = nowMs - DEFAULT_BLOCK_INTERVAL_MS;
    long endTime = nowMs + 86400000L * 365;
    return putAssetIssueV2(
        dbManager, tokenId, ownerHexAddress, name, totalSupply, startTime, endTime);
  }

  public static AssetIssueCapsule putAssetIssueV2(
      Manager dbManager,
      String tokenId,
      String ownerHexAddress,
      String name,
      long totalSupply,
      long startTime,
      long endTime) {

    String abbr = name.length() <= 5 ? name : name.substring(0, 5);

    AssetIssueContract assetIssue = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(ownerHexAddress)))
        .setName(ByteString.copyFromUtf8(name))
        .setAbbr(ByteString.copyFromUtf8(abbr))
        .setId(tokenId)
        .setTotalSupply(totalSupply)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Seeded TRC-10 asset for conformance fixtures"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .build();

    AssetIssueCapsule assetCapsule = new AssetIssueCapsule(assetIssue);
    dbManager.getAssetIssueV2Store().put(assetCapsule.createDbV2Key(), assetCapsule);
    return assetCapsule;
  }

  /**
   * Initialize common dynamic properties for fixture generation (baseline V1).
   * Sets properties that most actuators read, avoiding "not found KEY" exceptions.
   *
   * @param dbManager Database manager
   * @param headBlockNum Latest block header number
   * @param headBlockTime Latest block header timestamp in milliseconds
   */
  public static void initCommonDynamicPropsV1(
      Manager dbManager,
      long headBlockNum,
      long headBlockTime) {

    DynamicPropertiesStore dynamicStore = dbManager.getDynamicPropertiesStore();

    // Block header properties
    dynamicStore.saveLatestBlockHeaderNumber(headBlockNum);
    dynamicStore.saveLatestBlockHeaderTimestamp(headBlockTime);
    BlockCapsule headBlockCap =
        createBlockContext(headBlockNum, headBlockTime, generateAddress(0L), ZERO_BLOCK_HASH);
    dynamicStore.saveLatestBlockHeaderHash(headBlockCap.getBlockId().getByteString());

    // Behavior-affecting feature flags should be explicit so fixtures don't drift with defaults.
    dynamicStore.saveForbidTransferToContract(0);
    dynamicStore.saveAllowTvmCompatibleEvm(0);
    dynamicStore.saveAllowUpdateAccountName(0);
    dynamicStore.saveAllowSameTokenName(0);
    dynamicStore.saveAllowTvmTransferTrc10(0);
    dynamicStore.saveAllowTvmConstantinople(0);
    dynamicStore.saveAllowTvmSolidity059(0);
    dynamicStore.saveAllowTvmIstanbul(0);
    dynamicStore.saveAllowTvmLondon(0);
    dynamicStore.saveAllowTvmFreeze(0);
    dynamicStore.saveAllowTvmVote(0);
    dynamicStore.saveAllowTvmShangHai(0);
    dynamicStore.saveAllowTvmCancun(0);
    dynamicStore.saveAllowTvmBlob(0);

    // Account creation fees
    dynamicStore.saveCreateNewAccountFeeInSystemContract(ONE_TRX);
    dynamicStore.saveCreateAccountFee(ONE_TRX);

    // Multi-sig settings
    dynamicStore.saveAllowMultiSign(0);

    // Blackhole optimization (0 = credit blackhole, 1 = burn)
    dynamicStore.saveAllowBlackHoleOptimization(0);

    // V1 freeze settings (V2 disabled)
    dynamicStore.saveUnfreezeDelayDays(0);

    // Delegation settings
    dynamicStore.saveChangeDelegation(0);

    // Total weights (for freeze/resource calculations)
    dynamicStore.saveTotalNetWeight(0);
    dynamicStore.saveTotalEnergyWeight(0);

    // New reward model disabled
    dynamicStore.saveAllowNewReward(0);
    dynamicStore.saveAllowNewResourceModel(0);
  }

  /**
   * Initialize common dynamic properties for V2 freeze fixtures.
   *
   * @param dbManager Database manager
   * @param headBlockNum Latest block header number
   * @param headBlockTime Latest block header timestamp in milliseconds
   * @param unfreezeDelayDays Unfreeze delay in days (must be > 0 for V2)
   */
  public static void initCommonDynamicPropsV2(
      Manager dbManager,
      long headBlockNum,
      long headBlockTime,
      int unfreezeDelayDays) {

    // Start with V1 baseline
    initCommonDynamicPropsV1(dbManager, headBlockNum, headBlockTime);

    DynamicPropertiesStore dynamicStore = dbManager.getDynamicPropertiesStore();

    // Enable V2 freeze
    dynamicStore.saveUnfreezeDelayDays(unfreezeDelayDays);

    // Enable new resource model (for TRON_POWER)
    dynamicStore.saveAllowNewResourceModel(1);

    // Total TRON power weight
    dynamicStore.saveTotalTronPowerWeight(0);
  }

  /**
   * Initialize dynamic properties for TRC-10 (allowSameTokenName=1) fixtures.
   *
   * @param dbManager Database manager
   * @param headBlockNum Latest block header number
   * @param headBlockTime Latest block header timestamp in milliseconds
   */
  public static void initTrc10DynamicProps(
      Manager dbManager,
      long headBlockNum,
      long headBlockTime) {

    // Start with V1 baseline
    initCommonDynamicPropsV1(dbManager, headBlockNum, headBlockTime);

    DynamicPropertiesStore dynamicStore = dbManager.getDynamicPropertiesStore();

    // TRC-10 V2 (id-based) mode
    dynamicStore.saveAllowSameTokenName(1);

    // Asset issuance settings
    dynamicStore.saveAssetIssueFee(1024 * ONE_TRX); // 1024 TRX
    dynamicStore.saveTokenIdNum(1000000); // Starting token ID
    dynamicStore.saveMaxFrozenSupplyNumber(10);
    dynamicStore.saveOneDayNetLimit(300_000_000L);
    dynamicStore.saveMinFrozenSupplyTime(1);
    dynamicStore.saveMaxFrozenSupplyTime(3652);
  }

  /**
   * Initialize dynamic properties for witness/voting fixtures.
   *
   * @param dbManager Database manager
   * @param headBlockNum Latest block header number
   * @param headBlockTime Latest block header timestamp in milliseconds
   */
  public static void initWitnessDynamicProps(
      Manager dbManager,
      long headBlockNum,
      long headBlockTime) {

    // Start with V1 baseline
    initCommonDynamicPropsV1(dbManager, headBlockNum, headBlockTime);

    DynamicPropertiesStore dynamicStore = dbManager.getDynamicPropertiesStore();

    // Witness creation settings
    dynamicStore.saveAccountUpgradeCost(9999 * ONE_TRX); // 9999 TRX
    dynamicStore.saveTotalCreateWitnessFee(0);

    // Witness allowance settings
    dynamicStore.saveWitnessAllowanceFrozenTime(1); // 1 day cooldown
  }

  /**
   * Generate a deterministic hex address based on a seed.
   *
   * @param seed A seed value (will be converted to hex)
   * @return Hex address with proper prefix
   */
  public static String generateAddress(long seed) {
    // Use hex format and pad to 40 chars
    String seedHex = String.format("%040x", seed);
    return Wallet.getAddressPreFixString() + seedHex;
  }

  /**
   * Generate a deterministic hex address based on a string prefix.
   *
   * @param prefix A string prefix (will be hashed to generate valid hex)
   * @return Hex address with proper prefix
   */
  public static String generateAddress(String prefix) {
    // Convert the prefix string to its hex representation and pad/truncate to 40 chars
    StringBuilder hexBuilder = new StringBuilder();
    for (char c : prefix.toCharArray()) {
      hexBuilder.append(String.format("%02x", (int) c));
    }
    String hexStr = hexBuilder.toString();
    // Pad with zeros or truncate to 40 chars
    if (hexStr.length() < 40) {
      hexStr = hexStr + String.format("%0" + (40 - hexStr.length()) + "d", 0);
    } else if (hexStr.length() > 40) {
      hexStr = hexStr.substring(0, 40);
    }
    return Wallet.getAddressPreFixString() + hexStr;
  }
}
