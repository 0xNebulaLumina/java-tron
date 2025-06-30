package org.tron.core.db;

import lombok.extern.slf4j.Slf4j;
import org.tron.core.db2.common.DB;

import java.util.Iterator;
import java.util.Map;

/**
 * Adapter that implements DB<byte[], byte[]> using StorageBackend.
 * This is used for TronStoreWithRevoking's revoking database functionality.
 */
@Slf4j(topic = "DB")
public class StorageBackendDB implements DB<byte[], byte[]> {

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
        // This method is required by Instance interface but not used in practice
        throw new UnsupportedOperationException("newInstance not supported for StorageBackendDB");
    }
} 