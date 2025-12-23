package org.tron.core.conformance;

import static org.tron.core.conformance.ConformanceFixtureTestSupport.*;

import com.google.protobuf.ByteString;
import java.io.File;
import java.util.ArrayList;
import java.util.List;
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
import org.tron.protos.Protocol.Account.Frozen;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.BalanceContract.WithdrawBalanceContract;
import org.tron.protos.contract.WitnessContract.VoteWitnessContract;
import org.tron.protos.contract.WitnessContract.VoteWitnessContract.Vote;
import org.tron.protos.contract.WitnessContract.WitnessCreateContract;
import org.tron.protos.contract.WitnessContract.WitnessUpdateContract;

/**
 * Generates conformance test fixtures for witness and voting contracts:
 * - VoteWitnessContract (4)
 * - WitnessCreateContract (5)
 * - WitnessUpdateContract (8)
 * - WithdrawBalanceContract (13)
 *
 * <p>Run with: ./gradlew :framework:test --tests "WitnessVotingFixtureGeneratorTest"
 * -Dconformance.output=../conformance/fixtures --dependency-verification=off
 */
public class WitnessVotingFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(WitnessVotingFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String WITNESS_ADDRESS;
  private static final String CANDIDATE_ADDRESS;
  private static final long WITNESS_UPGRADE_COST = 9999 * ONE_TRX; // 9999 TRX

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049152";
    WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "548794500882809695a8a687866e76d4271a1abc";
    CANDIDATE_ADDRESS = Wallet.getAddressPreFixString() + "2222222222222222222222222222222222222222";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("WitnessVoting Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Initialize dynamic properties for witness/voting
    initWitnessDynamicProps(dbManager,
        DEFAULT_BLOCK_TIMESTAMP / 1000,
        DEFAULT_BLOCK_TIMESTAMP);

    // Disable delegation to simplify fixtures
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(0);

    // Create owner account with sufficient balance
    putAccount(dbManager, OWNER_ADDRESS, INITIAL_BALANCE, "owner");

    // Create and store existing witness
    putAccount(dbManager, WITNESS_ADDRESS, INITIAL_BALANCE, "witness");
    putWitness(dbManager, WITNESS_ADDRESS, "https://witness.network", 10_000_000L);

    // Create candidate account and witness for voting
    putAccount(dbManager, CANDIDATE_ADDRESS, INITIAL_BALANCE, "candidate");
    putWitness(dbManager, CANDIDATE_ADDRESS, "https://candidate.network", 0);
  }

  // ==========================================================================
  // VoteWitnessContract (4) Fixtures
  // ==========================================================================

  @Test
  public void generateVoteWitness_happyPathSingleVote() throws Exception {
    String voterAddress = generateAddress("voter_single_001");

    // Create voter account with TRON power (frozen balance)
    AccountCapsule voterAccount = putAccount(dbManager, voterAddress, INITIAL_BALANCE, "voter");
    // Add frozen balance for TRON power
    Protocol.Account.Builder builder = voterAccount.getInstance().toBuilder();
    builder.setFrozen(0, Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX) // 100 TRX frozen = 100 TRON power
        .setExpireTime(DEFAULT_BLOCK_TIMESTAMP + 86400000 * 3) // 3 days
        .build());
    voterAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(voterAccount.getAddress().toByteArray(), voterAccount);

    Vote vote = Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(CANDIDATE_ADDRESS)))
        .setVoteCount(1) // 1 TRX = 1 vote
        .build();

    VoteWitnessContract contract = VoteWitnessContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(voterAddress)))
        .addVotes(vote)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.VoteWitnessContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("VOTE_WITNESS_CONTRACT", 4)
        .caseName("happy_path_single_vote")
        .caseCategory("happy")
        .description("Vote for a single witness with sufficient TRON power")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(voterAddress)
        .dynamicProperty("CHANGE_DELEGATION", 0)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness single vote: success={}", result.isSuccess());
  }

  @Test
  public void generateVoteWitness_validateFailVoteCountZero() throws Exception {
    Vote vote = Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(CANDIDATE_ADDRESS)))
        .setVoteCount(0) // Zero vote count
        .build();

    VoteWitnessContract contract = VoteWitnessContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .addVotes(vote)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.VoteWitnessContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("VOTE_WITNESS_CONTRACT", 4)
        .caseName("validate_fail_vote_count_zero")
        .caseCategory("validate_fail")
        .description("Fail when vote count is zero")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("vote")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness zero count: validationError={}", result.getValidationError());
  }

  @Test
  public void generateVoteWitness_validateFailCandidateNotWitness() throws Exception {
    String nonWitnessAddress = generateAddress("non_witness_001");
    // Create account but not witness
    putAccount(dbManager, nonWitnessAddress, INITIAL_BALANCE, "non_witness");

    Vote vote = Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(nonWitnessAddress)))
        .setVoteCount(1)
        .build();

    VoteWitnessContract contract = VoteWitnessContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .addVotes(vote)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.VoteWitnessContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("VOTE_WITNESS_CONTRACT", 4)
        .caseName("validate_fail_candidate_not_witness")
        .caseCategory("validate_fail")
        .description("Fail when vote target is not a witness")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("witness")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness not witness: validationError={}", result.getValidationError());
  }

  @Test
  public void generateVoteWitness_validateFailVotesExceedTronPower() throws Exception {
    String smallPowerVoter = generateAddress("small_power_001");

    // Create account with small TRON power
    AccountCapsule voterAccount = putAccount(dbManager, smallPowerVoter, INITIAL_BALANCE, "small_power");
    Protocol.Account.Builder builder = voterAccount.getInstance().toBuilder();
    builder.setFrozen(0, Frozen.newBuilder()
        .setFrozenBalance(ONE_TRX) // 1 TRX frozen = 1 TRON power
        .setExpireTime(DEFAULT_BLOCK_TIMESTAMP + 86400000 * 3)
        .build());
    voterAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(voterAccount.getAddress().toByteArray(), voterAccount);

    Vote vote = Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(CANDIDATE_ADDRESS)))
        .setVoteCount(100) // 100 votes but only 1 TRON power
        .build();

    VoteWitnessContract contract = VoteWitnessContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(smallPowerVoter)))
        .addVotes(vote)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.VoteWitnessContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("VOTE_WITNESS_CONTRACT", 4)
        .caseName("validate_fail_votes_exceed_tron_power")
        .caseCategory("validate_fail")
        .description("Fail when total votes exceed TRON power")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(smallPowerVoter)
        .expectedError("power")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness exceed power: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // WitnessCreateContract (5) Fixtures
  // ==========================================================================

  @Test
  public void generateWitnessCreate_happyPath() throws Exception {
    String newWitnessOwner = generateAddress("new_witness_own1");
    putAccount(dbManager, newWitnessOwner, INITIAL_BALANCE, "new_witness_owner");

    WitnessCreateContract contract = WitnessCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(newWitnessOwner)))
        .setUrl(ByteString.copyFromUtf8("https://my-witness.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_CREATE_CONTRACT", 5)
        .caseName("happy_path_create_witness")
        .caseCategory("happy")
        .description("Create a new witness with valid URL and sufficient balance")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(newWitnessOwner)
        .dynamicProperty("ACCOUNT_UPGRADE_COST", WITNESS_UPGRADE_COST)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessCreate happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateWitnessCreate_validateFailInvalidUrl() throws Exception {
    String newWitnessOwner = generateAddress("new_witness_own2");
    putAccount(dbManager, newWitnessOwner, INITIAL_BALANCE, "new_witness_owner2");

    WitnessCreateContract contract = WitnessCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(newWitnessOwner)))
        .setUrl(ByteString.EMPTY) // Empty URL
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_CREATE_CONTRACT", 5)
        .caseName("validate_fail_invalid_url")
        .caseCategory("validate_fail")
        .description("Fail when URL is empty or invalid")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(newWitnessOwner)
        .expectedError("url")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessCreate invalid url: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWitnessCreate_validateFailWitnessExists() throws Exception {
    // Use existing witness address
    WitnessCreateContract contract = WitnessCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setUrl(ByteString.copyFromUtf8("https://another-witness.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_CREATE_CONTRACT", 5)
        .caseName("validate_fail_witness_exists")
        .caseCategory("validate_fail")
        .description("Fail when witness already exists for the account")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("witness")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessCreate exists: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWitnessCreate_validateFailInsufficientBalance() throws Exception {
    String poorOwner = generateAddress("poor_witness_own");
    putAccount(dbManager, poorOwner, ONE_TRX, "poor_witness_owner"); // Only 1 TRX

    WitnessCreateContract contract = WitnessCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(poorOwner)))
        .setUrl(ByteString.copyFromUtf8("https://poor-witness.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_CREATE_CONTRACT", 5)
        .caseName("validate_fail_insufficient_balance")
        .caseCategory("validate_fail")
        .description("Fail when owner has insufficient balance for upgrade cost")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(poorOwner)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessCreate insufficient: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // WitnessUpdateContract (8) Fixtures
  // ==========================================================================

  @Test
  public void generateWitnessUpdate_happyPathUpdateUrl() throws Exception {
    WitnessUpdateContract contract = WitnessUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setUpdateUrl(ByteString.copyFromUtf8("https://updated-witness.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_UPDATE_CONTRACT", 8)
        .caseName("happy_path_update_url")
        .caseCategory("happy")
        .description("Update witness URL to a new valid URL")
        .database("witness")
        .database("account")
        .ownerAddress(WITNESS_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessUpdate happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateWitnessUpdate_validateFailNotWitness() throws Exception {
    // Use owner address which is not a witness
    WitnessUpdateContract contract = WitnessUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setUpdateUrl(ByteString.copyFromUtf8("https://not-witness.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_UPDATE_CONTRACT", 8)
        .caseName("validate_fail_witness_missing")
        .caseCategory("validate_fail")
        .description("Fail when account exists but witness entry is missing")
        .database("witness")
        .database("account")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("witness")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessUpdate not witness: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWitnessUpdate_validateFailInvalidUrl() throws Exception {
    WitnessUpdateContract contract = WitnessUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setUpdateUrl(ByteString.EMPTY) // Empty URL
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_UPDATE_CONTRACT", 8)
        .caseName("validate_fail_invalid_url")
        .caseCategory("validate_fail")
        .description("Fail when update URL is empty or invalid")
        .database("witness")
        .database("account")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("url")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessUpdate invalid url: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // WithdrawBalanceContract (13) Fixtures
  // ==========================================================================

  @Test
  public void generateWithdrawBalance_happyPathWithdrawAllowance() throws Exception {
    String witnessWithAllowance = generateAddress("witness_allow_01");

    // Create witness account with allowance
    AccountCapsule witnessAccount = putAccount(dbManager, witnessWithAllowance, INITIAL_BALANCE, "witness_allowance");
    Protocol.Account.Builder builder = witnessAccount.getInstance().toBuilder();
    builder.setAllowance(10 * ONE_TRX); // 10 TRX allowance
    builder.setLatestWithdrawTime(0); // Never withdrawn before
    witnessAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(witnessAccount.getAddress().toByteArray(), witnessAccount);

    // Create witness entry
    putWitness(dbManager, witnessWithAllowance, "https://allowance-witness.network", 100);

    // Ensure sufficient cooldown has passed
    // witnessAllowanceFrozenTime = 1 day, so latestBlockHeaderTimestamp must be >= 86400000
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(DEFAULT_BLOCK_TIMESTAMP);

    WithdrawBalanceContract contract = WithdrawBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(witnessWithAllowance)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_BALANCE_CONTRACT", 13)
        .caseName("happy_path_withdraw_allowance")
        .caseCategory("happy")
        .description("Withdraw witness allowance after cooldown period")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(witnessWithAllowance)
        .dynamicProperty("CHANGE_DELEGATION", 0)
        .dynamicProperty("WITNESS_ALLOWANCE_FROZEN_TIME", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawBalance happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateWithdrawBalance_validateFailTooSoon() throws Exception {
    String witnessRecentWithdraw = generateAddress("witness_recent_01");

    // Create witness account with recent withdrawal
    AccountCapsule witnessAccount = putAccount(dbManager, witnessRecentWithdraw, INITIAL_BALANCE, "recent_withdraw");
    Protocol.Account.Builder builder = witnessAccount.getInstance().toBuilder();
    builder.setAllowance(10 * ONE_TRX);
    builder.setLatestWithdrawTime(DEFAULT_BLOCK_TIMESTAMP - 1000); // Just withdrew
    witnessAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(witnessAccount.getAddress().toByteArray(), witnessAccount);

    putWitness(dbManager, witnessRecentWithdraw, "https://recent-witness.network", 100);

    WithdrawBalanceContract contract = WithdrawBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(witnessRecentWithdraw)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_BALANCE_CONTRACT", 13)
        .caseName("validate_fail_too_soon")
        .caseCategory("validate_fail")
        .description("Fail when withdrawing before cooldown period ends")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(witnessRecentWithdraw)
        .expectedError("time")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawBalance too soon: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWithdrawBalance_validateFailNoReward() throws Exception {
    String witnessNoReward = generateAddress("witness_no_rew01");

    // Create witness account with zero allowance
    AccountCapsule witnessAccount = putAccount(dbManager, witnessNoReward, INITIAL_BALANCE, "no_reward");
    Protocol.Account.Builder builder = witnessAccount.getInstance().toBuilder();
    builder.setAllowance(0); // No allowance
    builder.setLatestWithdrawTime(0);
    witnessAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(witnessAccount.getAddress().toByteArray(), witnessAccount);

    putWitness(dbManager, witnessNoReward, "https://no-reward-witness.network", 100);

    WithdrawBalanceContract contract = WithdrawBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(witnessNoReward)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_BALANCE_CONTRACT", 13)
        .caseName("validate_fail_no_reward")
        .caseCategory("validate_fail")
        .description("Fail when there is no allowance/reward to withdraw")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(witnessNoReward)
        .expectedError("reward")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawBalance no reward: validationError={}", result.getValidationError());
  }
}
