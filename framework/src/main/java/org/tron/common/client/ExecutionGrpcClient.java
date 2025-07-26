package org.tron.common.client;

import io.grpc.LoadBalancerRegistry;
import io.grpc.ManagedChannel;
import io.grpc.ManagedChannelBuilder;
import io.grpc.StatusRuntimeException;
import io.grpc.internal.PickFirstLoadBalancerProvider;
import java.util.concurrent.TimeUnit;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import tron.backend.BackendGrpc;
import tron.backend.BackendOuterClass.*;

/**
 * gRPC client for the unified Rust backend execution service.
 * Provides methods to execute transactions, call contracts, and perform other EVM operations.
 */
public class ExecutionGrpcClient {
  private static final Logger logger = LoggerFactory.getLogger(ExecutionGrpcClient.class);

  private final ManagedChannel channel;
  private final BackendGrpc.BackendBlockingStub blockingStub;
  private final BackendGrpc.BackendFutureStub futureStub;

  // Default timeouts
  private static final long DEFAULT_DEADLINE_MS = 30000; // 30 seconds
  private static final long SHUTDOWN_TIMEOUT_MS = 5000; // 5 seconds

  static {
    // Register the PickFirst load balancer to avoid "Could not find policy 'pick_first'" errors
    LoadBalancerRegistry
        .getDefaultRegistry()
        .register(new PickFirstLoadBalancerProvider());
  }

  /**
   * Create ExecutionGrpcClient with host and port.
   *
   * @param host Remote execution service host
   * @param port Remote execution service port
   */
  public ExecutionGrpcClient(String host, int port) {
    if (host == null || host.trim().isEmpty()) {
      throw new IllegalArgumentException("Host cannot be null or empty");
    }
    if (port <= 0 || port > 65535) {
      throw new IllegalArgumentException("Port must be between 1 and 65535");
    }

    logger.info("Creating ExecutionGrpcClient for {}:{}", host, port);
    
    this.channel = ManagedChannelBuilder.forAddress(host, port)
        .usePlaintext()
        .build();
    
    this.blockingStub = BackendGrpc.newBlockingStub(channel);

    this.futureStub = BackendGrpc.newFutureStub(channel);
  }

  /**
   * Create ExecutionGrpcClient with target string.
   *
   * @param target Target string (e.g., "localhost:50012")
   */
  public ExecutionGrpcClient(String target) {
    if (target == null || target.trim().isEmpty()) {
      throw new IllegalArgumentException("Target cannot be null or empty");
    }

    logger.info("Creating ExecutionGrpcClient for target: {}", target);
    
    this.channel = ManagedChannelBuilder.forTarget(target)
        .usePlaintext()
        .build();
    
    this.blockingStub = BackendGrpc.newBlockingStub(channel)
        .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS);
    
    this.futureStub = BackendGrpc.newFutureStub(channel)
        .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS);
  }

  /**
   * Execute a transaction on the remote execution service.
   *
   * @param request ExecuteTransactionRequest
   * @return ExecuteTransactionResponse
   * @throws RuntimeException if the gRPC call fails
   */
  public ExecuteTransactionResponse executeTransaction(ExecuteTransactionRequest request) {
    try {
      logger.debug("Executing transaction via gRPC");
      return blockingStub
          .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS)
          .executeTransaction(request);
    } catch (StatusRuntimeException e) {
      logger.error("Failed to execute transaction via gRPC: {}", e.getMessage());
      throw new RuntimeException("Remote transaction execution failed: " + e.getMessage(), e);
    }
  }

  /**
   * Call a contract on the remote execution service.
   *
   * @param request CallContractRequest
   * @return CallContractResponse
   * @throws RuntimeException if the gRPC call fails
   */
  public CallContractResponse callContract(CallContractRequest request) {
    try {
      logger.debug("Calling contract via gRPC");
      return blockingStub
          .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS)
          .callContract(request);
    } catch (StatusRuntimeException e) {
      logger.error("Failed to call contract via gRPC: {}", e.getMessage());
      throw new RuntimeException("Remote contract call failed: " + e.getMessage(), e);
    }
  }

  /**
   * Estimate energy for a transaction on the remote execution service.
   *
   * @param request EstimateEnergyRequest
   * @return EstimateEnergyResponse
   * @throws RuntimeException if the gRPC call fails
   */
  public EstimateEnergyResponse estimateEnergy(EstimateEnergyRequest request) {
    try {
      logger.debug("Estimating energy via gRPC");
      return blockingStub
          .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS)
          .estimateEnergy(request);
    } catch (StatusRuntimeException e) {
      logger.error("Failed to estimate energy via gRPC: {}", e.getMessage());
      throw new RuntimeException("Remote energy estimation failed: " + e.getMessage(), e);
    }
  }

  /**
   * Get contract code from the remote execution service.
   *
   * @param request GetCodeRequest
   * @return GetCodeResponse
   * @throws RuntimeException if the gRPC call fails
   */
  public GetCodeResponse getCode(GetCodeRequest request) {
    try {
      logger.debug("Getting code via gRPC");
      return blockingStub
          .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS)
          .getCode(request);
    } catch (StatusRuntimeException e) {
      logger.error("Failed to get code via gRPC: {}", e.getMessage());
      throw new RuntimeException("Remote get code failed: " + e.getMessage(), e);
    }
  }

  /**
   * Get storage value from the remote execution service.
   *
   * @param request GetStorageAtRequest
   * @return GetStorageAtResponse
   * @throws RuntimeException if the gRPC call fails
   */
  public GetStorageAtResponse getStorageAt(GetStorageAtRequest request) {
    try {
      logger.debug("Getting storage at via gRPC");
      return blockingStub
          .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS)
          .getStorageAt(request);
    } catch (StatusRuntimeException e) {
      logger.error("Failed to get storage at via gRPC: {}", e.getMessage());
      throw new RuntimeException("Remote get storage at failed: " + e.getMessage(), e);
    }
  }

  /**
   * Get account nonce from the remote execution service.
   *
   * @param request GetNonceRequest
   * @return GetNonceResponse
   * @throws RuntimeException if the gRPC call fails
   */
  public GetNonceResponse getNonce(GetNonceRequest request) {
    try {
      logger.debug("Getting nonce via gRPC");
      return blockingStub
          .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS)
          .getNonce(request);
    } catch (StatusRuntimeException e) {
      logger.error("Failed to get nonce via gRPC: {}", e.getMessage());
      throw new RuntimeException("Remote get nonce failed: " + e.getMessage(), e);
    }
  }

  /**
   * Get account balance from the remote execution service.
   *
   * @param request GetBalanceRequest
   * @return GetBalanceResponse
   * @throws RuntimeException if the gRPC call fails
   */
  public GetBalanceResponse getBalance(GetBalanceRequest request) {
    try {
      logger.debug("Getting balance via gRPC");
      return blockingStub
          .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS)
          .getBalance(request);
    } catch (StatusRuntimeException e) {
      logger.error("Failed to get balance via gRPC: {}", e.getMessage());
      throw new RuntimeException("Remote get balance failed: " + e.getMessage(), e);
    }
  }

  /**
   * Create EVM snapshot on the remote execution service.
   *
   * @param request CreateEvmSnapshotRequest
   * @return CreateEvmSnapshotResponse
   * @throws RuntimeException if the gRPC call fails
   */
  public CreateEvmSnapshotResponse createEvmSnapshot(CreateEvmSnapshotRequest request) {
    try {
      logger.debug("Creating EVM snapshot via gRPC");
      return blockingStub
          .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS)
          .createEvmSnapshot(request);
    } catch (StatusRuntimeException e) {
      logger.error("Failed to create EVM snapshot via gRPC: {}", e.getMessage());
      throw new RuntimeException("Remote create EVM snapshot failed: " + e.getMessage(), e);
    }
  }

  /**
   * Revert to EVM snapshot on the remote execution service.
   *
   * @param request RevertToEvmSnapshotRequest
   * @return RevertToEvmSnapshotResponse
   * @throws RuntimeException if the gRPC call fails
   */
  public RevertToEvmSnapshotResponse revertToEvmSnapshot(RevertToEvmSnapshotRequest request) {
    try {
      logger.debug("Reverting to EVM snapshot via gRPC");
      return blockingStub
          .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS)
          .revertToEvmSnapshot(request);
    } catch (StatusRuntimeException e) {
      logger.error("Failed to revert to EVM snapshot via gRPC: {}", e.getMessage());
      throw new RuntimeException("Remote revert to EVM snapshot failed: " + e.getMessage(), e);
    }
  }

  /**
   * Check health of the remote execution service.
   *
   * @return HealthResponse
   * @throws RuntimeException if the gRPC call fails
   */
  public HealthResponse healthCheck() {
    try {
      logger.debug("Checking health via gRPC");
      HealthRequest request = HealthRequest.newBuilder().build();
      return blockingStub
          .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS)
          .health(request);
    } catch (StatusRuntimeException e) {
      logger.error("Failed to check health via gRPC: {}", e.getMessage());
      throw new RuntimeException("Remote health check failed: " + e.getMessage(), e);
    }
  }

  /**
   * Get metadata from the remote execution service.
   *
   * @return MetadataResponse
   * @throws RuntimeException if the gRPC call fails
   */
  public MetadataResponse getMetadata() {
    try {
      logger.debug("Getting metadata via gRPC");
      MetadataRequest request = MetadataRequest.newBuilder().build();
      return blockingStub
          .withDeadlineAfter(DEFAULT_DEADLINE_MS, TimeUnit.MILLISECONDS)
          .getMetadata(request);
    } catch (StatusRuntimeException e) {
      logger.error("Failed to get metadata via gRPC: {}", e.getMessage());
      throw new RuntimeException("Remote get metadata failed: " + e.getMessage(), e);
    }
  }

  /**
   * Shutdown the gRPC client and close the channel.
   */
  public void shutdown() {
    try {
      logger.info("Shutting down ExecutionGrpcClient");
      channel.shutdown().awaitTermination(SHUTDOWN_TIMEOUT_MS, TimeUnit.MILLISECONDS);
    } catch (InterruptedException e) {
      logger.warn("Interrupted while shutting down ExecutionGrpcClient", e);
      Thread.currentThread().interrupt();
    }
  }

  /**
   * Check if the channel is shutdown.
   *
   * @return true if the channel is shutdown
   */
  public boolean isShutdown() {
    return channel.isShutdown();
  }

  /**
   * Check if the channel is terminated.
   *
   * @return true if the channel is terminated
   */
  public boolean isTerminated() {
    return channel.isTerminated();
  }
}
