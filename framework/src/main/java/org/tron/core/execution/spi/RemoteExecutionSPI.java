package org.tron.core.execution.spi;

import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.Comparator;
import java.util.concurrent.CompletableFuture;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.client.ExecutionGrpcClient;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.core.exception.VMIllegalException;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.Protocol.Transaction.Result.contractResult;
import org.tron.protos.contract.AssetIssueContractOuterClass.TransferAssetContract;
import org.tron.protos.contract.BalanceContract.FreezeBalanceContract;
import org.tron.protos.contract.BalanceContract.TransferContract;
import org.tron.protos.contract.Common.ResourceCode;
import org.tron.protos.contract.SmartContractOuterClass.CreateSmartContract;
import org.tron.protos.contract.SmartContractOuterClass.TriggerSmartContract;
import org.tron.protos.contract.WitnessContract.WitnessCreateContract;
import org.tron.protos.contract.WitnessContract.WitnessUpdateContract;
import org.tron.protos.contract.WitnessContract.VoteWitnessContract;
import org.tron.protos.contract.AccountContract.AccountUpdateContract;
import tron.backend.BackendOuterClass.*;

/**
 * Remote execution implementation using the Rust backend service via gRPC. This implementation will
 * be completed in Task 2 with the ExecutionGrpcClient.
 */
public class RemoteExecutionSPI implements ExecutionSPI {
  private static final Logger logger = LoggerFactory.getLogger(RemoteExecutionSPI.class);

  // Canonical keccak256("") for empty code hash parity with embedded
  private static final byte[] KECCAK_EMPTY = new byte[] {
      (byte)0xc5,(byte)0xd2,(byte)0x46,(byte)0x01,(byte)0x86,(byte)0xf7,(byte)0x23,(byte)0x3c,
      (byte)0x92,(byte)0x7e,(byte)0x7d,(byte)0xb2,(byte)0xdc,(byte)0xc7,(byte)0x03,(byte)0xc0,
      (byte)0xe5,(byte)0x00,(byte)0xb6,(byte)0x53,(byte)0xca,(byte)0x82,(byte)0x27,(byte)0x3b,
      (byte)0x7b,(byte)0xfa,(byte)0xd8,(byte)0x04,(byte)0x5d,(byte)0x85,(byte)0xa4,(byte)0x70
  };

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

          } catch (UnsupportedOperationException | IllegalArgumentException e) {
            logger.warn(
                "Remote execution not supported for transaction {}: {}",
                context.getTrxCap().getTransactionId(),
                e.getMessage());
            throw e;
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

      // Determine transaction kind based on contract type
      TxKind txKind; // Will be set based on contract type
      tron.backend.BackendOuterClass.ContractType contractType; // Will be set based on contract type
      byte[] assetId = new byte[0]; // Default empty for TRX transfers

      // Extract specific data based on contract type
      switch (contract.getType()) {
        case TransferContract:
          TransferContract transferContract = contractParameter.unpack(TransferContract.class);
          toAddress = transferContract.getToAddress().toByteArray();
          value = transferContract.getAmount();
          txKind = TxKind.NON_VM; // Simple TRX transfer
          contractType = tron.backend.BackendOuterClass.ContractType.TRANSFER_CONTRACT;
          break;

        case TransferAssetContract:
          // Phase 3 Fix: Keep TRC-10 on Java path until Rust storage can handle TRC-10 ledgers
          boolean trc10RemoteEnabled = Boolean.parseBoolean(System.getProperty("remote.exec.trc10.enabled", "false"));
          if (!trc10RemoteEnabled) {
            logger.debug("TRC-10 remote execution disabled, throwing exception to fallback to Java actuators");
            throw new UnsupportedOperationException("TRC-10 execution via remote backend is disabled. Use -Dremote.exec.trc10.enabled=true to enable.");
          }

          TransferAssetContract transferAssetContract =
              contractParameter.unpack(TransferAssetContract.class);
          toAddress = transferAssetContract.getToAddress().toByteArray();
          value = transferAssetContract.getAmount();
          assetId = transferAssetContract.getAssetName().toByteArray(); // TRC-10 asset ID
          txKind = TxKind.NON_VM; // TRC-10 asset transfer (when enabled)
          contractType = tron.backend.BackendOuterClass.ContractType.TRANSFER_ASSET_CONTRACT;
          break;

        case AssetIssueContract:
          // TRC-10 Asset Issue: Gate behind the same TRC-10 feature flag
          boolean assetIssueRemoteEnabled = Boolean.parseBoolean(System.getProperty("remote.exec.trc10.enabled", "false"));
          if (!assetIssueRemoteEnabled) {
            logger.debug("TRC-10 AssetIssue remote execution disabled, throwing exception to fallback to Java actuators");
            throw new UnsupportedOperationException("AssetIssue execution via remote backend is disabled. Use -Dremote.exec.trc10.enabled=true to enable.");
          }

          org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract assetIssueContract =
              contractParameter.unpack(org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract.class);
          toAddress = new byte[0]; // System contract, no recipient
          value = 0; // Asset issue fee is charged, but not a value transfer
          data = assetIssueContract.toByteArray(); // Send full proto bytes for Rust parsing
          txKind = TxKind.NON_VM; // TRC-10 asset issuance
          contractType = tron.backend.BackendOuterClass.ContractType.ASSET_ISSUE_CONTRACT;
          logger.debug(
              "Mapped AssetIssueContract to remote request; owner={}, name={}, total_supply={}",
              org.tron.common.utils.ByteArray.toHexString(fromAddress),
              assetIssueContract.getName().toStringUtf8(),
              assetIssueContract.getTotalSupply());
          break;

        case CreateSmartContract:
          CreateSmartContract createContract = contractParameter.unpack(CreateSmartContract.class);
          if (createContract.getNewContract() != null) {
            toAddress = new byte[20]; // Contract creation uses zero address
            data = createContract.getNewContract().getBytecode().toByteArray();
            value = createContract.getNewContract().getCallValue();
          }
          txKind = TxKind.VM; // Smart contract creation requires VM
          contractType = tron.backend.BackendOuterClass.ContractType.CREATE_SMART_CONTRACT;
          break;

        case TriggerSmartContract:
          TriggerSmartContract triggerContract =
              contractParameter.unpack(TriggerSmartContract.class);
          toAddress = triggerContract.getContractAddress().toByteArray();
          data = triggerContract.getData().toByteArray();
          value = triggerContract.getCallValue();
          txKind = TxKind.VM; // Smart contract invocation requires VM
          contractType = tron.backend.BackendOuterClass.ContractType.TRIGGER_SMART_CONTRACT;
          break;

        case FreezeBalanceContract:
          FreezeBalanceContract freezeContract =
              contractParameter.unpack(FreezeBalanceContract.class);
          toAddress = new byte[0];
          data = freezeContract.toByteArray();
          txKind = TxKind.NON_VM;
          contractType = tron.backend.BackendOuterClass.ContractType.FREEZE_BALANCE_CONTRACT;
          logger.debug(
              "Mapped FreezeBalanceContract to remote request; owner={}, amount={}, duration={}",
              org.tron.common.utils.ByteArray.toHexString(fromAddress),
              freezeContract.getFrozenBalance(),
              freezeContract.getFrozenDuration());
          break;

        case WitnessCreateContract:
          WitnessCreateContract witnessCreateContract =
              contractParameter.unpack(WitnessCreateContract.class);
          // For witness creation, do NOT set toAddress to 0x00 - leave it empty
          toAddress = new byte[0]; // Empty instead of zero address
          // Include URL in execution data for Rust backend processing
          data = witnessCreateContract.getUrl().toByteArray();
          txKind = TxKind.NON_VM; // System contract
          contractType = tron.backend.BackendOuterClass.ContractType.WITNESS_CREATE_CONTRACT;
          break;

        case WitnessUpdateContract:
          WitnessUpdateContract witnessUpdateContract =
              contractParameter.unpack(WitnessUpdateContract.class);
          // For witness update, do NOT set toAddress to 0x00 - leave it empty
          toAddress = new byte[0]; // Empty instead of zero address
          // Include update URL in execution data for Rust backend processing
          data = witnessUpdateContract.getUpdateUrl().toByteArray();
          txKind = TxKind.NON_VM; // System contract
          contractType = tron.backend.BackendOuterClass.ContractType.WITNESS_UPDATE_CONTRACT;
          break;

        case VoteWitnessContract:
          VoteWitnessContract voteWitnessContract =
              contractParameter.unpack(VoteWitnessContract.class);
          // For vote witness, do NOT set toAddress to 0x00 - leave it empty
          toAddress = new byte[0]; // Empty instead of zero address
          // Serialize vote data for Rust backend processing (simplified for now)
          data = voteWitnessContract.toByteArray(); // Full contract data
          txKind = TxKind.NON_VM; // System contract
          contractType = tron.backend.BackendOuterClass.ContractType.VOTE_WITNESS_CONTRACT;
          break;

        case AccountUpdateContract:
          AccountUpdateContract accountUpdateContract =
              contractParameter.unpack(AccountUpdateContract.class);
          // Set fromAddress to owner
          fromAddress = accountUpdateContract.getOwnerAddress().toByteArray();
          // Leave toAddress empty (do not use zero address)
          toAddress = new byte[0];
          // Set value to 0
          value = 0;
          // Set data to account name bytes
          data = accountUpdateContract.getAccountName().toByteArray();
          txKind = TxKind.NON_VM;
          contractType = tron.backend.BackendOuterClass.ContractType.ACCOUNT_UPDATE_CONTRACT;
          logger.debug("Mapped AccountUpdateContract to remote request; owner={}, data_len={}",
              org.tron.common.utils.ByteArray.toHexString(accountUpdateContract.getOwnerAddress().toByteArray()), data.length);
          break;

        default:
          // Remove TRANSFER fallback - throw exception to fall back to embedded
          logger.error("Contract type {} not mapped to remote; falling back to embedded", contract.getType());
          throw new UnsupportedOperationException(contract.getType() + " not mapped to remote; falling back to embedded");
      }

      // Log transaction classification
      logger.debug("Classified transaction {} as {}: contract_type={}", 
          context.getTrxCap().getTransactionId().toString(), 
          txKind.name(), 
          contract.getType().name());

      // Build the transaction
      TronTransaction.Builder txBuilder =
          TronTransaction.newBuilder()
              .setFrom(ByteString.copyFrom(fromAddress))
              .setTo(ByteString.copyFrom(toAddress))
              .setValue(ByteString.copyFrom(longToBytes32(value)))
              .setData(ByteString.copyFrom(data))
              .setEnergyLimit(energyLimit)
              .setEnergyPrice(energyPrice)
              .setNonce(nonce)
              .setTxKind(txKind) // Set the transaction kind for proper processing
              .setContractType(contractType) // Phase 3: Add detailed contract type
              .setAssetId(ByteString.copyFrom(assetId)); // Phase 3: Add asset ID for TRC-10

      // Build the execution context - Phase 3 Fix: Require BlockCapsule for deterministic context
      BlockCapsule blockCap = context.getBlockCap();
      if (blockCap == null) {
        logger.warn("BlockCapsule is null - skipping transaction to avoid non-deterministic context");
        throw new IllegalArgumentException("BlockCapsule is required for deterministic execution context");
      }
      
      long blockNumber = blockCap.getNum();
      long blockTimestamp = blockCap.getTimeStamp();
      byte[] blockHash = blockCap.getBlockId().getBytes();
      byte[] coinbase = blockCap.getWitnessAddress().toByteArray();

      String transactionId = context.getTrxCap().getTransactionId().toString();

      ExecutionContext.Builder contextBuilder =
          ExecutionContext.newBuilder()
              .setBlockNumber(blockNumber)
              .setBlockTimestamp(blockTimestamp)
              .setBlockHash(ByteString.copyFrom(blockHash))
              .setCoinbase(ByteString.copyFrom(coinbase))
              .setEnergyLimit(energyLimit)
              .setEnergyPrice(energyPrice)
              .setTransactionId(transactionId);

      // Collect pre-execution AEXT snapshots for hybrid mode
      List<AccountAextSnapshot> preExecAextList = collectPreExecutionAext(context, fromAddress, toAddress, contract.getType());

      return ExecuteTransactionRequest.newBuilder()
          .setTransaction(txBuilder.build())
          .setContext(contextBuilder.build())
          .addAllPreExecutionAext(preExecAextList)
          .build();

    } catch (UnsupportedOperationException | IllegalArgumentException e) {
      throw e;
    } catch (Exception e) {
      logger.error("Failed to build ExecuteTransactionRequest", e);
      throw new RuntimeException("Failed to build ExecuteTransactionRequest", e);
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
   *         + optional [AEXT tail] for resource usage (when proto fields are present)
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

      // Ensure code hash is 32 bytes and normalize empty-code hash to KECCAK_EMPTY
      byte[] paddedCodeHash = new byte[32];
      boolean codeIsEmpty = (code == null || code.length == 0);
      boolean codeHashAllZeroOrEmpty = true;
      if (codeHash.length > 0) {
        // Check if codeHash is all zero bytes
        codeHashAllZeroOrEmpty = true;
        for (byte b : codeHash) {
          if (b != 0) { codeHashAllZeroOrEmpty = false; break; }
        }
        System.arraycopy(codeHash, 0, paddedCodeHash, Math.max(0, 32 - codeHash.length), Math.min(codeHash.length, 32));
      }
      if (codeIsEmpty && codeHashAllZeroOrEmpty) {
        // Overwrite with canonical empty-code hash
        System.arraycopy(KECCAK_EMPTY, 0, paddedCodeHash, 0, 32);
      }

      // Code length as 4 bytes (big-endian)
      byte[] codeLengthBytes = new byte[4];
      int codeLength = code.length;
      for (int i = 3; i >= 0; i--) {
        codeLengthBytes[3 - i] = (byte) (codeLength >>> (i * 8));
      }

      // Check if AEXT tail should be appended (based on property and proto field presence)
      boolean includeResourceUsage = Boolean.parseBoolean(
          System.getProperty("remote.exec.accountinfo.resources.enabled", "true"));
      byte[] aextTail = null;

      // Check presence of any optional resource field; append AEXT only if present
      boolean hasResourceFields =
          accountInfo.hasNetUsage()
              || accountInfo.hasFreeNetUsage()
              || accountInfo.hasEnergyUsage()
              || accountInfo.hasLatestConsumeTime()
              || accountInfo.hasLatestConsumeFreeTime()
              || accountInfo.hasLatestConsumeTimeForEnergy()
              || accountInfo.hasNetWindowSize()
              || accountInfo.hasNetWindowOptimized()
              || accountInfo.hasEnergyWindowSize()
              || accountInfo.hasEnergyWindowOptimized();

      if (includeResourceUsage && hasResourceFields) {
        try {
          aextTail = serializeAextTailFromProto(accountInfo);
          logger.debug("Appending AEXT tail ({} bytes) from proto resource fields", aextTail.length);
        } catch (Exception e) {
          logger.warn("Failed to serialize AEXT tail from proto, falling back to base format: {}", e.getMessage());
          // Continue with base format only
        }
      }

      // Calculate total size
      int baseSize = 32 + 8 + 32 + 4 + code.length;
      int totalSize = baseSize + (aextTail != null ? aextTail.length : 0);

      // Combine all components
      byte[] result = new byte[totalSize];
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
        offset += code.length;
      }

      // Append AEXT tail if present
      if (aextTail != null && aextTail.length > 0) {
        System.arraycopy(aextTail, 0, result, offset, aextTail.length);
      }

      logger.debug("Serialized AccountInfo: balance={} bytes, nonce={}, codeHash={} bytes, code={} bytes, aext={} bytes, total={} bytes",
          balance.length, nonce, codeHash.length, code.length, (aextTail != null ? aextTail.length : 0), result.length);

      return result;

    } catch (Exception e) {
      logger.error("Failed to serialize AccountInfo", e);
      return new byte[0]; // Return empty array on error
    }
  }

  /**
   * Serialize AEXT (Account EXTension) v1 tail from proto AccountInfo resource fields.
   * Format: magic(4) + version(2) + length(2) + payload(68)
   * Total: 76 bytes
   */
  private byte[] serializeAextTailFromProto(tron.backend.BackendOuterClass.AccountInfo accountInfo) {
    // AEXT v1 payload size: 8*8 (i64 fields) + 1 + 1 (booleans) + 2 (padding) = 68 bytes
    int payloadSize = 68;
    int totalSize = 4 + 2 + 2 + payloadSize; // magic + version + length + payload = 76 bytes
    byte[] result = new byte[totalSize];
    int offset = 0;

    // Magic: "AEXT" (0x41 0x45 0x58 0x54)
    result[offset++] = 0x41; // 'A'
    result[offset++] = 0x45; // 'E'
    result[offset++] = 0x58; // 'X'
    result[offset++] = 0x54; // 'T'

    // Version: 1 (u16 big-endian)
    result[offset++] = 0x00;
    result[offset++] = 0x01;

    // Length: 68 (u16 big-endian)
    result[offset++] = 0x00;
    result[offset++] = 0x44; // 0x44 = 68 in decimal

    // Payload: resource usage fields from proto (all i64 big-endian except booleans)
    offset = writeI64BigEndian(result, offset, accountInfo.getNetUsage());
    offset = writeI64BigEndian(result, offset, accountInfo.getFreeNetUsage());
    offset = writeI64BigEndian(result, offset, accountInfo.getEnergyUsage());
    offset = writeI64BigEndian(result, offset, accountInfo.getLatestConsumeTime());
    offset = writeI64BigEndian(result, offset, accountInfo.getLatestConsumeFreeTime());
    offset = writeI64BigEndian(result, offset, accountInfo.getLatestConsumeTimeForEnergy());
    offset = writeI64BigEndian(result, offset, accountInfo.getNetWindowSize());
    offset = writeI64BigEndian(result, offset, accountInfo.getEnergyWindowSize());

    // Booleans
    result[offset++] = (byte) (accountInfo.getNetWindowOptimized() ? 0x01 : 0x00);
    result[offset++] = (byte) (accountInfo.getEnergyWindowOptimized() ? 0x01 : 0x00);

    // Reserved/padding (2 bytes)
    result[offset++] = 0x00;
    result[offset++] = 0x00;

    logger.debug("Serialized AEXT v1 from proto: netUsage={}, freeNetUsage={}, energyUsage={}, times=[{},{},{}], windows=[{},{}], optimized=[{},{}]",
                 accountInfo.getNetUsage(), accountInfo.getFreeNetUsage(), accountInfo.getEnergyUsage(),
                 accountInfo.getLatestConsumeTime(), accountInfo.getLatestConsumeFreeTime(), accountInfo.getLatestConsumeTimeForEnergy(),
                 accountInfo.getNetWindowSize(), accountInfo.getEnergyWindowSize(),
                 accountInfo.getNetWindowOptimized(), accountInfo.getEnergyWindowOptimized());

    return result;
  }

  /**
   * Write an i64 value in big-endian format to the byte array.
   * Returns the new offset after writing.
   */
  private int writeI64BigEndian(byte[] buffer, int offset, long value) {
    for (int i = 7; i >= 0; i--) {
      buffer[offset++] = (byte) (value >>> (i * 8));
    }
    return offset;
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
          0, // bandwidthUsed
          new ArrayList<>(), // freezeChanges
          new ArrayList<>(), // globalResourceChanges
          new ArrayList<>(), // trc10Changes
          new ArrayList<>() // delegationChanges
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
        // Respect field presence: if old/new account is not set, keep value empty
        byte[] oldValue = accountChange.hasOldAccount()
            ? serializeAccountInfo(accountChange.getOldAccount())
            : new byte[0];
        byte[] newValue = accountChange.hasNewAccount()
            ? serializeAccountInfo(accountChange.getNewAccount())
            : new byte[0];
        
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
    
    logger.info("Remote execution returned {} state changes and {} logs",
        stateChanges.size(), logs.size());
    for (int i = 0; i < stateChanges.size(); i++) {
      StateChange change = stateChanges.get(i);
      logger.info("  State change {}: address={}, key_len={}, oldValue_len={}, newValue_len={}", 
          i, 
          change.getAddress() != null ? change.getAddress().length : 0,
          change.getKey() != null ? change.getKey().length : 0,
          change.getOldValue() != null ? change.getOldValue().length : 0,
          change.getNewValue() != null ? change.getNewValue().length : 0);
    }

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

    // Convert protobuf freeze changes to ExecutionSPI freeze changes (Phase 2)
    List<FreezeLedgerChange> freezeChanges = new ArrayList<>();
    for (tron.backend.BackendOuterClass.FreezeLedgerChange protoFreeze : protoResult.getFreezeChangesList()) {
      // Convert proto Resource enum to ExecutionSPI Resource enum
      FreezeLedgerChange.Resource resource;
      switch (protoFreeze.getResource()) {
        case BANDWIDTH:
          resource = FreezeLedgerChange.Resource.BANDWIDTH;
          break;
        case ENERGY:
          resource = FreezeLedgerChange.Resource.ENERGY;
          break;
        case TRON_POWER:
          resource = FreezeLedgerChange.Resource.TRON_POWER;
          break;
        default:
          logger.warn("Unknown freeze resource type: {}, skipping entry", protoFreeze.getResource());
          // Skip unknown resource types to avoid misapplication
          continue;
      }

      FreezeLedgerChange freezeChange = new FreezeLedgerChange(
          protoFreeze.getOwnerAddress().toByteArray(),
          resource,
          protoFreeze.getAmount(),
          protoFreeze.getExpirationMs(),
          protoFreeze.getV2Model());
      freezeChanges.add(freezeChange);

      logger.debug("Parsed freeze change: owner={}, resource={}, amount={}, expiration={}, v2={}",
          org.tron.common.utils.ByteArray.toHexString(protoFreeze.getOwnerAddress().toByteArray()),
          resource,
          protoFreeze.getAmount(),
          protoFreeze.getExpirationMs(),
          protoFreeze.getV2Model());
    }

    // Convert protobuf global resource changes (Phase 2)
    List<GlobalResourceTotalsChange> globalResourceChanges = new ArrayList<>();
    for (tron.backend.BackendOuterClass.GlobalResourceTotalsChange protoGlobal : protoResult.getGlobalResourceChangesList()) {
      GlobalResourceTotalsChange globalChange = new GlobalResourceTotalsChange(
          protoGlobal.getTotalNetWeight(),
          protoGlobal.getTotalNetLimit(),
          protoGlobal.getTotalEnergyWeight(),
          protoGlobal.getTotalEnergyLimit());
      globalResourceChanges.add(globalChange);

      logger.debug("Parsed global resource change: netWeight={}, netLimit={}, energyWeight={}, energyLimit={}",
          protoGlobal.getTotalNetWeight(),
          protoGlobal.getTotalNetLimit(),
          protoGlobal.getTotalEnergyWeight(),
          protoGlobal.getTotalEnergyLimit());
    }

    // Deterministically order freeze changes: by resource, then by owner address bytes
    if (freezeChanges.size() > 1) {
      freezeChanges.sort(new Comparator<FreezeLedgerChange>() {
        @Override
        public int compare(FreezeLedgerChange a, FreezeLedgerChange b) {
          int cmp = Integer.compare(a.getResource().getValue(), b.getResource().getValue());
          if (cmp != 0) {
            return cmp;
          }
          byte[] aa = a.getOwnerAddress();
          byte[] bb = b.getOwnerAddress();
          int min = Math.min(aa != null ? aa.length : 0, bb != null ? bb.length : 0);
          for (int i = 0; i < min; i++) {
            int da = aa[i] & 0xFF;
            int db = bb[i] & 0xFF;
            if (da != db) {
              return Integer.compare(da, db);
            }
          }
          // If equal up to min length, shorter array comes first
          return Integer.compare(aa != null ? aa.length : 0, bb != null ? bb.length : 0);
        }
      });
    }

    // Convert protobuf TRC-10 changes (Phase 2: full TRC-10 ledger semantics)
    List<Trc10Change> trc10Changes = new ArrayList<>();
    for (tron.backend.BackendOuterClass.Trc10Change protoTrc10 : protoResult.getTrc10ChangesList()) {
      // Handle the oneof union type
      if (protoTrc10.hasAssetIssued()) {
        tron.backend.BackendOuterClass.Trc10AssetIssued protoAssetIssued = protoTrc10.getAssetIssued();

        Trc10AssetIssued assetIssued = new Trc10AssetIssued(
            protoAssetIssued.getOwnerAddress().toByteArray(),
            protoAssetIssued.getName().toByteArray(),
            protoAssetIssued.getAbbr().toByteArray(),
            protoAssetIssued.getTotalSupply(),
            protoAssetIssued.getTrxNum(),
            protoAssetIssued.getPrecision(),
            protoAssetIssued.getNum(),
            protoAssetIssued.getStartTime(),
            protoAssetIssued.getEndTime(),
            protoAssetIssued.getDescription().toByteArray(),
            protoAssetIssued.getUrl().toByteArray(),
            protoAssetIssued.getFreeAssetNetLimit(),
            protoAssetIssued.getPublicFreeAssetNetLimit(),
            protoAssetIssued.getPublicFreeAssetNetUsage(),
            protoAssetIssued.getPublicLatestFreeNetTime(),
            protoAssetIssued.getTokenId());

        trc10Changes.add(new Trc10Change(assetIssued));

        logger.debug("Parsed TRC-10 asset issued: owner={}, name={}, totalSupply={}, precision={}, tokenId={}",
            org.tron.common.utils.ByteArray.toHexString(protoAssetIssued.getOwnerAddress().toByteArray()),
            new String(protoAssetIssued.getName().toByteArray(), java.nio.charset.StandardCharsets.UTF_8),
            protoAssetIssued.getTotalSupply(),
            protoAssetIssued.getPrecision(),
            protoAssetIssued.getTokenId());
      }
    }

    // Convert protobuf delegation changes (Phase 2: delegation parity)
    List<DelegationChange> delegationChanges = new ArrayList<>();
    for (tron.backend.BackendOuterClass.DelegationChange protoDelegation : protoResult.getDelegationChangesList()) {
      // Convert proto Resource enum to ExecutionSPI Resource enum
      DelegationChange.Resource resource;
      switch (protoDelegation.getResource()) {
        case BANDWIDTH:
          resource = DelegationChange.Resource.BANDWIDTH;
          break;
        case ENERGY:
          resource = DelegationChange.Resource.ENERGY;
          break;
        default:
          logger.warn("Unknown delegation resource type: {}, skipping entry", protoDelegation.getResource());
          continue;
      }

      // Convert proto Operation enum to ExecutionSPI Operation enum
      DelegationChange.Operation operation;
      switch (protoDelegation.getOp()) {
        case ADD:
          operation = DelegationChange.Operation.ADD;
          break;
        case REMOVE:
          operation = DelegationChange.Operation.REMOVE;
          break;
        case UNLOCK:
          operation = DelegationChange.Operation.UNLOCK;
          break;
        default:
          logger.warn("Unknown delegation operation type: {}, skipping entry", protoDelegation.getOp());
          continue;
      }

      DelegationChange delegationChange = new DelegationChange(
          protoDelegation.getFromAddress().toByteArray(),
          protoDelegation.getToAddress().toByteArray(),
          resource,
          protoDelegation.getAmount(),
          protoDelegation.getExpireTimeMs(),
          protoDelegation.getV2Model(),
          operation);
      delegationChanges.add(delegationChange);

      logger.debug("Parsed delegation change: from={}, to={}, resource={}, amount={}, expire={}, v2={}, op={}",
          org.tron.common.utils.ByteArray.toHexString(protoDelegation.getFromAddress().toByteArray()),
          org.tron.common.utils.ByteArray.toHexString(protoDelegation.getToAddress().toByteArray()),
          resource,
          protoDelegation.getAmount(),
          protoDelegation.getExpireTimeMs(),
          protoDelegation.getV2Model(),
          operation);
    }

    // Report metrics if callback is registered
    if (metricsCallback != null) {
      metricsCallback.onMetric("remote.energy_used", protoResult.getEnergyUsed());
      metricsCallback.onMetric(
          "remote.success",
          protoResult.getStatus() == tron.backend.BackendOuterClass.ExecutionResult.Status.SUCCESS
              ? 1.0
              : 0.0);
      metricsCallback.onMetric("remote.freeze_changes_count", freezeChanges.size());
    }

    return new ExecutionResult(
        protoResult.getStatus() == tron.backend.BackendOuterClass.ExecutionResult.Status.SUCCESS,
        protoResult.getReturnData().toByteArray(),
        protoResult.getEnergyUsed(),
        protoResult.getEnergyRefunded(),
        stateChanges,
        logs,
        protoResult.getErrorMessage(),
        protoResult.getBandwidthUsed(),
        freezeChanges,
        globalResourceChanges,
        trc10Changes,
        delegationChanges);
  }

  /**
   * Collect pre-execution AEXT snapshots for addresses involved in the transaction.
   * This allows the Rust backend to echo Java's AEXT values in state changes for parity.
   *
   * @param context Transaction context with access to stores
   * @param fromAddress Transaction sender address
   * @param toAddress Transaction recipient address (may be empty for some contracts)
   * @param contractType The type of contract being executed
   * @return List of AEXT snapshots for relevant addresses
   */
  private List<AccountAextSnapshot> collectPreExecutionAext(
      TransactionContext context, byte[] fromAddress, byte[] toAddress, Transaction.Contract.ContractType contractType) {

    List<AccountAextSnapshot> snapshots = new ArrayList<>();

    // Check if AEXT collection is enabled
    boolean enabled = Boolean.parseBoolean(
        System.getProperty("remote.exec.preexec.aext.enabled", "true"));

    if (!enabled) {
      logger.debug("Pre-execution AEXT collection disabled");
      return snapshots;
    }

    // Collect addresses to snapshot
    Set<byte[]> addressesToSnapshot = new HashSet<>();

    // Always include owner/from address
    if (fromAddress != null && fromAddress.length > 0) {
      addressesToSnapshot.add(fromAddress);
    }

    // Include recipient/to address for relevant contract types
    if (toAddress != null && toAddress.length > 0) {
      switch (contractType) {
        case TransferContract:
        case TransferAssetContract:
          addressesToSnapshot.add(toAddress);
          break;
        default:
          // For other contracts, toAddress might be zero or empty
          break;
      }
    }

    // Get AccountStore from context
    try {
      if (context.getStoreFactory() == null ||
          context.getStoreFactory().getChainBaseManager() == null) {
        logger.warn("StoreFactory or ChainBaseManager not available for AEXT collection");
        return snapshots;
      }

      org.tron.core.store.AccountStore accountStore = context.getStoreFactory().getChainBaseManager().getAccountStore();
      if (accountStore == null) {
        logger.warn("AccountStore not available for AEXT collection");
        return snapshots;
      }

      // Collect AEXT for each address
      for (byte[] address : addressesToSnapshot) {
        try {
          AccountCapsule account = accountStore.get(address);
          if (account == null) {
            logger.debug("Account not found for AEXT snapshot: {}",
                org.tron.common.utils.ByteArray.toHexString(address));
            continue;
          }

          // Build AccountAext message
          AccountAext.Builder aextBuilder = AccountAext.newBuilder()
              .setNetUsage(account.getNetUsage())
              .setFreeNetUsage(account.getFreeNetUsage())
              .setEnergyUsage(account.getEnergyUsage())
              .setLatestConsumeTime(account.getLatestConsumeTime())
              .setLatestConsumeFreeTime(account.getLatestConsumeFreeTime())
              .setLatestConsumeTimeForEnergy(account.getLatestConsumeTimeForEnergy())
              .setNetWindowSize(account.getWindowSize(ResourceCode.BANDWIDTH))
              .setNetWindowOptimized(account.getWindowOptimized(ResourceCode.BANDWIDTH))
              .setEnergyWindowSize(account.getWindowSize(ResourceCode.ENERGY))
              .setEnergyWindowOptimized(account.getWindowOptimized(ResourceCode.ENERGY));

          // Build snapshot
          AccountAextSnapshot snapshot = AccountAextSnapshot.newBuilder()
              .setAddress(ByteString.copyFrom(address))
              .setAext(aextBuilder.build())
              .build();

          snapshots.add(snapshot);

          logger.debug("Collected AEXT snapshot for address {}: netUsage={}, freeNetUsage={}, energyUsage={}",
              org.tron.common.utils.ByteArray.toHexString(address),
              account.getNetUsage(),
              account.getFreeNetUsage(),
              account.getEnergyUsage());

        } catch (Exception e) {
          logger.warn("Failed to collect AEXT for address {}: {}",
              org.tron.common.utils.ByteArray.toHexString(address),
              e.getMessage());
        }
      }

      logger.debug("Collected {} AEXT snapshots for contract type {}",
          snapshots.size(), contractType.name());

    } catch (Exception e) {
      logger.error("Failed to collect pre-execution AEXT snapshots", e);
    }

    return snapshots;
  }
}
