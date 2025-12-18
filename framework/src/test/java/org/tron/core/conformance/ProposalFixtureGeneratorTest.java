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
 * <p>Run with: ./gradlew :framework:test --tests "ProposalFixtureGeneratorTest" -Dconformance.output=conformance/fixtures
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
    String outputPath = System.getProperty("conformance.output", "conformance/fixtures");
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
}
