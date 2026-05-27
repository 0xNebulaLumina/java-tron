package org.tron.common.runtime;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

import com.google.protobuf.ByteString;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.tron.common.BaseTest;
import org.tron.common.utils.ByteArray;
import org.tron.core.Constant;
import org.tron.core.Wallet;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.config.args.Args;
import org.tron.core.db.TransactionContext;
import org.tron.core.execution.reporting.PreStateSnapshotRegistry;
import org.tron.core.execution.spi.ExecutionMode;
import org.tron.core.execution.spi.ExecutionProgramResult;
import org.tron.core.execution.spi.ExecutionSPI;
import org.tron.core.execution.spi.ExecutionSpiFactory;
import org.tron.core.storage.spi.StorageSPI;
import org.tron.core.store.StoreFactory;
import org.tron.protos.Protocol.AccountType;
import org.tron.protos.Protocol.Transaction.Contract.ContractType;
import org.tron.protos.contract.BalanceContract.FreezeBalanceContract;
import org.tron.protos.contract.BalanceContract.UnfreezeBalanceContract;
import org.tron.protos.contract.Common.ResourceCode;

/**
 * Test class for RuntimeSpiImpl ownership, pre-state snapshots, and sidecar parsing.
 */
public class RuntimeSpiImplTest extends BaseTest {

  private static final String OWNER_ADDRESS;
  private static final String NAME = "TestToken";
  private static final String ABBR = "TT";
  private static final long TOTAL_SUPPLY = 1000000L;
  private static final int TRX_NUM = 1;
  private static final int NUM = 1;
  private static final int PRECISION = 6;
  private static final String DESCRIPTION = "Test token for TRC-10";
  private static final String URL = "https://test.token";
  private static final long FREE_ASSET_NET_LIMIT = 0L;
  private static final long PUBLIC_FREE_ASSET_NET_LIMIT = 0L;
  private static final long PUBLIC_FREE_ASSET_NET_USAGE = 0L;
  private static final long PUBLIC_LATEST_FREE_NET_TIME = 0L;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049150";
  }

  @Before
  public void setUp() {
    // Initialize ExecutionSPI factory for testing
    try {
      ExecutionSpiFactory.initialize();
    } catch (Exception e) {
      // Factory may already be initialized
    }

    // Create test account with sufficient balance for asset issue fee
    AccountCapsule ownerCapsule = new AccountCapsule(
        ByteString.copyFromUtf8("testOwner"),
        ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)),
        AccountType.Normal,
        dbManager.getDynamicPropertiesStore().getAssetIssueFee());
    dbManager.getAccountStore().put(ownerCapsule.getAddress().toByteArray(), ownerCapsule);

    // Set up dynamic properties
    dbManager.getDynamicPropertiesStore()
        .saveLatestBlockHeaderTimestamp(System.currentTimeMillis());
    dbManager.getDynamicPropertiesStore().saveTokenIdNum(1000000L); // Start from 1000000
  }

  @After
  public void cleanup() {
    PreStateSnapshotRegistry.clearForCurrentTransaction();

    // Clean up test data
    byte[] ownerAddress = ByteArray.fromHexString(OWNER_ADDRESS);
    dbManager.getAccountStore().delete(ownerAddress);

    // Clean up any created assets
    try {
      dbManager.getAssetIssueStore().delete(NAME.getBytes());
    } catch (Exception e) {
      // May not exist
    }

    long tokenIdNum = dbManager.getDynamicPropertiesStore().getTokenIdNum();
    try {
      dbManager.getAssetIssueV2Store().delete(String.valueOf(tokenIdNum).getBytes());
    } catch (Exception e) {
      // May not exist
    }
  }

  /**
   * Test TRC-10 AssetIssued change parsing from ExecutionProgramResult.
   * Verifies that Trc10AssetIssued changes are correctly stored in ExecutionProgramResult.
   */
  @Test
  public void testTrc10AssetIssuedChangeParsing() {
    // Create a Trc10AssetIssued change
    byte[] ownerAddress = ByteArray.fromHexString(OWNER_ADDRESS);
    ExecutionSPI.Trc10AssetIssued assetIssued = new ExecutionSPI.Trc10AssetIssued(
        ownerAddress,
        NAME.getBytes(),
        ABBR.getBytes(),
        TOTAL_SUPPLY,
        TRX_NUM,
        PRECISION,
        NUM,
        System.currentTimeMillis(),
        System.currentTimeMillis() + 86400000L,
        DESCRIPTION.getBytes(),
        URL.getBytes(),
        FREE_ASSET_NET_LIMIT,
        PUBLIC_FREE_ASSET_NET_LIMIT,
        PUBLIC_FREE_ASSET_NET_USAGE,
        PUBLIC_LATEST_FREE_NET_TIME,
        "" // Empty token ID - reports may derive it from existing stores
    );

    ExecutionSPI.Trc10Change trc10Change = new ExecutionSPI.Trc10Change(assetIssued);

    // Create ExecutionProgramResult with TRC-10 change
    ExecutionProgramResult result = new ExecutionProgramResult();
    List<ExecutionSPI.Trc10Change> trc10Changes = new ArrayList<>();
    trc10Changes.add(trc10Change);
    result.setTrc10Changes(trc10Changes);

    // Verify parsing
    assertNotNull("Trc10Changes should not be null", result.getTrc10Changes());
    assertEquals("Should have 1 TRC-10 change", 1, result.getTrc10Changes().size());

    ExecutionSPI.Trc10Change parsedChange = result.getTrc10Changes().get(0);
    assertTrue("Should have assetIssued", parsedChange.hasAssetIssued());

    ExecutionSPI.Trc10AssetIssued parsedAsset = parsedChange.getAssetIssued();
    assertEquals("Owner address should match", ownerAddress, parsedAsset.getOwnerAddress());
    assertEquals("Name should match", NAME, new String(parsedAsset.getName()));
    assertEquals("Abbr should match", ABBR, new String(parsedAsset.getAbbr()));
    assertEquals("Total supply should match", TOTAL_SUPPLY, parsedAsset.getTotalSupply());
    assertEquals("TRX num should match", TRX_NUM, parsedAsset.getTrxNum());
    assertEquals("Precision should match", PRECISION, parsedAsset.getPrecision());
    assertEquals("Num should match", NUM, parsedAsset.getNum());
  }

  /**
   * Test TOKEN_ID_NUM store access used by TRC-10 reporting and parity fixtures.
   */
  @Test
  public void testTokenIdNumManagement() {
    long initialTokenId = 1000000L;
    dbManager.getDynamicPropertiesStore().saveTokenIdNum(initialTokenId);

    // Verify initial value
    assertEquals("Initial TOKEN_ID_NUM should be 1000000",
        initialTokenId, dbManager.getDynamicPropertiesStore().getTokenIdNum());

    // Simulate asset issuance (increment TOKEN_ID_NUM)
    long newTokenId = initialTokenId + 1;
    dbManager.getDynamicPropertiesStore().saveTokenIdNum(newTokenId);

    // Verify incremented value
    assertEquals("TOKEN_ID_NUM should be incremented to 1000001",
        newTokenId, dbManager.getDynamicPropertiesStore().getTokenIdNum());

    // Simulate multiple asset issuances
    for (int i = 0; i < 5; i++) {
      long currentTokenId = dbManager.getDynamicPropertiesStore().getTokenIdNum();
      dbManager.getDynamicPropertiesStore().saveTokenIdNum(currentTokenId + 1);
    }

    // Verify final value
    assertEquals("TOKEN_ID_NUM should be incremented to 1000006",
        initialTokenId + 6, dbManager.getDynamicPropertiesStore().getTokenIdNum());
  }

  @Test
  public void testSuccessfulComputeOnlyRemoteResultFailsFast() throws Exception {
    ExecutionProgramResult result = new ExecutionProgramResult();
    result.setWriteMode(ExecutionSPI.WriteMode.COMPUTE_ONLY);

    ExecutionSPI originalSpi = setExecutionSpiInstance(new FakeExecutionSPI(result));
    ExecutionMode originalMode = setExecutionSpiInitializedMode(ExecutionMode.REMOTE);
    try {
      RuntimeSpiImpl runtimeSpi = new RuntimeSpiImpl();
      try {
        runtimeSpi.execute(buildFreezeContext());
        fail("Successful non-persisted remote result should fail fast");
      } catch (IllegalStateException e) {
        assertTrue(e.getMessage().contains("non-persisted state"));
      }
    } finally {
      setExecutionSpiInstance(originalSpi);
      setExecutionSpiInitializedMode(originalMode);
    }
  }

  @Test
  public void testEffectfulFailedComputeOnlyRemoteResultFailsFast() throws Exception {
    ExecutionProgramResult result = new ExecutionProgramResult();
    result.setWriteMode(ExecutionSPI.WriteMode.COMPUTE_ONLY);
    result.setRuntimeError("remote execution failed");
    List<ExecutionSPI.StateChange> stateChanges = new ArrayList<>();
    stateChanges.add(new ExecutionSPI.StateChange(
        ByteArray.fromHexString(OWNER_ADDRESS), new byte[0], new byte[0], new byte[]{1}));
    result.setStateChanges(stateChanges);

    ExecutionSPI originalSpi = setExecutionSpiInstance(new FakeExecutionSPI(result));
    ExecutionMode originalMode = setExecutionSpiInitializedMode(ExecutionMode.REMOTE);
    try {
      RuntimeSpiImpl runtimeSpi = new RuntimeSpiImpl();
      try {
        runtimeSpi.execute(buildFreezeContext());
        fail("Effectful non-persisted remote result should fail fast");
      } catch (IllegalStateException e) {
        assertTrue(e.getMessage().contains("non-persisted state"));
      }
    } finally {
      setExecutionSpiInstance(originalSpi);
      setExecutionSpiInitializedMode(originalMode);
    }
  }

  @Test
  public void testTouchedKeyFailedComputeOnlyRemoteResultFailsFast() throws Exception {
    ExecutionProgramResult result = new ExecutionProgramResult();
    result.setWriteMode(ExecutionSPI.WriteMode.COMPUTE_ONLY);
    result.setRuntimeError("remote execution failed");
    List<ExecutionSPI.TouchedKey> touchedKeys = new ArrayList<>();
    touchedKeys.add(new ExecutionSPI.TouchedKey(
        "account", ByteArray.fromHexString(OWNER_ADDRESS), false));
    result.setTouchedKeys(touchedKeys);

    ExecutionSPI originalSpi = setExecutionSpiInstance(new FakeExecutionSPI(result));
    ExecutionMode originalMode = setExecutionSpiInitializedMode(ExecutionMode.REMOTE);
    try {
      RuntimeSpiImpl runtimeSpi = new RuntimeSpiImpl();
      try {
        runtimeSpi.execute(buildFreezeContext());
        fail("Touched-key non-persisted remote result should fail fast");
      } catch (IllegalStateException e) {
        assertTrue(e.getMessage().contains("non-persisted state"));
      }
    } finally {
      setExecutionSpiInstance(originalSpi);
      setExecutionSpiInitializedMode(originalMode);
    }
  }

  @Test
  public void testAllSidecarFailedComputeOnlyRemoteResultsFailFast() throws Exception {
    byte[] ownerAddress = ByteArray.fromHexString(OWNER_ADDRESS);

    ExecutionProgramResult freezeResult = failedComputeOnlyResult();
    freezeResult.setFreezeChanges(singletonFreezeChange(
        ownerAddress, ExecutionSPI.FreezeLedgerChange.Resource.BANDWIDTH, 1L, 0L, false));
    assertNonPersistedEffectFailsFast(freezeResult, "freeze sidecar");

    ExecutionProgramResult globalResult = failedComputeOnlyResult();
    List<ExecutionSPI.GlobalResourceTotalsChange> globalChanges = new ArrayList<>();
    globalChanges.add(new ExecutionSPI.GlobalResourceTotalsChange(1L, 2L, 3L, 4L));
    globalResult.setGlobalResourceChanges(globalChanges);
    assertNonPersistedEffectFailsFast(globalResult, "global resource sidecar");

    ExecutionProgramResult trc10Result = failedComputeOnlyResult();
    List<ExecutionSPI.Trc10Change> trc10Changes = new ArrayList<>();
    trc10Changes.add(new ExecutionSPI.Trc10Change(new ExecutionSPI.Trc10AssetTransferred(
        ownerAddress, ownerAddress, NAME.getBytes(), "1000001", 1L)));
    trc10Result.setTrc10Changes(trc10Changes);
    assertNonPersistedEffectFailsFast(trc10Result, "TRC-10 sidecar");

    ExecutionProgramResult voteResult = failedComputeOnlyResult();
    List<ExecutionSPI.VoteEntry> votes = new ArrayList<>();
    votes.add(new ExecutionSPI.VoteEntry(ownerAddress, 1L));
    List<ExecutionSPI.VoteChange> voteChanges = new ArrayList<>();
    voteChanges.add(new ExecutionSPI.VoteChange(ownerAddress, votes));
    voteResult.setVoteChanges(voteChanges);
    assertNonPersistedEffectFailsFast(voteResult, "vote sidecar");

    ExecutionProgramResult withdrawResult = failedComputeOnlyResult();
    List<ExecutionSPI.WithdrawChange> withdrawChanges = new ArrayList<>();
    withdrawChanges.add(new ExecutionSPI.WithdrawChange(ownerAddress, 1L, 2L));
    withdrawResult.setWithdrawChanges(withdrawChanges);
    assertNonPersistedEffectFailsFast(withdrawResult, "withdraw sidecar");

    ExecutionProgramResult contractAddressResult = failedComputeOnlyResult();
    contractAddressResult.setContractAddress(new byte[]{1});
    assertNonPersistedEffectFailsFast(contractAddressResult, "contract address");
  }

  @Test
  public void testFailedComputeOnlyRemoteResultWithoutEffectsPassesThrough() throws Exception {
    ExecutionProgramResult result = new ExecutionProgramResult();
    result.setWriteMode(ExecutionSPI.WriteMode.COMPUTE_ONLY);
    result.setRuntimeError("remote execution failed");

    ExecutionSPI originalSpi = setExecutionSpiInstance(new FakeExecutionSPI(result));
    ExecutionMode originalMode = setExecutionSpiInitializedMode(ExecutionMode.REMOTE);
    try {
      TransactionContext context = buildFreezeContext();
      new RuntimeSpiImpl().execute(context);
      assertEquals(result, context.getProgramResult());
    } finally {
      setExecutionSpiInstance(originalSpi);
      setExecutionSpiInitializedMode(originalMode);
    }
  }

  @Test
  public void testPersistedRemoteEffectsWithoutTouchedKeysFailFastBeforeStorageRead()
      throws Exception {
    ExecutionProgramResult result = new ExecutionProgramResult();
    result.setWriteMode(ExecutionSPI.WriteMode.PERSISTED);
    List<ExecutionSPI.StateChange> stateChanges = new ArrayList<>();
    stateChanges.add(new ExecutionSPI.StateChange(
        ByteArray.fromHexString(OWNER_ADDRESS), new byte[0], new byte[0], new byte[]{1}));
    result.setStateChanges(stateChanges);

    FakeStorageSPI storageSPI = new FakeStorageSPI();
    ExecutionSPI originalSpi = setExecutionSpiInstance(new FakeExecutionSPI(result));
    ExecutionMode originalMode = setExecutionSpiInitializedMode(ExecutionMode.REMOTE);
    StorageSPI originalStorageSpi = setMirrorRemoteStorageSPI(storageSPI);
    try {
      try {
        new RuntimeSpiImpl().execute(buildFreezeContext());
        fail("Persisted effects without touched keys should fail fast");
      } catch (IllegalStateException e) {
        assertTrue(e.getMessage().contains("could not be mirrored"));
        assertTrue(e.getCause().getMessage().contains("without touched keys"));
      }
      assertEquals("Remote storage should not be read", 0, storageSPI.getReadCount());
    } finally {
      setMirrorRemoteStorageSPI(originalStorageSpi);
      setExecutionSpiInstance(originalSpi);
      setExecutionSpiInitializedMode(originalMode);
    }
  }

  @Test
  public void testPersistedTouchedKeyRefreshesMirrorFromRemoteStorage() throws Exception {
    byte[] ownerAddress = ByteArray.fromHexString(OWNER_ADDRESS);
    long remoteBalance = 123_456_789L;
    AccountCapsule remoteAccount = new AccountCapsule(
        ByteString.copyFromUtf8("remoteOwner"),
        ByteString.copyFrom(ownerAddress),
        AccountType.Normal,
        remoteBalance);

    ExecutionProgramResult result = new ExecutionProgramResult();
    result.setWriteMode(ExecutionSPI.WriteMode.PERSISTED);
    List<ExecutionSPI.TouchedKey> touchedKeys = new ArrayList<>();
    touchedKeys.add(new ExecutionSPI.TouchedKey("account", ownerAddress, false));
    result.setTouchedKeys(touchedKeys);

    FakeStorageSPI storageSPI = new FakeStorageSPI();
    storageSPI.putRemoteValue("account", ownerAddress, remoteAccount.getData());
    ExecutionSPI originalSpi = setExecutionSpiInstance(new FakeExecutionSPI(result));
    ExecutionMode originalMode = setExecutionSpiInitializedMode(ExecutionMode.REMOTE);
    StorageSPI originalStorageSpi = setMirrorRemoteStorageSPI(storageSPI);
    try {
      new RuntimeSpiImpl().execute(buildFreezeContext());

      AccountCapsule mirroredAccount = dbManager.getAccountStore().get(ownerAddress);
      assertNotNull("Account should be refreshed from remote storage", mirroredAccount);
      assertEquals("Mirrored account balance should match remote storage",
          remoteBalance, mirroredAccount.getBalance());
      assertEquals("Batch remote storage read should be used", 1, storageSPI.batchGetCalls);
    } finally {
      setMirrorRemoteStorageSPI(originalStorageSpi);
      setExecutionSpiInstance(originalSpi);
      setExecutionSpiInitializedMode(originalMode);
    }
  }

  @Test
  public void testPersistedTouchedKeyDeletesMirrorEntry() throws Exception {
    byte[] ownerAddress = ByteArray.fromHexString(OWNER_ADDRESS);
    ExecutionProgramResult result = new ExecutionProgramResult();
    result.setWriteMode(ExecutionSPI.WriteMode.PERSISTED);
    List<ExecutionSPI.TouchedKey> touchedKeys = new ArrayList<>();
    touchedKeys.add(new ExecutionSPI.TouchedKey("account", ownerAddress, true));
    result.setTouchedKeys(touchedKeys);

    FakeStorageSPI storageSPI = new FakeStorageSPI();
    ExecutionSPI originalSpi = setExecutionSpiInstance(new FakeExecutionSPI(result));
    ExecutionMode originalMode = setExecutionSpiInitializedMode(ExecutionMode.REMOTE);
    StorageSPI originalStorageSpi = setMirrorRemoteStorageSPI(storageSPI);
    try {
      new RuntimeSpiImpl().execute(buildFreezeContext());

      assertEquals("Delete touched key should not read remote storage",
          0, storageSPI.getReadCount());
      assertEquals("Account should be deleted from local mirror", null,
          dbManager.getAccountStore().get(ownerAddress));
    } finally {
      setMirrorRemoteStorageSPI(originalStorageSpi);
      setExecutionSpiInstance(originalSpi);
      setExecutionSpiInitializedMode(originalMode);
    }
  }

  @Test
  public void testPersistedTouchedKeyFailsWhenMirrorDisabled() throws Exception {
    byte[] ownerAddress = ByteArray.fromHexString(OWNER_ADDRESS);
    ExecutionProgramResult result = new ExecutionProgramResult();
    result.setWriteMode(ExecutionSPI.WriteMode.PERSISTED);
    List<ExecutionSPI.TouchedKey> touchedKeys = new ArrayList<>();
    touchedKeys.add(new ExecutionSPI.TouchedKey("account", ownerAddress, false));
    result.setTouchedKeys(touchedKeys);

    FakeStorageSPI storageSPI = new FakeStorageSPI();
    FakeExecutionSPI executionSPI = new FakeExecutionSPI(result);
    ExecutionSPI originalSpi = setExecutionSpiInstance(executionSPI);
    ExecutionMode originalMode = setExecutionSpiInitializedMode(ExecutionMode.REMOTE);
    StorageSPI originalStorageSpi = setMirrorRemoteStorageSPI(storageSPI);
    String originalMirrorProperty = System.getProperty("remote.exec.postexec.mirror");
    System.setProperty("remote.exec.postexec.mirror", "false");
    try {
      try {
        new RuntimeSpiImpl().execute(buildFreezeContext());
        fail("Persisted touched keys should fail when post-exec mirror is disabled");
      } catch (IllegalStateException e) {
        assertTrue(e.getMessage().contains("post-exec mirror"));
      }
      assertEquals("Disabled mirror should not dispatch remote execution",
          0, executionSPI.executeTransactionCalls);
      assertEquals("Disabled mirror should not read remote storage", 0, storageSPI.getReadCount());
    } finally {
      restoreSystemProperty("remote.exec.postexec.mirror", originalMirrorProperty);
      setMirrorRemoteStorageSPI(originalStorageSpi);
      setExecutionSpiInstance(originalSpi);
      setExecutionSpiInitializedMode(originalMode);
    }
  }

  @Test
  public void testExecutionModeDetection() {
    // Test that execution mode can be determined
    ExecutionMode mode = ExecutionSpiFactory.determineExecutionMode();
    assertNotNull("Execution mode should not be null", mode);

    // Default mode should be EMBEDDED
    assertEquals("Default execution mode should be EMBEDDED", ExecutionMode.EMBEDDED, mode);
  }

  @Test
  public void testExecutionSpiFactoryInitialization() {
    // Test that ExecutionSPI factory is properly initialized
    assertNotNull("ExecutionSPI instance should be available", ExecutionSpiFactory.getInstance());
  }

  @Test
  public void testConfigurationInfo() {
    // Test that configuration information can be retrieved
    String configInfo = ExecutionSpiFactory.getConfigurationInfo();
    assertNotNull("Configuration info should not be null", configInfo);
    assertTrue("Configuration info should contain mode information", configInfo.contains("Mode:"));
  }

  @Test
  public void testExecutionModeFromString() {
    // Test ExecutionMode enum parsing
    assertEquals(
        "EMBEDDED mode should parse correctly",
        ExecutionMode.EMBEDDED,
        ExecutionMode.fromString("EMBEDDED"));
    assertEquals(
        "REMOTE mode should parse correctly",
        ExecutionMode.REMOTE,
        ExecutionMode.fromString("REMOTE"));
    assertEquals(
        "SHADOW mode should parse correctly",
        ExecutionMode.SHADOW,
        ExecutionMode.fromString("SHADOW"));

    // Test case insensitive parsing
    assertEquals(
        "Lowercase embedded should parse correctly",
        ExecutionMode.EMBEDDED,
        ExecutionMode.fromString("embedded"));
  }

  @Test
  public void testDefaultExecutionMode() {
    // Test that default execution mode is EMBEDDED
    ExecutionMode defaultMode = ExecutionMode.getDefault();
    assertEquals("Default execution mode should be EMBEDDED", ExecutionMode.EMBEDDED, defaultMode);
  }

  @Test
  public void testCapturePreStateSnapshotZerosV1UnfreezeExpireTimeForParity() throws Exception {
    byte[] ownerAddress = ByteArray.fromHexString(OWNER_ADDRESS);
    long frozenAmount = 1_000_000_000L;
    long expireTimeMs = 1_530_160_422_000L;

    AccountCapsule ownerAccount = dbManager.getAccountStore().get(ownerAddress);
    ownerAccount.setFrozenForBandwidth(frozenAmount, expireTimeMs);
    dbManager.getAccountStore().put(ownerAddress, ownerAccount);

    ExecutionProgramResult result = new ExecutionProgramResult();
    result.setFreezeChanges(singletonFreezeChange(
        ownerAddress,
        ExecutionSPI.FreezeLedgerChange.Resource.BANDWIDTH,
        0L,
        0L,
        false));

    UnfreezeBalanceContract contract = UnfreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerAddress))
        .setResource(ResourceCode.BANDWIDTH)
        .build();
    TransactionContext context = buildContext(contract, ContractType.UnfreezeBalanceContract);

    invokeCapturePreStateSnapshot(result, context);

    PreStateSnapshotRegistry.FreezeSnapshot snapshot =
        PreStateSnapshotRegistry.getFreeze(ownerAddress, "BANDWIDTH", null);
    assertNotNull("Freeze snapshot should be captured", snapshot);
    assertEquals("Old amount should still reflect the live account state", frozenAmount,
        snapshot.getAmount());
    assertEquals("V1 unfreeze expire time should match embedded journal parity", 0L,
        snapshot.getExpireTimeMs());
  }

  @Test
  public void testCapturePreStateSnapshotKeepsExpireTimeForV1Freeze() throws Exception {
    byte[] ownerAddress = ByteArray.fromHexString(OWNER_ADDRESS);
    long frozenAmount = 1_000_000_000L;
    long expireTimeMs = 1_530_160_422_000L;

    AccountCapsule ownerAccount = dbManager.getAccountStore().get(ownerAddress);
    ownerAccount.setFrozenForBandwidth(frozenAmount, expireTimeMs);
    dbManager.getAccountStore().put(ownerAddress, ownerAccount);

    ExecutionProgramResult result = new ExecutionProgramResult();
    result.setFreezeChanges(singletonFreezeChange(
        ownerAddress,
        ExecutionSPI.FreezeLedgerChange.Resource.BANDWIDTH,
        frozenAmount + 1_000_000L,
        expireTimeMs + 86_400_000L,
        false));

    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerAddress))
        .setFrozenBalance(1_000_000L)
        .setFrozenDuration(3)
        .build();
    TransactionContext context = buildContext(contract, ContractType.FreezeBalanceContract);

    invokeCapturePreStateSnapshot(result, context);

    PreStateSnapshotRegistry.FreezeSnapshot snapshot =
        PreStateSnapshotRegistry.getFreeze(ownerAddress, "BANDWIDTH", null);
    assertNotNull("Freeze snapshot should be captured", snapshot);
    assertEquals("Old amount should still reflect the live account state", frozenAmount,
        snapshot.getAmount());
    assertEquals("Non-unfreeze contracts should preserve the live expire time", expireTimeMs,
        snapshot.getExpireTimeMs());
  }

  private ExecutionProgramResult failedComputeOnlyResult() {
    ExecutionProgramResult result = new ExecutionProgramResult();
    result.setWriteMode(ExecutionSPI.WriteMode.COMPUTE_ONLY);
    result.setRuntimeError("remote execution failed");
    return result;
  }

  private void assertNonPersistedEffectFailsFast(ExecutionProgramResult result, String effect)
      throws Exception {
    ExecutionSPI originalSpi = setExecutionSpiInstance(new FakeExecutionSPI(result));
    ExecutionMode originalMode = setExecutionSpiInitializedMode(ExecutionMode.REMOTE);
    try {
      new RuntimeSpiImpl().execute(buildFreezeContext());
      fail(effect + " should fail fast");
    } catch (IllegalStateException e) {
      assertTrue(e.getMessage().contains("non-persisted state"));
    } finally {
      setExecutionSpiInstance(originalSpi);
      setExecutionSpiInitializedMode(originalMode);
    }
  }

  private TransactionContext buildFreezeContext() {
    FreezeBalanceContract contract = FreezeBalanceContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(OWNER_ADDRESS)))
        .setFrozenBalance(1_000_000L)
        .setFrozenDuration(3)
        .build();
    return buildContext(contract, ContractType.FreezeBalanceContract);
  }

  private List<ExecutionSPI.FreezeLedgerChange> singletonFreezeChange(
      byte[] ownerAddress,
      ExecutionSPI.FreezeLedgerChange.Resource resource,
      long amount,
      long expirationMs,
      boolean v2Model) {
    List<ExecutionSPI.FreezeLedgerChange> freezeChanges = new ArrayList<>();
    freezeChanges.add(new ExecutionSPI.FreezeLedgerChange(
        ownerAddress, resource, amount, expirationMs, v2Model));
    return freezeChanges;
  }

  private TransactionContext buildContext(com.google.protobuf.Message contract, ContractType type) {
    return new TransactionContext(
        null,
        new TransactionCapsule(contract, type),
        StoreFactory.getInstance(),
        false,
        false);
  }

  private void invokeCapturePreStateSnapshot(ExecutionProgramResult result,
                                             TransactionContext context) throws Exception {
    ExecutionSPI originalSpi = setExecutionSpiInstance(
        new FakeExecutionSPI(new ExecutionProgramResult()));
    ExecutionMode originalMode = setExecutionSpiInitializedMode(ExecutionMode.REMOTE);
    try {
      RuntimeSpiImpl runtimeSpi = new RuntimeSpiImpl();
      java.lang.reflect.Method method = RuntimeSpiImpl.class.getDeclaredMethod(
          "capturePreStateSnapshot",
          ExecutionProgramResult.class,
          TransactionContext.class);
      method.setAccessible(true);

      PreStateSnapshotRegistry.initializeForCurrentTransaction();
      method.invoke(runtimeSpi, result, context);
    } finally {
      setExecutionSpiInstance(originalSpi);
      setExecutionSpiInitializedMode(originalMode);
    }
  }

  private ExecutionMode setExecutionSpiInitializedMode(ExecutionMode mode) throws Exception {
    java.lang.reflect.Field field = ExecutionSpiFactory.class.getDeclaredField("initializedMode");
    field.setAccessible(true);
    ExecutionMode originalMode = (ExecutionMode) field.get(null);
    field.set(null, mode);
    return originalMode;
  }

  private ExecutionSPI setExecutionSpiInstance(ExecutionSPI executionSPI) throws Exception {
    java.lang.reflect.Field field = ExecutionSpiFactory.class.getDeclaredField("instance");
    field.setAccessible(true);
    ExecutionSPI originalSpi = (ExecutionSPI) field.get(null);
    field.set(null, executionSPI);
    return originalSpi;
  }

  private StorageSPI setMirrorRemoteStorageSPI(StorageSPI storageSPI) throws Exception {
    java.lang.reflect.Field field = RuntimeSpiImpl.class.getDeclaredField("mirrorRemoteStorageSPI");
    field.setAccessible(true);
    StorageSPI originalStorageSPI = (StorageSPI) field.get(null);
    field.set(null, storageSPI);
    return originalStorageSPI;
  }

  private void restoreSystemProperty(String key, String value) {
    if (value == null) {
      System.clearProperty(key);
    } else {
      System.setProperty(key, value);
    }
  }

  private static class FakeStorageSPI implements StorageSPI {
    private final Map<String, Map<String, byte[]>> values = new HashMap<>();
    private int getCalls;
    private int batchGetCalls;

    void putRemoteValue(String dbName, byte[] key, byte[] value) {
      values.computeIfAbsent(dbName, ignored -> new HashMap<>())
          .put(ByteArray.toHexString(key), value);
    }

    int getReadCount() {
      return getCalls + batchGetCalls;
    }

    @Override
    public CompletableFuture<byte[]> get(String dbName, byte[] key) {
      getCalls++;
      return CompletableFuture.completedFuture(getValue(dbName, key));
    }

    @Override
    public CompletableFuture<Map<byte[], byte[]>> batchGet(String dbName, List<byte[]> keys) {
      batchGetCalls++;
      Map<byte[], byte[]> result = new HashMap<>();
      for (byte[] key : keys) {
        result.put(key, getValue(dbName, key));
      }
      return CompletableFuture.completedFuture(result);
    }

    private byte[] getValue(String dbName, byte[] key) {
      Map<String, byte[]> dbValues = values.get(dbName);
      return dbValues == null ? null : dbValues.get(ByteArray.toHexString(key));
    }

    @Override
    public CompletableFuture<Void> put(String dbName, byte[] key, byte[] value) {
      return CompletableFuture.completedFuture(null);
    }

    @Override
    public CompletableFuture<Void> delete(String dbName, byte[] key) {
      return CompletableFuture.completedFuture(null);
    }

    @Override
    public CompletableFuture<Boolean> has(String dbName, byte[] key) {
      return CompletableFuture.completedFuture(getValue(dbName, key) != null);
    }

    @Override
    public CompletableFuture<Void> batchWrite(String dbName, Map<byte[], byte[]> operations) {
      return CompletableFuture.completedFuture(null);
    }

    @Override
    public CompletableFuture<org.tron.core.storage.spi.StorageIterator> iterator(String dbName) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<org.tron.core.storage.spi.StorageIterator> iterator(
        String dbName, byte[] startKey) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<List<byte[]>> getKeysNext(String dbName, byte[] startKey, int limit) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<List<byte[]>> getValuesNext(
        String dbName, byte[] startKey, int limit) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<Map<byte[], byte[]>> getNext(
        String dbName, byte[] startKey, int limit) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<Map<byte[], byte[]>> prefixQuery(String dbName, byte[] prefix) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<Void> initDB(
        String dbName, org.tron.core.storage.spi.StorageConfig config) {
      return CompletableFuture.completedFuture(null);
    }

    @Override
    public CompletableFuture<Void> closeDB(String dbName) {
      return CompletableFuture.completedFuture(null);
    }

    @Override
    public CompletableFuture<Void> resetDB(String dbName) {
      return CompletableFuture.completedFuture(null);
    }

    @Override
    public CompletableFuture<Boolean> isAlive(String dbName) {
      return CompletableFuture.completedFuture(true);
    }

    @Override
    public CompletableFuture<Long> size(String dbName) {
      return CompletableFuture.completedFuture(0L);
    }

    @Override
    public CompletableFuture<Boolean> isEmpty(String dbName) {
      return CompletableFuture.completedFuture(false);
    }

    @Override
    public CompletableFuture<String> beginTransaction(String dbName) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<Void> commitTransaction(String transactionId) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<Void> rollbackTransaction(String transactionId) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<String> createSnapshot(String dbName) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<Void> deleteSnapshot(String snapshotId) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<byte[]> getFromSnapshot(String snapshotId, byte[] key) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<org.tron.core.storage.spi.StorageStats> getStats(String dbName) {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<List<String>> listDatabases() {
      return unsupportedFuture();
    }

    @Override
    public CompletableFuture<org.tron.core.storage.spi.HealthStatus> healthCheck() {
      return CompletableFuture.completedFuture(org.tron.core.storage.spi.HealthStatus.HEALTHY);
    }

    @Override
    public void registerMetricsCallback(org.tron.core.storage.spi.MetricsCallback callback) {
    }

    private <T> CompletableFuture<T> unsupportedFuture() {
      CompletableFuture<T> future = new CompletableFuture<>();
      future.completeExceptionally(new UnsupportedOperationException());
      return future;
    }
  }

  private static class FakeExecutionSPI implements ExecutionSPI {
    private final ExecutionProgramResult result;
    private int executeTransactionCalls;

    FakeExecutionSPI(ExecutionProgramResult result) {
      this.result = result;
    }

    @Override
    public CompletableFuture<ExecutionProgramResult> executeTransaction(
        TransactionContext context) {
      executeTransactionCalls++;
      return CompletableFuture.completedFuture(result);
    }

    @Override
    public CompletableFuture<ExecutionProgramResult> callContract(TransactionContext context) {
      return CompletableFuture.completedFuture(result);
    }

    @Override
    public CompletableFuture<Long> estimateEnergy(TransactionContext context) {
      return CompletableFuture.completedFuture(0L);
    }

    @Override
    public CompletableFuture<byte[]> getCode(byte[] address, String snapshotId) {
      return CompletableFuture.completedFuture(new byte[0]);
    }

    @Override
    public CompletableFuture<byte[]> getStorageAt(byte[] address, byte[] key, String snapshotId) {
      return CompletableFuture.completedFuture(new byte[0]);
    }

    @Override
    public CompletableFuture<Long> getNonce(byte[] address, String snapshotId) {
      return CompletableFuture.completedFuture(0L);
    }

    @Override
    public CompletableFuture<byte[]> getBalance(byte[] address, String snapshotId) {
      return CompletableFuture.completedFuture(new byte[0]);
    }

    @Override
    public CompletableFuture<String> createSnapshot() {
      return CompletableFuture.completedFuture("snapshot");
    }

    @Override
    public CompletableFuture<Boolean> revertToSnapshot(String snapshotId) {
      return CompletableFuture.completedFuture(true);
    }

    @Override
    public CompletableFuture<HealthStatus> healthCheck() {
      return CompletableFuture.completedFuture(new HealthStatus(true, "ok"));
    }

    @Override
    public void registerMetricsCallback(MetricsCallback callback) {
    }
  }
}
