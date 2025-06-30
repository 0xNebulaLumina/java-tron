package org.tron.core.storage.spi;

import java.util.HashMap;
import java.util.Iterator;
import java.util.Map;
import java.util.Set;
import java.util.stream.Collectors;
import lombok.extern.slf4j.Slf4j;
import org.tron.common.storage.WriteOptionsWrapper;
import org.tron.core.db.common.DbSourceInter;
import org.tron.core.db2.common.WrappedByteArray;

/**
 * Adapter that implements DbSourceInter&lt;byte[]&gt; using StorageSPI backend. This bridges the gap
 * between the existing TronDatabase interface and our new StorageSPI.
 */
@Slf4j(topic = "DB")
public class StorageSpiDbSource implements DbSourceInter<byte[]> {

  private final StorageSPI storageSPI;
  private final String dbName;
  private volatile boolean alive = false;

  public StorageSpiDbSource(String dbName, StorageSPI storageSPI) {
    this.dbName = dbName;
    this.storageSPI = storageSPI;
  }

  @Override
  public String getDBName() {
    return dbName;
  }

  @Override
  public void setDBName(String name) {
    // StorageSPI doesn't support changing database name after creation
    throw new UnsupportedOperationException("Cannot change database name after creation");
  }

  @Override
  public void initDB() {
    try {
      // Initialize the storage SPI
      storageSPI.initialize(new StorageConfig()).get();
      alive = true;
      logger.info("Initialized StorageSPI database: {}", dbName);
    } catch (Exception e) {
      logger.error("Failed to initialize StorageSPI database: {}", dbName, e);
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
      storageSPI.close().get();
      alive = false;
      logger.info("Closed StorageSPI database: {}", dbName);
    } catch (Exception e) {
      logger.error("Failed to close StorageSPI database: {}", dbName, e);
    }
  }

  @Override
  public void resetDb() {
    try {
      // Clear all data in the database
      storageSPI.clear().get();
      logger.info("Reset StorageSPI database: {}", dbName);
    } catch (Exception e) {
      logger.error("Failed to reset StorageSPI database: {}", dbName, e);
      throw new RuntimeException("Failed to reset database: " + dbName, e);
    }
  }

  @Override
  public void putData(byte[] key, byte[] value) {
    try {
      storageSPI.put(key, value).get();
    } catch (Exception e) {
      logger.error("Failed to put data in database: {}", dbName, e);
      throw new RuntimeException("Failed to put data", e);
    }
  }

  @Override
  public byte[] getData(byte[] key) {
    try {
      return storageSPI.get(key).get();
    } catch (Exception e) {
      logger.error("Failed to get data from database: {}", dbName, e);
      return null;
    }
  }

  @Override
  public void deleteData(byte[] key) {
    try {
      storageSPI.delete(key).get();
    } catch (Exception e) {
      logger.error("Failed to delete data from database: {}", dbName, e);
      throw new RuntimeException("Failed to delete data", e);
    }
  }

  @Override
  public boolean flush() {
    try {
      storageSPI.flush().get();
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
      storageSPI.batchPut(rows).get();
    } catch (Exception e) {
      logger.error("Failed to batch update database: {}", dbName, e);
      throw new RuntimeException("Failed to batch update", e);
    }
  }

  @Override
  public Set<byte[]> allKeys() throws RuntimeException {
    try {
      return storageSPI.getAllKeys().get();
    } catch (Exception e) {
      logger.error("Failed to get all keys from database: {}", dbName, e);
      throw new RuntimeException("Failed to get all keys", e);
    }
  }

  @Override
  public Set<byte[]> allValues() throws RuntimeException {
    try {
      return storageSPI.getAllValues().get();
    } catch (Exception e) {
      logger.error("Failed to get all values from database: {}", dbName, e);
      throw new RuntimeException("Failed to get all values", e);
    }
  }

  @Override
  public long getTotal() throws RuntimeException {
    try {
      return storageSPI.getSize().get();
    } catch (Exception e) {
      logger.error("Failed to get total count from database: {}", dbName, e);
      throw new RuntimeException("Failed to get total count", e);
    }
  }

  @Override
  public void stat() {
    try {
      Map<String, String> stats = storageSPI.getStats().get();
      logger.info("Database {} stats: {}", dbName, stats);
    } catch (Exception e) {
      logger.error("Failed to get stats from database: {}", dbName, e);
    }
  }

  @Override
  public Map<WrappedByteArray, byte[]> prefixQuery(byte[] key) {
    try {
      Map<byte[], byte[]> results = storageSPI.prefixScan(key, Integer.MAX_VALUE).get();
      return results.entrySet().stream()
          .collect(
              Collectors.toMap(entry -> WrappedByteArray.of(entry.getKey()), Map.Entry::getValue));
    } catch (Exception e) {
      logger.error("Failed to perform prefix query in database: {}", dbName, e);
      return new HashMap<>();
    }
  }

  @Override
  public Iterator<Map.Entry<byte[], byte[]>> iterator() {
    try {
      StorageIterator storageIterator = storageSPI.iterator().get();
      return new StorageIteratorAdapter(storageIterator);
    } catch (Exception e) {
      logger.error("Failed to create iterator for database: {}", dbName, e);
      throw new RuntimeException("Failed to create iterator", e);
    }
  }

  /** Adapter to convert StorageIterator to Java Iterator */
  private static class StorageIteratorAdapter implements Iterator<Map.Entry<byte[], byte[]>> {
    private final StorageIterator storageIterator;
    private Map.Entry<byte[], byte[]> nextEntry;
    private boolean hasCheckedNext = false;

    public StorageIteratorAdapter(StorageIterator storageIterator) {
      this.storageIterator = storageIterator;
    }

    @Override
    public boolean hasNext() {
      if (!hasCheckedNext) {
        try {
          nextEntry = storageIterator.next().get();
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
