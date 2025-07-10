package org.tron.core.storage.spi;

import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.concurrent.TimeUnit;
import org.junit.Test;

/** Debug test to understand batch get behavior */
public class DebugBatchTest {

  @Test
  public void testEmbeddedBatchGet() throws Exception {
    System.setProperty("storage.mode", "embedded");
    System.setProperty("storage.embedded.basePath", "test-data/debug");

    StorageSPI storage = StorageSpiFactory.createStorage();
    String testDbName = "debug-test-" + System.currentTimeMillis();

    try {
      // Initialize database
      StorageConfig config = new StorageConfig("ROCKSDB");
      storage.initDB(testDbName, config).get(10, TimeUnit.SECONDS);

      // Store some test data
      storage.put(testDbName, "key1".getBytes(), "value1".getBytes()).get();
      storage.put(testDbName, "key2".getBytes(), "value2".getBytes()).get();
      storage.put(testDbName, "key3".getBytes(), "value3".getBytes()).get();

      // Test batch get with same key instances
      byte[] key1 = "key1".getBytes();
      byte[] key2 = "key2".getBytes();
      byte[] key3 = "key3".getBytes();
      byte[] keyNonExistent = "nonexistent".getBytes();

      List<byte[]> keys = Arrays.asList(key1, key2, key3, keyNonExistent);

      Map<byte[], byte[]> results = storage.batchGet(testDbName, keys).get();

      System.out.println("Results size: " + results.size());
      System.out.println("Expected size: 4");

      // Check each key
      for (byte[] key : keys) {
        boolean found = results.containsKey(key);
        byte[] value = results.get(key);
        System.out.println(
            "Key: "
                + new String(key)
                + ", Found: "
                + found
                + ", Value: "
                + (value != null ? new String(value) : "null"));
      }

      // Test with new byte array instances (this is what the failing test does)
      System.out.println("\n--- Testing with new byte array instances ---");
      byte[] newKey1 = "key1".getBytes();
      boolean foundWithNewKey = results.containsKey(newKey1);
      byte[] valueWithNewKey = results.get(newKey1);
      System.out.println(
          "New key1 found: "
              + foundWithNewKey
              + ", value: "
              + (valueWithNewKey != null ? new String(valueWithNewKey) : "null"));

    } finally {
      if (storage instanceof EmbeddedStorageSPI) {
        ((EmbeddedStorageSPI) storage).close();
      }
    }
  }
}
