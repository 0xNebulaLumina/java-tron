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
  public DbSourceInter<byte[]> getDbSource() {
    return dbSource;
  }

  @Override
  public void close() {
    dbSource.closeDB();
  }

  @Override
  public void reset() {
    dbSource.resetDb();
  }

  @Override
  public Iterator<Map.Entry<byte[], byte[]>> iterator() {
    return dbSource.iterator();
  }

  @Override
  public Set<byte[]> allKeys() {
    return dbSource.allKeys();
  }

  @Override
  public Set<byte[]> allValues() {
    return dbSource.allValues();
  }
}
