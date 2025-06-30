package org.tron.core.storage.spi;

import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;

/**
 * Storage Service Provider Interface (SPI) for abstracting database operations. This interface
 * supports both embedded and remote storage implementations.
 */
public interface StorageSPI {

  // Basic Operations
  CompletableFuture<byte[]> get(String dbName, byte[] key);

  CompletableFuture<Void> put(String dbName, byte[] key, byte[] value);

  CompletableFuture<Void> delete(String dbName, byte[] key);

  CompletableFuture<Boolean> has(String dbName, byte[] key);

  // Batch Operations
  CompletableFuture<Void> batchWrite(String dbName, Map<byte[], byte[]> operations);

  CompletableFuture<Map<byte[], byte[]>> batchGet(String dbName, List<byte[]> keys);

  // Iterator Operations
  CompletableFuture<StorageIterator> iterator(String dbName);

  CompletableFuture<StorageIterator> iterator(String dbName, byte[] startKey);

  CompletableFuture<List<byte[]>> getKeysNext(String dbName, byte[] startKey, int limit);

  CompletableFuture<List<byte[]>> getValuesNext(String dbName, byte[] startKey, int limit);

  CompletableFuture<Map<byte[], byte[]>> getNext(String dbName, byte[] startKey, int limit);

  CompletableFuture<Map<byte[], byte[]>> prefixQuery(String dbName, byte[] prefix);

  // Database Management
  CompletableFuture<Void> initDB(String dbName, StorageConfig config);

  CompletableFuture<Void> closeDB(String dbName);

  CompletableFuture<Void> resetDB(String dbName);

  CompletableFuture<Boolean> isAlive(String dbName);

  CompletableFuture<Long> size(String dbName);

  CompletableFuture<Boolean> isEmpty(String dbName);

  // Transaction Support
  CompletableFuture<String> beginTransaction(String dbName);

  CompletableFuture<Void> commitTransaction(String transactionId);

  CompletableFuture<Void> rollbackTransaction(String transactionId);

  // Snapshot Support
  CompletableFuture<String> createSnapshot(String dbName);

  CompletableFuture<Void> deleteSnapshot(String snapshotId);

  CompletableFuture<byte[]> getFromSnapshot(String snapshotId, byte[] key);

  // Metadata
  CompletableFuture<StorageStats> getStats(String dbName);

  CompletableFuture<List<String>> listDatabases();

  // Health & Monitoring
  CompletableFuture<HealthStatus> healthCheck();

  void registerMetricsCallback(MetricsCallback callback);
}
