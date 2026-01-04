package org.tron.core.conformance;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import java.io.File;
import java.util.Arrays;
import java.util.HashMap;
import java.util.Map;
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
import org.tron.core.capsule.ProposalCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.capsule.WitnessCapsule;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.ProposalContract.ProposalApproveContract;
import org.tron.protos.contract.ProposalContract.ProposalCreateContract;
import org.tron.protos.contract.ProposalContract.ProposalDeleteContract;

/**
 * Generates conformance test fixtures for Proposal contracts (16, 17, 18).
 *
 * <p>Run with: ./gradlew :framework:test --tests "ProposalFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures
 */
public class ProposalFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(ProposalFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String WITNESS_URL = "https://tron.network";
  private static final long INITIAL_BALANCE = 300_000_000L;

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049abc";
  }

  @Before
  public void setup() {
    // Initialize test accounts and witnesses
    initializeTestData();

    // Configure fixture generator
    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Create owner account
    AccountCapsule ownerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("owner"),
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Create witness for owner
    WitnessCapsule witness = new WitnessCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        10_000_000L,
        WITNESS_URL);
    dbManager.getWitnessStore().put(witness.getAddress().toByteArray(), witness);

    // Set dynamic properties
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(1000000);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
    dbManager.getDynamicPropertiesStore().saveNextMaintenanceTime(2000000);
    dbManager.getDynamicPropertiesStore().saveLatestProposalNum(0);
  }

  // ==========================================================================
  // ProposalCreate (16) Fixtures
  // ==========================================================================

  @Test
  public void generateProposalCreate_happyPath() throws Exception {
    // Build proposal create contract
    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 1000000L); // MAINTENANCE_TIME_INTERVAL

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("happy_path_create")
        .caseCategory("happy")
        .description("Create a new proposal with valid parameters")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("MAINTENANCE_TIME_INTERVAL", 1000000L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateProposalCreate_invalidOwner() throws Exception {
    // Build proposal with non-witness owner
    String nonWitnessAddress = Wallet.getAddressPreFixString() + "548794500882809695a8a687866e76d4271a1abc";

    // Create non-witness account
    AccountCapsule nonWitnessAccount = new AccountCapsule(
        ByteString.copyFromUtf8("non-witness"),
        ByteString.copyFrom(ByteArray.fromHexString(nonWitnessAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(nonWitnessAccount.getAddress().toByteArray(), nonWitnessAccount);

    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 1000000L);

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonWitnessAddress)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_not_witness")
        .caseCategory("validate_fail")
        .description("Fail when owner is not a witness")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(nonWitnessAddress)
        .expectedError("Witness")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate invalid owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalCreate_emptyParameters() throws Exception {
    // Build proposal with empty parameters
    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_empty_params")
        .caseCategory("validate_fail")
        .description("Fail when proposal has no parameters")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("parameters")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate empty params: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // ProposalApprove (17) Fixtures
  // ==========================================================================

  @Test
  public void generateProposalApprove_happyPath() throws Exception {
    // First create a proposal
    createProposal(1);

    // Build proposal approve contract
    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(1)
        .setIsAddApproval(true)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("happy_path_approve")
        .caseCategory("happy")
        .description("Approve an existing proposal")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("proposal_id", 1)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateProposalApprove_nonexistentProposal() throws Exception {
    // Build proposal approve contract for non-existent proposal
    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(999)
        .setIsAddApproval(true)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("validate_fail_nonexistent")
        .caseCategory("validate_fail")
        .description("Fail when approving non-existent proposal")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Proposal")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove nonexistent: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // ProposalDelete (18) Fixtures
  // ==========================================================================

  @Test
  public void generateProposalDelete_happyPath() throws Exception {
    // First create a proposal
    createProposal(2);

    // Build proposal delete contract
    ProposalDeleteContract contract = ProposalDeleteContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(2)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalDeleteContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_DELETE_CONTRACT", 18)
        .caseName("happy_path_delete")
        .caseCategory("happy")
        .description("Delete an existing proposal by its creator")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("proposal_id", 2)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalDelete happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateProposalDelete_notOwner() throws Exception {
    // First create a proposal
    createProposal(3);

    // Create another witness to attempt deletion
    String otherAddress = Wallet.getAddressPreFixString() + "1234567890123456789012345678901234567890";
    AccountCapsule otherAccount = new AccountCapsule(
        ByteString.copyFromUtf8("other"),
        ByteString.copyFrom(ByteArray.fromHexString(otherAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(otherAccount.getAddress().toByteArray(), otherAccount);

    WitnessCapsule otherWitness = new WitnessCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(otherAddress)),
        10_000_000L,
        "https://other.network");
    dbManager.getWitnessStore().put(otherWitness.getAddress().toByteArray(), otherWitness);

    // Build proposal delete contract from different owner
    ProposalDeleteContract contract = ProposalDeleteContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(otherAddress)))
        .setProposalId(3)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalDeleteContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_DELETE_CONTRACT", 18)
        .caseName("validate_fail_not_owner")
        .caseCategory("validate_fail")
        .description("Fail when non-creator tries to delete proposal")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(otherAddress)
        .expectedError("creator")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalDelete not owner: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Edge Case Fixtures
  // ==========================================================================

  @Test
  public void generateProposalApprove_removeApproval() throws Exception {
    // First create a proposal and approve it
    createProposal(10);
    approveProposal(10);

    // Now remove the approval
    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(10)
        .setIsAddApproval(false)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("happy_path_remove_approval")
        .caseCategory("happy")
        .description("Remove approval from a proposal that was previously approved")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("proposal_id", 10)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove remove approval: success={}", result.isSuccess());
  }

  @Test
  public void generateProposalApprove_repeatApproval() throws Exception {
    // First create a proposal and approve it
    createProposal(11);
    approveProposal(11);

    // Try to approve again (should fail)
    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(11)
        .setIsAddApproval(true)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("validate_fail_repeat_approval")
        .caseCategory("validate_fail")
        .description("Fail when trying to approve a proposal that is already approved by this witness")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("approved")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove repeat approval: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalApprove_removeNotApproved() throws Exception {
    // Create a proposal but do NOT approve it
    createProposal(12);

    // Try to remove approval (should fail since we haven't approved)
    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(12)
        .setIsAddApproval(false)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("validate_fail_remove_not_approved")
        .caseCategory("validate_fail")
        .description("Fail when trying to remove approval from a proposal that was never approved")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("not approved")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove remove not approved: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalApprove_expiredProposal() throws Exception {
    // Create an expired proposal
    createExpiredProposal(13);

    // Try to approve expired proposal
    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(13)
        .setIsAddApproval(true)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("validate_fail_expired")
        .caseCategory("validate_fail")
        .description("Fail when trying to approve an expired proposal")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("expired")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove expired: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalApprove_canceledProposal() throws Exception {
    // Create a canceled proposal
    createCanceledProposal(14);

    // Try to approve canceled proposal
    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(14)
        .setIsAddApproval(true)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("validate_fail_canceled")
        .caseCategory("validate_fail")
        .description("Fail when trying to approve a canceled proposal")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("canceled")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove canceled: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalDelete_canceledProposal() throws Exception {
    // Create a canceled proposal
    createCanceledProposal(15);

    // Try to delete canceled proposal
    ProposalDeleteContract contract = ProposalDeleteContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(15)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalDeleteContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_DELETE_CONTRACT", 18)
        .caseName("validate_fail_already_canceled")
        .caseCategory("validate_fail")
        .description("Fail when trying to delete an already canceled proposal")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("canceled")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalDelete canceled: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalDelete_nonexistent() throws Exception {
    // Try to delete non-existent proposal
    ProposalDeleteContract contract = ProposalDeleteContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(9999)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalDeleteContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_DELETE_CONTRACT", 18)
        .caseName("validate_fail_nonexistent")
        .caseCategory("validate_fail")
        .description("Fail when trying to delete a non-existent proposal")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalDelete nonexistent: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalDelete_expiredProposal() throws Exception {
    // Create an expired proposal
    createExpiredProposal(16);

    // Try to delete expired proposal
    ProposalDeleteContract contract = ProposalDeleteContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(16)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalDeleteContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_DELETE_CONTRACT", 18)
        .caseName("validate_fail_expired")
        .caseCategory("validate_fail")
        .description("Fail when trying to delete an expired proposal")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("expired")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalDelete expired: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalCreate_multipleParameters() throws Exception {
    // Build proposal with multiple parameters
    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 1000000L);  // MAINTENANCE_TIME_INTERVAL
    params.put(1L, 3L);        // ACCOUNT_UPGRADE_COST
    params.put(2L, 200L);      // CREATE_ACCOUNT_FEE

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("happy_path_multiple_params")
        .caseCategory("happy")
        .description("Create a proposal with multiple parameters")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("MAINTENANCE_TIME_INTERVAL", 1000000L)
        .dynamicProperty("ACCOUNT_UPGRADE_COST", 3L)
        .dynamicProperty("CREATE_ACCOUNT_FEE", 200L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate multiple params: success={}", result.isSuccess());
  }

  // ==========================================================================
  // Phase 1: ProposalCreateContract Additional Edge Cases
  // ==========================================================================

  // --- Owner address / account / witness validation ---

  @Test
  public void generateProposalCreate_invalidOwnerAddressShort() throws Exception {
    // Build proposal with invalid (too short) owner address - 2 bytes
    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 1000000L);

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString("aaaa")))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_owner_address_invalid_short")
        .caseCategory("validate_fail")
        .description("Fail when owner address is invalid (too short)")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate invalid address short: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalCreate_ownerAccountNotExist() throws Exception {
    // Build proposal with valid-looking address but no account exists
    String nonExistentAddress = Wallet.getAddressPreFixString() + "abcdef1234567890abcdef1234567890abcdef12";

    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 1000000L);

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .expectedError("Account[")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate account not exist: validationError={}", result.getValidationError());
  }

  // --- Parameter id/value validation ---

  @Test
  public void generateProposalCreate_paramCodeUnsupported() throws Exception {
    // Build proposal with unsupported parameter code
    Map<Long, Long> params = new HashMap<>();
    params.put(9999L, 1L);  // Unsupported code

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_param_code_unsupported")
        .caseCategory("validate_fail")
        .description("Fail when parameter code is not supported")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Does not support code : 9999")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate unsupported code: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalCreate_maintenanceIntervalTooLow() throws Exception {
    // Build proposal with MAINTENANCE_TIME_INTERVAL below minimum (< 3 * 27 * 1000 = 81000)
    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 1L);  // Way below minimum

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_maintenance_interval_too_low")
        .caseCategory("validate_fail")
        .description("Fail when MAINTENANCE_TIME_INTERVAL is below minimum")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("valid range is [3 * 27 * 1000,24 * 3600 * 1000]")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate maintenance too low: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalCreate_maintenanceIntervalTooHigh() throws Exception {
    // Build proposal with MAINTENANCE_TIME_INTERVAL above maximum (> 24 * 3600 * 1000 = 86400000)
    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 24L * 3600L * 1000L + 1L);  // Just above maximum

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_maintenance_interval_too_high")
        .caseCategory("validate_fail")
        .description("Fail when MAINTENANCE_TIME_INTERVAL is above maximum")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("valid range is [3 * 27 * 1000,24 * 3600 * 1000]")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate maintenance too high: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalCreate_negativeFeeParam() throws Exception {
    // Build proposal with negative value for fee-like parameter (CREATE_ACCOUNT_FEE = 2)
    Map<Long, Long> params = new HashMap<>();
    params.put(2L, -1L);  // Negative CREATE_ACCOUNT_FEE

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_negative_fee_like_param")
        .caseCategory("validate_fail")
        .description("Fail when fee-like parameter has negative value")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("valid range is [0,")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate negative fee: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalCreate_allowCreationOfContractsValueZero() throws Exception {
    // Build proposal with ALLOW_CREATION_OF_CONTRACTS (9) = 0 (must be 1)
    Map<Long, Long> params = new HashMap<>();
    params.put(9L, 0L);  // ALLOW_CREATION_OF_CONTRACTS must be 1

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_allow_creation_of_contracts_value_zero")
        .caseCategory("validate_fail")
        .description("Fail when ALLOW_CREATION_OF_CONTRACTS is not 1")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("ALLOW_CREATION_OF_CONTRACTS")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate allow creation zero: validationError={}", result.getValidationError());
  }

  // --- Parameter prerequisite / dependency ---

  @Test
  public void generateProposalCreate_allowTvmTransferTrc10PrereqNotMet() throws Exception {
    // Set ALLOW_SAME_TOKEN_NAME to 0 before proposing ALLOW_TVM_TRANSFER_TRC10
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(0);

    Map<Long, Long> params = new HashMap<>();
    params.put(18L, 1L);  // ALLOW_TVM_TRANSFER_TRC10 requires ALLOW_SAME_TOKEN_NAME == 1

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_allow_tvm_transfer_trc10_prereq_not_met")
        .caseCategory("validate_fail")
        .description("Fail when ALLOW_SAME_TOKEN_NAME not approved before ALLOW_TVM_TRANSFER_TRC10")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("[ALLOW_SAME_TOKEN_NAME] proposal must be approved before [ALLOW_TVM_TRANSFER_TRC10] can be proposed")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate TRC10 prereq not met: validationError={}", result.getValidationError());
  }

  // --- One-time proposal validation ---

  @Test
  public void generateProposalCreate_removePowerGrAlreadyExecuted() throws Exception {
    // Set REMOVE_THE_POWER_OF_THE_GR to -1 (already executed)
    dbManager.getDynamicPropertiesStore().saveRemoveThePowerOfTheGr(-1);

    Map<Long, Long> params = new HashMap<>();
    params.put(10L, 1L);  // REMOVE_THE_POWER_OF_THE_GR

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_remove_power_gr_already_executed")
        .caseCategory("validate_fail")
        .description("Fail when REMOVE_THE_POWER_OF_THE_GR already executed")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("only allowed to be executed once")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate GR already executed: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalCreate_removePowerGrValueNotOne() throws Exception {
    // Set REMOVE_THE_POWER_OF_THE_GR to 0 (not executed yet)
    dbManager.getDynamicPropertiesStore().saveRemoveThePowerOfTheGr(0);

    Map<Long, Long> params = new HashMap<>();
    params.put(10L, 0L);  // REMOVE_THE_POWER_OF_THE_GR must be 1

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("validate_fail_remove_power_gr_value_not_one")
        .caseCategory("validate_fail")
        .description("Fail when REMOVE_THE_POWER_OF_THE_GR value is not 1")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("REMOVE_THE_POWER_OF_THE_GR")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate GR value not one: validationError={}", result.getValidationError());
  }

  // --- Boundary-happy fixtures ---

  @Test
  public void generateProposalCreate_maintenanceIntervalMinBound() throws Exception {
    // Build proposal with MAINTENANCE_TIME_INTERVAL at minimum boundary (3 * 27 * 1000 = 81000)
    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 3L * 27L * 1000L);  // Exactly at minimum

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("happy_path_maintenance_interval_min_bound")
        .caseCategory("happy")
        .description("Create proposal with MAINTENANCE_TIME_INTERVAL at minimum boundary")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("MAINTENANCE_TIME_INTERVAL", 3L * 27L * 1000L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate maintenance min bound: success={}", result.isSuccess());
  }

  @Test
  public void generateProposalCreate_maintenanceIntervalMaxBound() throws Exception {
    // Build proposal with MAINTENANCE_TIME_INTERVAL at maximum boundary (24 * 3600 * 1000 = 86400000)
    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 24L * 3600L * 1000L);  // Exactly at maximum

    ProposalCreateContract contract = ProposalCreateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .putAllParameters(params)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalCreateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_CREATE_CONTRACT", 16)
        .caseName("happy_path_maintenance_interval_max_bound")
        .caseCategory("happy")
        .description("Create proposal with MAINTENANCE_TIME_INTERVAL at maximum boundary")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("MAINTENANCE_TIME_INTERVAL", 24L * 3600L * 1000L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalCreate maintenance max bound: success={}", result.isSuccess());
  }

  // ==========================================================================
  // Phase 2: ProposalApproveContract Additional Edge Cases
  // ==========================================================================

  // --- Owner/witness validation ---

  @Test
  public void generateProposalApprove_invalidOwnerAddressShort() throws Exception {
    // Build proposal approve with invalid (too short) owner address - 2 bytes
    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString("aaaa")))
        .setProposalId(1)
        .setIsAddApproval(true)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("validate_fail_owner_address_invalid_short")
        .caseCategory("validate_fail")
        .description("Fail when owner address is invalid (too short)")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove invalid address short: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalApprove_ownerAccountNotExist() throws Exception {
    // Build proposal approve with valid-looking address but no account exists
    String nonExistentAddress = Wallet.getAddressPreFixString() + "abcdef1234567890abcdef1234567890abcdef12";

    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
        .setProposalId(1)
        .setIsAddApproval(true)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .expectedError("Account[")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove account not exist: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalApprove_ownerNotWitness() throws Exception {
    // Create an account that is NOT a witness
    String nonWitnessAddress = Wallet.getAddressPreFixString() + "9876543210fedcba9876543210fedcba98765432";

    AccountCapsule nonWitnessAccount = new AccountCapsule(
        ByteString.copyFromUtf8("non-witness-approve"),
        ByteString.copyFrom(ByteArray.fromHexString(nonWitnessAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(nonWitnessAccount.getAddress().toByteArray(), nonWitnessAccount);

    // Create a proposal to approve
    createProposal(20);

    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonWitnessAddress)))
        .setProposalId(20)
        .setIsAddApproval(true)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("validate_fail_owner_not_witness")
        .caseCategory("validate_fail")
        .description("Fail when owner is not a witness")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(nonWitnessAddress)
        .expectedError("Witness[")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove not witness: validationError={}", result.getValidationError());
  }

  // --- Proposal store / dynamic property inconsistency ---

  @Test
  public void generateProposalApprove_proposalMissingButLatestNumAllows() throws Exception {
    // Set latestProposalNum to 100 but don't create proposal 100
    dbManager.getDynamicPropertiesStore().saveLatestProposalNum(100);

    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(100)
        .setIsAddApproval(true)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("validate_fail_proposal_missing_but_latest_num_allows_it")
        .caseCategory("validate_fail")
        .description("Fail when proposal is missing from ProposalStore but latestProposalNum allows it")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Proposal[100]")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove missing proposal: validationError={}", result.getValidationError());
  }

  // --- Expiration boundary ---

  @Test
  public void generateProposalApprove_expiredAtExactBoundary() throws Exception {
    // Create proposal where expirationTime == current timestamp (exact boundary)
    long currentTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    createProposalWithExpiration(21, currentTime);

    ProposalApproveContract contract = ProposalApproveContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(21)
        .setIsAddApproval(true)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalApproveContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_APPROVE_CONTRACT", 17)
        .caseName("validate_fail_expired_at_exact_boundary")
        .caseCategory("validate_fail")
        .description("Fail when now == expirationTime (exact boundary)")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("expired")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalApprove expired boundary: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Phase 3: ProposalDeleteContract Additional Edge Cases
  // ==========================================================================

  // --- Owner address / account existence ---

  @Test
  public void generateProposalDelete_invalidOwnerAddressShort() throws Exception {
    // Build proposal delete with invalid (too short) owner address - 2 bytes
    ProposalDeleteContract contract = ProposalDeleteContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString("aaaa")))
        .setProposalId(1)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalDeleteContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_DELETE_CONTRACT", 18)
        .caseName("validate_fail_owner_address_invalid_short")
        .caseCategory("validate_fail")
        .description("Fail when owner address is invalid (too short)")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .expectedError("Invalid address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalDelete invalid address short: validationError={}", result.getValidationError());
  }

  @Test
  public void generateProposalDelete_ownerAccountNotExist() throws Exception {
    // Build proposal delete with valid-looking address but no account exists
    String nonExistentAddress = Wallet.getAddressPreFixString() + "abcdef1234567890abcdef1234567890abcdef12";

    ProposalDeleteContract contract = ProposalDeleteContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
        .setProposalId(1)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalDeleteContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_DELETE_CONTRACT", 18)
        .caseName("validate_fail_owner_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .expectedError("Account[")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalDelete account not exist: validationError={}", result.getValidationError());
  }

  // --- Semantic nuance: delete does not require witness membership ---

  @Test
  public void generateProposalDelete_happyPathWithoutWitnessEntry() throws Exception {
    // Create proposal first (while owner is still a witness)
    createProposal(30);

    // Remove the owner from WitnessStore (delete does NOT require witness)
    dbManager.getWitnessStore().delete(ByteArray.fromHexString(OWNER_ADDRESS));

    ProposalDeleteContract contract = ProposalDeleteContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(30)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalDeleteContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_DELETE_CONTRACT", 18)
        .caseName("happy_path_delete_without_witness_entry")
        .caseCategory("happy")
        .description("Delete succeeds even if owner is no longer a witness (only account + proposer match required)")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("proposal_id", 30)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalDelete without witness: success={}", result.isSuccess());

    // Re-add witness for other tests
    WitnessCapsule witness = new WitnessCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        10_000_000L,
        WITNESS_URL);
    dbManager.getWitnessStore().put(witness.getAddress().toByteArray(), witness);
  }

  // --- Proposal store / dynamic property inconsistency ---

  @Test
  public void generateProposalDelete_proposalMissingButLatestNumAllows() throws Exception {
    // Set latestProposalNum to 200 but don't create proposal 200
    dbManager.getDynamicPropertiesStore().saveLatestProposalNum(200);

    ProposalDeleteContract contract = ProposalDeleteContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(200)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalDeleteContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_DELETE_CONTRACT", 18)
        .caseName("validate_fail_proposal_missing_but_latest_num_allows_it")
        .caseCategory("validate_fail")
        .description("Fail when proposal is missing from ProposalStore but latestProposalNum allows it")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Proposal[200]")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalDelete missing proposal: validationError={}", result.getValidationError());
  }

  // --- Expiration boundary ---

  @Test
  public void generateProposalDelete_expiredAtExactBoundary() throws Exception {
    // Create proposal where expirationTime == current timestamp (exact boundary)
    long currentTime = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    createProposalWithExpiration(31, currentTime);

    ProposalDeleteContract contract = ProposalDeleteContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(31)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalDeleteContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_DELETE_CONTRACT", 18)
        .caseName("validate_fail_expired_at_exact_boundary")
        .caseCategory("validate_fail")
        .description("Fail when now == expirationTime (exact boundary)")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("expired")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalDelete expired boundary: validationError={}", result.getValidationError());
  }

  // --- Optional: cancellation with existing approvals ---

  @Test
  public void generateProposalDelete_happyPathEvenIfApproved() throws Exception {
    // Create proposal and approve it, then delete by creator
    createProposal(32);
    approveProposal(32);

    ProposalDeleteContract contract = ProposalDeleteContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setProposalId(32)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.ProposalDeleteContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("PROPOSAL_DELETE_CONTRACT", 18)
        .caseName("happy_path_delete_even_if_already_approved_by_someone")
        .caseCategory("happy")
        .description("Delete succeeds even if proposal already has approvals")
        .database("account")
        .database("proposal")
        .database("dynamic-properties")
        .database("witness")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("proposal_id", 32)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("ProposalDelete with approvals: success={}", result.isSuccess());
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

  private void createProposal(long id) {
    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 1000000L);

    ProposalCapsule proposal = new ProposalCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        id);
    proposal.setParameters(params);
    proposal.setCreateTime(
        chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp());
    proposal.setExpirationTime(
        chainBaseManager.getDynamicPropertiesStore().getNextMaintenanceTime() + 3 * 4 * 21600000);

    chainBaseManager.getProposalStore().put(proposal.createDbKey(), proposal);
    chainBaseManager.getDynamicPropertiesStore().saveLatestProposalNum(id);

    log.info("Created proposal {} for testing", id);
  }

  private void approveProposal(long id) {
    try {
      ProposalCapsule proposal = chainBaseManager.getProposalStore().get(
          ByteArray.fromLong(id));
      if (proposal != null) {
        proposal.addApproval(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)));
        chainBaseManager.getProposalStore().put(proposal.createDbKey(), proposal);
        log.info("Approved proposal {} by {}", id, OWNER_ADDRESS);
      }
    } catch (Exception e) {
      log.error("Failed to approve proposal {}: {}", id, e.getMessage());
    }
  }

  private void createExpiredProposal(long id) {
    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 1000000L);

    ProposalCapsule proposal = new ProposalCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        id);
    proposal.setParameters(params);
    proposal.setCreateTime(1);  // Very old creation time
    proposal.setExpirationTime(100);  // Already expired

    chainBaseManager.getProposalStore().put(proposal.createDbKey(), proposal);
    chainBaseManager.getDynamicPropertiesStore().saveLatestProposalNum(id);

    log.info("Created expired proposal {} for testing", id);
  }

  private void createCanceledProposal(long id) {
    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 1000000L);

    ProposalCapsule proposal = new ProposalCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        id);
    proposal.setParameters(params);
    proposal.setCreateTime(
        chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp());
    proposal.setExpirationTime(
        chainBaseManager.getDynamicPropertiesStore().getNextMaintenanceTime() + 3 * 4 * 21600000);
    // Set state to CANCELED (3)
    proposal.setState(Protocol.Proposal.State.CANCELED);

    chainBaseManager.getProposalStore().put(proposal.createDbKey(), proposal);
    chainBaseManager.getDynamicPropertiesStore().saveLatestProposalNum(id);

    log.info("Created canceled proposal {} for testing", id);
  }

  private void createProposalWithExpiration(long id, long expirationTime) {
    Map<Long, Long> params = new HashMap<>();
    params.put(0L, 1000000L);

    ProposalCapsule proposal = new ProposalCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        id);
    proposal.setParameters(params);
    proposal.setCreateTime(
        chainBaseManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp());
    proposal.setExpirationTime(expirationTime);

    chainBaseManager.getProposalStore().put(proposal.createDbKey(), proposal);
    chainBaseManager.getDynamicPropertiesStore().saveLatestProposalNum(id);

    log.info("Created proposal {} with expiration {} for testing", id, expirationTime);
  }
}
