package org.tron.core.storage.spi;

import java.util.HashMap;
import java.util.HashSet;
import java.util.Map;
import java.util.Set;
import lombok.extern.slf4j.Slf4j;

/**
 * Adapter that implements the simplified StorageBackend interface using the full StorageSPI. This
 * bridges the gap between the chainbase module's simplified interface and the framework module's
 * full StorageSPI implementation.
 */
@Slf4j(topic = "Storage")
public class StorageSpiBackendAdapter implements StorageBackend {

  private final StorageSPI storageSPI;
  private final String dbName;

  public StorageSpiBackendAdapter(StorageSPI storageSPI, String dbName) {
    this.storageSPI = storageSPI;
    this.dbName = dbName;
  }

  @Override
  public void initialize() throws Exception {
    StorageConfig config = new StorageConfig();
    storageSPI.initDB(dbName, config).get();
  }

  @Override
  public byte[] get(byte[] key) throws Exception {
    return storageSPI.get(dbName, key).get();
  }

  @Override
  public void put(byte[] key, byte[] value) throws Exception {
    storageSPI.put(dbName, key, value).get();
  }

  @Override
  public void delete(byte[] key) throws Exception {
    storageSPI.delete(dbName, key).get();
  }

  @Override
  public void batchPut(Map<byte[], byte[]> batch) throws Exception {
    storageSPI.batchWrite(dbName, batch).get();
  }

  @Override
  public boolean exists(byte[] key) throws Exception {
    return storageSPI.has(dbName, key).get();
  }

  @Override
  public Set<byte[]> getAllKeys() throws Exception {
    // Use prefixQuery with empty prefix to get all keys
    Map<byte[], byte[]> allData = storageSPI.prefixQuery(dbName, new byte[0]).get();
    return allData.keySet();
  }

  @Override
  public Set<byte[]> getAllValues() throws Exception {
    // Use prefixQuery with empty prefix to get all values
    Map<byte[], byte[]> allData = storageSPI.prefixQuery(dbName, new byte[0]).get();
    return new HashSet<>(allData.values());
  }

  @Override
  public long getSize() throws Exception {
    return storageSPI.size(dbName).get();
  }

  @Override
  public void clear() throws Exception {
    storageSPI.resetDB(dbName).get();
  }

  @Override
  public void flush() throws Exception {
    // StorageSPI doesn't have a direct flush method, but we can check if database is alive
    storageSPI.isAlive(dbName).get();
  }

  @Override
  public void close() throws Exception {
    storageSPI.closeDB(dbName).get();
  }

  @Override
  public Map<String, String> getStats() throws Exception {
    StorageStats stats = storageSPI.getStats(dbName).get();
    // Convert StorageStats to Map<String, String> - this will depend on StorageStats implementation
    // For now, return a simple representation
    Map<String, String> result = new HashMap<>();
    result.put("totalSize", String.valueOf(stats.getTotalSize()));
    result.put("totalKeys", String.valueOf(stats.getTotalKeys()));
    result.put("lastModified", String.valueOf(stats.getLastModified()));
    result.put("dbName", dbName);
    return result;
  }

  @Override
  public Map<byte[], byte[]> prefixScan(byte[] prefix, int limit) throws Exception {
    // Use getNext to simulate prefix scan with limit
    return storageSPI.getNext(dbName, prefix, limit).get();
  }

  @Override
  public StorageBackend.StorageIterator iterator() throws Exception {
    org.tron.core.storage.spi.StorageIterator spiIterator = storageSPI.iterator(dbName).get();
    return new StorageIteratorAdapter(spiIterator);
  }

  /**
   * Adapter that implements the simplified StorageIterator interface using the full
   * StorageIterator.
   */
  private static class StorageIteratorAdapter implements StorageBackend.StorageIterator {
    private final org.tron.core.storage.spi.StorageIterator spiIterator;

    public StorageIteratorAdapter(org.tron.core.storage.spi.StorageIterator spiIterator) {
      this.spiIterator = spiIterator;
    }

    @Override
    public boolean hasNext() throws Exception {
      return spiIterator.hasNext().get();
    }

    @Override
    public Map.Entry<byte[], byte[]> next() throws Exception {
      return spiIterator.next().get();
    }

    @Override
    public void close() throws Exception {
      // StorageIterator close method returns void, not CompletableFuture
      spiIterator.close();
    }
  }
}
