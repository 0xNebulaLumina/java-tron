package org.tron.core.storage.spi;

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
    storageSPI.initialize(new StorageConfig()).get();
  }

  @Override
  public byte[] get(byte[] key) throws Exception {
    return storageSPI.get(key).get();
  }

  @Override
  public void put(byte[] key, byte[] value) throws Exception {
    storageSPI.put(key, value).get();
  }

  @Override
  public void delete(byte[] key) throws Exception {
    storageSPI.delete(key).get();
  }

  @Override
  public void batchPut(Map<byte[], byte[]> batch) throws Exception {
    storageSPI.batchPut(batch).get();
  }

  @Override
  public boolean exists(byte[] key) throws Exception {
    return storageSPI.exists(key).get();
  }

  @Override
  public Set<byte[]> getAllKeys() throws Exception {
    return storageSPI.getAllKeys().get();
  }

  @Override
  public Set<byte[]> getAllValues() throws Exception {
    return storageSPI.getAllValues().get();
  }

  @Override
  public long getSize() throws Exception {
    return storageSPI.getSize().get();
  }

  @Override
  public void clear() throws Exception {
    storageSPI.clear().get();
  }

  @Override
  public void flush() throws Exception {
    storageSPI.flush().get();
  }

  @Override
  public void close() throws Exception {
    storageSPI.close().get();
  }

  @Override
  public Map<String, String> getStats() throws Exception {
    return storageSPI.getStats().get();
  }

  @Override
  public Map<byte[], byte[]> prefixScan(byte[] prefix, int limit) throws Exception {
    return storageSPI.prefixScan(prefix, limit).get();
  }

  @Override
  public StorageBackend.StorageIterator iterator() throws Exception {
    org.tron.core.storage.spi.StorageIterator spiIterator = storageSPI.iterator().get();
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
      spiIterator.close().get();
    }
  }
}
