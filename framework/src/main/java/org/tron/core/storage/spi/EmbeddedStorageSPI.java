package org.tron.core.storage.spi;

import java.io.File;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.setting.RocksDbSettings;
import org.tron.common.storage.rocksdb.RocksDbDataSourceImpl;
import org.tron.core.db.common.iterator.DBIterator;

/**
 * Embedded StorageSPI implementation using RocksDbDataSourceImpl directly. This implementation
 * provides embedded RocksDB storage within the same JVM process.
 *
 * <p>Pros: - Low latency (no IPC overhead) - Simple deployment (single process) - Direct access to
 * RocksDB features
 *
 * <p>Cons: - No crash isolation (RocksDB issues can crash JVM) - Harder to scale horizontally -
 * Memory pressure shared with JVM
 */
public class EmbeddedStorageSPI implements StorageSPI {
  private static final Logger logger = LoggerFactory.getLogger(EmbeddedStorageSPI.class);

  private final String basePath;
  private final Map<String, RocksDbDataSourceImpl> databases = new ConcurrentHashMap<>();

  public EmbeddedStorageSPI(String basePath) {
    this.basePath = basePath;
    // Ensure base directory exists
    new File(basePath).mkdirs();
    logger.info("Initialized embedded storage with base path: {}", basePath);
  }

  /**
   * Get or create a database instance. This method provides lazy initialization to ensure databases
   * are available when needed, even if initDB() wasn't called explicitly.
   */
  private RocksDbDataSourceImpl getOrCreateDatabase(String dbName) {
    RocksDbDataSourceImpl db = databases.get(dbName);
    if (db == null) {
      synchronized (this) {
        // Double-check locking pattern
        db = databases.get(dbName);
        if (db == null) {
          try {
            logger.info("Auto-initializing database: {}", dbName);
            // Use default settings for auto-initialized databases
            RocksDbSettings settings = RocksDbSettings.getDefaultSettings();
            db = new RocksDbDataSourceImpl(basePath, dbName, settings);
            db.initDB();
            databases.put(dbName, db);
            logger.info(
                "Auto-initialized embedded database: {} at {}/{}", dbName, basePath, dbName);
          } catch (Exception e) {
            logger.error("Failed to auto-initialize embedded database: {}", dbName, e);
            throw new RuntimeException("Database auto-initialization failed: " + dbName, e);
          }
        }
      }
    }
    return db;
  }

  @Override
  public CompletableFuture<byte[]> get(String dbName, byte[] key) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          return db.getData(key);
        });
  }

  @Override
  public CompletableFuture<Void> put(String dbName, byte[] key, byte[] value) {
    return CompletableFuture.runAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          db.putData(key, value);
        });
  }

  @Override
  public CompletableFuture<Void> delete(String dbName, byte[] key) {
    return CompletableFuture.runAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          db.deleteData(key);
        });
  }

  @Override
  public CompletableFuture<Boolean> has(String dbName, byte[] key) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          return db.getData(key) != null;
        });
  }

  @Override
  public CompletableFuture<Void> batchWrite(String dbName, Map<byte[], byte[]> operations) {
    return CompletableFuture.runAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          db.updateByBatch(operations);
        });
  }

  @Override
  public CompletableFuture<Map<byte[], byte[]>> batchGet(String dbName, List<byte[]> keys) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);

          Map<byte[], byte[]> results = new HashMap<>();
          for (byte[] key : keys) {
            byte[] value = db.getData(key);
            // Always put the key in the results map, even if value is null
            // This matches the behavior expected by the tests and the gRPC implementation
            results.put(key, value);
          }
          return results;
        });
  }

  @Override
  public CompletableFuture<StorageIterator> iterator(String dbName) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          return new EmbeddedStorageIterator(db.iterator());
        });
  }

  @Override
  public CompletableFuture<StorageIterator> iterator(String dbName, byte[] startKey) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          DBIterator iter = db.iterator();
          iter.seek(startKey);
          return new EmbeddedStorageIterator(iter);
        });
  }

  @Override
  public CompletableFuture<List<byte[]>> getKeysNext(String dbName, byte[] startKey, int limit) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          return db.getKeysNext(startKey, limit);
        });
  }

  @Override
  public CompletableFuture<List<byte[]>> getValuesNext(String dbName, byte[] startKey, int limit) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          return new ArrayList<>(db.getValuesNext(startKey, limit));
        });
  }

  @Override
  public CompletableFuture<Map<byte[], byte[]>> getNext(String dbName, byte[] startKey, int limit) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          return db.getNext(startKey, limit);
        });
  }

  @Override
  public CompletableFuture<Map<byte[], byte[]>> prefixQuery(String dbName, byte[] prefix) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);

          Map<byte[], byte[]> results = new HashMap<>();
          Map<org.tron.core.db2.common.WrappedByteArray, byte[]> prefixResults =
              db.prefixQuery(prefix);
          prefixResults.forEach(
              (wrappedKey, value) -> {
                results.put(wrappedKey.getBytes(), value);
              });
          return results;
        });
  }

  @Override
  public CompletableFuture<Void> initDB(String dbName, StorageConfig config) {
    return CompletableFuture.runAsync(
        () -> {
          // Check if database is already initialized
          if (databases.containsKey(dbName)) {
            logger.debug("Database {} already initialized, skipping", dbName);
            return;
          }

          try {
            // Configure RocksDB settings based on StorageConfig
            RocksDbSettings settings = RocksDbSettings.getDefaultSettings();
            if (config.getMaxOpenFiles() > 0) {
              settings = settings.withMaxOpenFiles(config.getMaxOpenFiles());
            }
            if (config.isEnableStatistics()) {
              settings = settings.withEnableStatistics(true);
            }

            RocksDbDataSourceImpl db = new RocksDbDataSourceImpl(basePath, dbName, settings);
            db.initDB();
            databases.put(dbName, db);
            logger.info("Initialized embedded database: {} at {}/{}", dbName, basePath, dbName);
          } catch (Exception e) {
            logger.error("Failed to initialize embedded database: {}", dbName, e);
            throw new RuntimeException("Database initialization failed: " + dbName, e);
          }
        });
  }

  @Override
  public CompletableFuture<Void> closeDB(String dbName) {
    return CompletableFuture.runAsync(
        () -> {
          RocksDbDataSourceImpl db = databases.remove(dbName);
          if (db != null) {
            try {
              db.closeDB();
              logger.info("Closed embedded database: {}", dbName);
            } catch (Exception e) {
              logger.error("Error closing embedded database: {}", dbName, e);
            }
          }
        });
  }

  @Override
  public CompletableFuture<Void> resetDB(String dbName) {
    return CompletableFuture.runAsync(
        () -> {
          RocksDbDataSourceImpl db = databases.get(dbName);
          if (db != null) {
            try {
              db.resetDb();
              logger.info("Reset embedded database: {}", dbName);
            } catch (Exception e) {
              logger.error("Error resetting embedded database: {}", dbName, e);
              throw new RuntimeException("Database reset failed: " + dbName, e);
            }
          }
        });
  }

  @Override
  public CompletableFuture<Boolean> isAlive(String dbName) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = databases.get(dbName);
          return db != null && db.isAlive();
        });
  }

  @Override
  public CompletableFuture<Long> size(String dbName) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          return db.getTotal();
        });
  }

  @Override
  public CompletableFuture<Boolean> isEmpty(String dbName) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);
          return db.getTotal() == 0;
        });
  }

  @Override
  public CompletableFuture<String> beginTransaction(String dbName) {
    // Embedded RocksDB doesn't support explicit transactions in this implementation
    // In a full implementation, you would use RocksDB transactions
    return CompletableFuture.completedFuture("embedded-tx-" + System.currentTimeMillis());
  }

  @Override
  public CompletableFuture<Void> commitTransaction(String transactionId) {
    // Embedded RocksDB doesn't support explicit transactions in this implementation
    return CompletableFuture.completedFuture(null);
  }

  @Override
  public CompletableFuture<Void> rollbackTransaction(String transactionId) {
    // Embedded RocksDB doesn't support explicit transactions in this implementation
    return CompletableFuture.completedFuture(null);
  }

  // Storage snapshot APIs — Phase 1: explicitly UNSUPPORTED.
  //
  // Both EE and RR storage paths previously had fake-success snapshot
  // implementations: createSnapshot returned a synthetic id, deleteSnapshot
  // completed silently, and getFromSnapshot fell through to a live-DB read.
  // That gave callers fake point-in-time semantics. Phase 1 hardening
  // (planning/close_loop.snapshot.md) replaces all three with explicit
  // unsupported errors so callers cannot silently rely on isolation that
  // does not exist. If a real snapshot implementation is needed later,
  // update the planning note first and back the snapshot with a real
  // RocksDB snapshot handle.

  @Override
  public CompletableFuture<String> createSnapshot(String dbName) {
    CompletableFuture<String> future = new CompletableFuture<>();
    future.completeExceptionally(
        new UnsupportedOperationException(
            "Embedded storage snapshot is not supported in close_loop Phase 1 "
                + "(see planning/close_loop.snapshot.md). The previous placeholder "
                + "returned a synthetic id without taking a real RocksDB snapshot, "
                + "and getFromSnapshot fell through to a live-DB read. Both have "
                + "been replaced with explicit unsupported errors."));
    return future;
  }

  @Override
  public CompletableFuture<Void> deleteSnapshot(String snapshotId) {
    CompletableFuture<Void> future = new CompletableFuture<>();
    future.completeExceptionally(
        new UnsupportedOperationException(
            "Embedded storage deleteSnapshot is not supported in close_loop Phase 1 "
                + "(see planning/close_loop.snapshot.md). deleteSnapshot is a "
                + "no-op error rather than a fake success."));
    return future;
  }

  @Override
  public CompletableFuture<byte[]> getFromSnapshot(String snapshotId, byte[] key) {
    CompletableFuture<byte[]> future = new CompletableFuture<>();
    future.completeExceptionally(
        new UnsupportedOperationException(
            "Embedded storage getFromSnapshot is not supported in close_loop Phase 1 "
                + "(see planning/close_loop.snapshot.md). The previous implementation "
                + "fell through to a live-DB read, masquerading as a point-in-time "
                + "snapshot read. That has been replaced with an explicit error."));
    return future;
  }

  @Override
  public CompletableFuture<StorageStats> getStats(String dbName) {
    return CompletableFuture.supplyAsync(
        () -> {
          RocksDbDataSourceImpl db = getOrCreateDatabase(dbName);

          long totalKeys = db.getTotal();
          long totalSize = totalKeys * 256; // Approximate size estimation

          StorageStats stats = new StorageStats();
          stats.setTotalKeys(totalKeys);
          stats.setTotalSize(totalSize);
          stats.addEngineStat("engine", "EMBEDDED_ROCKSDB");
          stats.addEngineStat("basePath", basePath);
          return stats;
        });
  }

  @Override
  public CompletableFuture<List<String>> listDatabases() {
    return CompletableFuture.supplyAsync(() -> new ArrayList<>(databases.keySet()));
  }

  @Override
  public CompletableFuture<HealthStatus> healthCheck() {
    return CompletableFuture.supplyAsync(
        () -> {
          try {
            // Check if we can access the base directory
            File baseDir = new File(basePath);
            if (!baseDir.exists() || !baseDir.canRead() || !baseDir.canWrite()) {
              return HealthStatus.UNHEALTHY;
            }

            // Check if any databases are alive
            boolean hasAliveDbs =
                databases.values().stream().anyMatch(RocksDbDataSourceImpl::isAlive);

            return hasAliveDbs || databases.isEmpty()
                ? HealthStatus.HEALTHY
                : HealthStatus.DEGRADED;
          } catch (Exception e) {
            logger.error("Health check failed", e);
            return HealthStatus.UNHEALTHY;
          }
        });
  }

  @Override
  public void registerMetricsCallback(MetricsCallback callback) {
    // No-op for embedded implementation
    // In a full implementation, you could periodically collect RocksDB stats
    logger.debug("Metrics callback registration not supported in embedded mode");
  }

  /**
   * Close all databases and clean up resources. This method should be called during application
   * shutdown.
   */
  public void close() {
    logger.info("Closing embedded storage with {} databases", databases.size());
    databases
        .values()
        .forEach(
            db -> {
              try {
                db.closeDB();
              } catch (Exception e) {
                logger.error("Error closing database", e);
              }
            });
    databases.clear();
  }

  // extractDbNameFromSnapshot was a helper for the fake-success snapshot
  // implementation. Phase 1 marks snapshots as explicitly unsupported (see
  // planning/close_loop.snapshot.md), so this helper is dead code. It is
  // retained as a private method to make the deletion a single hunk if
  // snapshots remain unsupported in a later phase.
  @SuppressWarnings("unused")
  private String extractDbNameFromSnapshot(String snapshotId) {
    if (databases.isEmpty()) {
      throw new RuntimeException("No databases available for snapshot");
    }
    return databases.keySet().iterator().next();
  }

  /** Simple wrapper around DBIterator to implement StorageIterator */
  private static class EmbeddedStorageIterator implements StorageIterator {
    private final DBIterator iterator;

    public EmbeddedStorageIterator(DBIterator iterator) {
      this.iterator = iterator;
    }

    @Override
    public CompletableFuture<Boolean> hasNext() {
      return CompletableFuture.completedFuture(iterator.hasNext());
    }

    @Override
    public CompletableFuture<Map.Entry<byte[], byte[]>> next() {
      return CompletableFuture.supplyAsync(
          () -> {
            if (!iterator.hasNext()) {
              throw new RuntimeException("No more elements");
            }
            Map.Entry<byte[], byte[]> entry = iterator.next();
            return new HashMap.SimpleEntry<>(entry.getKey(), entry.getValue());
          });
    }

    @Override
    public CompletableFuture<Void> seek(byte[] key) {
      return CompletableFuture.runAsync(() -> iterator.seek(key));
    }

    @Override
    public CompletableFuture<Void> seekToFirst() {
      return CompletableFuture.runAsync(() -> iterator.seekToFirst());
    }

    @Override
    public CompletableFuture<Void> seekToLast() {
      return CompletableFuture.runAsync(() -> iterator.seekToLast());
    }

    @Override
    public void close() {
      try {
        iterator.close();
      } catch (Exception e) {
        // Ignore close errors
      }
    }
  }
}
