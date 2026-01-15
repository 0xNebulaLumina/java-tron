package org.tron.core.conformance;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.SortedMap;
import java.util.TreeMap;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.ChainBaseManager;
import org.tron.core.actuator.Actuator;
import org.tron.core.actuator.ActuatorFactory;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.capsule.TransactionResultCapsule;
import org.tron.core.db.Manager;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.AccountContract.AccountCreateContract;
import org.tron.protos.contract.AccountContract.AccountUpdateContract;
import org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract;
import org.tron.protos.contract.AssetIssueContractOuterClass.ParticipateAssetIssueContract;
import org.tron.protos.contract.AssetIssueContractOuterClass.TransferAssetContract;
import org.tron.protos.contract.BalanceContract.FreezeBalanceContract;
import org.tron.protos.contract.BalanceContract.FreezeBalanceV2Contract;
import org.tron.protos.contract.BalanceContract.TransferContract;
import org.tron.protos.contract.BalanceContract.UnfreezeBalanceContract;
import org.tron.protos.contract.BalanceContract.UnfreezeBalanceV2Contract;
import org.tron.protos.contract.BalanceContract.WithdrawBalanceContract;
import org.tron.protos.contract.Common.ResourceCode;
import org.tron.protos.contract.WitnessContract.VoteWitnessContract;
import org.tron.protos.contract.WitnessContract.WitnessCreateContract;
import org.tron.protos.contract.WitnessContract.WitnessUpdateContract;
import tron.backend.BackendOuterClass.AccountAext;
import tron.backend.BackendOuterClass.AccountAextSnapshot;
import tron.backend.BackendOuterClass.ExecuteTransactionRequest;
import tron.backend.BackendOuterClass.ExecutionContext;
import tron.backend.BackendOuterClass.TronTransaction;
import tron.backend.BackendOuterClass.TxKind;

/**
 * Generates conformance test fixtures by running embedded actuator execution
 * and capturing pre/post database state.
 *
 * <p>Usage:
 * <pre>
 * FixtureGenerator generator = new FixtureGenerator(dbManager, chainBaseManager);
 * generator.setOutputDir(new File("../conformance/fixtures"));
 * generator.generate(transactionCapsule, blockCapsule, metadata);
 * </pre>
 */
public class FixtureGenerator {

  private static final Logger logger = LoggerFactory.getLogger(FixtureGenerator.class);

  private final Manager dbManager;
  private final ChainBaseManager chainBaseManager;
  private File outputDir;

  /**
   * Database names that can be captured for conformance testing.
   * Maps internal store names to RocksDB database names.
   */
  private static final Map<String, String> DB_NAME_MAPPING = new HashMap<>();

  static {
    DB_NAME_MAPPING.put("account", "account");
    DB_NAME_MAPPING.put("account-index", "account-index");
    DB_NAME_MAPPING.put("accountid-index", "accountid-index");
    DB_NAME_MAPPING.put("proposal", "proposal");
    DB_NAME_MAPPING.put("witness", "witness");
    DB_NAME_MAPPING.put("dynamic-properties", "properties");
    DB_NAME_MAPPING.put("contract", "contract");
    DB_NAME_MAPPING.put("code", "code");
    DB_NAME_MAPPING.put("abi", "abi");
    DB_NAME_MAPPING.put("contract-state", "contract-state");
    DB_NAME_MAPPING.put("DelegatedResource", "DelegatedResource");
    DB_NAME_MAPPING.put("DelegatedResourceAccountIndex", "DelegatedResourceAccountIndex");
    DB_NAME_MAPPING.put("delegation", "delegation");
    DB_NAME_MAPPING.put("exchange", "exchange");
    DB_NAME_MAPPING.put("exchange-v2", "exchange-v2");
    DB_NAME_MAPPING.put("asset-issue", "asset-issue");
    DB_NAME_MAPPING.put("asset-issue-v2", "asset-issue-v2");
    DB_NAME_MAPPING.put("votes", "votes");
    DB_NAME_MAPPING.put("market-account", "market-account");
    DB_NAME_MAPPING.put("market-order", "market-order");
    DB_NAME_MAPPING.put("market-pair-price-to-order", "market-pair-price-to-order");
    DB_NAME_MAPPING.put("market-pair-to-price", "market-pair-to-price");
  }

  public FixtureGenerator(Manager dbManager, ChainBaseManager chainBaseManager) {
    this.dbManager = dbManager;
    this.chainBaseManager = chainBaseManager;
    this.outputDir = new File("../conformance/fixtures");
  }

  public void setOutputDir(File outputDir) {
    this.outputDir = outputDir;
  }

  /**
   * Generate a conformance fixture for a transaction.
   *
   * @param trxCap Transaction to execute
   * @param blockCap Block context
   * @param metadata Fixture metadata
   * @return FixtureResult containing execution outcome
   * @throws IOException If file writing fails
   */
  public FixtureResult generate(TransactionCapsule trxCap, BlockCapsule blockCap,
                                 FixtureMetadata metadata) throws IOException {

    // Create fixture directory
    File fixtureDir = new File(outputDir,
        metadata.getContractType().toLowerCase() + "/" + metadata.getCaseName());
    fixtureDir.mkdirs();

    List<String> databases = metadata.getDatabasesTouched();
    FixtureResult result = new FixtureResult();

    try {
      // Capture pre-execution state for relevant databases
      logger.info("Capturing pre-execution state for {} databases", databases.size());
      File preDbDir = new File(fixtureDir, "pre_db");
      preDbDir.mkdirs();
      captureDbState(databases, preDbDir);

      // Build and save the ExecuteTransactionRequest
      ExecuteTransactionRequest request = buildRequest(trxCap, blockCap, metadata);
      File requestFile = new File(fixtureDir, "request.pb");
      try (FileOutputStream fos = new FileOutputStream(requestFile)) {
        request.writeTo(fos);
      }
      logger.info("Saved request.pb ({} bytes)", requestFile.length());

      // Execute using embedded actuator
      result = executeEmbedded(trxCap, blockCap);

      // Capture post-execution state
      logger.info("Capturing post-execution state");
      File expectedDir = new File(fixtureDir, "expected");
      File postDbDir = new File(expectedDir, "post_db");
      postDbDir.mkdirs();
      captureDbState(databases, postDbDir);

      // Save execution result
      if (result.getResultProto() != null) {
        File resultFile = new File(expectedDir, "result.pb");
        try (FileOutputStream fos = new FileOutputStream(resultFile)) {
          result.getResultProto().writeTo(fos);
        }
        logger.info("Saved result.pb ({} bytes)", resultFile.length());
      }

      // Update and save metadata
      metadata.setBlockNumber(blockCap.getNum());
      metadata.setBlockTimestamp(blockCap.getTimeStamp());
      if (result.isSuccess()) {
        metadata.setExpectedStatus("SUCCESS");
      } else if (result.getValidationError() != null) {
        metadata.setExpectedStatus("VALIDATION_FAILED");
        metadata.setExpectedErrorMessage(result.getValidationError());
      } else {
        metadata.setExpectedStatus("REVERT");
        if (result.getExecutionError() != null) {
          metadata.setExpectedErrorMessage(result.getExecutionError());
        }
      }
      metadata.toFile(new File(fixtureDir, "metadata.json"));

      logger.info("Generated fixture: {}/{} (status={})",
          metadata.getContractType(), metadata.getCaseName(), metadata.getExpectedStatus());

    } catch (Exception e) {
      logger.error("Failed to generate fixture", e);
      result.setExecutionError(e.getMessage());
    }

    return result;
  }

  /**
   * Capture current database state for specified databases.
   */
  private void captureDbState(List<String> databases, File outputDir) throws IOException {
    for (String dbName : databases) {
      SortedMap<byte[], byte[]> kvData = new TreeMap<>(KvFileFormat.BYTE_ARRAY_COMPARATOR);

      try {
        // Get the appropriate store and iterate its contents
        Iterator<Map.Entry<byte[], byte[]>> iterator = getStoreIterator(dbName);
        if (iterator != null) {
          while (iterator.hasNext()) {
            Map.Entry<byte[], byte[]> entry = iterator.next();
            kvData.put(entry.getKey(), entry.getValue());
          }
        }
      } catch (Exception e) {
        logger.warn("Failed to capture state for database {}: {}", dbName, e.getMessage());
      }

      File kvFile = new File(outputDir, dbName + ".kv");
      KvFileFormat.write(kvFile, kvData);
      logger.debug("Captured {} entries from {}", kvData.size(), dbName);
    }
  }

  /**
   * Get an iterator for a named store.
   * Returns null if the store doesn't exist or isn't accessible.
   */
  private Iterator<Map.Entry<byte[], byte[]>> getStoreIterator(String dbName) {
    try {
      switch (dbName) {
        case "account":
          return convertIterator(chainBaseManager.getAccountStore().iterator());
        case "account-index":
          return convertIterator(chainBaseManager.getAccountIndexStore().iterator());
        case "accountid-index":
          return convertIterator(chainBaseManager.getAccountIdIndexStore().iterator());
        case "proposal":
          return convertIterator(chainBaseManager.getProposalStore().iterator());
        case "witness":
          return convertIterator(chainBaseManager.getWitnessStore().iterator());
        case "dynamic-properties":
          return convertIterator(chainBaseManager.getDynamicPropertiesStore().iterator());
        case "contract":
          return convertIterator(chainBaseManager.getContractStore().iterator());
        case "code":
          return convertIterator(chainBaseManager.getCodeStore().iterator());
        case "abi":
          return convertIterator(chainBaseManager.getAbiStore().iterator());
        case "DelegatedResource":
          return convertIterator(chainBaseManager.getDelegatedResourceStore().iterator());
        case "DelegatedResourceAccountIndex":
          return convertIterator(chainBaseManager.getDelegatedResourceAccountIndexStore().iterator());
        case "delegation":
          return convertIterator(chainBaseManager.getDelegationStore().iterator());
        case "exchange":
          return convertIterator(chainBaseManager.getExchangeStore().iterator());
        case "exchange-v2":
          return convertIterator(chainBaseManager.getExchangeV2Store().iterator());
        case "votes":
          return convertIterator(chainBaseManager.getVotesStore().iterator());
        case "asset-issue":
          return convertIterator(chainBaseManager.getAssetIssueStore().iterator());
        case "asset-issue-v2":
          return convertIterator(chainBaseManager.getAssetIssueV2Store().iterator());
        case "market_account":
          return convertIterator(chainBaseManager.getMarketAccountStore().iterator());
        case "market_order":
          return convertIterator(chainBaseManager.getMarketOrderStore().iterator());
        case "market_pair_to_price":
          return convertIterator(chainBaseManager.getMarketPairToPriceStore().iterator());
        case "market_pair_price_to_order":
          return convertIterator(chainBaseManager.getMarketPairPriceToOrderStore().iterator());
        default:
          logger.warn("Unknown database: {}", dbName);
          return null;
      }
    } catch (Exception e) {
      logger.warn("Failed to get iterator for {}: {}", dbName, e.getMessage());
      return null;
    }
  }

  /**
   * Convert a store iterator to a simple key-value iterator.
   * Handles capsule objects by extracting their serialized data.
   */
  @SuppressWarnings("unchecked")
  private Iterator<Map.Entry<byte[], byte[]>> convertIterator(Iterator<?> storeIterator) {
    List<Map.Entry<byte[], byte[]>> entries = new ArrayList<>();
    while (storeIterator.hasNext()) {
      Object entry = storeIterator.next();
      if (entry instanceof Map.Entry) {
        Map.Entry<?, ?> mapEntry = (Map.Entry<?, ?>) entry;
        byte[] key = (byte[]) mapEntry.getKey();
        Object value = mapEntry.getValue();

        // Convert value to bytes - handle capsule objects
        byte[] valueBytes;
        if (value instanceof byte[]) {
          valueBytes = (byte[]) value;
        } else if (value instanceof org.tron.core.capsule.ProtoCapsule) {
          valueBytes = ((org.tron.core.capsule.ProtoCapsule<?>) value).getData();
        } else if (value != null) {
          logger.warn("Unknown value type in store iterator: {}", value.getClass().getName());
          continue;
        } else {
          valueBytes = new byte[0];
        }

        // Defensive copy: some store iterators may reuse backing buffers.
        byte[] keyCopy = key == null ? new byte[0] : Arrays.copyOf(key, key.length);
        byte[] valueCopy = valueBytes == null ? new byte[0] : Arrays.copyOf(valueBytes, valueBytes.length);
        entries.add(new java.util.AbstractMap.SimpleEntry<>(keyCopy, valueCopy));
      }
    }
    return entries.iterator();
  }

  private List<AccountAextSnapshot> collectPreExecutionAext(
      Transaction.Contract.ContractType contractType, byte[] fromAddress, byte[] toAddress) {
    List<AccountAextSnapshot> snapshots = new ArrayList<>();

    boolean enabled =
        Boolean.parseBoolean(System.getProperty("remote.exec.preexec.aext.enabled", "true"));
    if (!enabled) {
      return snapshots;
    }

    List<byte[]> addressesToSnapshot = new ArrayList<>();
    if (fromAddress != null && fromAddress.length > 0) {
      addressesToSnapshot.add(fromAddress);
    }

    if (toAddress != null && toAddress.length > 0) {
      switch (contractType) {
        case TransferContract:
        case TransferAssetContract:
          if (!Arrays.equals(fromAddress, toAddress)) {
            addressesToSnapshot.add(toAddress);
          }
          break;
        default:
          break;
      }
    }

    try {
      org.tron.core.store.AccountStore accountStore = chainBaseManager.getAccountStore();
      if (accountStore == null) {
        logger.warn("AccountStore not available for AEXT collection");
        return snapshots;
      }

      for (byte[] address : addressesToSnapshot) {
        try {
          AccountCapsule account = accountStore.get(address);
          if (account == null) {
            continue;
          }

          AccountAext aext = AccountAext.newBuilder()
              .setNetUsage(account.getNetUsage())
              .setFreeNetUsage(account.getFreeNetUsage())
              .setEnergyUsage(account.getEnergyUsage())
              .setLatestConsumeTime(account.getLatestConsumeTime())
              .setLatestConsumeFreeTime(account.getLatestConsumeFreeTime())
              .setLatestConsumeTimeForEnergy(account.getLatestConsumeTimeForEnergy())
              .setNetWindowSize(account.getWindowSize(ResourceCode.BANDWIDTH))
              .setNetWindowOptimized(account.getWindowOptimized(ResourceCode.BANDWIDTH))
              .setEnergyWindowSize(account.getWindowSize(ResourceCode.ENERGY))
              .setEnergyWindowOptimized(account.getWindowOptimized(ResourceCode.ENERGY))
              .build();

          snapshots.add(AccountAextSnapshot.newBuilder()
              .setAddress(ByteString.copyFrom(address))
              .setAext(aext)
              .build());
        } catch (Exception e) {
          logger.warn("Failed to collect AEXT for address: {}", e.getMessage());
        }
      }
    } catch (Exception e) {
      logger.warn("Failed to collect pre-execution AEXT snapshots: {}", e.getMessage());
    }

    return snapshots;
  }

  /**
   * Build ExecuteTransactionRequest from transaction and block.
   *
   * <p>This method maps contract fields to the appropriate TronTransaction fields
   * to match the RemoteExecutionSPI mapping behavior:
   * - TRANSFER_CONTRACT: to = to_address, value = amount, data = empty
   * - TRANSFER_ASSET_CONTRACT: to = to_address, value = amount, asset_id = asset_name, data = empty
   * - ACCOUNT_UPDATE_CONTRACT: data = account_name bytes
   * - WITNESS_CREATE_CONTRACT: data = url bytes
   * - WITNESS_UPDATE_CONTRACT: data = update_url bytes
   * - Other system contracts: data = contract.toByteArray() (raw proto)
   */
  private ExecuteTransactionRequest buildRequest(TransactionCapsule trxCap,
                                                   BlockCapsule blockCap,
                                                   FixtureMetadata metadata) {
    Transaction transaction = trxCap.getInstance();
    Transaction.Contract contract = transaction.getRawData().getContract(0);
    Any contractParameter = contract.getParameter();

    byte[] fromAddress = trxCap.getOwnerAddress();
    byte[] toAddress = new byte[0]; // Empty by default for system contracts
    byte[] data = new byte[0];
    byte[] assetId = new byte[0];
    long value = 0;

    // Map contract fields based on contract type
    try {
      switch (contract.getType()) {
        case TransferContract:
          TransferContract transferContract = contractParameter.unpack(TransferContract.class);
          toAddress = transferContract.getToAddress().toByteArray();
          value = transferContract.getAmount();
          // data remains empty to match RemoteExecutionSPI
          break;

        case TransferAssetContract:
          TransferAssetContract transferAssetContract =
              contractParameter.unpack(TransferAssetContract.class);
          toAddress = transferAssetContract.getToAddress().toByteArray();
          value = transferAssetContract.getAmount();
          assetId = transferAssetContract.getAssetName().toByteArray();
          // data remains empty
          break;

        case ParticipateAssetIssueContract:
          ParticipateAssetIssueContract participateAssetIssueContract =
              contractParameter.unpack(ParticipateAssetIssueContract.class);
          value = participateAssetIssueContract.getAmount();
          data = participateAssetIssueContract.toByteArray();
          break;

        case AccountUpdateContract:
          AccountUpdateContract accountUpdateContract =
              contractParameter.unpack(AccountUpdateContract.class);
          fromAddress = accountUpdateContract.getOwnerAddress().toByteArray();
          data = accountUpdateContract.getAccountName().toByteArray();
          // toAddress remains empty, value = 0
          break;

        case WitnessCreateContract:
          WitnessCreateContract witnessCreateContract =
              contractParameter.unpack(WitnessCreateContract.class);
          data = witnessCreateContract.getUrl().toByteArray();
          // toAddress remains empty
          break;

        case WitnessUpdateContract:
          WitnessUpdateContract witnessUpdateContract =
              contractParameter.unpack(WitnessUpdateContract.class);
          data = witnessUpdateContract.getUpdateUrl().toByteArray();
          // toAddress remains empty
          break;

        case WithdrawBalanceContract:
          // WithdrawBalanceContract only has owner_address, no extra data needed
          // data remains empty, toAddress remains empty, value = 0
          break;

        case AccountCreateContract:
          AccountCreateContract accountCreateContract =
              contractParameter.unpack(AccountCreateContract.class);
          data = accountCreateContract.toByteArray();
          break;

        case VoteWitnessContract:
          VoteWitnessContract voteWitnessContract =
              contractParameter.unpack(VoteWitnessContract.class);
          data = voteWitnessContract.toByteArray();
          break;

        case AssetIssueContract:
          AssetIssueContract assetIssueContract =
              contractParameter.unpack(AssetIssueContract.class);
          data = assetIssueContract.toByteArray();
          break;

        case FreezeBalanceContract:
          FreezeBalanceContract freezeContract =
              contractParameter.unpack(FreezeBalanceContract.class);
          data = freezeContract.toByteArray();
          break;

        case UnfreezeBalanceContract:
          UnfreezeBalanceContract unfreezeContract =
              contractParameter.unpack(UnfreezeBalanceContract.class);
          data = unfreezeContract.toByteArray();
          break;

        case FreezeBalanceV2Contract:
          FreezeBalanceV2Contract freezeV2Contract =
              contractParameter.unpack(FreezeBalanceV2Contract.class);
          data = freezeV2Contract.toByteArray();
          break;

        case UnfreezeBalanceV2Contract:
          UnfreezeBalanceV2Contract unfreezeV2Contract =
              contractParameter.unpack(UnfreezeBalanceV2Contract.class);
          data = unfreezeV2Contract.toByteArray();
          break;

        default:
          // For most system contracts, RemoteExecutionSPI forwards the inner proto bytes.
          data = contractParameter.getValue().toByteArray();
          break;
      }
    } catch (Exception e) {
      logger.warn("Failed to extract contract data for type {}: {}",
          contract.getType(), e.getMessage());
      data = contractParameter.getValue().toByteArray();
    }

    logger.debug("buildRequest: type={}, from_len={}, to_len={}, value={}, data_len={}, asset_id_len={}",
        contract.getType().name(),
        fromAddress.length,
        toAddress.length,
        value,
        data.length,
        assetId.length);

    TronTransaction tronTx = TronTransaction.newBuilder()
        .setFrom(ByteString.copyFrom(fromAddress))
        .setTo(ByteString.copyFrom(toAddress))
        .setValue(ByteString.copyFrom(longToBytes32(value)))
        .setData(ByteString.copyFrom(data))
        .setAssetId(ByteString.copyFrom(assetId))
        .setEnergyLimit(transaction.getRawData().getFeeLimit())
        .setEnergyPrice(1)
        .setNonce(0)
        .setTxKind(TxKind.NON_VM)
        .setContractType(
            tron.backend.BackendOuterClass.ContractType.forNumber(contract.getType().getNumber()))
        // Preserve raw Any (type_url + value) for Rust-side any.is/any.unpack validation parity.
        .setContractParameter(contractParameter)
        .build();

    ExecutionContext context = ExecutionContext.newBuilder()
        .setBlockNumber(blockCap.getNum())
        .setBlockTimestamp(blockCap.getTimeStamp())
        .setBlockHash(ByteString.copyFrom(blockCap.getBlockId().getBytes()))
        .setCoinbase(ByteString.copyFrom(blockCap.getWitnessAddress().toByteArray()))
        .setEnergyLimit(transaction.getRawData().getFeeLimit())
        .setEnergyPrice(1)
        .setTransactionId(trxCap.getTransactionId().toString())
        .build();

    List<AccountAextSnapshot> preExecAext =
        collectPreExecutionAext(contract.getType(), fromAddress, toAddress);
    return ExecuteTransactionRequest.newBuilder()
        .setTransaction(tronTx)
        .setContext(context)
        .addAllPreExecutionAext(preExecAext)
        .build();
  }

  /**
   * Execute transaction using embedded actuator.
   */
  private FixtureResult executeEmbedded(TransactionCapsule trxCap, BlockCapsule blockCap) {
    FixtureResult result = new FixtureResult();

    try {
      Transaction.Contract contract = trxCap.getInstance().getRawData().getContract(0);

      // Create actuator
      List<Actuator> actuatorList = ActuatorFactory.createActuator(trxCap, chainBaseManager);
      if (actuatorList == null || actuatorList.isEmpty()) {
        result.setValidationError("No actuator found for contract type: " + contract.getType());
        return result;
      }
      Actuator actuator = actuatorList.get(0);

      // Validate
      try {
        actuator.validate();
      } catch (ContractValidateException e) {
        result.setValidationError(e.getMessage());
        return result;
      }

      // Execute
      TransactionResultCapsule ret = new TransactionResultCapsule();
      try {
        actuator.execute(ret);
        result.setSuccess(true);
        result.setResultProto(ret.getInstance());
      } catch (ContractExeException e) {
        result.setExecutionError(e.getMessage());
      }

    } catch (Exception e) {
      result.setExecutionError("Unexpected error: " + e.getMessage());
    }

    return result;
  }

  private byte[] longToBytes32(long value) {
    byte[] result = new byte[32];
    for (int i = 7; i >= 0; i--) {
      result[31 - i] = (byte) (value >>> (i * 8));
    }
    return result;
  }

  /**
   * Result of fixture generation.
   */
  public static class FixtureResult {
    private boolean success;
    private String validationError;
    private String executionError;
    private Protocol.Transaction.Result resultProto;

    public boolean isSuccess() {
      return success;
    }

    public void setSuccess(boolean success) {
      this.success = success;
    }

    public String getValidationError() {
      return validationError;
    }

    public void setValidationError(String error) {
      this.validationError = error;
    }

    public String getExecutionError() {
      return executionError;
    }

    public void setExecutionError(String error) {
      this.executionError = error;
    }

    public Protocol.Transaction.Result getResultProto() {
      return resultProto;
    }

    public void setResultProto(Protocol.Transaction.Result result) {
      this.resultProto = result;
    }
  }
}
