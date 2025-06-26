package org.tron.core.storage.spi;

import org.tron.common.storage.rocksdb.RocksDbDataSourceImpl;
import org.tron.common.setting.RocksDbSettings;
import org.tron.core.db.common.iterator.DBIterator;

import java.util.List;
import java.util.Map;
import java.util.HashMap;
import java.util.ArrayList;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.io.File;

/**
 * Embedded StorageSPI implementation using RocksDbDataSourceImpl directly.
 * This implementation is used for benchmarking embedded RocksDB performance.
 */
public class EmbeddedStorageSPI implements StorageSPI {
    
    private final String basePath;
    private final Map<String, RocksDbDataSourceImpl> databases = new ConcurrentHashMap<>();
    
    public EmbeddedStorageSPI(String basePath) {
        this.basePath = basePath;
        // Ensure base directory exists
        new File(basePath).mkdirs();
    }
    
    @Override
    public CompletableFuture<byte[]> get(String dbName, byte[] key) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            return db.getData(key);
        });
    }
    
    @Override
    public CompletableFuture<Void> put(String dbName, byte[] key, byte[] value) {
        return CompletableFuture.runAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            db.putData(key, value);
        });
    }
    
    @Override
    public CompletableFuture<Void> delete(String dbName, byte[] key) {
        return CompletableFuture.runAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            db.deleteData(key);
        });
    }
    
    @Override
    public CompletableFuture<Boolean> has(String dbName, byte[] key) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            return db.getData(key) != null;
        });
    }
    
    @Override
    public CompletableFuture<Void> batchWrite(String dbName, Map<byte[], byte[]> operations) {
        return CompletableFuture.runAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            db.updateByBatch(operations);
        });
    }
    
    @Override
    public CompletableFuture<Map<byte[], byte[]>> batchGet(String dbName, List<byte[]> keys) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            
            Map<byte[], byte[]> results = new HashMap<>();
            for (byte[] key : keys) {
                byte[] value = db.getData(key);
                if (value != null) {
                    results.put(key, value);
                }
            }
            return results;
        });
    }
    
    @Override
    public CompletableFuture<StorageIterator> iterator(String dbName) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            return new EmbeddedStorageIterator(db.iterator());
        });
    }
    
    @Override
    public CompletableFuture<StorageIterator> iterator(String dbName, byte[] startKey) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            DBIterator iter = db.iterator();
            iter.seek(startKey);
            return new EmbeddedStorageIterator(iter);
        });
    }
    
    @Override
    public CompletableFuture<List<byte[]>> getKeysNext(String dbName, byte[] startKey, int limit) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            return db.getKeysNext(startKey, limit);
        });
    }
    
    @Override
    public CompletableFuture<List<byte[]>> getValuesNext(String dbName, byte[] startKey, int limit) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            return new ArrayList<>(db.getValuesNext(startKey, limit));
        });
    }
    
    @Override
    public CompletableFuture<Map<byte[], byte[]>> getNext(String dbName, byte[] startKey, int limit) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            return db.getNext(startKey, limit);
        });
    }
    
    @Override
    public CompletableFuture<Map<byte[], byte[]>> prefixQuery(String dbName, byte[] prefix) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            
            Map<byte[], byte[]> results = new HashMap<>();
            Map<org.tron.core.db2.common.WrappedByteArray, byte[]> prefixResults = db.prefixQuery(prefix);
            prefixResults.forEach((wrappedKey, value) -> {
                results.put(wrappedKey.getBytes(), value);
            });
            return results;
        });
    }
    
    @Override
    public CompletableFuture<Void> initDB(String dbName, StorageConfig config) {
        return CompletableFuture.runAsync(() -> {
            String dbPath = basePath + "/" + dbName;
            
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
        });
    }
    
    @Override
    public CompletableFuture<Void> closeDB(String dbName) {
        return CompletableFuture.runAsync(() -> {
            RocksDbDataSourceImpl db = databases.remove(dbName);
            if (db != null) {
                db.closeDB();
            }
        });
    }
    
    @Override
    public CompletableFuture<Void> resetDB(String dbName) {
        return CompletableFuture.runAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db != null) {
                db.resetDb();
            }
        });
    }
    
    @Override
    public CompletableFuture<Boolean> isAlive(String dbName) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            return db != null && db.isAlive();
        });
    }
    
    @Override
    public CompletableFuture<Long> size(String dbName) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            return db.getTotal();
        });
    }
    
    @Override
    public CompletableFuture<Boolean> isEmpty(String dbName) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            return db.getTotal() == 0;
        });
    }
    
    @Override
    public CompletableFuture<String> beginTransaction(String dbName) {
        // Embedded RocksDB doesn't support explicit transactions in this implementation
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
    
    @Override
    public CompletableFuture<String> createSnapshot(String dbName) {
        // Simplified snapshot implementation
        return CompletableFuture.completedFuture("embedded-snapshot-" + System.currentTimeMillis());
    }
    
    @Override
    public CompletableFuture<Void> deleteSnapshot(String snapshotId) {
        // Simplified snapshot implementation
        return CompletableFuture.completedFuture(null);
    }
    
    @Override
    public CompletableFuture<byte[]> getFromSnapshot(String snapshotId, byte[] key) {
        // Simplified snapshot implementation - just return current value
        String dbName = extractDbNameFromSnapshot(snapshotId);
        return get(dbName, key);
    }
    
    @Override
    public CompletableFuture<StorageStats> getStats(String dbName) {
        return CompletableFuture.supplyAsync(() -> {
            RocksDbDataSourceImpl db = databases.get(dbName);
            if (db == null) {
                throw new RuntimeException("Database not found: " + dbName);
            }
            
            long totalKeys = db.getTotal();
            long totalSize = totalKeys * 256; // Approximate size estimation
            
            StorageStats stats = new StorageStats();
            stats.setTotalKeys(totalKeys);
            stats.setTotalSize(totalSize);
            stats.addEngineStat("engine", "EMBEDDED_ROCKSDB");
            return stats;
        });
    }
    
    @Override
    public CompletableFuture<List<String>> listDatabases() {
        return CompletableFuture.supplyAsync(() -> new ArrayList<>(databases.keySet()));
    }
    
    @Override
    public CompletableFuture<HealthStatus> healthCheck() {
        return CompletableFuture.completedFuture(HealthStatus.HEALTHY);
    }
    
    @Override
    public void registerMetricsCallback(MetricsCallback callback) {
        // No-op for embedded implementation
    }
    
    public void close() {
        databases.values().forEach(RocksDbDataSourceImpl::closeDB);
        databases.clear();
    }
    
    private String extractDbNameFromSnapshot(String snapshotId) {
        // Simple implementation - assume first database
        return databases.keySet().iterator().next();
    }
    
    /**
     * Simple wrapper around DBIterator to implement StorageIterator
     */
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
            return CompletableFuture.supplyAsync(() -> {
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