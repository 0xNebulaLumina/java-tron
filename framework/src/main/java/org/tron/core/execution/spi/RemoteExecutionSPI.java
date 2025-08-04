package org.tron.core.execution.spi;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.client.ExecutionGrpcClient;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.core.exception.VMIllegalException;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.Protocol.Transaction.Result.contractResult;
import org.tron.protos.contract.AssetIssueContractOuterClass.TransferAssetContract;
import org.tron.protos.contract.BalanceContract.TransferContract;
import org.tron.protos.contract.SmartContractOuterClass.CreateSmartContract;
import org.tron.protos.contract.SmartContractOuterClass.TriggerSmartContract;
import tron.backend.BackendOuterClass.*;

/**
 * Remote execution implementation using the Rust backend service via gRPC. This implementation will
 * be completed in Task 2 with the ExecutionGrpcClient.
 */
public class RemoteExecutionSPI implements ExecutionSPI {
  private static final Logger logger = LoggerFactory.getLogger(RemoteExecutionSPI.class);

  private final String host;
  private final int port;
  private MetricsCallback metricsCallback;
  private final ExecutionGrpcClient grpcClient;

  public RemoteExecutionSPI(String host, int port) {
    this.host = host;
    this.port = port;
    this.grpcClient = new ExecutionGrpcClient(host, port);
    logger.info("Initialized remote execution SPI with host: {}:{}", host, port);
  }

  @Override
  public CompletableFuture<ExecutionProgramResult> executeTransaction(TransactionContext context)
      throws ContractValidateException, ContractExeException, VMIllegalException {

    return CompletableFuture.supplyAsync(
        () -> {
          try {
            logger.debug(
                "Executing transaction with remote Rust EVM: {}",
                context.getTrxCap().getTransactionId());

            // Convert TransactionContext to gRPC request
            ExecuteTransactionRequest request = buildExecuteTransactionRequest(context);

            // Call grpcClient.executeTransaction()
            ExecuteTransactionResponse response = grpcClient.executeTransaction(request);

            // Convert gRPC response to ExecutionProgramResult
            return ExecutionProgramResult.fromExecutionResult(
                convertExecuteTransactionResponse(response));

          } catch (Exception e) {
            logger.error("Remote execution failed", e);
            // Create a failed ExecutionProgramResult
            ExecutionProgramResult result = new ExecutionProgramResult();
            result.setRuntimeError("Remote execution failed: " + e.getMessage());
            result.setRevert();
            result.setResultCode(contractResult.UNKNOWN);
            return result;
          }
        });
  }

  @Override
  public CompletableFuture<ExecutionProgramResult> callContract(TransactionContext context)
      throws ContractValidateException, VMIllegalException {

    return CompletableFuture.supplyAsync(
        () -> {
          logger.debug(
              "Calling contract with remote Rust EVM: {}", context.getTrxCap().getTransactionId());

          // TODO: Implement in Task 2 with ExecutionGrpcClient

          // Placeholder implementation
          logger.warn("Remote contract call not yet implemented - returning placeholder result");
          ExecutionProgramResult result = new ExecutionProgramResult();
          result.setRuntimeError("Remote contract call not yet implemented");
          result.setRevert();
          result.setResultCode(contractResult.UNKNOWN);
          return result;
        });
  }

  @Override
  public CompletableFuture<Long> estimateEnergy(TransactionContext context)
      throws ContractValidateException {

    return CompletableFuture.supplyAsync(
        () -> {
          logger.debug(
              "Estimating energy with remote Rust EVM: {}", context.getTrxCap().getTransactionId());

          // TODO: Implement in Task 2 with ExecutionGrpcClient

          // Placeholder implementation
          logger.warn("Remote energy estimation not yet implemented - returning 0");
          return 0L;
        });
  }

  @Override
  public CompletableFuture<byte[]> getCode(byte[] address, String snapshotId) {
    return CompletableFuture.supplyAsync(
        () -> {
          logger.debug("Getting code for address: {} via remote service", address);

          // TODO: Implement in Task 2 with ExecutionGrpcClient

          // Placeholder implementation
          logger.warn("Remote getCode not yet implemented - returning empty");
          return new byte[0];
        });
  }

  @Override
  public CompletableFuture<byte[]> getStorageAt(byte[] address, byte[] key, String snapshotId) {
    return CompletableFuture.supplyAsync(
        () -> {
          logger.debug("Getting storage at address: {}, key: {} via remote service", address, key);

          // TODO: Implement in Task 2 with ExecutionGrpcClient

          // Placeholder implementation
          logger.warn("Remote getStorageAt not yet implemented - returning empty");
          return new byte[0];
        });
  }

  @Override
  public CompletableFuture<Long> getNonce(byte[] address, String snapshotId) {
    return CompletableFuture.supplyAsync(
        () -> {
          logger.debug("Getting nonce for address: {} via remote service", address);

          // TODO: Implement in Task 2 with ExecutionGrpcClient

          // Placeholder implementation
          logger.warn("Remote getNonce not yet implemented - returning 0");
          return 0L;
        });
  }

  @Override
  public CompletableFuture<byte[]> getBalance(byte[] address, String snapshotId) {
    return CompletableFuture.supplyAsync(
        () -> {
          logger.debug("Getting balance for address: {} via remote service", address);

          // TODO: Implement in Task 2 with ExecutionGrpcClient

          // Placeholder implementation
          logger.warn("Remote getBalance not yet implemented - returning empty");
          return new byte[0];
        });
  }

  @Override
  public CompletableFuture<String> createSnapshot() {
    return CompletableFuture.supplyAsync(
        () -> {
          logger.debug("Creating EVM snapshot via remote service");

          // TODO: Implement in Task 2 with ExecutionGrpcClient

          // Placeholder implementation
          logger.warn("Remote createSnapshot not yet implemented - returning placeholder");
          return "remote_snapshot_" + System.currentTimeMillis();
        });
  }

  @Override
  public CompletableFuture<Boolean> revertToSnapshot(String snapshotId) {
    return CompletableFuture.supplyAsync(
        () -> {
          logger.debug("Reverting to snapshot: {} via remote service", snapshotId);

          // TODO: Implement in Task 2 with ExecutionGrpcClient

          // Placeholder implementation
          logger.warn("Remote revertToSnapshot not yet implemented - returning false");
          return false;
        });
  }

  @Override
  public CompletableFuture<HealthStatus> healthCheck() {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            logger.debug("Checking health of remote execution service at {}:{}", host, port);

            // TODO: Implement in Task 2 with ExecutionGrpcClient
            // Call grpcClient.healthCheck()

            // Placeholder implementation
            logger.warn("Remote health check not yet implemented - returning unhealthy");
            return new HealthStatus(false, "Remote execution service not yet implemented");

          } catch (Exception e) {
            logger.error("Remote execution health check failed", e);
            return new HealthStatus(
                false, "Remote execution health check failed: " + e.getMessage());
          }
        });
  }

  @Override
  public void registerMetricsCallback(MetricsCallback callback) {
    this.metricsCallback = callback;
    logger.info("Registered metrics callback for remote execution");
  }

  /**
   * Get the configured host.
   *
   * @return Remote host
   */
  public String getHost() {
    return host;
  }

  /**
   * Get the configured port.
   *
   * @return Remote port
   */
  public int getPort() {
    return port;
  }

  /** Shutdown the remote connection. */
  public void shutdown() {
    logger.info("Shutting down remote execution SPI");
    if (grpcClient != null) {
      grpcClient.shutdown();
    }
  }

  /** Build ExecuteTransactionRequest from TransactionContext. */
  private ExecuteTransactionRequest buildExecuteTransactionRequest(TransactionContext context) {
    try {
      TransactionCapsule trxCap = context.getTrxCap();
      Transaction transaction = trxCap.getInstance();
      Transaction.Contract contract = transaction.getRawData().getContract(0);
      Any contractParameter = contract.getParameter();

      // Extract transaction data based on contract type
      byte[] fromAddress = trxCap.getOwnerAddress();
      byte[] toAddress = new byte[20]; // Default empty address
      byte[] data = new byte[0]; // Default empty data
      long value = 0; // Default zero value
      long energyLimit = transaction.getRawData().getFeeLimit();
      long energyPrice = 1; // Default energy price
      long nonce = 0; // TRON doesn't use nonce like Ethereum

      // Extract specific data based on contract type
      switch (contract.getType()) {
        case TransferContract:
          TransferContract transferContract = contractParameter.unpack(TransferContract.class);
          toAddress = transferContract.getToAddress().toByteArray();
          value = transferContract.getAmount();
          break;

        case TransferAssetContract:
          TransferAssetContract transferAssetContract =
              contractParameter.unpack(TransferAssetContract.class);
          toAddress = transferAssetContract.getToAddress().toByteArray();
          value = transferAssetContract.getAmount();
          break;

        case CreateSmartContract:
          CreateSmartContract createContract = contractParameter.unpack(CreateSmartContract.class);
          if (createContract.getNewContract() != null) {
            toAddress = new byte[20]; // Contract creation uses zero address
            data = createContract.getNewContract().getBytecode().toByteArray();
            value = createContract.getNewContract().getCallValue();
          }
          break;

        case TriggerSmartContract:
          TriggerSmartContract triggerContract =
              contractParameter.unpack(TriggerSmartContract.class);
          toAddress = triggerContract.getContractAddress().toByteArray();
          data = triggerContract.getData().toByteArray();
          value = triggerContract.getCallValue();
          break;

        default:
          // For other contract types, use default values
          logger.debug("Using default values for contract type: {}", contract.getType());
          break;
      }

      // Build the transaction
      TronTransaction.Builder txBuilder =
          TronTransaction.newBuilder()
              .setFrom(ByteString.copyFrom(fromAddress))
              .setTo(ByteString.copyFrom(toAddress))
              .setValue(ByteString.copyFrom(longToBytes32(value)))
              .setData(ByteString.copyFrom(data))
              .setEnergyLimit(energyLimit)
              .setEnergyPrice(energyPrice)
              .setNonce(nonce);

      // Build the execution context
      BlockCapsule blockCap = context.getBlockCap();
      long blockNumber = blockCap != null ? blockCap.getNum() : 0;
      long blockTimestamp = blockCap != null ? blockCap.getTimeStamp() : System.currentTimeMillis();
      byte[] blockHash = blockCap != null ? blockCap.getBlockId().getBytes() : new byte[32];
      byte[] coinbase =
          blockCap != null ? blockCap.getWitnessAddress().toByteArray() : new byte[20];

      ExecutionContext.Builder contextBuilder =
          ExecutionContext.newBuilder()
              .setBlockNumber(blockNumber)
              .setBlockTimestamp(blockTimestamp)
              .setBlockHash(ByteString.copyFrom(blockHash))
              .setCoinbase(ByteString.copyFrom(coinbase))
              .setEnergyLimit(energyLimit)
              .setEnergyPrice(energyPrice);

      return ExecuteTransactionRequest.newBuilder()
          .setDatabase("default") // TODO: Get actual database name from configuration
          .setTransaction(txBuilder.build())
          .setContext(contextBuilder.build())
          .build();

    } catch (Exception e) {
      logger.error("Failed to build ExecuteTransactionRequest", e);
      // Fallback to minimal request to avoid complete failure
      return ExecuteTransactionRequest.newBuilder()
          .setDatabase("default")
          .setTransaction(
              TronTransaction.newBuilder()
                  .setFrom(ByteString.copyFrom(new byte[20]))
                  .setTo(ByteString.copyFrom(new byte[20]))
                  .setValue(ByteString.copyFrom(new byte[32]))
                  .setData(ByteString.copyFrom(new byte[0]))
                  .setEnergyLimit(1000000)
                  .setEnergyPrice(1)
                  .setNonce(0)
                  .build())
          .setContext(
              ExecutionContext.newBuilder()
                  .setBlockNumber(0)
                  .setBlockTimestamp(System.currentTimeMillis())
                  .setBlockHash(ByteString.copyFrom(new byte[32]))
                  .setCoinbase(ByteString.copyFrom(new byte[20]))
                  .setEnergyLimit(1000000)
                  .setEnergyPrice(1)
                  .build())
          .build();
    }
  }

  /** Convert long value to 32-byte array (big-endian). */
  private byte[] longToBytes32(long value) {
    byte[] result = new byte[32];
    for (int i = 7; i >= 0; i--) {
      result[31 - i] = (byte) (value >>> (i * 8));
    }
    return result;
  }

  /** 
   * Serialize AccountInfo to byte array for state synchronization.
   * Format: [balance(32)] + [nonce(8)] + [code_hash(32)] + [code_length(4)] + [code(variable)]
   */
  private byte[] serializeAccountInfo(tron.backend.BackendOuterClass.AccountInfo accountInfo) {
    if (accountInfo == null) {
      return new byte[0]; // Empty for null account (creation/deletion cases)
    }
    
    try {
      // Extract account info components
      byte[] balance = accountInfo.getBalance().toByteArray();
      long nonce = accountInfo.getNonce();
      byte[] codeHash = accountInfo.getCodeHash().toByteArray();
      byte[] code = accountInfo.getCode().toByteArray();
      
      // Ensure balance is 32 bytes (pad with zeros if needed)
      byte[] paddedBalance = new byte[32];
      if (balance.length > 0) {
        System.arraycopy(balance, 0, paddedBalance, Math.max(0, 32 - balance.length), Math.min(balance.length, 32));
      }
      
      // Convert nonce to 8 bytes (big-endian)
      byte[] nonceBytes = new byte[8];
      for (int i = 7; i >= 0; i--) {
        nonceBytes[7 - i] = (byte) (nonce >>> (i * 8));
      }
      
      // Ensure code hash is 32 bytes (pad with zeros if needed)
      byte[] paddedCodeHash = new byte[32];
      if (codeHash.length > 0) {
        System.arraycopy(codeHash, 0, paddedCodeHash, Math.max(0, 32 - codeHash.length), Math.min(codeHash.length, 32));
      }
      
      // Code length as 4 bytes (big-endian)
      byte[] codeLengthBytes = new byte[4];
      int codeLength = code.length;
      for (int i = 3; i >= 0; i--) {
        codeLengthBytes[3 - i] = (byte) (codeLength >>> (i * 8));
      }
      
      // Combine all components
      byte[] result = new byte[32 + 8 + 32 + 4 + code.length];
      int offset = 0;
      
      System.arraycopy(paddedBalance, 0, result, offset, 32);
      offset += 32;
      
      System.arraycopy(nonceBytes, 0, result, offset, 8);
      offset += 8;
      
      System.arraycopy(paddedCodeHash, 0, result, offset, 32);
      offset += 32;
      
      System.arraycopy(codeLengthBytes, 0, result, offset, 4);
      offset += 4;
      
      if (code.length > 0) {
        System.arraycopy(code, 0, result, offset, code.length);
      }
      
      logger.debug("Serialized AccountInfo: balance={} bytes, nonce={}, codeHash={} bytes, code={} bytes, total={} bytes",
          balance.length, nonce, codeHash.length, code.length, result.length);
      
      return result;
      
    } catch (Exception e) {
      logger.error("Failed to serialize AccountInfo", e);
      return new byte[0]; // Return empty array on error
    }
  }

  /** Convert ExecuteTransactionResponse to ExecutionResult. */
  private ExecutionResult convertExecuteTransactionResponse(ExecuteTransactionResponse response) {
    if (!response.getSuccess()) {
      return new ExecutionResult(
          false, // success
          new byte[0], // returnData
          0, // energyUsed
          0, // energyRefunded
          new ArrayList<>(), // stateChanges
          new ArrayList<>(), // logs
          response.getErrorMessage(), // errorMessage
          0 // bandwidthUsed
          );
    }

    tron.backend.BackendOuterClass.ExecutionResult protoResult = response.getResult();
    List<StateChange> stateChanges = new ArrayList<>();
    List<LogEntry> logs = new ArrayList<>();

    // Convert protobuf state changes to ExecutionSPI state changes
    for (tron.backend.BackendOuterClass.StateChange protoChange :
        protoResult.getStateChangesList()) {
      
      // Handle the oneof union type
      if (protoChange.hasStorageChange()) {
        // Handle storage change
        tron.backend.BackendOuterClass.StorageChange storageChange = protoChange.getStorageChange();
        StateChange stateChange = new StateChange(
            storageChange.getAddress().toByteArray(),
            storageChange.getKey().toByteArray(),
            storageChange.getOldValue().toByteArray(),
            storageChange.getNewValue().toByteArray());
        stateChanges.add(stateChange);
        
        logger.debug("Remote execution storage change - Address: {}, Key: {}, OldValue: {}, NewValue: {}",
            storageChange.getAddress().toByteArray(),
            storageChange.getKey().toByteArray(),
            storageChange.getOldValue().toByteArray(),
            storageChange.getNewValue().toByteArray());
            
      } else if (protoChange.hasAccountChange()) {
        // Handle account change - serialize AccountInfo properly
        tron.backend.BackendOuterClass.AccountChange accountChange = protoChange.getAccountChange();
        
        // For account changes, we'll use empty key to indicate it's an account-level change
        // and serialize account info in the values
        byte[] address = accountChange.getAddress().toByteArray();
        byte[] emptyKey = new byte[0]; // Empty key indicates account change
        byte[] oldValue = serializeAccountInfo(accountChange.getOldAccount());
        byte[] newValue = serializeAccountInfo(accountChange.getNewAccount());
        
        StateChange stateChange = new StateChange(address, emptyKey, oldValue, newValue);
        stateChanges.add(stateChange);
        
        logger.debug("Remote execution account change - Address: {}, IsCreation: {}, IsDeletion: {}, OldValue size: {}, NewValue size: {}",
            address,
            accountChange.getIsCreation(),
            accountChange.getIsDeletion(),
            oldValue.length,
            newValue.length);
      }
    }
    
    logger.debug("Remote execution returned {} state changes and {} logs",
        stateChanges.size(), logs.size());

    // Convert protobuf logs to ExecutionSPI logs
    for (tron.backend.BackendOuterClass.LogEntry protoLog : protoResult.getLogsList()) {
      List<byte[]> topics = new ArrayList<>();
      for (ByteString topic : protoLog.getTopicsList()) {
        topics.add(topic.toByteArray());
      }
      logs.add(
          new LogEntry(
              protoLog.getAddress().toByteArray(), topics, protoLog.getData().toByteArray()));
    }

    // Report metrics if callback is registered
    if (metricsCallback != null) {
      metricsCallback.onMetric("remote.energy_used", protoResult.getEnergyUsed());
      metricsCallback.onMetric(
          "remote.success",
          protoResult.getStatus() == tron.backend.BackendOuterClass.ExecutionResult.Status.SUCCESS
              ? 1.0
              : 0.0);
    }

    return new ExecutionResult(
        protoResult.getStatus() == tron.backend.BackendOuterClass.ExecutionResult.Status.SUCCESS,
        protoResult.getReturnData().toByteArray(),
        protoResult.getEnergyUsed(),
        protoResult.getEnergyRefunded(),
        stateChanges,
        logs,
        protoResult.getErrorMessage(),
        protoResult.getBandwidthUsed());
  }
}
