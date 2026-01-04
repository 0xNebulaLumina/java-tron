package org.tron.core.conformance;

import static org.tron.core.conformance.ConformanceFixtureTestSupport.*;

import com.google.protobuf.ByteString;
import java.io.File;
import org.junit.Before;
import org.junit.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.BaseTest;
import org.tron.common.utils.ByteArray;
import org.tron.core.Constant;
import org.tron.core.Wallet;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.AssetIssueCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.ContractCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract;
import org.tron.protos.contract.AssetIssueContractOuterClass.TransferAssetContract;
import org.tron.protos.contract.BalanceContract.TransferContract;
import org.tron.protos.contract.SmartContractOuterClass.SmartContract;

/**
 * Generates conformance test fixtures for transfer contracts:
 * - TransferContract (1)
 * - TransferAssetContract (2)
 *
 * <p>Run with: ./gradlew :framework:test --tests "TransferFixtureGeneratorTest"
 * -Dconformance.output=../conformance/fixtures --dependency-verification=off
 */
public class TransferFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(TransferFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String RECEIVER_ADDRESS;
  private static final String WITNESS_ADDRESS;
  private static final String TOKEN_ID = "1000001";
  private static final long CREATE_ACCOUNT_FEE = ONE_TRX;

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049151";
    RECEIVER_ADDRESS = Wallet.getAddressPreFixString() + "1111111111111111111111111111111111111111";
    WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "548794500882809695a8a687866e76d4271a1abc";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("Transfer Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Initialize dynamic properties for TRC-10
    initTrc10DynamicProps(dbManager,
        DEFAULT_BLOCK_TIMESTAMP / 1000,
        DEFAULT_BLOCK_TIMESTAMP);

    // Set create account fee for new recipient path
    dbManager.getDynamicPropertiesStore().saveCreateNewAccountFeeInSystemContract(CREATE_ACCOUNT_FEE);

    // Create owner account with sufficient TRX and TRC-10 tokens
    AccountCapsule ownerAccount = putAccount(dbManager, OWNER_ADDRESS, INITIAL_BALANCE, "owner");
    ownerAccount.addAssetAmountV2(TOKEN_ID.getBytes(), 1_000_000_000L, dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Create receiver account
    putAccount(dbManager, RECEIVER_ADDRESS, INITIAL_BALANCE, "receiver");

    // Create witness
    putAccount(dbManager, WITNESS_ADDRESS, INITIAL_BALANCE, "witness");
    putWitness(dbManager, WITNESS_ADDRESS, "https://witness.network", 10_000_000L);

    // Create TRC-10 asset
    putAssetIssueV2(dbManager, TOKEN_ID, OWNER_ADDRESS, "TestToken", 1_000_000_000_000L);
  }

  // ==========================================================================
  // TransferContract (1) Fixtures
  // ==========================================================================

  @Test
  public void generateTransfer_happyPathExistingRecipient() throws Exception {
    long amount = 10 * ONE_TRX;

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("happy_path_existing_recipient")
        .caseCategory("happy")
        .description("Normal TRX transfer to an existing account")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("transfer_amount", amount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer happy path existing: success={}", result.isSuccess());
  }

  @Test
  public void generateTransfer_happyPathCreatesRecipient() throws Exception {
    String newRecipient = generateAddress("new_recipient_01");
    long amount = 10 * ONE_TRX;

    // Ensure new recipient doesn't exist
    // (not created in setup)

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(newRecipient)))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("happy_path_creates_recipient")
        .caseCategory("happy")
        .description("TRX transfer that creates the recipient account (pays create-account-fee)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("transfer_amount", amount)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer creates recipient: success={}", result.isSuccess());
  }

  @Test
  public void generateTransfer_validateFailToSelf() throws Exception {
    long amount = 10 * ONE_TRX;

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS))) // Self
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_to_self")
        .caseCategory("validate_fail")
        .description("Fail when transferring to self")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("self")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer to self: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransfer_validateFailAmountZero() throws Exception {
    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAmount(0) // Zero amount
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_amount_zero")
        .caseCategory("validate_fail")
        .description("Fail when transfer amount is zero")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("amount")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer amount zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransfer_validateFailInsufficientBalance() throws Exception {
    String poorOwner = generateAddress("poor_owner_0001");
    putAccount(dbManager, poorOwner, ONE_TRX, "poor");

    // Try to transfer more than balance
    long amount = INITIAL_BALANCE + ONE_TRX;

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(poorOwner)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_insufficient_balance")
        .caseCategory("validate_fail")
        .description("Fail when owner has insufficient balance")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(poorOwner)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer insufficient: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // TransferContract (1) - Phase 1 Edge Cases
  // --------------------------------------------------------------------------

  @Test
  public void generateTransfer_validateFailOwnerAddressInvalid() throws Exception {
    // Empty owner address (invalid)
    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Invalid - empty
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAmount(10 * ONE_TRX)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_owner_address_invalid")
        .caseCategory("validate_fail")
        .description("Fail when ownerAddress is invalid (empty)")
        .database("account")
        .database("dynamic-properties")
        .expectedError("Invalid ownerAddress!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer owner invalid: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransfer_validateFailToAddressInvalid() throws Exception {
    // Empty to address (invalid)
    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.EMPTY) // Invalid - empty
        .setAmount(10 * ONE_TRX)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_to_address_invalid")
        .caseCategory("validate_fail")
        .description("Fail when toAddress is invalid (empty)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid toAddress!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer to invalid: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransfer_validateFailOwnerAccountNotFound() throws Exception {
    // Use a valid-looking address that is NOT in the account store
    String unknownOwner = generateAddress("unknown_owner_001");
    // Note: NOT calling putAccount for this address

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unknownOwner)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAmount(10 * ONE_TRX)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_owner_account_not_found")
        .caseCategory("validate_fail")
        .description("Fail when owner address is valid but account not in store")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(unknownOwner)
        .expectedError("Validate TransferContract error, no OwnerAccount.")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer owner not found: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransfer_validateFailAmountNegative() throws Exception {
    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAmount(-1) // Negative amount
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_amount_negative")
        .caseCategory("validate_fail")
        .description("Fail when transfer amount is negative")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Amount must be greater than 0.")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer amount negative: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransfer_validateFailRecipientBalanceOverflow() throws Exception {
    // Create recipient with balance near Long.MAX_VALUE
    String maxBalanceRecipient = generateAddress("max_bal_recv_01");
    AccountCapsule maxAccount = new AccountCapsule(
        ByteString.copyFromUtf8("max_balance"),
        ByteString.copyFrom(ByteArray.fromHexString(maxBalanceRecipient)),
        AccountType.Normal,
        Long.MAX_VALUE); // Max balance
    dbManager.getAccountStore().put(maxAccount.getAddress().toByteArray(), maxAccount);

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(maxBalanceRecipient)))
        .setAmount(1) // Just 1 SUN should overflow
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_recipient_balance_overflow")
        .caseCategory("validate_fail")
        .description("Fail when recipient balance + amount would overflow Long.MAX_VALUE")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("long overflow")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer recipient overflow: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransfer_validateFailCreateRecipientInsufficientForFee() throws Exception {
    // Create owner with just enough for the amount but not for amount + fee
    String tightOwner = generateAddress("tight_owner_001");
    long transferAmount = 10 * ONE_TRX;
    // Owner has exactly transferAmount, but recipient doesn't exist,
    // so CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT will be added
    putAccount(dbManager, tightOwner, transferAmount, "tight_owner");

    String newRecipient = generateAddress("new_recv_tight_1");
    // Recipient does NOT exist (not created in DB)

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(tightOwner)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(newRecipient)))
        .setAmount(transferAmount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_create_recipient_insufficient_for_fee")
        .caseCategory("validate_fail")
        .description("Fail when owner has enough for amount but not amount + create-account fee")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(tightOwner)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .expectedError("balance is not sufficient")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer create recipient insufficient fee: validationError={}",
        result.getValidationError());
  }

  @Test
  public void generateTransfer_edgePerfectTransferDrainsOwner() throws Exception {
    // Create owner with exactly the amount to transfer (recipient exists, so no fee)
    String drainOwner = generateAddress("drain_owner_001");
    long drainAmount = 100 * ONE_TRX;
    putAccount(dbManager, drainOwner, drainAmount, "drain_owner");

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(drainOwner)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAmount(drainAmount) // Transfer entire balance
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("edge_perfect_transfer_drains_owner")
        .caseCategory("happy")
        .description("Boundary: transfer exact balance, owner ends with 0 TRX")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(drainOwner)
        .dynamicProperty("transfer_amount", drainAmount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer perfect drain: success={}", result.isSuccess());
  }

  @Test
  public void generateTransfer_validateFailForbidTransferToContract() throws Exception {
    // Create a Contract type account as recipient
    String contractRecipient = generateAddress("contract_recv_01");
    AccountCapsule contractAccount = new AccountCapsule(
        ByteString.copyFromUtf8("contract_account"),
        ByteString.copyFrom(ByteArray.fromHexString(contractRecipient)),
        AccountType.Contract, // Contract type
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(contractAccount.getAddress().toByteArray(), contractAccount);

    // Enable forbidTransferToContract
    dbManager.getDynamicPropertiesStore().saveForbidTransferToContract(1);

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(contractRecipient)))
        .setAmount(10 * ONE_TRX)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_forbid_transfer_to_contract")
        .caseCategory("validate_fail")
        .description("Fail when forbidTransferToContract=1 and recipient is Contract type")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("forbidTransferToContract", 1)
        .expectedError("Cannot transfer TRX to a smartContract.")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer forbid to contract: validationError={}", result.getValidationError());

    // Reset to default
    dbManager.getDynamicPropertiesStore().saveForbidTransferToContract(0);
  }

  @Test
  public void generateTransfer_validateFailEvmCompatibleVersionOneContract() throws Exception {
    // Create a Contract type account as recipient
    String contractRecipient = generateAddress("evm_contract_01");
    AccountCapsule contractAccount = new AccountCapsule(
        ByteString.copyFromUtf8("evm_contract"),
        ByteString.copyFrom(ByteArray.fromHexString(contractRecipient)),
        AccountType.Contract,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(contractAccount.getAddress().toByteArray(), contractAccount);

    // Create ContractCapsule with version=1
    SmartContract.Builder smartContractBuilder = SmartContract.newBuilder()
        .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(contractRecipient)))
        .setOriginAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setVersion(1); // Version 1
    ContractCapsule contractCapsule = new ContractCapsule(smartContractBuilder.build());
    dbManager.getContractStore().put(
        ByteArray.fromHexString(contractRecipient), contractCapsule);

    // Enable allowTvmCompatibleEvm but NOT forbidTransferToContract
    dbManager.getDynamicPropertiesStore().saveForbidTransferToContract(0);
    dbManager.getDynamicPropertiesStore().saveAllowTvmCompatibleEvm(1);

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(contractRecipient)))
        .setAmount(10 * ONE_TRX)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("validate_fail_evm_compatible_version_one_contract")
        .caseCategory("validate_fail")
        .description("Fail when allowTvmCompatibleEvm=1 and recipient contract has version=1")
        .database("account")
        .database("dynamic-properties")
        .database("contract")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("allowTvmCompatibleEvm", 1)
        .expectedError("Cannot transfer TRX to a smartContract which version is one")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer evm version one: validationError={}", result.getValidationError());

    // Reset to default
    dbManager.getDynamicPropertiesStore().saveAllowTvmCompatibleEvm(0);
  }

  @Test
  public void generateTransfer_edgeCreateRecipientAllowMultiSign() throws Exception {
    String newRecipient = generateAddress("multisig_recv_01");
    long amount = 10 * ONE_TRX;

    // Enable multi-sign
    dbManager.getDynamicPropertiesStore().saveAllowMultiSign(1);

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(newRecipient)))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("edge_create_recipient_allow_multisign")
        .caseCategory("happy")
        .description("Create recipient with allowMultiSign=1 (default permissions initialized)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("allowMultiSign", 1)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer multisign create: success={}", result.isSuccess());

    // Reset to default
    dbManager.getDynamicPropertiesStore().saveAllowMultiSign(0);
  }

  @Test
  public void generateTransfer_edgeCreateRecipientBurnFee() throws Exception {
    String newRecipient = generateAddress("burnfee_recv_01");
    long amount = 10 * ONE_TRX;

    // Enable blackhole optimization (burn instead of credit)
    dbManager.getDynamicPropertiesStore().saveAllowBlackHoleOptimization(1);

    TransferContract contract = TransferContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(newRecipient)))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_CONTRACT", 1)
        .caseName("edge_create_recipient_burn_fee")
        .caseCategory("happy")
        .description("Create recipient with allowBlackHoleOptimization=1 (fee is burned)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("allowBlackHoleOptimization", 1)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("Transfer burn fee: success={}", result.isSuccess());

    // Reset to default
    dbManager.getDynamicPropertiesStore().saveAllowBlackHoleOptimization(0);
  }

  // ==========================================================================
  // TransferAssetContract (2) Fixtures
  // ==========================================================================

  @Test
  public void generateTransferAsset_happyPathExistingRecipient() throws Exception {
    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("happy_path_transfer_asset_existing_recipient")
        .caseCategory("happy")
        .description("Transfer TRC-10 asset to existing account")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("token_id", TOKEN_ID)
        .dynamicProperty("amount", amount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset happy path existing: success={}", result.isSuccess());
  }

  @Test
  public void generateTransferAsset_happyPathCreatesRecipient() throws Exception {
    String newRecipient = generateAddress("new_asset_recv01");
    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(newRecipient)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("happy_path_transfer_asset_creates_recipient")
        .caseCategory("happy")
        .description("Transfer TRC-10 asset that creates recipient account")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("token_id", TOKEN_ID)
        .dynamicProperty("amount", amount)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset creates recipient: success={}", result.isSuccess());
  }

  @Test
  public void generateTransferAsset_validateFailAssetNotFound() throws Exception {
    String nonExistentToken = "9999999";
    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(nonExistentToken))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_asset_not_found")
        .caseCategory("validate_fail")
        .description("Fail when token ID does not exist in asset-issue-v2")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("asset")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset asset not found: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailInsufficientAssetBalance() throws Exception {
    String poorAssetOwner = generateAddress("poor_asset_own1");
    putAccount(dbManager, poorAssetOwner, INITIAL_BALANCE, "poor_asset");
    // Don't add any tokens to this account

    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(poorAssetOwner)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_insufficient_asset_balance")
        .caseCategory("validate_fail")
        .description("Fail when owner has insufficient asset balance")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(poorAssetOwner)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset insufficient asset: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailToSelf() throws Exception {
    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS))) // Self
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_to_self")
        .caseCategory("validate_fail")
        .description("Fail when transferring asset to self")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("self")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset to self: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // TransferAssetContract (2) - Phase 2 Edge Cases
  // --------------------------------------------------------------------------

  @Test
  public void generateTransferAsset_validateFailOwnerAddressInvalid() throws Exception {
    // Empty owner address (invalid)
    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY) // Invalid - empty
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(1000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_owner_address_invalid")
        .caseCategory("validate_fail")
        .description("Fail when ownerAddress is invalid (empty)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset owner invalid: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailToAddressInvalid() throws Exception {
    // Empty to address (invalid)
    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.EMPTY) // Invalid - empty
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(1000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_to_address_invalid")
        .caseCategory("validate_fail")
        .description("Fail when toAddress is invalid (empty)")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid toAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset to invalid: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailOwnerAccountNotFound() throws Exception {
    // Use a valid-looking address that is NOT in the account store
    String unknownOwner = generateAddress("unknown_asset_owner");
    // Note: NOT calling putAccount for this address

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(unknownOwner)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(1000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_owner_account_not_found")
        .caseCategory("validate_fail")
        .description("Fail when owner address is valid but account not in store")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(unknownOwner)
        .expectedError("No owner account!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset owner not found: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailAmountZero() throws Exception {
    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(0) // Zero amount
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_amount_zero")
        .caseCategory("validate_fail")
        .description("Fail when transfer amount is zero")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Amount must be greater than 0.")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset amount zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailAmountNegative() throws Exception {
    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(-1) // Negative amount
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_amount_negative")
        .caseCategory("validate_fail")
        .description("Fail when transfer amount is negative")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Amount must be greater than 0.")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset amount negative: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailInsufficientAssetBalanceNonzero() throws Exception {
    // Create owner with some TRC-10 tokens, but less than needed
    String smallTokenOwner = generateAddress("small_token_own1");
    AccountCapsule smallTokenAccount = putAccount(dbManager, smallTokenOwner, INITIAL_BALANCE, "small_token");
    // Add a small token balance (5 tokens)
    smallTokenAccount.addAssetAmountV2(TOKEN_ID.getBytes(), 5,
        dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(smallTokenAccount.getAddress().toByteArray(), smallTokenAccount);

    // Attempt to transfer more than available
    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(smallTokenOwner)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(6) // More than the 5 tokens owned
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_insufficient_asset_balance_nonzero")
        .caseCategory("validate_fail")
        .description("Fail when owner has nonzero but insufficient token balance")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(smallTokenOwner)
        .dynamicProperty("token_id", TOKEN_ID)
        .expectedError("assetBalance is not sufficient.")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset insufficient nonzero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailRecipientAssetBalanceOverflow() throws Exception {
    // Create recipient with asset balance near Long.MAX_VALUE
    String maxAssetRecipient = generateAddress("max_asset_recv1");
    AccountCapsule maxAssetAccount = putAccount(dbManager, maxAssetRecipient, INITIAL_BALANCE, "max_asset");
    // Add max token balance
    maxAssetAccount.addAssetAmountV2(TOKEN_ID.getBytes(), Long.MAX_VALUE,
        dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(maxAssetAccount.getAddress().toByteArray(), maxAssetAccount);

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(maxAssetRecipient)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(1) // Just 1 token should overflow
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_recipient_asset_balance_overflow")
        .caseCategory("validate_fail")
        .description("Fail when recipient asset balance + amount would overflow Long.MAX_VALUE")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("token_id", TOKEN_ID)
        .expectedError("long overflow")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset recipient overflow: validationError={}", result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailCreateRecipientInsufficientTrxFee() throws Exception {
    // Create owner with TRC-10 tokens but very low TRX balance
    String lowTrxOwner = generateAddress("low_trx_owner_01");
    AccountCapsule lowTrxAccount = new AccountCapsule(
        ByteString.copyFromUtf8("low_trx"),
        ByteString.copyFrom(ByteArray.fromHexString(lowTrxOwner)),
        AccountType.Normal,
        100L); // Only 100 SUN - not enough for create account fee
    // Add tokens
    lowTrxAccount.addAssetAmountV2(TOKEN_ID.getBytes(), 1_000_000L,
        dbManager.getDynamicPropertiesStore(), dbManager.getAssetIssueStore());
    dbManager.getAccountStore().put(lowTrxAccount.getAddress().toByteArray(), lowTrxAccount);

    String newRecipient = generateAddress("new_asset_recv_1");
    // Recipient does NOT exist

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(lowTrxOwner)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(newRecipient)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(1000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_create_recipient_insufficient_trx_fee")
        .caseCategory("validate_fail")
        .description("Fail when owner has tokens but not enough TRX for create-account fee")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(lowTrxOwner)
        .dynamicProperty("token_id", TOKEN_ID)
        .dynamicProperty("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", CREATE_ACCOUNT_FEE)
        .expectedError("Validate TransferAssetActuator error, insufficient fee.")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset create recipient insufficient TRX fee: validationError={}",
        result.getValidationError());
  }

  @Test
  public void generateTransferAsset_validateFailForbidTransferAssetToContract() throws Exception {
    // Create a Contract type account as recipient
    String contractRecipient = generateAddress("asset_contract_1");
    AccountCapsule contractAccount = new AccountCapsule(
        ByteString.copyFromUtf8("contract_for_asset"),
        ByteString.copyFrom(ByteArray.fromHexString(contractRecipient)),
        AccountType.Contract, // Contract type
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(contractAccount.getAddress().toByteArray(), contractAccount);

    // Enable forbidTransferToContract
    dbManager.getDynamicPropertiesStore().saveForbidTransferToContract(1);

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(contractRecipient)))
        .setAssetName(ByteString.copyFromUtf8(TOKEN_ID))
        .setAmount(1000)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("validate_fail_forbid_transfer_asset_to_contract")
        .caseCategory("validate_fail")
        .description("Fail when forbidTransferToContract=1 and recipient is Contract type")
        .database("account")
        .database("asset-issue-v2")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("token_id", TOKEN_ID)
        .dynamicProperty("forbidTransferToContract", 1)
        .expectedError("Cannot transfer asset to smartContract.")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset forbid to contract: validationError={}", result.getValidationError());

    // Reset to default
    dbManager.getDynamicPropertiesStore().saveForbidTransferToContract(0);
  }

  // --------------------------------------------------------------------------
  // TransferAssetContract (2) - Legacy Mode (allowSameTokenName=0) Fixtures
  // --------------------------------------------------------------------------

  private static final String LEGACY_TOKEN_NAME = "LegacyToken";

  /**
   * Helper to seed a TRC-10 asset in V1 (name-based) mode.
   * Uses asset-issue store (not V2) and sets assetName to token name.
   */
  private AssetIssueCapsule putAssetIssueV1(String tokenName, String ownerHexAddress, long totalSupply) {
    long nowMs;
    try {
      nowMs = dbManager.getDynamicPropertiesStore().getLatestBlockHeaderTimestamp();
    } catch (IllegalArgumentException e) {
      nowMs = DEFAULT_BLOCK_TIMESTAMP;
    }
    long startTime = nowMs - DEFAULT_BLOCK_INTERVAL_MS;
    long endTime = nowMs + 86400000L * 365;

    AssetIssueContract assetIssue = AssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(ownerHexAddress)))
        .setName(ByteString.copyFromUtf8(tokenName))
        .setAbbr(ByteString.copyFromUtf8(tokenName.length() <= 5 ? tokenName : tokenName.substring(0, 5)))
        .setTotalSupply(totalSupply)
        .setPrecision(6)
        .setTrxNum(1)
        .setNum(1)
        .setStartTime(startTime)
        .setEndTime(endTime)
        .setDescription(ByteString.copyFromUtf8("Legacy TRC-10 asset for conformance fixtures"))
        .setUrl(ByteString.copyFromUtf8("https://example.com"))
        .setFreeAssetNetLimit(1000)
        .setPublicFreeAssetNetLimit(1000)
        .build();

    AssetIssueCapsule assetCapsule = new AssetIssueCapsule(assetIssue);
    // V1 mode uses createDbKey() which returns the name bytes
    dbManager.getAssetIssueStore().put(assetCapsule.createDbKey(), assetCapsule);
    return assetCapsule;
  }

  @Test
  public void generateTransferAsset_legacyHappyPathExistingRecipient() throws Exception {
    // Switch to legacy mode (V1, name-based)
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(0);

    // Seed V1 asset
    putAssetIssueV1(LEGACY_TOKEN_NAME, OWNER_ADDRESS, 1_000_000_000_000L);

    // Add legacy tokens to owner account using V1 method
    AccountCapsule ownerAccount = dbManager.getAccountStore().get(
        ByteArray.fromHexString(OWNER_ADDRESS));
    ownerAccount.addAsset(LEGACY_TOKEN_NAME.getBytes(), 1_000_000_000L);
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(LEGACY_TOKEN_NAME)) // Name, not ID
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("legacy_happy_path_existing_recipient")
        .caseCategory("happy")
        .description("Legacy mode (V1): Transfer TRC-10 asset by name to existing account")
        .database("account")
        .database("asset-issue")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("allowSameTokenName", 0)
        .dynamicProperty("token_name", LEGACY_TOKEN_NAME)
        .dynamicProperty("amount", amount)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset legacy happy path: success={}", result.isSuccess());

    // Reset to V2 mode
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(1);
  }

  @Test
  public void generateTransferAsset_legacyValidateFailAssetNotFound() throws Exception {
    // Switch to legacy mode (V1, name-based)
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(0);

    String nonExistentToken = "NonExistentLegacy";
    long amount = 1000;

    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(nonExistentToken)) // Non-existent name
        .setAmount(amount)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("legacy_validate_fail_asset_not_found")
        .caseCategory("validate_fail")
        .description("Legacy mode (V1): Fail when token name does not exist in asset-issue store")
        .database("account")
        .database("asset-issue")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("allowSameTokenName", 0)
        .expectedError("No asset!")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset legacy asset not found: validationError={}", result.getValidationError());

    // Reset to V2 mode
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(1);
  }

  @Test
  public void generateTransferAsset_legacyValidateFailInsufficientBalance() throws Exception {
    // Switch to legacy mode (V1, name-based)
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(0);

    String legacyTokenForInsufficient = "LegacyInsuf";

    // Seed V1 asset
    putAssetIssueV1(legacyTokenForInsufficient, OWNER_ADDRESS, 1_000_000_000_000L);

    // Create owner with small token balance in legacy mode
    String legacySmallOwner = generateAddress("legacy_small_own");
    AccountCapsule legacySmallAccount = putAccount(dbManager, legacySmallOwner, INITIAL_BALANCE, "legacy_small");
    // Add only 5 tokens using V1 method
    legacySmallAccount.addAsset(legacyTokenForInsufficient.getBytes(), 5);
    dbManager.getAccountStore().put(legacySmallAccount.getAddress().toByteArray(), legacySmallAccount);

    // Attempt to transfer more than available
    TransferAssetContract contract = TransferAssetContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(legacySmallOwner)))
        .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(RECEIVER_ADDRESS)))
        .setAssetName(ByteString.copyFromUtf8(legacyTokenForInsufficient)) // Name, not ID
        .setAmount(10) // More than the 5 tokens owned
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.TransferAssetContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRANSFER_ASSET_CONTRACT", 2)
        .caseName("legacy_validate_fail_insufficient_balance")
        .caseCategory("validate_fail")
        .description("Legacy mode (V1): Fail when owner has insufficient token balance")
        .database("account")
        .database("asset-issue")
        .database("dynamic-properties")
        .ownerAddress(legacySmallOwner)
        .dynamicProperty("allowSameTokenName", 0)
        .dynamicProperty("token_name", legacyTokenForInsufficient)
        .expectedError("assetBalance is not sufficient.")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("TransferAsset legacy insufficient balance: validationError={}", result.getValidationError());

    // Reset to V2 mode
    dbManager.getDynamicPropertiesStore().saveAllowSameTokenName(1);
  }
}
