package org.tron.core.db;

import lombok.extern.slf4j.Slf4j;
import org.tron.common.storage.WriteOptionsWrapper;
import org.tron.core.db.common.DbSourceInter;
import org.tron.core.db2.common.WrappedByteArray;
import org.tron.core.storage.spi.StorageBackend;

import java.util.HashMap;
import java.util.Iterator;
import java.util.Map;
import java.util.Set;
import java.util.stream.Collectors;

/**
 * Adapter that implements DbSourceInter<byte[]> using StorageBackend.
 * This bridges the gap between the existing TronDatabase interface and the new storage backend.
 */
@Slf4j(topic = "DB")
public class StorageBackendDbSource implements DbSourceInter<byte[]> {

    private final StorageBackend storageBackend;
    private final String dbName;
    private volatile boolean alive = false;

    public StorageBackendDbSource(String dbName, StorageBackend storageBackend) {
        this.dbName = dbName;
        this.storageBackend = storageBackend;
    }

    /**
     * Get the underlying storage backend. This is used by StorageBackendDB
     * to implement the Flusher interface.
     */
    public StorageBackend getStorageBackend() {
        return storageBackend;
    }

    @Override
    public String getDBName() {
        return dbName;
    }

    @Override
    public void setDBName(String name) {
        // StorageBackend doesn't support changing database name after creation
        throw new UnsupportedOperationException("Cannot change database name after creation");
    }

    @Override
    public void initDB() {
        try {
            storageBackend.initialize();
            alive = true;
            logger.info("Initialized StorageBackend database: {}", dbName);
        } catch (Exception e) {
            logger.error("Failed to initialize StorageBackend database: {}", dbName, e);
            throw new RuntimeException("Failed to initialize database: " + dbName, e);
        }
    }

    @Override
    public boolean isAlive() {
        return alive;
    }

    @Override
    public void closeDB() {
        try {
            storageBackend.close();
            alive = false;
            logger.info("Closed StorageBackend database: {}", dbName);
        } catch (Exception e) {
            logger.error("Failed to close StorageBackend database: {}", dbName, e);
        }
    }

    @Override
    public void resetDb() {
        try {
            storageBackend.clear();
            logger.info("Reset StorageBackend database: {}", dbName);
        } catch (Exception e) {
            logger.error("Failed to reset StorageBackend database: {}", dbName, e);
            throw new RuntimeException("Failed to reset database: " + dbName, e);
        }
    }

    @Override
    public void putData(byte[] key, byte[] value) {
        try {
            storageBackend.put(key, value);
        } catch (Exception e) {
            logger.error("Failed to put data in database: {}", dbName, e);
            throw new RuntimeException("Failed to put data", e);
        }
    }

    @Override
    public byte[] getData(byte[] key) {
        try {
            return storageBackend.get(key);
        } catch (Exception e) {
            logger.error("Failed to get data from database: {}", dbName, e);
            return null;
        }
    }

    @Override
    public void deleteData(byte[] key) {
        try {
            storageBackend.delete(key);
        } catch (Exception e) {
            logger.error("Failed to delete data from database: {}", dbName, e);
            throw new RuntimeException("Failed to delete data", e);
        }
    }

    @Override
    public boolean flush() {
        try {
            storageBackend.flush();
            return true;
        } catch (Exception e) {
            logger.error("Failed to flush database: {}", dbName, e);
            return false;
        }
    }

    @Override
    public void updateByBatch(Map<byte[], byte[]> rows) {
        updateByBatch(rows, null);
    }

    @Override
    public void updateByBatch(Map<byte[], byte[]> rows, WriteOptionsWrapper writeOptions) {
        try {
            storageBackend.batchPut(rows);
        } catch (Exception e) {
            logger.error("Failed to batch update database: {}", dbName, e);
            throw new RuntimeException("Failed to batch update", e);
        }
    }

    @Override
    public Set<byte[]> allKeys() throws RuntimeException {
        try {
            return storageBackend.getAllKeys();
        } catch (Exception e) {
            logger.error("Failed to get all keys from database: {}", dbName, e);
            throw new RuntimeException("Failed to get all keys", e);
        }
    }

    @Override
    public Set<byte[]> allValues() throws RuntimeException {
        try {
            return storageBackend.getAllValues();
        } catch (Exception e) {
            logger.error("Failed to get all values from database: {}", dbName, e);
            throw new RuntimeException("Failed to get all values", e);
        }
    }

    @Override
    public long getTotal() throws RuntimeException {
        try {
            return storageBackend.getSize();
        } catch (Exception e) {
            logger.error("Failed to get total count from database: {}", dbName, e);
            throw new RuntimeException("Failed to get total count", e);
        }
    }

    @Override
    public void stat() {
        try {
            Map<String, String> stats = storageBackend.getStats();
            logger.info("Database {} stats: {}", dbName, stats);
        } catch (Exception e) {
            logger.error("Failed to get stats from database: {}", dbName, e);
        }
    }

    @Override
    public Map<WrappedByteArray, byte[]> prefixQuery(byte[] key) {
        try {
            Map<byte[], byte[]> results = storageBackend.prefixScan(key, Integer.MAX_VALUE);
            return results.entrySet().stream()
                    .collect(Collectors.toMap(
                            entry -> WrappedByteArray.of(entry.getKey()),
                            Map.Entry::getValue
                    ));
        } catch (Exception e) {
            logger.error("Failed to perform prefix query in database: {}", dbName, e);
            return new HashMap<>();
        }
    }

    @Override
    public Iterator<Map.Entry<byte[], byte[]>> iterator() {
        try {
            StorageBackend.StorageIterator storageIterator = storageBackend.iterator();
            return new StorageIteratorAdapter(storageIterator);
        } catch (Exception e) {
            logger.error("Failed to create iterator for database: {}", dbName, e);
            throw new RuntimeException("Failed to create iterator", e);
        }
    }

    /**
     * Adapter to convert StorageIterator to Java Iterator
     */
    private static class StorageIteratorAdapter implements Iterator<Map.Entry<byte[], byte[]>> {
        private final StorageBackend.StorageIterator storageIterator;
        private Map.Entry<byte[], byte[]> nextEntry;
        private boolean hasCheckedNext = false;

        public StorageIteratorAdapter(StorageBackend.StorageIterator storageIterator) {
            this.storageIterator = storageIterator;
        }

        @Override
        public boolean hasNext() {
            if (!hasCheckedNext) {
                try {
                    if (storageIterator.hasNext()) {
                        nextEntry = storageIterator.next();
                    } else {
                        nextEntry = null;
                    }
                    hasCheckedNext = true;
                } catch (Exception e) {
                    nextEntry = null;
                    hasCheckedNext = true;
                }
            }
            return nextEntry != null;
        }

        @Override
        public Map.Entry<byte[], byte[]> next() {
            if (!hasNext()) {
                throw new java.util.NoSuchElementException();
            }
            Map.Entry<byte[], byte[]> result = nextEntry;
            nextEntry = null;
            hasCheckedNext = false;
            return result;
        }
    }
} 