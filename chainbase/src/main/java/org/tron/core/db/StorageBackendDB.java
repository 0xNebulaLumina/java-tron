package org.tron.core.db;

import lombok.extern.slf4j.Slf4j;
import org.tron.core.db2.common.DB;
import org.tron.core.db2.common.Flusher;
import org.tron.core.db2.common.WrappedByteArray;
import org.tron.core.storage.spi.StorageBackend;
import org.tron.core.storage.spi.StorageBackendFactory;
import org.tron.core.storage.spi.StorageMode;

import java.util.HashMap;
import java.util.Iterator;
import java.util.Map;

/**
 * Adapter that implements DB<byte[], byte[]> and Flusher using StorageBackend.
 * This is used for TronStoreWithRevoking's revoking database functionality.
 */
@Slf4j(topic = "DB")
public class StorageBackendDB implements DB<byte[], byte[]>, Flusher {

    private final StorageBackendDbSource dbSource;

    public StorageBackendDB(StorageBackendDbSource dbSource) {
        this.dbSource = dbSource;
    }

    @Override
    public byte[] get(byte[] key) {
        return dbSource.getData(key);
    }

    @Override
    public void put(byte[] key, byte[] value) {
        dbSource.putData(key, value);
    }

    @Override
    public long size() {
        return dbSource.getTotal();
    }

    @Override
    public boolean isEmpty() {
        return size() == 0;
    }

    @Override
    public void remove(byte[] key) {
        dbSource.deleteData(key);
    }

    @Override
    public String getDbName() {
        return dbSource.getDBName();
    }

    @Override
    public void close() {
        dbSource.closeDB();
    }

    @Override
    public Iterator<Map.Entry<byte[], byte[]>> iterator() {
        return dbSource.iterator();
    }

    @Override
    public void stat() {
        dbSource.stat();
    }

    @Override
    public DB<byte[], byte[]> newInstance() {
        try {
            // Create a new storage backend with the same mode and database name
            StorageBackendFactory factory = StorageBackendFactory.getInstance();
            if (factory == null) {
                throw new RuntimeException("StorageBackendFactory not available for newInstance");
            }
            
            // Determine the storage mode from the current configuration
            String storageMode = org.tron.common.parameter.CommonParameter.getInstance().getStorage().getStorageMode();
            if (storageMode == null) {
                storageMode = "embedded"; // Default to embedded mode
            }
            
            StorageMode mode = org.tron.core.storage.spi.StorageMode.fromString(storageMode);
            StorageBackend newStorageBackend = factory.createStorageBackend(mode, getDbName());
            StorageBackendDbSource newDbSource = new StorageBackendDbSource(getDbName(), newStorageBackend);
            
            return new StorageBackendDB(newDbSource);
        } catch (Exception e) {
            logger.error("Failed to create new instance of StorageBackendDB for database: {}", getDbName(), e);
            throw new RuntimeException("Failed to create new instance", e);
        }
    }

    // Flusher interface methods
    @Override
    public void flush(Map<WrappedByteArray, WrappedByteArray> batch) {
        try {
            // Convert WrappedByteArray to byte[] for the storage backend
            Map<byte[], byte[]> convertedBatch = new HashMap<>();
            batch.forEach((key, value) -> {
                convertedBatch.put(key.getBytes(), value.getBytes());
            });
            
            // Use the storage backend's batch put method
            dbSource.getStorageBackend().batchPut(convertedBatch);
        } catch (Exception e) {
            logger.error("Failed to flush batch to database: {}", getDbName(), e);
            throw new RuntimeException("Failed to flush batch", e);
        }
    }

    @Override
    public void reset() {
        dbSource.resetDb();
    }
} 