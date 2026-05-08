package org.tron.core.storage.spi;

import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;

/**
 * Storage Service Provider Interface (SPI) for abstracting database operations. This interface
 * supports both embedded and remote storage implementations.
 *
 * <p>Phase 1 semantics for {@code beginTransaction} / {@code commitTransaction} /
 * {@code rollbackTransaction} and {@code createSnapshot} are frozen in
 * {@code planning/close_loop.storage_transactions.md} and
 * {@code planning/close_loop.snapshot.md}. The Javadoc on each method intentionally
 * does not duplicate the spec — the planning notes are the source of truth, and
 * the implementations must match them, not the other way around.
 *
 * <p>Notable anti-goals (do not reach for these without updating the planning notes):
 * cross-DB transactions, transactional iterators, read-your-writes on
 * {@code get}/{@code has}/{@code batchGet}, savepoints, and generic DB-product
 * semantics. This SPI is a narrow helper for execution atomicity and future
 * block-importer needs only.
 */
public interface StorageSPI {

  // Basic Operations
  CompletableFuture<byte[]> get(String dbName, byte[] key);

  CompletableFuture<Void> put(String dbName, byte[] key, byte[] value);

  default CompletableFuture<Void> put(
      String dbName, byte[] key, byte[] value, String transactionId) {
    CompletableFuture<Void> future = new CompletableFuture<>();
    future.completeExceptionally(
        new UnsupportedOperationException("transaction-scoped put is not supported"));
    return future;
  }

  CompletableFuture<Void> delete(String dbName, byte[] key);

  default CompletableFuture<Void> delete(String dbName, byte[] key, String transactionId) {
    CompletableFuture<Void> future = new CompletableFuture<>();
    future.completeExceptionally(
        new UnsupportedOperationException("transaction-scoped delete is not supported"));
    return future;
  }

  CompletableFuture<Boolean> has(String dbName, byte[] key);

  // Batch Operations
  CompletableFuture<Void> batchWrite(String dbName, Map<byte[], byte[]> operations);

  default CompletableFuture<Void> batchWrite(
      String dbName, Map<byte[], byte[]> operations, String transactionId) {
    CompletableFuture<Void> future = new CompletableFuture<>();
    future.completeExceptionally(
        new UnsupportedOperationException("transaction-scoped batchWrite is not supported"));
    return future;
  }

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
