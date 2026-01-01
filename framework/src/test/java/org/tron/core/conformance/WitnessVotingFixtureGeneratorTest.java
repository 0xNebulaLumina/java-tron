package org.tron.core.conformance;

import static org.tron.core.conformance.ConformanceFixtureTestSupport.*;
import static org.tron.core.config.Parameter.ChainConstant.FROZEN_PERIOD;

import com.google.protobuf.ByteString;
import java.io.File;
import java.util.ArrayList;
import java.util.List;
import org.junit.Before;
import org.junit.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.BaseTest;
import org.tron.common.parameter.CommonParameter;
import org.tron.common.utils.ByteArray;
import org.tron.core.Constant;
import org.tron.core.Wallet;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.capsule.VotesCapsule;
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
    builder.addFrozen(Frozen.newBuilder()
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
    builder.addFrozen(Frozen.newBuilder()
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

  // --------------------------------------------------------------------------
  // VoteWitnessContract (4) — Missing edge cases from planning
  // --------------------------------------------------------------------------

  @Test
  public void generateVoteWitness_validateFailOwnerAddressInvalidEmpty() throws Exception {
    Vote vote = Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(CANDIDATE_ADDRESS)))
        .setVoteCount(1)
        .build();

    VoteWitnessContract contract = VoteWitnessContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Empty owner address
        .addVotes(vote)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.VoteWitnessContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("VOTE_WITNESS_CONTRACT", 4)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty (invalid address)")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness empty owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateVoteWitness_validateFailOwnerAccountNotExist() throws Exception {
    String nonExistentOwner = generateAddress("nonexist_voter001");
    // Do NOT create account - it should not exist

    Vote vote = Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(CANDIDATE_ADDRESS)))
        .setVoteCount(1)
        .build();

    VoteWitnessContract contract = VoteWitnessContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentOwner)))
        .addVotes(vote)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.VoteWitnessContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("VOTE_WITNESS_CONTRACT", 4)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(nonExistentOwner)
        .expectedError("does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness owner not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateVoteWitness_validateFailVotesListEmpty() throws Exception {
    VoteWitnessContract contract = VoteWitnessContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        // No .addVotes(...) - empty votes list
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.VoteWitnessContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("VOTE_WITNESS_CONTRACT", 4)
        .caseName("validate_fail_votes_list_empty")
        .caseCategory("validate_fail")
        .description("Fail when votes list is empty (VoteNumber must more than 0)")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("VoteNumber must more than 0")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness empty votes: validationError={}", result.getValidationError());
  }

  @Test
  public void generateVoteWitness_validateFailVotesCountOverMax30() throws Exception {
    String voter31 = generateAddress("voter_31_entries");
    AccountCapsule voterAccount = putAccount(dbManager, voter31, INITIAL_BALANCE, "voter31");
    // Add enough frozen balance for 31 votes
    Protocol.Account.Builder builder = voterAccount.getInstance().toBuilder();
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX)
        .setExpireTime(DEFAULT_BLOCK_TIMESTAMP + 86400000 * 3)
        .build());
    voterAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(voterAccount.getAddress().toByteArray(), voterAccount);

    // Create 31 witnesses to vote for (exceeds MAX_VOTE_NUMBER = 30)
    VoteWitnessContract.Builder contractBuilder = VoteWitnessContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(voter31)));

    for (int i = 0; i < 31; i++) {
      String witnessAddr = generateAddress("witness_31_" + String.format("%02d", i));
      putAccount(dbManager, witnessAddr, INITIAL_BALANCE, "witness" + i);
      putWitness(dbManager, witnessAddr, "https://witness" + i + ".network", 0);

      contractBuilder.addVotes(Vote.newBuilder()
          .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(witnessAddr)))
          .setVoteCount(1)
          .build());
    }

    VoteWitnessContract contract = contractBuilder.build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.VoteWitnessContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("VOTE_WITNESS_CONTRACT", 4)
        .caseName("validate_fail_votes_count_over_max_30")
        .caseCategory("validate_fail")
        .description("Fail when votes count exceeds MAX_VOTE_NUMBER (30)")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(voter31)
        .expectedError("VoteNumber more than maxVoteNumber 30")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness over max 30: validationError={}", result.getValidationError());
  }

  @Test
  public void generateVoteWitness_validateFailVoteAddressInvalid() throws Exception {
    // Invalid vote address: wrong length (10 bytes instead of 21)
    byte[] invalidVoteAddr = new byte[10];
    for (int i = 0; i < 10; i++) {
      invalidVoteAddr[i] = (byte) (0xAB + i);
    }

    Vote vote = Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(invalidVoteAddr))
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
        .caseName("validate_fail_vote_address_invalid")
        .caseCategory("validate_fail")
        .description("Fail when vote address is invalid (wrong length bytes)")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid vote address!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness invalid vote addr: validationError={}", result.getValidationError());
  }

  @Test
  public void generateVoteWitness_validateFailVoteTargetAccountNotExist() throws Exception {
    // Valid-looking address that doesn't exist in AccountStore
    String nonExistentTarget = generateAddress("target_noexist01");
    // Do NOT create account

    Vote vote = Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentTarget)))
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
        .caseName("validate_fail_vote_target_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when vote target account does not exist in AccountStore")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness target not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateVoteWitness_edgeTronPowerExactMatch() throws Exception {
    String exactPowerVoter = generateAddress("exact_power_vtr1");

    // Create voter account with exactly 100 TRX frozen = 100 TRON power
    AccountCapsule voterAccount = putAccount(dbManager, exactPowerVoter, INITIAL_BALANCE, "exact_power");
    Protocol.Account.Builder builder = voterAccount.getInstance().toBuilder();
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX) // 100 TRX frozen = 100 TRON power (votes)
        .setExpireTime(DEFAULT_BLOCK_TIMESTAMP + 86400000 * 3)
        .build());
    voterAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(voterAccount.getAddress().toByteArray(), voterAccount);

    // Vote for exactly 100 votes (should pass: sum * TRX_PRECISION == tronPower)
    Vote vote = Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(CANDIDATE_ADDRESS)))
        .setVoteCount(100) // Exactly matches TRON power
        .build();

    VoteWitnessContract contract = VoteWitnessContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(exactPowerVoter)))
        .addVotes(vote)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.VoteWitnessContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("VOTE_WITNESS_CONTRACT", 4)
        .caseName("edge_tron_power_exact_match")
        .caseCategory("edge")
        .description("Edge case: votes exactly equal TRON power (boundary test for >= vs >)")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(exactPowerVoter)
        .dynamicProperty("CHANGE_DELEGATION", 0)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness exact power match: success={}", result.isSuccess());
  }

  @Test
  public void generateVoteWitness_edgeRevotingReplacesPreviousVotes() throws Exception {
    String revoteVoter = generateAddress("revote_voter_001");

    // Create voter account with TRON power
    AccountCapsule voterAccount = putAccount(dbManager, revoteVoter, INITIAL_BALANCE, "revote_voter");
    Protocol.Account.Builder builder = voterAccount.getInstance().toBuilder();
    builder.addFrozen(Frozen.newBuilder()
        .setFrozenBalance(100 * ONE_TRX)
        .setExpireTime(DEFAULT_BLOCK_TIMESTAMP + 86400000 * 3)
        .build());
    // Pre-seed with existing votes for CANDIDATE_ADDRESS
    builder.addVotes(Protocol.Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(CANDIDATE_ADDRESS)))
        .setVoteCount(50)
        .build());
    voterAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(voterAccount.getAddress().toByteArray(), voterAccount);

    // Pre-seed VotesStore entry
    VotesCapsule votesCapsule = new VotesCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(revoteVoter)),
        voterAccount.getVotesList());
    votesCapsule.addNewVotes(
        ByteString.copyFrom(ByteArray.fromHexString(CANDIDATE_ADDRESS)), 50);
    dbManager.getVotesStore().put(
        ByteArray.fromHexString(revoteVoter), votesCapsule);

    // Create a second witness to vote for (different from original)
    String newWitness = generateAddress("new_witness_revt");
    putAccount(dbManager, newWitness, INITIAL_BALANCE, "new_witness");
    putWitness(dbManager, newWitness, "https://new-witness.network", 0);

    // Submit a new vote tx with a DIFFERENT witness (should replace, not merge)
    Vote vote = Vote.newBuilder()
        .setVoteAddress(ByteString.copyFrom(ByteArray.fromHexString(newWitness)))
        .setVoteCount(75)
        .build();

    VoteWitnessContract contract = VoteWitnessContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(revoteVoter)))
        .addVotes(vote)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.VoteWitnessContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("VOTE_WITNESS_CONTRACT", 4)
        .caseName("edge_revoting_replaces_previous_votes")
        .caseCategory("edge")
        .description("Edge case: revoting clears old votes and replaces with new (not merged)")
        .database("account")
        .database("votes")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(revoteVoter)
        .dynamicProperty("CHANGE_DELEGATION", 0)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("VoteWitness revote replace: success={}", result.isSuccess());
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

  // --------------------------------------------------------------------------
  // WitnessCreateContract (5) — Missing edge cases from planning
  // --------------------------------------------------------------------------

  @Test
  public void generateWitnessCreate_validateFailOwnerAddressInvalidEmpty() throws Exception {
    WitnessCreateContract contract = WitnessCreateContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Empty owner address
        .setUrl(ByteString.copyFromUtf8("https://my-witness.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_CREATE_CONTRACT", 5)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty (invalid address)")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessCreate empty owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWitnessCreate_validateFailOwnerAccountNotExist() throws Exception {
    String nonExistentOwner = generateAddress("nonexist_witcre1");
    // Do NOT create account

    WitnessCreateContract contract = WitnessCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentOwner)))
        .setUrl(ByteString.copyFromUtf8("https://no-account-witness.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_CREATE_CONTRACT", 5)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(nonExistentOwner)
        .expectedError("does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessCreate owner not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWitnessCreate_validateFailUrlTooLong257() throws Exception {
    String newWitnessOwner = generateAddress("witness_url_long");
    putAccount(dbManager, newWitnessOwner, INITIAL_BALANCE, "url_too_long");

    // Create a URL that is 257 bytes (exceeds MAX_URL_LEN = 256)
    StringBuilder longUrl = new StringBuilder("https://");
    for (int i = 0; i < 249; i++) { // 8 + 249 = 257 bytes
      longUrl.append('a');
    }

    WitnessCreateContract contract = WitnessCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(newWitnessOwner)))
        .setUrl(ByteString.copyFromUtf8(longUrl.toString()))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_CREATE_CONTRACT", 5)
        .caseName("validate_fail_url_too_long_257")
        .caseCategory("validate_fail")
        .description("Fail when URL length exceeds MAX_URL_LEN (256 bytes)")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(newWitnessOwner)
        .expectedError("Invalid url")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessCreate url too long: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWitnessCreate_edgeBalanceEqualsUpgradeCost() throws Exception {
    String exactBalanceOwner = generateAddress("exact_balance_wit");
    // Set balance to exactly WITNESS_UPGRADE_COST (9999 TRX)
    putAccount(dbManager, exactBalanceOwner, WITNESS_UPGRADE_COST, "exact_balance");

    WitnessCreateContract contract = WitnessCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(exactBalanceOwner)))
        .setUrl(ByteString.copyFromUtf8("https://exact-balance-witness.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_CREATE_CONTRACT", 5)
        .caseName("edge_balance_equals_upgrade_cost")
        .caseCategory("edge")
        .description("Edge case: balance exactly equals ACCOUNT_UPGRADE_COST (boundary check for < vs <=)")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(exactBalanceOwner)
        .dynamicProperty("ACCOUNT_UPGRADE_COST", WITNESS_UPGRADE_COST)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessCreate exact balance: success={}", result.isSuccess());
  }

  @Test
  public void generateWitnessCreate_edgeBlackholeOptimizationBurnsTrx() throws Exception {
    String burnWitnessOwner = generateAddress("burn_witness_own1");
    putAccount(dbManager, burnWitnessOwner, INITIAL_BALANCE, "burn_witness");

    // Enable blackhole optimization (1 = burn, 0 = credit blackhole)
    dbManager.getDynamicPropertiesStore().saveAllowBlackHoleOptimization(1);

    WitnessCreateContract contract = WitnessCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(burnWitnessOwner)))
        .setUrl(ByteString.copyFromUtf8("https://burn-witness.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessCreateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_CREATE_CONTRACT", 5)
        .caseName("edge_blackhole_optimization_burns_trx")
        .caseCategory("edge")
        .description("Edge case: with ALLOW_BLACK_HOLE_OPTIMIZATION=1, TRX is burned instead of credited to blackhole")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(burnWitnessOwner)
        .dynamicProperty("ACCOUNT_UPGRADE_COST", WITNESS_UPGRADE_COST)
        .dynamicProperty("ALLOW_BLACK_HOLE_OPTIMIZATION", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessCreate blackhole burn: success={}", result.isSuccess());

    // Reset blackhole optimization
    dbManager.getDynamicPropertiesStore().saveAllowBlackHoleOptimization(0);
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

  // --------------------------------------------------------------------------
  // WitnessUpdateContract (8) — Missing edge cases from planning
  // --------------------------------------------------------------------------

  @Test
  public void generateWitnessUpdate_validateFailOwnerAddressInvalidEmpty() throws Exception {
    WitnessUpdateContract contract = WitnessUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Empty owner address
        .setUpdateUrl(ByteString.copyFromUtf8("https://updated-witness.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_UPDATE_CONTRACT", 8)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty (invalid address)")
        .database("witness")
        .database("account")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessUpdate empty owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWitnessUpdate_validateFailOwnerAccountNotExist() throws Exception {
    String nonExistentOwner = generateAddress("nonexist_witupd1");
    // Do NOT create account

    WitnessUpdateContract contract = WitnessUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentOwner)))
        .setUpdateUrl(ByteString.copyFromUtf8("https://no-account-witness.network"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_UPDATE_CONTRACT", 8)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("witness")
        .database("account")
        .ownerAddress(nonExistentOwner)
        .expectedError("account does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessUpdate owner not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWitnessUpdate_validateFailUrlTooLong257() throws Exception {
    // Create a URL that is 257 bytes (exceeds MAX_URL_LEN = 256)
    StringBuilder longUrl = new StringBuilder("https://");
    for (int i = 0; i < 249; i++) { // 8 + 249 = 257 bytes
      longUrl.append('a');
    }

    WitnessUpdateContract contract = WitnessUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setUpdateUrl(ByteString.copyFromUtf8(longUrl.toString()))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WitnessUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITNESS_UPDATE_CONTRACT", 8)
        .caseName("validate_fail_url_too_long_257")
        .caseCategory("validate_fail")
        .description("Fail when update URL length exceeds MAX_URL_LEN (256 bytes)")
        .database("witness")
        .database("account")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("Invalid url")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WitnessUpdate url too long: validationError={}", result.getValidationError());
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

  // --------------------------------------------------------------------------
  // WithdrawBalanceContract (13) — Missing edge cases from planning
  // --------------------------------------------------------------------------

  @Test
  public void generateWithdrawBalance_validateFailOwnerAddressInvalidEmpty() throws Exception {
    WithdrawBalanceContract contract = WithdrawBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Empty owner address
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_BALANCE_CONTRACT", 13)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty (invalid address)")
        .database("account")
        .database("dynamic-properties")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawBalance empty owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWithdrawBalance_validateFailOwnerAccountNotExist() throws Exception {
    String nonExistentOwner = generateAddress("nonexist_withdr1");
    // Do NOT create account

    WithdrawBalanceContract contract = WithdrawBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentOwner)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_BALANCE_CONTRACT", 13)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist in AccountStore")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(nonExistentOwner)
        .expectedError("does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawBalance owner not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWithdrawBalance_validateFailGuardRepresentativeWithdraw() throws Exception {
    // Get a genesis witness address (guard representative)
    org.tron.core.config.args.Args.Witness genesisWitness =
        CommonParameter.getInstance().getGenesisBlock().getWitnesses().get(0);
    byte[] grAddress = genesisWitness.getAddress();
    String grHexAddress = ByteArray.toHexString(grAddress);

    // Create account for the genesis witness with allowance
    AccountCapsule grAccount = new AccountCapsule(
        ByteString.copyFromUtf8("guard_rep"),
        ByteString.copyFrom(grAddress),
        Protocol.AccountType.Normal,
        INITIAL_BALANCE);
    Protocol.Account.Builder builder = grAccount.getInstance().toBuilder();
    builder.setAllowance(10 * ONE_TRX);
    builder.setLatestWithdrawTime(0);
    grAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(grAccount.getAddress().toByteArray(), grAccount);

    WithdrawBalanceContract contract = WithdrawBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(grAddress))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_BALANCE_CONTRACT", 13)
        .caseName("validate_fail_guard_representative_withdraw")
        .caseCategory("validate_fail")
        .description("Fail when guard representative (genesis witness) tries to withdraw")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(grHexAddress)
        .expectedError("guard representative")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawBalance guard rep: validationError={}", result.getValidationError());
  }

  @Test
  public void generateWithdrawBalance_edgeWithdrawAtExactCooldownBoundary() throws Exception {
    String witnessBoundary = generateAddress("witness_boundary1");

    // witnessAllowanceFrozenTime = 1 day, FROZEN_PERIOD = 86400000 ms
    // Set latestWithdrawTime so that now - latestWithdrawTime == cooldown exactly
    long witnessAllowanceFrozenTime = 1; // days
    long cooldownMs = witnessAllowanceFrozenTime * FROZEN_PERIOD; // 86400000 ms
    long now = DEFAULT_BLOCK_TIMESTAMP;
    long latestWithdrawTime = now - cooldownMs; // Exactly at boundary

    AccountCapsule witnessAccount = putAccount(dbManager, witnessBoundary, INITIAL_BALANCE, "boundary_witness");
    Protocol.Account.Builder builder = witnessAccount.getInstance().toBuilder();
    builder.setAllowance(10 * ONE_TRX);
    builder.setLatestWithdrawTime(latestWithdrawTime);
    witnessAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(witnessAccount.getAddress().toByteArray(), witnessAccount);

    putWitness(dbManager, witnessBoundary, "https://boundary-witness.network", 100);

    // Ensure block timestamp matches 'now'
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(now);

    WithdrawBalanceContract contract = WithdrawBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(witnessBoundary)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_BALANCE_CONTRACT", 13)
        .caseName("edge_withdraw_at_exact_cooldown_boundary")
        .caseCategory("edge")
        .description("Edge case: withdraw exactly at cooldown boundary (tests < vs <= validation)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(witnessBoundary)
        .dynamicProperty("WITNESS_ALLOWANCE_FROZEN_TIME", witnessAllowanceFrozenTime)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawBalance boundary: success={}", result.isSuccess());
  }

  @Test
  public void generateWithdrawBalance_validateFailBalanceAllowanceOverflow() throws Exception {
    String witnessOverflow = generateAddress("witness_overflow1");

    // Set balance near Long.MAX_VALUE so balance + allowance overflows
    long nearMaxBalance = Long.MAX_VALUE - 1000;
    AccountCapsule witnessAccount = putAccount(dbManager, witnessOverflow, nearMaxBalance, "overflow_witness");
    Protocol.Account.Builder builder = witnessAccount.getInstance().toBuilder();
    builder.setAllowance(2000); // This + balance would overflow
    builder.setLatestWithdrawTime(0);
    witnessAccount = new AccountCapsule(builder.build());
    dbManager.getAccountStore().put(witnessAccount.getAddress().toByteArray(), witnessAccount);

    putWitness(dbManager, witnessOverflow, "https://overflow-witness.network", 100);

    WithdrawBalanceContract contract = WithdrawBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(witnessOverflow)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.WithdrawBalanceContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("WITHDRAW_BALANCE_CONTRACT", 13)
        .caseName("validate_fail_balance_allowance_overflow")
        .caseCategory("validate_fail")
        .description("Fail when balance + allowance would overflow (LongMath.checkedAdd)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(witnessOverflow)
        .expectedError("overflow")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("WithdrawBalance overflow: validationError={}", result.getValidationError());
  }
}
