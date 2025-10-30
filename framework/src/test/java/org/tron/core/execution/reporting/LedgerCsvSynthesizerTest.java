package org.tron.core.execution.reporting;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.when;

import com.google.protobuf.ByteString;
import java.util.ArrayList;
import java.util.List;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.tron.common.utils.ByteArray;
import org.tron.core.ChainBaseManager;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.db.TransactionTrace;
import org.tron.core.execution.spi.ExecutionProgramResult;
import org.tron.core.execution.spi.ExecutionSPI;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.core.execution.spi.ExecutionSPI.Trc10LedgerChange;
import org.tron.core.execution.spi.ExecutionSPI.Trc10Op;
import org.tron.core.store.AccountStore;
import org.tron.core.store.DynamicPropertiesStore;
import org.tron.core.store.StoreFactory;
import org.tron.protos.Protocol.Account;

/**
 * Unit tests for LedgerCsvSynthesizer with mocked stores.
 * Tests TRC-10 state change synthesis for AssetIssue and ParticipateAssetIssue operations.
 */
public class LedgerCsvSynthesizerTest {

  private static final long ASSET_ISSUE_FEE = 1024_000000L; // 1024 TRX in SUN
  private static final long OWNER_INITIAL_BALANCE = 10000_000000L; // 10000 TRX
  private static final long ISSUER_INITIAL_BALANCE = 5000_000000L; // 5000 TRX
  private static final long BLACKHOLE_INITIAL_BALANCE = 1000000_000000L; // 1M TRX

  // Mock objects
  private TransactionTrace mockTrace;
  private TransactionContext mockContext;
  private StoreFactory mockStoreFactory;
  private ChainBaseManager mockChainBaseManager;
  private DynamicPropertiesStore mockDynamicStore;
  private AccountStore mockAccountStore;
  private ExecutionProgramResult mockExecResult;

  // Test addresses
  private byte[] ownerAddress;
  private byte[] issuerAddress;
  private byte[] blackholeAddress;

  @Before
  public void setUp() {
    // Enable synthesis for tests
    System.setProperty(LedgerCsvSynthesizer.PROPERTY_INCLUDE_TRC10, "true");
    System.setProperty(LedgerCsvSynthesizer.PROPERTY_STRICT_MODE, "false");

    // Initialize test addresses
    ownerAddress = ByteArray.fromHexString("41d1ef8673f916debb7e2515a8f3ecaf2611034aa1");
    issuerAddress = ByteArray.fromHexString("4177944d19c052b73ee2286823aa83f8138cb70320");
    blackholeAddress = ByteArray.fromHexString("410000000000000000000000000000000000000000");

    // Create mocks
    mockTrace = mock(TransactionTrace.class);
    mockContext = mock(TransactionContext.class);
    mockStoreFactory = mock(StoreFactory.class);
    mockChainBaseManager = mock(ChainBaseManager.class);
    mockDynamicStore = mock(DynamicPropertiesStore.class);
    mockAccountStore = mock(AccountStore.class);
    mockExecResult = mock(ExecutionProgramResult.class);

    // Wire up mock chain
    when(mockTrace.getTransactionContext()).thenReturn(mockContext);
    when(mockContext.getStoreFactory()).thenReturn(mockStoreFactory);
    when(mockStoreFactory.getChainBaseManager()).thenReturn(mockChainBaseManager);
    when(mockChainBaseManager.getDynamicPropertiesStore()).thenReturn(mockDynamicStore);
    when(mockChainBaseManager.getAccountStore()).thenReturn(mockAccountStore);

    // Configure dynamic store defaults
    when(mockDynamicStore.getAssetIssueFee()).thenReturn(ASSET_ISSUE_FEE);
    when(mockDynamicStore.supportBlackHoleOptimization()).thenReturn(false); // Use blackhole by default
  }

  @After
  public void tearDown() {
    // Clean up system properties
    System.clearProperty(LedgerCsvSynthesizer.PROPERTY_INCLUDE_TRC10);
    System.clearProperty(LedgerCsvSynthesizer.PROPERTY_STRICT_MODE);
  }

  @Test
  public void testIsEnabled() {
    System.setProperty(LedgerCsvSynthesizer.PROPERTY_INCLUDE_TRC10, "true");
    assertTrue("Synthesis should be enabled", LedgerCsvSynthesizer.isEnabled());

    System.setProperty(LedgerCsvSynthesizer.PROPERTY_INCLUDE_TRC10, "false");
    assertTrue("Synthesis should be disabled", !LedgerCsvSynthesizer.isEnabled());
  }

  @Test
  public void testSynthesizeDisabled() {
    System.setProperty(LedgerCsvSynthesizer.PROPERTY_INCLUDE_TRC10, "false");

    List<Trc10LedgerChange> trc10Changes = createAssetIssueChange();
    when(mockExecResult.getTrc10Changes()).thenReturn(trc10Changes);

    List<StateChange> result = LedgerCsvSynthesizer.synthesize(mockExecResult, mockTrace);

    assertNotNull("Result should not be null", result);
    assertEquals("Should return empty list when disabled", 0, result.size());
  }

  @Test
  public void testSynthesizeAssetIssueWithBlackhole() {
    // Setup: Owner has 10000 TRX, blackhole has 1M TRX
    // After issue: Owner has 10000-1024 = 8976 TRX, blackhole has 1M+1024 TRX
    AccountCapsule ownerAccount = createAccount(OWNER_INITIAL_BALANCE - ASSET_ISSUE_FEE, ownerAddress);
    AccountCapsule blackholeAccount = createAccount(BLACKHOLE_INITIAL_BALANCE + ASSET_ISSUE_FEE, blackholeAddress);

    when(mockAccountStore.get(ownerAddress)).thenReturn(ownerAccount);
    when(mockAccountStore.get(blackholeAddress)).thenReturn(blackholeAccount); // Mock by address too
    when(mockAccountStore.getBlackhole()).thenReturn(blackholeAccount);
    when(mockDynamicStore.supportBlackHoleOptimization()).thenReturn(false); // Use blackhole

    List<Trc10LedgerChange> trc10Changes = createAssetIssueChange();
    when(mockExecResult.getTrc10Changes()).thenReturn(trc10Changes);

    List<StateChange> result = LedgerCsvSynthesizer.synthesize(mockExecResult, mockTrace);

    assertNotNull("Result should not be null", result);
    assertEquals("Should have 2 state changes (owner + blackhole)", 2, result.size());

    // Verify owner change
    StateChange ownerChange = findChangeByAddress(result, ownerAddress);
    assertNotNull("Should have owner change", ownerChange);
    assertEquals("Owner key should be empty", 0, ownerChange.getKey().length);

    // Extract balances from serialized data
    long ownerOldBalance = extractBalanceFromSerialized(ownerChange.getOldValue());
    long ownerNewBalance = extractBalanceFromSerialized(ownerChange.getNewValue());
    assertEquals("Owner old balance should be initial", OWNER_INITIAL_BALANCE, ownerOldBalance);
    assertEquals("Owner new balance should be initial - fee",
        OWNER_INITIAL_BALANCE - ASSET_ISSUE_FEE, ownerNewBalance);

    // Verify blackhole change
    StateChange blackholeChange = findChangeByAddress(result, blackholeAccount.getAddress().toByteArray());
    assertNotNull("Should have blackhole change", blackholeChange);

    long blackholeOldBalance = extractBalanceFromSerialized(blackholeChange.getOldValue());
    long blackholeNewBalance = extractBalanceFromSerialized(blackholeChange.getNewValue());
    assertEquals("Blackhole old balance should be initial", BLACKHOLE_INITIAL_BALANCE, blackholeOldBalance);
    assertEquals("Blackhole new balance should be initial + fee",
        BLACKHOLE_INITIAL_BALANCE + ASSET_ISSUE_FEE, blackholeNewBalance);
  }

  @Test
  public void testSynthesizeAssetIssueWithBurn() {
    // Setup: Owner has 10000 TRX, burning is enabled (no blackhole)
    AccountCapsule ownerAccount = createAccount(OWNER_INITIAL_BALANCE - ASSET_ISSUE_FEE, ownerAddress);

    when(mockAccountStore.get(ownerAddress)).thenReturn(ownerAccount);
    when(mockDynamicStore.supportBlackHoleOptimization()).thenReturn(true); // Burn enabled

    List<Trc10LedgerChange> trc10Changes = createAssetIssueChange();
    when(mockExecResult.getTrc10Changes()).thenReturn(trc10Changes);

    List<StateChange> result = LedgerCsvSynthesizer.synthesize(mockExecResult, mockTrace);

    assertNotNull("Result should not be null", result);
    assertEquals("Should have 1 state change (owner only, no blackhole)", 1, result.size());

    // Verify owner change
    StateChange ownerChange = findChangeByAddress(result, ownerAddress);
    assertNotNull("Should have owner change", ownerChange);

    long ownerOldBalance = extractBalanceFromSerialized(ownerChange.getOldValue());
    long ownerNewBalance = extractBalanceFromSerialized(ownerChange.getNewValue());
    assertEquals("Owner old balance should be initial", OWNER_INITIAL_BALANCE, ownerOldBalance);
    assertEquals("Owner new balance should be initial - fee",
        OWNER_INITIAL_BALANCE - ASSET_ISSUE_FEE, ownerNewBalance);
  }

  @Test
  public void testSynthesizeAssetIssueWithFeeSunHint() {
    // Test that feeSun from Rust is used when present
    long customFee = 500_000000L; // 500 TRX
    AccountCapsule ownerAccount = createAccount(OWNER_INITIAL_BALANCE - customFee, ownerAddress);

    when(mockAccountStore.get(ownerAddress)).thenReturn(ownerAccount);
    when(mockDynamicStore.supportBlackHoleOptimization()).thenReturn(true); // Burn

    List<Trc10LedgerChange> trc10Changes = createAssetIssueChangeWithFee(customFee);
    when(mockExecResult.getTrc10Changes()).thenReturn(trc10Changes);

    List<StateChange> result = LedgerCsvSynthesizer.synthesize(mockExecResult, mockTrace);

    assertEquals("Should have 1 state change", 1, result.size());

    StateChange ownerChange = result.get(0);
    long ownerOldBalance = extractBalanceFromSerialized(ownerChange.getOldValue());
    long ownerNewBalance = extractBalanceFromSerialized(ownerChange.getNewValue());
    assertEquals("Owner old balance should use custom fee", OWNER_INITIAL_BALANCE, ownerOldBalance);
    assertEquals("Owner new balance should use custom fee",
        OWNER_INITIAL_BALANCE - customFee, ownerNewBalance);
  }

  @Test
  public void testSynthesizeParticipate() {
    // Setup: Owner pays 1000 TRX to issuer for tokens
    long trxAmount = 1000_000000L;
    AccountCapsule ownerAccount = createAccount(OWNER_INITIAL_BALANCE - trxAmount, ownerAddress);
    AccountCapsule issuerAccount = createAccount(ISSUER_INITIAL_BALANCE + trxAmount, issuerAddress);

    when(mockAccountStore.get(ownerAddress)).thenReturn(ownerAccount);
    when(mockAccountStore.get(issuerAddress)).thenReturn(issuerAccount);

    List<Trc10LedgerChange> trc10Changes = createParticipateChange(trxAmount);
    when(mockExecResult.getTrc10Changes()).thenReturn(trc10Changes);

    List<StateChange> result = LedgerCsvSynthesizer.synthesize(mockExecResult, mockTrace);

    assertNotNull("Result should not be null", result);
    assertEquals("Should have 2 state changes (owner + issuer)", 2, result.size());

    // Verify owner change (pays TRX)
    StateChange ownerChange = findChangeByAddress(result, ownerAddress);
    assertNotNull("Should have owner change", ownerChange);

    long ownerOldBalance = extractBalanceFromSerialized(ownerChange.getOldValue());
    long ownerNewBalance = extractBalanceFromSerialized(ownerChange.getNewValue());
    assertEquals("Owner old balance should be initial", OWNER_INITIAL_BALANCE, ownerOldBalance);
    assertEquals("Owner new balance should be initial - amount",
        OWNER_INITIAL_BALANCE - trxAmount, ownerNewBalance);

    // Verify issuer change (receives TRX)
    StateChange issuerChange = findChangeByAddress(result, issuerAddress);
    assertNotNull("Should have issuer change", issuerChange);

    long issuerOldBalance = extractBalanceFromSerialized(issuerChange.getOldValue());
    long issuerNewBalance = extractBalanceFromSerialized(issuerChange.getNewValue());
    assertEquals("Issuer old balance should be initial", ISSUER_INITIAL_BALANCE, issuerOldBalance);
    assertEquals("Issuer new balance should be initial + amount",
        ISSUER_INITIAL_BALANCE + trxAmount, issuerNewBalance);
  }

  @Test
  public void testSynthesizeStrictModeAccountMissing() {
    // Enable strict mode
    System.setProperty(LedgerCsvSynthesizer.PROPERTY_STRICT_MODE, "true");

    // Owner account missing
    when(mockAccountStore.get(ownerAddress)).thenReturn(null);

    List<Trc10LedgerChange> trc10Changes = createAssetIssueChange();
    when(mockExecResult.getTrc10Changes()).thenReturn(trc10Changes);

    List<StateChange> result = LedgerCsvSynthesizer.synthesize(mockExecResult, mockTrace);

    assertNotNull("Result should not be null", result);
    assertEquals("Should return empty list in strict mode when account missing", 0, result.size());
  }

  @Test
  public void testSynthesizeNonStrictModeAccountMissing() {
    // Non-strict mode (default)
    System.setProperty(LedgerCsvSynthesizer.PROPERTY_STRICT_MODE, "false");

    // Owner account present, blackhole missing
    AccountCapsule ownerAccount = createAccount(OWNER_INITIAL_BALANCE - ASSET_ISSUE_FEE, ownerAddress);
    when(mockAccountStore.get(ownerAddress)).thenReturn(ownerAccount);
    when(mockAccountStore.getBlackhole()).thenReturn(null); // Blackhole missing
    when(mockDynamicStore.supportBlackHoleOptimization()).thenReturn(false);

    List<Trc10LedgerChange> trc10Changes = createAssetIssueChange();
    when(mockExecResult.getTrc10Changes()).thenReturn(trc10Changes);

    List<StateChange> result = LedgerCsvSynthesizer.synthesize(mockExecResult, mockTrace);

    assertNotNull("Result should not be null", result);
    assertEquals("Should have 1 state change (owner only, blackhole skipped)", 1, result.size());

    StateChange ownerChange = result.get(0);
    assertNotNull("Should have owner change", ownerChange);
  }

  @Test
  public void testSynthesizeMultipleTrc10Changes() {
    // Test multiple TRC-10 operations in one transaction
    AccountCapsule ownerAccount = createAccount(OWNER_INITIAL_BALANCE - ASSET_ISSUE_FEE, ownerAddress);
    AccountCapsule blackholeAccount = createAccount(BLACKHOLE_INITIAL_BALANCE + ASSET_ISSUE_FEE, blackholeAddress);

    when(mockAccountStore.get(ownerAddress)).thenReturn(ownerAccount);
    when(mockAccountStore.get(blackholeAddress)).thenReturn(blackholeAccount); // Mock by address too
    when(mockAccountStore.getBlackhole()).thenReturn(blackholeAccount);
    when(mockDynamicStore.supportBlackHoleOptimization()).thenReturn(false);

    List<Trc10LedgerChange> trc10Changes = new ArrayList<>();
    trc10Changes.add(createSingleAssetIssueChange());

    when(mockExecResult.getTrc10Changes()).thenReturn(trc10Changes);

    List<StateChange> result = LedgerCsvSynthesizer.synthesize(mockExecResult, mockTrace);

    assertNotNull("Result should not be null", result);
    // Should have changes for the issue operation
    assertTrue("Should have at least one state change", result.size() >= 1);
  }

  @Test
  public void testSynthesizeNullInputs() {
    List<StateChange> result1 = LedgerCsvSynthesizer.synthesize(null, mockTrace);
    assertNotNull("Result should not be null", result1);
    assertEquals("Should return empty list for null execResult", 0, result1.size());

    List<StateChange> result2 = LedgerCsvSynthesizer.synthesize(mockExecResult, null);
    assertNotNull("Result should not be null", result2);
    assertEquals("Should return empty list for null trace", 0, result2.size());
  }

  @Test
  public void testSynthesizeEmptyTrc10Changes() {
    when(mockExecResult.getTrc10Changes()).thenReturn(new ArrayList<>());

    List<StateChange> result = LedgerCsvSynthesizer.synthesize(mockExecResult, mockTrace);

    assertNotNull("Result should not be null", result);
    assertEquals("Should return empty list when no TRC-10 changes", 0, result.size());
  }

  // Helper methods

  private AccountCapsule createAccount(long balance) {
    return createAccount(balance, ownerAddress);
  }

  private AccountCapsule createAccount(long balance, byte[] address) {
    Account.Builder accountBuilder = Account.newBuilder();
    accountBuilder.setBalance(balance);
    accountBuilder.setCreateTime(System.currentTimeMillis());
    accountBuilder.setAddress(ByteString.copyFrom(address));
    return new AccountCapsule(accountBuilder.build());
  }

  private List<Trc10LedgerChange> createAssetIssueChange() {
    List<Trc10LedgerChange> changes = new ArrayList<>();
    changes.add(createSingleAssetIssueChange());
    return changes;
  }

  private Trc10LedgerChange createSingleAssetIssueChange() {
    return new Trc10LedgerChange(
        Trc10Op.ISSUE,
        ownerAddress,
        null, // toAddress not used for ISSUE
        null, // assetId not used for ISSUE
        0L, // amount not used for ISSUE
        "TestToken".getBytes(),
        "TT".getBytes(),
        1000000L, // totalSupply
        6, // precision
        new ArrayList<>(), // frozenSupply
        1L, // trxNum
        1L, // num
        System.currentTimeMillis(), // startTime
        System.currentTimeMillis() + 86400000L, // endTime
        "Test token".getBytes(),
        "http://test.com".getBytes(),
        0L, // freeAssetNetLimit
        0L, // publicFreeAssetNetLimit
        null // feeSun - use dynamic store
    );
  }

  private List<Trc10LedgerChange> createAssetIssueChangeWithFee(long feeSun) {
    List<Trc10LedgerChange> changes = new ArrayList<>();
    changes.add(new Trc10LedgerChange(
        Trc10Op.ISSUE,
        ownerAddress,
        null,
        null,
        0L,
        "TestToken".getBytes(),
        "TT".getBytes(),
        1000000L,
        6,
        new ArrayList<>(),
        1L,
        1L,
        System.currentTimeMillis(),
        System.currentTimeMillis() + 86400000L,
        "Test token".getBytes(),
        "http://test.com".getBytes(),
        0L,
        0L,
        feeSun // Custom fee
    ));
    return changes;
  }

  private List<Trc10LedgerChange> createParticipateChange(long trxAmount) {
    List<Trc10LedgerChange> changes = new ArrayList<>();
    changes.add(new Trc10LedgerChange(
        Trc10Op.PARTICIPATE,
        ownerAddress,
        issuerAddress, // toAddress is issuer
        "1000001".getBytes(), // assetId
        trxAmount, // amount to pay
        null, // name not used for PARTICIPATE
        null, // abbr not used
        0L, // totalSupply not used
        0, // precision not used
        new ArrayList<>(),
        0L,
        0L,
        0L,
        0L,
        null, // description
        null, // url
        0L,
        0L,
        null
    ));
    return changes;
  }

  private StateChange findChangeByAddress(List<StateChange> changes, byte[] address) {
    String targetHex = ByteArray.toHexString(address).toLowerCase();
    for (StateChange change : changes) {
      String changeAddressHex = ByteArray.toHexString(change.getAddress()).toLowerCase();
      if (changeAddressHex.equals(targetHex)) {
        return change;
      }
    }
    return null;
  }

  private long extractBalanceFromSerialized(byte[] serialized) {
    if (serialized == null || serialized.length < 32) {
      return 0L;
    }
    // Balance is in first 32 bytes, big-endian, value in last 8 bytes
    long balance = 0;
    for (int i = 0; i < 8; i++) {
      balance = (balance << 8) | (serialized[24 + i] & 0xFF);
    }
    return balance;
  }
}
