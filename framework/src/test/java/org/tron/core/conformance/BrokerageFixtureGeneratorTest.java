package org.tron.core.conformance;

import static org.tron.core.conformance.ConformanceFixtureTestSupport.*;

import com.google.protobuf.Any;
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
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.StorageContract.UpdateBrokerageContract;

/**
 * Generates conformance test fixtures for UpdateBrokerageContract (49).
 *
 * <p>Run with: ./gradlew :framework:test --tests "BrokerageFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures
 */
public class BrokerageFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(BrokerageFixtureGeneratorTest.class);
  private static final String WITNESS_ADDRESS;
  private static final String NON_WITNESS_ADDRESS;
  private static final String WITNESS_ONLY_ADDRESS; // has witness entry but no account
  private static final long INITIAL_BALANCE = 100_000_000_000L;

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049abc";
    NON_WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "1111111111111111111111111111111111111111";
    // Address that will have witness entry but no account entry (for "Account does not exist" test)
    WITNESS_ONLY_ADDRESS = Wallet.getAddressPreFixString() + "2222222222222222222222222222222222222222";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("Brokerage Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Initialize common dynamic properties with deterministic timestamps
    initCommonDynamicPropsV1(dbManager, 10, DEFAULT_BLOCK_TIMESTAMP);

    // Create witness account and witness entry
    putAccount(dbManager, WITNESS_ADDRESS, INITIAL_BALANCE, "witness");
    putWitness(dbManager, WITNESS_ADDRESS, "https://witness.network", 10_000_000L);

    // Create non-witness account (no witness entry)
    putAccount(dbManager, NON_WITNESS_ADDRESS, INITIAL_BALANCE, "non_witness");

    // Create witness entry WITHOUT corresponding account (for "Account does not exist" test)
    // This tests the branch where witness exists but account doesn't
    putWitness(dbManager, WITNESS_ONLY_ADDRESS, "https://witness-only.network", 10_000_000L);
    // Intentionally NOT creating an account for WITNESS_ONLY_ADDRESS

    // Enable change delegation feature (required for UpdateBrokerage)
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(1);
  }

  // ==========================================================================
  // UpdateBrokerageContract (49) Fixtures
  // ==========================================================================

  @Test
  public void generateUpdateBrokerage_happyPath() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(20) // 20% brokerage
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Update brokerage rate for a witness to 20%")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .dynamicProperty("brokerage", 20)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateBrokerage_zeroPercent() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(0) // 0% - witness takes no commission
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("happy_path_zero")
        .caseCategory("happy")
        .description("Set brokerage to 0% (witness takes no commission)")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage zero: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateBrokerage_100Percent() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(100) // 100% - witness takes all commission
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("happy_path_100")
        .caseCategory("happy")
        .description("Set brokerage to 100% (witness takes all commission)")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage 100: success={}", result.isSuccess());
  }

  @Test
  public void generateUpdateBrokerage_notWitness() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(NON_WITNESS_ADDRESS)))
        .setBrokerage(20)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_not_witness")
        .caseCategory("validate_fail")
        .description("Fail when owner is not a witness/SR")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(NON_WITNESS_ADDRESS)
        .expectedError("witness")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage not witness: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateBrokerage_negativePercent() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(-1) // Invalid: negative
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_negative")
        .caseCategory("validate_fail")
        .description("Fail when brokerage is negative")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("brokerage")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage negative: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateBrokerage_over100Percent() throws Exception {
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(101) // Invalid: > 100
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_over_100")
        .caseCategory("validate_fail")
        .description("Fail when brokerage is greater than 100")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("brokerage")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage over 100: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateBrokerage_changeDelegationDisabled() throws Exception {
    // Disable change delegation
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(0);

    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setBrokerage(20)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_disabled")
        .caseCategory("validate_fail")
        .description("Fail when change delegation feature is not enabled")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("delegation")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage disabled: validationError={}", result.getValidationError());

    // Re-enable for other tests
    dbManager.getDynamicPropertiesStore().saveChangeDelegation(1);
  }

  @Test
  public void generateUpdateBrokerage_witnessNotExist() throws Exception {
    // This address has neither witness nor account entry - validation fails at witness check first
    String nonExistentAddress = Wallet.getAddressPreFixString() + "9999999999999999999999999999999999999999";

    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonExistentAddress)))
        .setBrokerage(20)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_witness_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner is not a registered witness (witness check precedes account check)")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(nonExistentAddress)
        .expectedError("Not existed witness")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage witness not exist: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Phase 1: Invalid Owner Address Fixtures
  // ==========================================================================

  @Test
  public void generateUpdateBrokerage_ownerAddressEmpty() throws Exception {
    // Empty owner address fails DecodeUtil.addressValid check
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setBrokerage(20)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_owner_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner_address is empty (0 bytes)")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress("")
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage owner empty: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateBrokerage_ownerAddressWrongLength() throws Exception {
    // Wrong length (20 bytes instead of required 21 bytes) fails addressValid check
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(new byte[20]))
        .setBrokerage(20)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_owner_address_wrong_length")
        .caseCategory("validate_fail")
        .description("Fail when owner_address has wrong length (20 bytes instead of 21)")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(ByteArray.toHexString(new byte[20]))
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage wrong length: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateBrokerage_ownerAddressWrongPrefix() throws Exception {
    // Wrong prefix (0xa0 testnet prefix instead of 0x41 mainnet) fails addressValid check
    // Construct 21-byte address with wrong prefix
    byte[] wrongPrefixAddress = new byte[21];
    wrongPrefixAddress[0] = (byte) 0xa0; // testnet prefix instead of 0x41 mainnet
    for (int i = 1; i < 21; i++) {
      wrongPrefixAddress[i] = (byte) i;
    }

    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(wrongPrefixAddress))
        .setBrokerage(20)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_owner_address_wrong_prefix")
        .caseCategory("validate_fail")
        .description("Fail when owner_address has wrong network prefix (0xa0 instead of 0x41)")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(ByteArray.toHexString(wrongPrefixAddress))
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage wrong prefix: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Phase 2: Account Missing (Witness Exists) Fixture
  // ==========================================================================

  @Test
  public void generateUpdateBrokerage_accountMissingWitnessExists() throws Exception {
    // WITNESS_ONLY_ADDRESS has witness entry but no account entry
    // This reaches the "Account does not exist" branch (after witness check passes)
    UpdateBrokerageContract contract = UpdateBrokerageContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ONLY_ADDRESS)))
        .setBrokerage(20)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.UpdateBrokerageContract, contract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_account_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when witness exists but account does not exist")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ONLY_ADDRESS)
        .expectedError("Account does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage account missing (witness exists): validationError={}",
        result.getValidationError());
  }

  // ==========================================================================
  // Phase 4 (Optional): Contract Encoding / Type Mismatch Fixtures
  // ==========================================================================

  @Test
  public void generateUpdateBrokerage_contractParameterWrongType() throws Exception {
    // Contract type is UpdateBrokerageContract but parameter packs a different message
    // This covers the !any.is(UpdateBrokerageContract.class) branch
    // We pack a TransferContract into an UpdateBrokerageContract transaction
    org.tron.protos.contract.BalanceContract.TransferContract wrongContract =
        org.tron.protos.contract.BalanceContract.TransferContract.newBuilder()
            .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(NON_WITNESS_ADDRESS)))
            .setAmount(1000)
            .build();

    TransactionCapsule trxCap = createTransactionWithMismatchedType(
        Transaction.Contract.ContractType.UpdateBrokerageContract, wrongContract);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_contract_parameter_wrong_type")
        .caseCategory("validate_fail")
        .description("Fail when contract type is UpdateBrokerageContract but parameter is a different message type")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("contract type error")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage wrong type: validationError={}", result.getValidationError());
  }

  @Test
  public void generateUpdateBrokerage_invalidProtobufBytes() throws Exception {
    // Manually build Any with correct type_url but invalid/corrupted value bytes
    // This covers the InvalidProtocolBufferException catch block in validate()
    Any invalidAny = Any.newBuilder()
        .setTypeUrl("type.googleapis.com/" + UpdateBrokerageContract.getDescriptor().getFullName())
        .setValue(ByteString.copyFrom(new byte[]{0x08, (byte) 0xFF, (byte) 0xFF, (byte) 0xFF})) // invalid varint
        .build();

    TransactionCapsule trxCap = createTransactionWithRawAny(
        Transaction.Contract.ContractType.UpdateBrokerageContract, invalidAny);

    BlockCapsule blockCap = createBlockContext(dbManager, WITNESS_ADDRESS);

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("UPDATE_BROKERAGE_CONTRACT", 49)
        .caseName("validate_fail_invalid_protobuf_bytes")
        .caseCategory("validate_fail")
        .description("Fail when contract parameter contains invalid/corrupted protobuf bytes")
        .database("account")
        .database("witness")
        .database("delegation")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("Protocol") // InvalidProtocolBufferException message contains "Protocol"
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("UpdateBrokerage invalid protobuf: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // Helper Methods (specialized - standard createTransaction/createBlockContext
  // are inherited from ConformanceFixtureTestSupport via static import)
  // ==========================================================================

  /**
   * Creates a transaction with mismatched contract type (type says X but parameter is Y).
   * This is used to test the !any.is(ExpectedContract.class) validation branch.
   */
  private TransactionCapsule createTransactionWithMismatchedType(
      Transaction.Contract.ContractType declaredType,
      com.google.protobuf.Message actualContract) {
    Transaction.Contract protoContract = Transaction.Contract.newBuilder()
        .setType(declaredType)
        .setParameter(Any.pack(actualContract))
        .build();

    Transaction transaction = Transaction.newBuilder()
        .setRawData(Transaction.raw.newBuilder()
            .addContract(protoContract)
            .setTimestamp(DEFAULT_TX_TIMESTAMP)
            .setExpiration(DEFAULT_TX_EXPIRATION)
            .build())
        .build();

    return new TransactionCapsule(transaction);
  }

  /**
   * Creates a transaction with a pre-built Any parameter (for testing invalid protobuf bytes).
   * This allows injecting malformed protobuf data to test error handling.
   */
  private TransactionCapsule createTransactionWithRawAny(
      Transaction.Contract.ContractType declaredType,
      Any rawAny) {
    Transaction.Contract protoContract = Transaction.Contract.newBuilder()
        .setType(declaredType)
        .setParameter(rawAny)
        .build();

    Transaction transaction = Transaction.newBuilder()
        .setRawData(Transaction.raw.newBuilder()
            .addContract(protoContract)
            .setTimestamp(DEFAULT_TX_TIMESTAMP)
            .setExpiration(DEFAULT_TX_EXPIRATION)
            .build())
        .build();

    return new TransactionCapsule(transaction);
  }
}
