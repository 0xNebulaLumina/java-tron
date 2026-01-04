package org.tron.core.conformance;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import java.io.File;
import java.util.ArrayList;
import java.util.List;
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
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.capsule.WitnessCapsule;
import org.tron.core.config.args.Args;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Key;
import org.tron.protos.Protocol.Permission;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.AccountContract.AccountPermissionUpdateContract;
import org.tron.protos.contract.AccountContract.SetAccountIdContract;

/**
 * Generates conformance test fixtures for Account contracts:
 * - SetAccountIdContract (19)
 * - AccountPermissionUpdateContract (46)
 *
 * <p>Run with: ./gradlew :framework:test --tests "AccountFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures
 */
public class AccountFixtureGeneratorTest extends BaseTest {

  private static final Logger log = LoggerFactory.getLogger(AccountFixtureGeneratorTest.class);
  private static final String OWNER_ADDRESS;
  private static final String WITNESS_ADDRESS;
  private static final long INITIAL_BALANCE = 300_000_000_000L; // 300,000 TRX for fees

  private FixtureGenerator generator;
  private File outputDir;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049abc";
    WITNESS_ADDRESS = Wallet.getAddressPreFixString() + "548794500882809695a8a687866e76d4271a1abc";
  }

  @Before
  public void setup() {
    initializeTestData();

    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    generator = new FixtureGenerator(dbManager, chainBaseManager);
    generator.setOutputDir(outputDir);

    log.info("Account Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  private void initializeTestData() {
    // Create owner account with sufficient balance for permission update fees
    AccountCapsule ownerAccount = new AccountCapsule(
        ByteString.copyFromUtf8("owner"),
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Create witness account - mark as witness for proper witness permission tests
    AccountCapsule witnessAccount = new AccountCapsule(
        ByteString.copyFromUtf8("witness"),
        ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)),
        AccountType.Normal,
        INITIAL_BALANCE);
    witnessAccount.setIsWitness(true);  // Critical: mark as witness for validation
    dbManager.getAccountStore().put(witnessAccount.getAddress().toByteArray(), witnessAccount);

    // Create witness entry in witness store
    WitnessCapsule witness = new WitnessCapsule(
        ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)),
        10_000_000L,
        "https://witness.network");
    dbManager.getWitnessStore().put(witness.getAddress().toByteArray(), witness);

    // Enable multi-sign feature
    dbManager.getDynamicPropertiesStore().saveAllowMultiSign(1);
    dbManager.getDynamicPropertiesStore().saveTotalSignNum(5);
    dbManager.getDynamicPropertiesStore().saveUpdateAccountPermissionFee(100_000_000L); // 100 TRX

    // Set timestamps
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderTimestamp(1000000);
    dbManager.getDynamicPropertiesStore().saveLatestBlockHeaderNumber(10);
  }

  // ==========================================================================
  // SetAccountId (19) Fixtures
  // ==========================================================================

  @Test
  public void generateSetAccountId_happyPath() throws Exception {
    String accountId = "my_account_id_123";

    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountId(ByteString.copyFromUtf8(accountId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Set account ID for an account without existing ID")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateSetAccountId_tooShort() throws Exception {
    String accountId = "short"; // Less than 8 characters

    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountId(ByteString.copyFromUtf8(accountId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_too_short")
        .caseCategory("validate_fail")
        .description("Fail when account ID is less than 8 characters")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("length")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId too short: validationError={}", result.getValidationError());
  }

  @Test
  public void generateSetAccountId_tooLong() throws Exception {
    String accountId = "this_account_id_is_way_too_long_for_the_system"; // More than 32 characters

    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountId(ByteString.copyFromUtf8(accountId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_too_long")
        .caseCategory("validate_fail")
        .description("Fail when account ID is more than 32 characters")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("length")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId too long: validationError={}", result.getValidationError());
  }

  @Test
  public void generateSetAccountId_invalidCharacters() throws Exception {
    String accountId = "invalid@#$%id"; // Contains invalid characters

    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountId(ByteString.copyFromUtf8(accountId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_invalid_chars")
        .caseCategory("validate_fail")
        .description("Fail when account ID contains invalid characters (only alphanumeric and underscore allowed)")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("character")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId invalid chars: validationError={}", result.getValidationError());
  }

  @Test
  public void generateSetAccountId_duplicateId() throws Exception {
    // First set an ID for owner
    String accountId = "existing_id_12";

    // Create another account that already has this ID
    String otherAddress = Wallet.getAddressPreFixString() + "1234567890123456789012345678901234567890";
    AccountCapsule otherAccount = new AccountCapsule(
        ByteString.copyFromUtf8("other"),
        ByteString.copyFrom(ByteArray.fromHexString(otherAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);
    otherAccount.setAccountId(accountId.getBytes());
    dbManager.getAccountStore().put(otherAccount.getAddress().toByteArray(), otherAccount);

    // Store in accountid-index
    dbManager.getAccountIdIndexStore().put(otherAccount);

    // Now try to set the same ID for owner
    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountId(ByteString.copyFromUtf8(accountId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_duplicate")
        .caseCategory("validate_fail")
        .description("Fail when account ID is already taken by another account")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId duplicate: validationError={}", result.getValidationError());
  }

  @Test
  public void generateSetAccountId_alreadyHasId() throws Exception {
    // Set an ID for owner first
    String existingId = "existing_id_1";
    AccountCapsule ownerAccount = dbManager.getAccountStore()
        .get(ByteArray.fromHexString(OWNER_ADDRESS));
    ownerAccount.setAccountId(existingId.getBytes());
    dbManager.getAccountStore().put(ownerAccount.getAddress().toByteArray(), ownerAccount);

    // Try to set a new ID
    String newId = "new_account_id";
    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountId(ByteString.copyFromUtf8(newId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_already_has_id")
        .caseCategory("validate_fail")
        .description("Fail when account already has an ID set")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("already")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId already has ID: validationError={}", result.getValidationError());
  }

  @Test
  public void generateSetAccountId_nonexistentOwner() throws Exception {
    String nonexistentAddress = Wallet.getAddressPreFixString() + "9999999999999999999999999999999999999999";

    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonexistentAddress)))
        .setAccountId(ByteString.copyFromUtf8("valid_id_123"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_owner_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(nonexistentAddress)
        .expectedError("exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId nonexistent owner: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // SetAccountId - Edge Cases: Invalid Owner Address
  // --------------------------------------------------------------------------

  @Test
  public void generateSetAccountId_invalidOwnerAddressEmpty() throws Exception {
    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setAccountId(ByteString.copyFromUtf8("valid_id_123"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_owner_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty bytes")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress("")
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId empty owner address: validationError={}", result.getValidationError());
  }

  @Test
  public void generateSetAccountId_invalidOwnerAddressWrongLength() throws Exception {
    // 10-byte address (wrong length - should be 21 bytes)
    byte[] wrongLengthAddress = new byte[10];
    for (int i = 0; i < 10; i++) {
      wrongLengthAddress[i] = (byte) (0x41 + i);
    }

    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(wrongLengthAddress))
        .setAccountId(ByteString.copyFromUtf8("valid_id_123"))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_owner_address_wrong_length")
        .caseCategory("validate_fail")
        .description("Fail when owner address has wrong length (not 21 bytes)")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(ByteArray.toHexString(wrongLengthAddress))
        .expectedError("Invalid ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId wrong length owner: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // SetAccountId - Edge Cases: Invalid AccountId (unreadable bytes)
  // --------------------------------------------------------------------------

  @Test
  public void generateSetAccountId_accountIdContainsSpace() throws Exception {
    // Account ID with space (0x20) - fails validReadableBytes check (< 0x21)
    String accountIdWithSpace = "ab  cdefgh";  // contains space

    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountId(ByteString.copyFromUtf8(accountIdWithSpace))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_account_id_contains_space")
        .caseCategory("validate_fail")
        .description("Fail when account ID contains space character (0x20 < 0x21)")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid accountId")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId with space: validationError={}", result.getValidationError());
  }

  @Test
  public void generateSetAccountId_accountIdContainsControlChar() throws Exception {
    // Account ID with newline control character (0x0A) - fails validReadableBytes check
    byte[] idWithControlChar = "abcde\n1234".getBytes();

    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountId(ByteString.copyFrom(idWithControlChar))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_account_id_control_char")
        .caseCategory("validate_fail")
        .description("Fail when account ID contains control character (newline)")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid accountId")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId with control char: validationError={}", result.getValidationError());
  }

  @Test
  public void generateSetAccountId_accountIdNonAscii() throws Exception {
    // Account ID with non-ASCII byte (> 0x7E) - fails validReadableBytes check
    byte[] idWithNonAscii = new byte[10];
    for (int i = 0; i < 9; i++) {
      idWithNonAscii[i] = (byte) ('a' + i);  // 'a' to 'i'
    }
    idWithNonAscii[9] = (byte) 0x80;  // non-ASCII byte (128)

    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountId(ByteString.copyFrom(idWithNonAscii))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_account_id_non_ascii")
        .caseCategory("validate_fail")
        .description("Fail when account ID contains non-ASCII byte (> 0x7E)")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid accountId")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId non-ASCII: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // SetAccountId - Boundary Cases (min/max length)
  // --------------------------------------------------------------------------

  @Test
  public void generateSetAccountId_boundaryMinLength8() throws Exception {
    // Exactly 8 bytes - minimum valid length
    String accountId = "abcdefgh";  // exactly 8 printable ASCII chars

    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountId(ByteString.copyFromUtf8(accountId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("happy_path_min_len_8")
        .caseCategory("edge")
        .description("Success with account ID at minimum length (exactly 8 bytes)")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId min length 8: success={}", result.isSuccess());
  }

  @Test
  public void generateSetAccountId_boundaryMaxLength32() throws Exception {
    // Exactly 32 bytes - maximum valid length
    String accountId = "abcdefghijklmnopqrstuvwxyz123456";  // exactly 32 chars

    // Need fresh account without ID for this test
    String freshAddress = Wallet.getAddressPreFixString() + "7777777777777777777777777777777777777777";
    AccountCapsule freshAccount = new AccountCapsule(
        ByteString.copyFromUtf8("fresh32"),
        ByteString.copyFrom(ByteArray.fromHexString(freshAddress)),
        AccountType.Normal,
        INITIAL_BALANCE);
    dbManager.getAccountStore().put(freshAccount.getAddress().toByteArray(), freshAccount);

    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(freshAddress)))
        .setAccountId(ByteString.copyFromUtf8(accountId))
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("happy_path_max_len_32")
        .caseCategory("edge")
        .description("Success with account ID at maximum length (exactly 32 bytes)")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(freshAddress)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId max length 32: success={}", result.isSuccess());
  }

  // --------------------------------------------------------------------------
  // SetAccountId - Explicit empty accountId
  // --------------------------------------------------------------------------

  @Test
  public void generateSetAccountId_accountIdEmpty() throws Exception {
    SetAccountIdContract contract = SetAccountIdContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setAccountId(ByteString.EMPTY)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.SetAccountIdContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("SET_ACCOUNT_ID_CONTRACT", 19)
        .caseName("validate_fail_account_id_empty")
        .caseCategory("validate_fail")
        .description("Fail when account ID is empty (0 bytes)")
        .database("account")
        .database("accountid-index")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("Invalid accountId")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("SetAccountId empty: validationError={}", result.getValidationError());
  }

  // ==========================================================================
  // AccountPermissionUpdate (46) Fixtures
  // ==========================================================================

  @Test
  public void generateAccountPermissionUpdate_happyPath() throws Exception {
    // Build owner permission
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    // Build active permission
    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32])) // All operations disabled
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Update account permissions with valid owner and active permissions")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .dynamicProperty("UPDATE_ACCOUNT_PERMISSION_FEE", 100_000_000L)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate happy path: success={}", result.isSuccess());
  }

  @Test
  public void generateAccountPermissionUpdate_multiSign() throws Exception {
    // Create second key address
    String secondKeyAddress = Wallet.getAddressPreFixString() + "2222222222222222222222222222222222222222";

    // Build owner permission with 2-of-2 multi-sig
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(2)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(secondKeyAddress)))
            .setWeight(1)
            .build())
        .build();

    // Build active permission
    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("happy_path_multisig")
        .caseCategory("happy")
        .description("Set up 2-of-2 multi-signature for owner permission")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate multi-sig: success={}", result.isSuccess());
  }

  @Test
  public void generateAccountPermissionUpdate_witnessPermission() throws Exception {
    // Build owner permission
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    // Build witness permission
    Permission witnessPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Witness)
        .setId(1)
        .setPermissionName("witness")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    // Build active permission
    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setOwner(ownerPermission)
        .setWitness(witnessPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("happy_path_witness")
        .caseCategory("happy")
        .description("Update permissions including witness permission for a witness account")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate witness: success={}", result.isSuccess());
  }

  @Test
  public void generateAccountPermissionUpdate_multiSignDisabled() throws Exception {
    // Disable multi-sign
    dbManager.getDynamicPropertiesStore().saveAllowMultiSign(0);

    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_multisign_disabled")
        .caseCategory("validate_fail")
        .description("Fail when multi-sign feature is not enabled")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("multi")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate multi-sign disabled: validationError={}", result.getValidationError());

    // Re-enable multi-sign for other tests
    dbManager.getDynamicPropertiesStore().saveAllowMultiSign(1);
  }

  @Test
  public void generateAccountPermissionUpdate_insufficientBalance() throws Exception {
    // Create account with low balance
    String lowBalanceAddress = Wallet.getAddressPreFixString() + "3333333333333333333333333333333333333333";
    AccountCapsule lowBalanceAccount = new AccountCapsule(
        ByteString.copyFromUtf8("low_balance"),
        ByteString.copyFrom(ByteArray.fromHexString(lowBalanceAddress)),
        AccountType.Normal,
        1_000_000L); // Only 1 TRX, fee is 100 TRX
    dbManager.getAccountStore().put(lowBalanceAccount.getAddress().toByteArray(), lowBalanceAccount);

    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(lowBalanceAddress)))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(lowBalanceAddress)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(lowBalanceAddress)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_insufficient_balance")
        .caseCategory("validate_fail")
        .description("Fail when account has insufficient balance for permission update fee")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(lowBalanceAddress)
        .expectedError("balance")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate insufficient balance: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_tooManyKeys() throws Exception {
    // Set total sign num to 2 for this test
    dbManager.getDynamicPropertiesStore().saveTotalSignNum(2);

    // Build permission with 3 keys (exceeds limit)
    List<Key> keys = new ArrayList<>();
    for (int i = 0; i < 3; i++) {
      String keyAddress = Wallet.getAddressPreFixString()
          + String.format("%040d", 1000 + i);
      keys.add(Key.newBuilder()
          .setAddress(ByteString.copyFrom(ByteArray.fromHexString(keyAddress)))
          .setWeight(1)
          .build());
    }

    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(2)
        .addAllKeys(keys)
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_too_many_keys")
        .caseCategory("validate_fail")
        .description("Fail when permission has more keys than allowed by TOTAL_SIGN_NUM")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("key")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate too many keys: validationError={}", result.getValidationError());

    // Restore total sign num
    dbManager.getDynamicPropertiesStore().saveTotalSignNum(5);
  }

  @Test
  public void generateAccountPermissionUpdate_duplicateKeys() throws Exception {
    // Build permission with duplicate key addresses
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(2)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS))) // Duplicate
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_duplicate_keys")
        .caseCategory("validate_fail")
        .description("Fail when permission contains duplicate key addresses")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("duplicate")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate duplicate keys: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_thresholdTooHigh() throws Exception {
    // Build permission with threshold higher than total weight
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(10) // Threshold 10, but only 1 key with weight 1
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_threshold_too_high")
        .caseCategory("validate_fail")
        .description("Fail when permission threshold is higher than sum of key weights")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("threshold")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate threshold too high: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_witnessNotWitness() throws Exception {
    // Try to set witness permission on a non-witness account
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    Permission witnessPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Witness)
        .setId(1)
        .setPermissionName("witness")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .setWitness(witnessPermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_witness_not_sr")
        .caseCategory("validate_fail")
        .description("Fail when trying to set witness permission on non-witness account")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("witness")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate witness not SR: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountPermissionUpdate - Edge Cases: Owner Address Validation
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountPermissionUpdate_invalidOwnerAddressEmpty() throws Exception {
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_owner_address_empty")
        .caseCategory("validate_fail")
        .description("Fail when owner address is empty")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress("")
        .expectedError("invalidate ownerAddress")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate empty owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_ownerAccountNotExist() throws Exception {
    String nonexistentAddress = Wallet.getAddressPreFixString() + "8888888888888888888888888888888888888888";

    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(nonexistentAddress)))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(nonexistentAddress)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(nonexistentAddress)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_owner_not_exist")
        .caseCategory("validate_fail")
        .description("Fail when owner account does not exist")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(nonexistentAddress)
        .expectedError("ownerAddress account does not exist")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate nonexistent owner: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountPermissionUpdate - Edge Cases: Missing Required Fields
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountPermissionUpdate_ownerPermissionMissing() throws Exception {
    // Build contract without owner permission but with active permission
    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        // No .setOwner() call
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_owner_permission_missing")
        .caseCategory("validate_fail")
        .description("Fail when owner permission is not provided")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("owner permission is missed")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate owner missing: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_activePermissionMissing() throws Exception {
    // Build contract with owner permission but no active permissions
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        // No .addActives() call
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_active_permission_missing")
        .caseCategory("validate_fail")
        .description("Fail when no active permissions are provided")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("active permission is missed")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate active missing: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_activePermissionTooMany() throws Exception {
    // Build owner permission
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    // Build 9 active permissions (> 8 limit)
    List<Permission> activePermissions = new ArrayList<>();
    for (int i = 0; i < 9; i++) {
      Permission activePermission = Permission.newBuilder()
          .setType(Permission.PermissionType.Active)
          .setId(2 + i)
          .setPermissionName("active" + i)
          .setThreshold(1)
          .setOperations(ByteString.copyFrom(new byte[32]))
          .addKeys(Key.newBuilder()
              .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
              .setWeight(1)
              .build())
          .build();
      activePermissions.add(activePermission);
    }

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addAllActives(activePermissions)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_active_too_many")
        .caseCategory("validate_fail")
        .description("Fail when more than 8 active permissions are provided")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("active permission is too many")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate too many actives: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_witnessPermissionMissing() throws Exception {
    // For witness account, witness permission is required
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    // Omit witness permission for a witness account
    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        // No .setWitness() - missing for witness account
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_witness_permission_missing")
        .caseCategory("validate_fail")
        .description("Fail when witness account lacks witness permission")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("witness permission is missed")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate witness perm missing: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountPermissionUpdate - Edge Cases: Wrong Permission Types
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountPermissionUpdate_ownerPermissionTypeWrong() throws Exception {
    // Owner permission with wrong type (Active instead of Owner)
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)  // Wrong type!
        .setPermissionName("owner")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_owner_type_wrong")
        .caseCategory("validate_fail")
        .description("Fail when owner permission has wrong type (Active instead of Owner)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("owner permission type is error")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate owner type wrong: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_activePermissionTypeWrong() throws Exception {
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    // Active permission with wrong type (Owner instead of Active)
    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)  // Wrong type!
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_active_type_wrong")
        .caseCategory("validate_fail")
        .description("Fail when active permission has wrong type (Owner instead of Active)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("active permission type is error")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate active type wrong: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_witnessPermissionTypeWrong() throws Exception {
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    // Witness permission with wrong type (Active instead of Witness)
    Permission witnessPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)  // Wrong type!
        .setId(1)
        .setPermissionName("witness")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setOwner(ownerPermission)
        .setWitness(witnessPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_witness_type_wrong")
        .caseCategory("validate_fail")
        .description("Fail when witness permission has wrong type (Active instead of Witness)")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("witness permission type is error")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate witness type wrong: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountPermissionUpdate - checkPermission() Edge Cases
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountPermissionUpdate_keysCountZero() throws Exception {
    // Owner permission with zero keys
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        // No keys added
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_keys_count_zero")
        .caseCategory("validate_fail")
        .description("Fail when permission has zero keys")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("key's count should be greater than 0")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate keys zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_witnessKeysCountNotOne() throws Exception {
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    // Witness permission with 2 keys (should be exactly 1)
    String secondKeyAddress = Wallet.getAddressPreFixString() + "4444444444444444444444444444444444444444";
    Permission witnessPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Witness)
        .setId(1)
        .setPermissionName("witness")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setWeight(1)
            .build())
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(secondKeyAddress)))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(WITNESS_ADDRESS)))
        .setOwner(ownerPermission)
        .setWitness(witnessPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_witness_keys_not_one")
        .caseCategory("validate_fail")
        .description("Fail when witness permission has more than 1 key")
        .database("account")
        .database("witness")
        .database("dynamic-properties")
        .ownerAddress(WITNESS_ADDRESS)
        .expectedError("Witness permission's key count should be 1")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate witness keys not 1: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_thresholdZero() throws Exception {
    // Owner permission with threshold = 0
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(0)  // Invalid: must be > 0
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_threshold_zero")
        .caseCategory("validate_fail")
        .description("Fail when permission threshold is 0")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("permission's threshold should be greater than 0")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate threshold zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_permissionNameTooLong() throws Exception {
    // Permission name > 32 characters
    String longName = "this_permission_name_is_way_too_long";  // 36 chars

    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName(longName)
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_name_too_long")
        .caseCategory("validate_fail")
        .description("Fail when permission name exceeds 32 characters")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("permission's name is too long")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate name too long: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_parentIdNotOwner() throws Exception {
    // Permission with parentId != 0
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .setParentId(1)  // Invalid: must be 0
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_parent_id_not_owner")
        .caseCategory("validate_fail")
        .description("Fail when permission parentId is not 0 (owner)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("permission's parent should be owner")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate parent not owner: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_keyAddressInvalid() throws Exception {
    // Key with invalid address (10 bytes instead of 21)
    byte[] invalidKeyAddress = new byte[10];
    for (int i = 0; i < 10; i++) {
      invalidKeyAddress[i] = (byte) (0x41 + i);
    }

    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(invalidKeyAddress))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_key_address_invalid")
        .caseCategory("validate_fail")
        .description("Fail when key address is invalid (wrong length)")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("key is not a validate address")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate key address invalid: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_keyWeightZero() throws Exception {
    // Key with weight = 0
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(0)  // Invalid: must be > 0
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_key_weight_zero")
        .caseCategory("validate_fail")
        .description("Fail when key weight is 0")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("key's weight should be greater than 0")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate key weight zero: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_nonActiveHasOperations() throws Exception {
    // Owner permission with operations (should be empty for non-Active)
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))  // Invalid for Owner permission
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[32]))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_non_active_has_operations")
        .caseCategory("validate_fail")
        .description("Fail when non-Active permission has operations set")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("permission needn't operations")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate non-active ops: validationError={}", result.getValidationError());
  }

  // --------------------------------------------------------------------------
  // AccountPermissionUpdate - Active Operations Validation
  // --------------------------------------------------------------------------

  @Test
  public void generateAccountPermissionUpdate_activeOperationsEmpty() throws Exception {
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    // Active permission with empty operations
    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.EMPTY)  // Invalid: must be 32 bytes
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_active_operations_empty")
        .caseCategory("validate_fail")
        .description("Fail when active permission operations is empty")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("operations size must 32")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate ops empty: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_activeOperationsWrongSize() throws Exception {
    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    // Active permission with 31-byte operations (wrong size)
    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(new byte[31]))  // Invalid: must be exactly 32 bytes
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_active_operations_wrong_size")
        .caseCategory("validate_fail")
        .description("Fail when active permission operations is not 32 bytes")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("operations size must 32")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate ops wrong size: validationError={}", result.getValidationError());
  }

  @Test
  public void generateAccountPermissionUpdate_activeOperationsInvalidContractType() throws Exception {
    // Clear available contract types to make all bits invalid
    byte[] noContractTypes = new byte[32];  // all zeros = no valid contract types
    dbManager.getDynamicPropertiesStore().saveAvailableContractType(noContractTypes);

    Permission ownerPermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Owner)
        .setPermissionName("owner")
        .setThreshold(1)
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    // Active permission with bit 0 set (but contract type 0 is not available)
    byte[] operations = new byte[32];
    operations[0] = 0x01;  // Set bit 0

    Permission activePermission = Permission.newBuilder()
        .setType(Permission.PermissionType.Active)
        .setId(2)
        .setPermissionName("active")
        .setThreshold(1)
        .setOperations(ByteString.copyFrom(operations))
        .addKeys(Key.newBuilder()
            .setAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
            .setWeight(1)
            .build())
        .build();

    AccountPermissionUpdateContract contract = AccountPermissionUpdateContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setOwner(ownerPermission)
        .addActives(activePermission)
        .build();

    TransactionCapsule trxCap = createTransaction(
        Transaction.Contract.ContractType.AccountPermissionUpdateContract, contract);

    BlockCapsule blockCap = createBlockContext();

    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("ACCOUNT_PERMISSION_UPDATE_CONTRACT", 46)
        .caseName("validate_fail_active_invalid_contract_type")
        .caseCategory("validate_fail")
        .description("Fail when active permission enables a contract type not in AVAILABLE_CONTRACT_TYPE")
        .database("account")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError("isn't a validate ContractType")
        .build();

    FixtureGenerator.FixtureResult result = generator.generate(trxCap, blockCap, metadata);
    log.info("AccountPermissionUpdate invalid contract type: validationError={}", result.getValidationError());

    // Restore available contract types (enable all)
    byte[] allContractTypes = new byte[32];
    for (int i = 0; i < 32; i++) {
      allContractTypes[i] = (byte) 0xFF;
    }
    dbManager.getDynamicPropertiesStore().saveAvailableContractType(allContractTypes);
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
}
