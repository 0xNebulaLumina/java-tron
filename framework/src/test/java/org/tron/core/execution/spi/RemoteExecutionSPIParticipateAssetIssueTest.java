package org.tron.core.execution.spi;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import java.lang.reflect.Method;
import java.util.Arrays;
import java.util.HashSet;
import java.util.Set;
import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.mockito.Mockito;
import org.tron.common.utils.Sha256Hash;
import org.tron.core.actuator.TransactionFactory;
import org.tron.core.ChainBaseManager;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.store.AccountStore;
import org.tron.core.store.StoreFactory;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.Transaction.Contract.ContractType;
import org.tron.protos.contract.AssetIssueContractOuterClass.ParticipateAssetIssueContract;
import org.tron.protos.contract.Common.ResourceCode;
import tron.backend.BackendOuterClass.AccountAextSnapshot;
import tron.backend.BackendOuterClass.ExecuteTransactionRequest;

public class RemoteExecutionSPIParticipateAssetIssueTest {

  private RemoteExecutionSPI remoteSPI;
  private static final byte[] OWNER_ADDRESS = new byte[21];
  private static final byte[] ISSUER_ADDRESS = new byte[21];

  static {
    OWNER_ADDRESS[0] = 0x41;
    ISSUER_ADDRESS[0] = 0x41;
    for (int i = 1; i < OWNER_ADDRESS.length; i++) {
      OWNER_ADDRESS[i] = (byte) i;
      ISSUER_ADDRESS[i] = (byte) (i + 1);
    }
  }

  @Before
  public void setUp() {
    TransactionFactory.register(
        ContractType.ParticipateAssetIssueContract, null, ParticipateAssetIssueContract.class);
    remoteSPI = new RemoteExecutionSPI("localhost", 50011);
  }

  @After
  public void tearDown() {
    System.clearProperty("remote.exec.trc10.enabled");
    System.clearProperty("remote.exec.preexec.aext.enabled");
    if (remoteSPI != null) {
      remoteSPI.shutdown();
    }
  }

  private TransactionCapsule createParticipateAssetIssueTransaction() {
    ParticipateAssetIssueContract participateContract = ParticipateAssetIssueContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(OWNER_ADDRESS))
        .setToAddress(ByteString.copyFrom(ISSUER_ADDRESS))
        .setAssetName(ByteString.copyFromUtf8("1000007"))
        .setAmount(1L)
        .build();

    Any contractParameter = Any.pack(participateContract);

    Protocol.Transaction.Contract contract = Protocol.Transaction.Contract.newBuilder()
        .setType(ContractType.ParticipateAssetIssueContract)
        .setParameter(contractParameter)
        .build();

    Protocol.Transaction transaction = Protocol.Transaction.newBuilder()
        .setRawData(Protocol.Transaction.raw.newBuilder()
            .addContract(contract)
            .setFeeLimit(0L)
            .setTimestamp(System.currentTimeMillis())
            .build())
        .build();

    return new TransactionCapsule(transaction);
  }

  private ExecuteTransactionRequest buildRequest(TransactionContext context) throws Exception {
    Method method = RemoteExecutionSPI.class.getDeclaredMethod(
        "buildExecuteTransactionRequest", TransactionContext.class);
    method.setAccessible(true);
    return (ExecuteTransactionRequest) method.invoke(remoteSPI, context);
  }

  @Test
  public void testParticipateAssetIssueContractIncludesToAddressAndAext() throws Exception {
    System.setProperty("remote.exec.trc10.enabled", "true");
    System.setProperty("remote.exec.preexec.aext.enabled", "true");

    TransactionCapsule trxCap = createParticipateAssetIssueTransaction();
    BlockCapsule blockCap = new BlockCapsule(
        1L, Sha256Hash.ZERO_HASH, System.currentTimeMillis(), ByteString.copyFrom(OWNER_ADDRESS));

    AccountStore accountStore = Mockito.mock(AccountStore.class);
    AccountCapsule ownerAccount = Mockito.mock(AccountCapsule.class);
    AccountCapsule issuerAccount = Mockito.mock(AccountCapsule.class);

    Mockito.when(ownerAccount.getNetUsage()).thenReturn(1L);
    Mockito.when(ownerAccount.getFreeNetUsage()).thenReturn(2L);
    Mockito.when(ownerAccount.getEnergyUsage()).thenReturn(3L);
    Mockito.when(ownerAccount.getLatestConsumeTime()).thenReturn(4L);
    Mockito.when(ownerAccount.getLatestConsumeFreeTime()).thenReturn(5L);
    Mockito.when(ownerAccount.getLatestConsumeTimeForEnergy()).thenReturn(6L);
    Mockito.when(ownerAccount.getWindowSize(Mockito.any(ResourceCode.class))).thenReturn(28800L);
    Mockito.when(ownerAccount.getWindowOptimized(Mockito.any(ResourceCode.class))).thenReturn(false);

    Mockito.when(issuerAccount.getNetUsage()).thenReturn(10L);
    Mockito.when(issuerAccount.getFreeNetUsage()).thenReturn(20L);
    Mockito.when(issuerAccount.getEnergyUsage()).thenReturn(30L);
    Mockito.when(issuerAccount.getLatestConsumeTime()).thenReturn(40L);
    Mockito.when(issuerAccount.getLatestConsumeFreeTime()).thenReturn(50L);
    Mockito.when(issuerAccount.getLatestConsumeTimeForEnergy()).thenReturn(60L);
    Mockito.when(issuerAccount.getWindowSize(Mockito.any(ResourceCode.class))).thenReturn(28800L);
    Mockito.when(issuerAccount.getWindowOptimized(Mockito.any(ResourceCode.class))).thenReturn(false);

    Mockito.when(accountStore.get(Mockito.any(byte[].class))).thenAnswer(invocation -> {
      byte[] address = invocation.getArgument(0);
      if (Arrays.equals(address, OWNER_ADDRESS)) {
        return ownerAccount;
      }
      if (Arrays.equals(address, ISSUER_ADDRESS)) {
        return issuerAccount;
      }
      return null;
    });

    ChainBaseManager chainBaseManager = Mockito.mock(ChainBaseManager.class);
    Mockito.when(chainBaseManager.getAccountStore()).thenReturn(accountStore);

    StoreFactory storeFactory = Mockito.mock(StoreFactory.class);
    Mockito.when(storeFactory.getChainBaseManager()).thenReturn(chainBaseManager);

    TransactionContext context = new TransactionContext(blockCap, trxCap, storeFactory, false, false);
    ExecuteTransactionRequest request = buildRequest(context);

    Assert.assertArrayEquals(
        "ParticipateAssetIssueContract should set request.to to contract.to_address",
        ISSUER_ADDRESS, request.getTransaction().getTo().toByteArray());

    Set<ByteString> snapAddresses = new HashSet<>();
    for (AccountAextSnapshot snapshot : request.getPreExecutionAextList()) {
      snapAddresses.add(snapshot.getAddress());
    }
    Assert.assertTrue(snapAddresses.contains(ByteString.copyFrom(OWNER_ADDRESS)));
    Assert.assertTrue(snapAddresses.contains(ByteString.copyFrom(ISSUER_ADDRESS)));
    Assert.assertEquals(2, snapAddresses.size());
  }
}
