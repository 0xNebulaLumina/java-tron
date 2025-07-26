package org.tron.core.storage.spi;

import com.google.protobuf.ByteString;
import io.grpc.LoadBalancerRegistry;
import io.grpc.ManagedChannel;
import io.grpc.ManagedChannelBuilder;
import io.grpc.StatusRuntimeException;
import io.grpc.internal.PickFirstLoadBalancerProvider;
import java.util.AbstractMap;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import java.util.stream.Collectors;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import tron.backend.BackendGrpc;
import tron.backend.BackendOuterClass.BatchGetRequest;
import tron.backend.BackendOuterClass.BatchGetResponse;
import tron.backend.BackendOuterClass.WriteOperation;
import tron.backend.BackendOuterClass.BatchWriteRequest;
import tron.backend.BackendOuterClass.BeginTransactionRequest;
import tron.backend.BackendOuterClass.BeginTransactionResponse;
import tron.backend.BackendOuterClass.CloseDBRequest;
import tron.backend.BackendOuterClass.CommitTransactionRequest;
import tron.backend.BackendOuterClass.CreateSnapshotRequest;
import tron.backend.BackendOuterClass.CreateSnapshotResponse;
import tron.backend.BackendOuterClass.DeleteRequest;
import tron.backend.BackendOuterClass.DeleteSnapshotRequest;
import tron.backend.BackendOuterClass.GetFromSnapshotRequest;
import tron.backend.BackendOuterClass.GetFromSnapshotResponse;
import tron.backend.BackendOuterClass.GetKeysNextRequest;
import tron.backend.BackendOuterClass.GetKeysNextResponse;
import tron.backend.BackendOuterClass.GetNextRequest;
import tron.backend.BackendOuterClass.GetNextResponse;
import tron.backend.BackendOuterClass.GetRequest;
import tron.backend.BackendOuterClass.GetResponse;
import tron.backend.BackendOuterClass.GetStatsRequest;
import tron.backend.BackendOuterClass.GetStatsResponse;
import tron.backend.BackendOuterClass.GetValuesNextRequest;
import tron.backend.BackendOuterClass.GetValuesNextResponse;
import tron.backend.BackendOuterClass.HasRequest;
import tron.backend.BackendOuterClass.HasResponse;
import tron.backend.BackendOuterClass.HealthRequest;
import tron.backend.BackendOuterClass.HealthResponse;
import tron.backend.BackendOuterClass.InitDBRequest;
import tron.backend.BackendOuterClass.IsAliveRequest;
import tron.backend.BackendOuterClass.IsAliveResponse;
import tron.backend.BackendOuterClass.IsEmptyRequest;
import tron.backend.BackendOuterClass.IsEmptyResponse;
import tron.backend.BackendOuterClass.KeyValue;
import tron.backend.BackendOuterClass.ListDatabasesRequest;
import tron.backend.BackendOuterClass.ListDatabasesResponse;
import tron.backend.BackendOuterClass.PrefixQueryRequest;
import tron.backend.BackendOuterClass.PrefixQueryResponse;
import tron.backend.BackendOuterClass.PutRequest;
import tron.backend.BackendOuterClass.ResetDBRequest;
import tron.backend.BackendOuterClass.RollbackTransactionRequest;
import tron.backend.BackendOuterClass.SizeRequest;
import tron.backend.BackendOuterClass.SizeResponse;

/**
 * gRPC-based implementation of StorageSPI that communicates with Rust backend service. This
 * implementation replaces placeholder calls with actual gRPC communication.
 */
public class RemoteStorageSPI implements StorageSPI {
  private static final Logger logger = LoggerFactory.getLogger(RemoteStorageSPI.class);

  // Register the PickFirstLoadBalancerProvider to avoid "Could not find policy 'pick_first'" errors
  static {
    LoadBalancerRegistry.getDefaultRegistry().register(new PickFirstLoadBalancerProvider());
  }

  private final ManagedChannel channel;
  private final BackendGrpc.BackendBlockingStub blockingStub;
  private final BackendGrpc.BackendFutureStub futureStub;
  private final String host;
  private final int port;
  private volatile boolean closed = false;

  public RemoteStorageSPI(String host, int port) {
    // Validate parameters to prevent NullPointerException
    if (host == null || host.trim().isEmpty()) {
      throw new IllegalArgumentException("Host cannot be null or empty");
    }
    if (port <= 0 || port > 65535) {
      throw new IllegalArgumentException("Port must be between 1 and 65535, got: " + port);
    }
    
    this.host = host.trim();
    this.port = port;
    
    try {
      this.channel = ManagedChannelBuilder.forAddress(this.host, this.port).usePlaintext().build();
    } catch (Exception e) {
      throw new RuntimeException("Failed to create gRPC channel for " + this.host + ":" + this.port, e);
    }

    this.blockingStub = BackendGrpc.newBlockingStub(channel);
    this.futureStub = BackendGrpc.newFutureStub(channel);

    logger.info("Initialized gRPC storage client for {}:{}", this.host, this.port);
  }

  @Override
  public CompletableFuture<byte[]> get(String dbName, byte[] key) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            GetRequest request =
                GetRequest.newBuilder().setDatabase(dbName).setKey(ByteString.copyFrom(key)).build();

            GetResponse response = blockingStub.get(request);
            logger.debug(
                "Get operation: db={}, key.length={}, found={}",
                dbName,
                key.length,
                response.getFound());

            return response.getFound() ? response.getValue().toByteArray() : null;
          } catch (StatusRuntimeException e) {
            logger.error("gRPC get failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage get operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Void> put(String dbName, byte[] key, byte[] value) {
    return CompletableFuture.runAsync(
        () -> {
          try {
            PutRequest request =
                PutRequest.newBuilder()
                    .setDatabase(dbName)
                    .setKey(ByteString.copyFrom(key))
                    .setValue(ByteString.copyFrom(value))
                    .build();

            blockingStub.put(request);
            logger.debug(
                "Put operation: db={}, key.length={}, value.length={}",
                dbName,
                key.length,
                value.length);
          } catch (StatusRuntimeException e) {
            logger.error("gRPC put failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage put operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Void> delete(String dbName, byte[] key) {
    return CompletableFuture.runAsync(
        () -> {
          try {
            DeleteRequest request =
                DeleteRequest.newBuilder()
                    .setDatabase(dbName)
                    .setKey(ByteString.copyFrom(key))
                    .build();

            blockingStub.delete(request);
            logger.debug("Delete operation: db={}, key.length={}", dbName, key.length);
          } catch (StatusRuntimeException e) {
            logger.error("gRPC delete failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage delete operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Boolean> has(String dbName, byte[] key) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            HasRequest request =
                HasRequest.newBuilder().setDatabase(dbName).setKey(ByteString.copyFrom(key)).build();

            HasResponse response = blockingStub.has(request);
            logger.debug(
                "Has operation: db={}, key.length={}, exists={}",
                dbName,
                key.length,
                response.getExists());

            return response.getExists();
          } catch (StatusRuntimeException e) {
            logger.error("gRPC has failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage has operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Void> batchWrite(String dbName, Map<byte[], byte[]> operations) {
    return CompletableFuture.runAsync(
        () -> {
          try {
            BatchWriteRequest.Builder requestBuilder =
                BatchWriteRequest.newBuilder().setDatabase(dbName);

            for (Map.Entry<byte[], byte[]> entry : operations.entrySet()) {
              WriteOperation.Builder opBuilder =
                  WriteOperation.newBuilder().setKey(ByteString.copyFrom(entry.getKey()));

              // Handle null values as delete operations
              if (entry.getValue() == null) {
                opBuilder.setType(WriteOperation.Type.DELETE);
              } else {
                opBuilder
                    .setType(WriteOperation.Type.PUT)
                    .setValue(ByteString.copyFrom(entry.getValue()));
              }

              requestBuilder.addOperations(opBuilder.build());
            }

            blockingStub.batchWrite(requestBuilder.build());
            logger.debug(
                "Batch write operation: db={}, operations.size={}", dbName, operations.size());
          } catch (StatusRuntimeException e) {
            logger.error("gRPC batch write failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage batch write operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Map<byte[], byte[]>> batchGet(String dbName, List<byte[]> keys) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            BatchGetRequest.Builder requestBuilder = BatchGetRequest.newBuilder().setDatabase(dbName);

            for (byte[] key : keys) {
              requestBuilder.addKeys(ByteString.copyFrom(key));
            }

            BatchGetResponse response = blockingStub.batchGet(requestBuilder.build());

            Map<byte[], byte[]> result = new HashMap<>();
            for (KeyValue kv : response.getPairsList()) {
              if (kv.getFound()) {
                result.put(kv.getKey().toByteArray(), kv.getValue().toByteArray());
              } else {
                result.put(kv.getKey().toByteArray(), null);
              }
            }

            logger.debug(
                "Batch get operation: db={}, keys.size={}, found={}",
                dbName,
                keys.size(),
                result.size());
            return result;
          } catch (StatusRuntimeException e) {
            logger.error("gRPC batch get failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage batch get operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<StorageIterator> iterator(String dbName) {
    return iterator(dbName, new byte[0]);
  }

  @Override
  public CompletableFuture<StorageIterator> iterator(String dbName, byte[] startKey) {
    return CompletableFuture.supplyAsync(
        () -> {
          logger.debug("Iterator operation: db={}, startKey.length={}", dbName, startKey.length);
          return new RemoteStorageIterator(dbName, startKey);
        });
  }

  @Override
  public CompletableFuture<List<byte[]>> getKeysNext(String dbName, byte[] startKey, int limit) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            GetKeysNextRequest request =
                GetKeysNextRequest.newBuilder()
                    .setDatabase(dbName)
                    .setStartKey(ByteString.copyFrom(startKey))
                    .setLimit(limit)
                    .build();

            GetKeysNextResponse response = blockingStub.getKeysNext(request);
            List<byte[]> keys =
                response.getKeysList().stream()
                    .map(ByteString::toByteArray)
                    .collect(Collectors.toList());

            logger.debug(
                "Get keys next operation: db={}, startKey.length={}, limit={}, found={}",
                dbName,
                startKey.length,
                limit,
                keys.size());
            return keys;
          } catch (StatusRuntimeException e) {
            logger.error("gRPC get keys next failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage get keys next operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<List<byte[]>> getValuesNext(String dbName, byte[] startKey, int limit) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            GetValuesNextRequest request =
                GetValuesNextRequest.newBuilder()
                    .setDatabase(dbName)
                    .setStartKey(ByteString.copyFrom(startKey))
                    .setLimit(limit)
                    .build();

            GetValuesNextResponse response = blockingStub.getValuesNext(request);
            List<byte[]> values =
                response.getValuesList().stream()
                    .map(ByteString::toByteArray)
                    .collect(Collectors.toList());

            logger.debug(
                "Get values next operation: db={}, startKey.length={}, limit={}, found={}",
                dbName,
                startKey.length,
                limit,
                values.size());
            return values;
          } catch (StatusRuntimeException e) {
            logger.error("gRPC get values next failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage get values next operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Map<byte[], byte[]>> getNext(String dbName, byte[] startKey, int limit) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            GetNextRequest request =
                GetNextRequest.newBuilder()
                    .setDatabase(dbName)
                    .setStartKey(ByteString.copyFrom(startKey))
                    .setLimit(limit)
                    .build();

            GetNextResponse response = blockingStub.getNext(request);
            Map<byte[], byte[]> result = new HashMap<>();
            for (KeyValue kv : response.getPairsList()) {
              result.put(kv.getKey().toByteArray(), kv.getValue().toByteArray());
            }

            logger.debug(
                "Get next operation: db={}, startKey.length={}, limit={}, found={}",
                dbName,
                startKey.length,
                limit,
                result.size());
            return result;
          } catch (StatusRuntimeException e) {
            logger.error("gRPC get next failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage get next operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Map<byte[], byte[]>> prefixQuery(String dbName, byte[] prefix) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            PrefixQueryRequest request =
                PrefixQueryRequest.newBuilder()
                    .setDatabase(dbName)
                    .setPrefix(ByteString.copyFrom(prefix))
                    .build();

            PrefixQueryResponse response = blockingStub.prefixQuery(request);
            Map<byte[], byte[]> result = new HashMap<>();
            for (KeyValue kv : response.getPairsList()) {
              result.put(kv.getKey().toByteArray(), kv.getValue().toByteArray());
            }

            logger.debug(
                "Prefix query operation: db={}, prefix.length={}, found={}",
                dbName,
                prefix.length,
                result.size());
            return result;
          } catch (StatusRuntimeException e) {
            logger.error("gRPC prefix query failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage prefix query operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Void> initDB(String dbName, StorageConfig config) {
    return CompletableFuture.runAsync(
        () -> {
          try {
            tron.backend.BackendOuterClass.StorageConfig.Builder configBuilder =
                tron.backend.BackendOuterClass.StorageConfig.newBuilder()
                    .setEngine(config.getEngine())
                    .setEnableStatistics(config.isEnableStatistics())
                    .setMaxOpenFiles(config.getMaxOpenFiles())
                    .setBlockCacheSize(config.getBlockCacheSize());

            if (config.getEngineOptions() != null) {
              Map<String, String> stringOptions = new HashMap<>();
              for (Map.Entry<String, Object> entry : config.getEngineOptions().entrySet()) {
                stringOptions.put(entry.getKey(), String.valueOf(entry.getValue()));
              }
              configBuilder.putAllEngineOptions(stringOptions);
            }

            InitDBRequest request =
                InitDBRequest.newBuilder()
                    .setDatabase(dbName)
                    .setConfig(configBuilder.build())
                    .build();

            blockingStub.initDB(request);
            logger.info("Init DB operation: db={}, config={}", dbName, config);
          } catch (StatusRuntimeException e) {
            logger.error("gRPC init DB failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage init DB operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Void> closeDB(String dbName) {
    return CompletableFuture.runAsync(
        () -> {
          try {
            CloseDBRequest request = CloseDBRequest.newBuilder().setDatabase(dbName).build();

            blockingStub.closeDB(request);
            logger.info("Close DB operation: db={}", dbName);
          } catch (StatusRuntimeException e) {
            logger.error("gRPC close DB failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage close DB operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Void> resetDB(String dbName) {
    return CompletableFuture.runAsync(
        () -> {
          try {
            ResetDBRequest request = ResetDBRequest.newBuilder().setDatabase(dbName).build();

            blockingStub.resetDB(request);
            logger.info("Reset DB operation: db={}", dbName);
          } catch (StatusRuntimeException e) {
            logger.error("gRPC reset DB failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage reset DB operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Boolean> isAlive(String dbName) {
    return CompletableFuture.supplyAsync(
        () -> {
          if (closed) {
            return false;
          }

          try {
            IsAliveRequest request = IsAliveRequest.newBuilder().setDatabase(dbName).build();

            IsAliveResponse response = blockingStub.isAlive(request);
            logger.debug("Is alive operation: db={}, alive={}", dbName, response.getAlive());

            return response.getAlive();
          } catch (StatusRuntimeException e) {
            logger.error("gRPC is alive failed: db={}, error={}", dbName, e.getStatus());
            return false;
          }
        });
  }

  @Override
  public CompletableFuture<Long> size(String dbName) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            SizeRequest request = SizeRequest.newBuilder().setDatabase(dbName).build();

            SizeResponse response = blockingStub.size(request);
            logger.debug("Size operation: db={}, size={}", dbName, response.getSize());

            return response.getSize();
          } catch (StatusRuntimeException e) {
            logger.error("gRPC size failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage size operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Boolean> isEmpty(String dbName) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            IsEmptyRequest request = IsEmptyRequest.newBuilder().setDatabase(dbName).build();

            IsEmptyResponse response = blockingStub.isEmpty(request);
            logger.debug("Is empty operation: db={}, empty={}", dbName, response.getEmpty());

            return response.getEmpty();
          } catch (StatusRuntimeException e) {
            logger.error("gRPC is empty failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage is empty operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<String> beginTransaction(String dbName) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            BeginTransactionRequest request =
                BeginTransactionRequest.newBuilder().setDatabase(dbName).build();

            BeginTransactionResponse response = blockingStub.beginTransaction(request);
            logger.debug(
                "Begin transaction operation: db={}, txId={}", dbName, response.getTransactionId());

            return response.getTransactionId();
          } catch (StatusRuntimeException e) {
            logger.error("gRPC begin transaction failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage begin transaction operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Void> commitTransaction(String transactionId) {
    return CompletableFuture.runAsync(
        () -> {
          try {
            CommitTransactionRequest request =
                CommitTransactionRequest.newBuilder().setTransactionId(transactionId).build();

            blockingStub.commitTransaction(request);
            logger.debug("Commit transaction operation: txId={}", transactionId);
          } catch (StatusRuntimeException e) {
            logger.error(
                "gRPC commit transaction failed: txId={}, error={}", transactionId, e.getStatus());
            throw new RuntimeException("Storage commit transaction operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Void> rollbackTransaction(String transactionId) {
    return CompletableFuture.runAsync(
        () -> {
          try {
            RollbackTransactionRequest request =
                RollbackTransactionRequest.newBuilder().setTransactionId(transactionId).build();

            blockingStub.rollbackTransaction(request);
            logger.debug("Rollback transaction operation: txId={}", transactionId);
          } catch (StatusRuntimeException e) {
            logger.error(
                "gRPC rollback transaction failed: txId={}, error={}",
                transactionId,
                e.getStatus());
            throw new RuntimeException("Storage rollback transaction operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<String> createSnapshot(String dbName) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            CreateSnapshotRequest request =
                CreateSnapshotRequest.newBuilder().setDatabase(dbName).build();

            CreateSnapshotResponse response = blockingStub.createSnapshot(request);
            logger.debug(
                "Create snapshot operation: db={}, snapId={}", dbName, response.getSnapshotId());

            return response.getSnapshotId();
          } catch (StatusRuntimeException e) {
            logger.error("gRPC create snapshot failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage create snapshot operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<Void> deleteSnapshot(String snapshotId) {
    return CompletableFuture.runAsync(
        () -> {
          try {
            DeleteSnapshotRequest request =
                DeleteSnapshotRequest.newBuilder().setSnapshotId(snapshotId).build();

            blockingStub.deleteSnapshot(request);
            logger.debug("Delete snapshot operation: snapId={}", snapshotId);
          } catch (StatusRuntimeException e) {
            logger.error(
                "gRPC delete snapshot failed: snapId={}, error={}", snapshotId, e.getStatus());
            throw new RuntimeException("Storage delete snapshot operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<byte[]> getFromSnapshot(String snapshotId, byte[] key) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            GetFromSnapshotRequest request =
                GetFromSnapshotRequest.newBuilder()
                    .setSnapshotId(snapshotId)
                    .setKey(ByteString.copyFrom(key))
                    .build();

            GetFromSnapshotResponse response = blockingStub.getFromSnapshot(request);
            logger.debug(
                "Get from snapshot operation: snapId={}, key.length={}, found={}",
                snapshotId,
                key.length,
                response.getFound());

            return response.getFound() ? response.getValue().toByteArray() : null;
          } catch (StatusRuntimeException e) {
            logger.error(
                "gRPC get from snapshot failed: snapId={}, error={}", snapshotId, e.getStatus());
            throw new RuntimeException("Storage get from snapshot operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<org.tron.core.storage.spi.StorageStats> getStats(String dbName) {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            GetStatsRequest request = GetStatsRequest.newBuilder().setDatabase(dbName).build();

            GetStatsResponse response = blockingStub.getStats(request);
            tron.backend.BackendOuterClass.StorageStats protoStats = response.getStats();

            org.tron.core.storage.spi.StorageStats stats =
                new org.tron.core.storage.spi.StorageStats(
                    protoStats.getTotalKeys(),
                    protoStats.getTotalSize(),
                    new HashMap<>(protoStats.getEngineStatsMap()),
                    protoStats.getLastModified());

            logger.debug("Get stats operation: db={}, totalKeys={}", dbName, stats.getTotalKeys());
            return stats;
          } catch (StatusRuntimeException e) {
            logger.error("gRPC get stats failed: db={}, error={}", dbName, e.getStatus());
            throw new RuntimeException("Storage get stats operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<List<String>> listDatabases() {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            ListDatabasesRequest request = ListDatabasesRequest.newBuilder().build();
            ListDatabasesResponse response = blockingStub.listDatabases(request);

            List<String> databases = new ArrayList<>(response.getDatabasesList());
            logger.debug("List databases operation: count={}", databases.size());

            return databases;
          } catch (StatusRuntimeException e) {
            logger.error("gRPC list databases failed: error={}", e.getStatus());
            throw new RuntimeException("Storage list databases operation failed", e);
          }
        });
  }

  @Override
  public CompletableFuture<HealthStatus> healthCheck() {
    return CompletableFuture.supplyAsync(
        () -> {
          if (closed) {
            return HealthStatus.UNHEALTHY;
          }

          try {
            HealthRequest request = HealthRequest.newBuilder().build();
            HealthResponse response = blockingStub.health(request);

            HealthStatus status;
            switch (response.getStatus()) {
              case HEALTHY:
                status = HealthStatus.HEALTHY;
                break;
              case DEGRADED:
                status = HealthStatus.DEGRADED;
                break;
              case UNHEALTHY:
                status = HealthStatus.UNHEALTHY;
                break;
              default:
                status = HealthStatus.UNHEALTHY;
            }

            logger.debug("Health check operation: status={}", status);
            return status;
          } catch (StatusRuntimeException e) {
            logger.error("gRPC health check failed: error={}", e.getStatus());
            return HealthStatus.UNHEALTHY;
          }
        });
  }

  @Override
  public void registerMetricsCallback(MetricsCallback callback) {
    logger.debug("Register metrics callback");
    // TODO: Implement streaming metrics using futureStub.streamMetrics()
    // This would require a separate thread to handle the streaming response
  }

  /** Close the gRPC channel and cleanup resources. */
  public void close() {
    if (!closed) {
      closed = true;
      try {
        channel.shutdown().awaitTermination(5, TimeUnit.SECONDS);
        logger.info("gRPC storage client closed");
      } catch (InterruptedException e) {
        logger.warn("Failed to close gRPC channel gracefully", e);
        Thread.currentThread().interrupt();
      }
    }
  }

  /**
   * gRPC-based implementation of StorageIterator. Note: This is a simplified implementation. In a
   * full implementation, you would use the streaming iterator RPC method.
   */
  private class RemoteStorageIterator implements StorageIterator {
    private final String dbName;
    private final byte[] startKey;
    private boolean closed = false;
    private byte[] currentKey;
    private boolean hasNextCached = false;
    private Map.Entry<byte[], byte[]> nextEntry = null;
    private boolean reachedEnd = false;

    public RemoteStorageIterator(String dbName, byte[] startKey) {
      this.dbName = dbName;
      this.startKey = startKey;
      this.currentKey = startKey;
    }

    @Override
    public CompletableFuture<Boolean> hasNext() {
      if (closed || reachedEnd) {
        return CompletableFuture.completedFuture(false);
      }

      if (hasNextCached) {
        return CompletableFuture.completedFuture(true);
      }

      return getNext(dbName, currentKey, 1)
          .thenApply(
              entries -> {
                if (entries.isEmpty()) {
                  reachedEnd = true;
                  return false;
                } else {
                  Map.Entry<byte[], byte[]> firstEntry = entries.entrySet().iterator().next();
                  nextEntry =
                      new AbstractMap.SimpleEntry<>(firstEntry.getKey(), firstEntry.getValue());
                  hasNextCached = true;
                  return true;
                }
              })
          .exceptionally(
              throwable -> {
                logger.error("Error in hasNext() for iterator on db={}", dbName, throwable);
                reachedEnd = true;
                return false;
              });
    }

    @Override
    public CompletableFuture<Map.Entry<byte[], byte[]>> next() {
      return hasNext()
          .thenCompose(
              hasNext -> {
                if (!hasNext) {
                  throw new RuntimeException("No more elements in iterator for db: " + dbName);
                }

                Map.Entry<byte[], byte[]> result = nextEntry;

                // CRITICAL FIX: Properly advance the iterator position
                currentKey = incrementKey(result.getKey());

                hasNextCached = false;
                nextEntry = null;

                logger.debug(
                    "Iterator next: db={}, key.length={}, value.length={}",
                    dbName,
                    result.getKey().length,
                    result.getValue().length);

                return CompletableFuture.completedFuture(result);
              })
          .exceptionally(
              throwable -> {
                logger.error("Error in next() for iterator on db={}", dbName, throwable);
                throw new RuntimeException("Iterator next() failed for db: " + dbName, throwable);
              });
    }

    @Override
    public CompletableFuture<Void> seek(byte[] key) {
      return CompletableFuture.runAsync(
          () -> {
            currentKey = key;
            hasNextCached = false;
            nextEntry = null;
            reachedEnd = false;
            logger.debug("Iterator seek: db={}, key.length={}", dbName, key.length);
          });
    }

    @Override
    public CompletableFuture<Void> seekToFirst() {
      return CompletableFuture.runAsync(
          () -> {
            currentKey = new byte[0]; // Empty key means start from beginning
            hasNextCached = false;
            nextEntry = null;
            reachedEnd = false;
            logger.debug("Iterator seekToFirst: db={}", dbName);
          });
    }

    @Override
    public CompletableFuture<Void> seekToLast() {
      return CompletableFuture.runAsync(
          () -> {
            // For seekToLast, we'd need a special implementation
            // For now, just mark as reached end since this is complex with gRPC
            reachedEnd = true;
            hasNextCached = false;
            nextEntry = null;
            logger.debug("Iterator seekToLast: db={} (not fully implemented)", dbName);
          });
    }

    @Override
    public void close() {
      closed = true;
      hasNextCached = false;
      nextEntry = null;
      logger.debug("Closed iterator for db={}", dbName);
    }

    /**
     * Increment a byte array key to get the next possible key. This is crucial for proper iterator
     * advancement.
     *
     * @param key the current key
     * @return the next key in lexicographic order
     */
    private byte[] incrementKey(byte[] key) {
      if (key == null || key.length == 0) {
        return new byte[] {0x01};
      }

      // Create a copy to avoid modifying the original
      byte[] nextKey = new byte[key.length];
      System.arraycopy(key, 0, nextKey, 0, key.length);

      // Increment the key by finding the rightmost byte that can be incremented
      for (int i = nextKey.length - 1; i >= 0; i--) {
        if (nextKey[i] != (byte) 0xFF) {
          nextKey[i]++;
          return nextKey;
        } else {
          nextKey[i] = 0x00;
        }
      }

      // If all bytes were 0xFF, we need to extend the key
      byte[] extendedKey = new byte[nextKey.length + 1];
      System.arraycopy(nextKey, 0, extendedKey, 0, nextKey.length);
      extendedKey[nextKey.length] = 0x01;

      return extendedKey;
    }
  }
}
