package org.tron.core.storage.spi;

import java.util.Iterator;
import java.util.Map;
import java.util.Set;
import lombok.extern.slf4j.Slf4j;
import org.tron.core.db.common.DbSourceInter;
import org.tron.core.db2.common.DB;

/**
 * Adapter that implements DB&lt;byte[], byte[]&gt; using StorageSPI backend. This is used for
 * TronStoreWithRevoking's revoking database functionality.
 */
@Slf4j(topic = "DB")
public class StorageSpiDB implements DB<byte[], byte[]> {

  private final StorageSpiDbSource dbSource;

  public StorageSpiDB(StorageSpiDbSource dbSource) {
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
  public void stat() {
    dbSource.stat();
  }

  @Override
  public Iterator<Map.Entry<byte[], byte[]>> iterator() {
    return dbSource.iterator();
  }

  @Override
  public void close() {
    dbSource.closeDB();
  }

  @Override
  public DB<byte[], byte[]> newInstance() {
    // Create a new instance with the same configuration
    try {
      StorageSPI newStorageSPI = StorageSpiFactory.createStorage();
      StorageSpiDbSource newDbSource = new StorageSpiDbSource(dbSource.getDBName(), newStorageSPI);
      return new StorageSpiDB(newDbSource);
    } catch (Exception e) {
      logger.error("Failed to create new instance of StorageSpiDB", e);
      throw new RuntimeException("Failed to create new instance", e);
    }
  }

  // Remove these methods - they're not part of the DB interface
  public void reset() {
    dbSource.resetDb();
  }

  public Set<byte[]> allKeys() {
    return dbSource.allKeys();
  }

  public Set<byte[]> allValues() {
    return dbSource.allValues();
  }

  public DbSourceInter<byte[]> getDbSource() {
    return dbSource;
  }
}
