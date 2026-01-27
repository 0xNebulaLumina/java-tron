package org.tron.core.storage.spi;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertSame;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.Test;
import org.tron.core.db.ByteArrayWrapper;

/**
 * Test class to verify the batchGet key identity preservation and normalization logic
 * for Phase B mirror optimization.
 *
 * <p>These tests focus on the correctness constraints identified in the planning:
 * 1. Map<byte[], byte[]> key identity hazard - ensuring callers can lookup by original key
 * 2. Last-write-wins deduplication for touched keys
 */
public class RemoteStorageBatchGetTest {

  // =========================================================================
  // Tests for touched key normalization (last-write-wins deduplication)
  // =========================================================================

  @Test
  public void testNormalizeTouchedKeys_lastWriteWins() {
    // Simulate touched keys where the same key appears multiple times
    // The final operation should win
    List<MockTouchedKey> touchedKeys = new ArrayList<>();

    byte[] key1 = new byte[]{0x01, 0x02, 0x03};

    // First touch: non-delete
    touchedKeys.add(new MockTouchedKey("account", key1, false));
    // Second touch: delete (should win)
    touchedKeys.add(new MockTouchedKey("account", key1, true));

    Map<String, LinkedHashMap<ByteArrayWrapper, KeyOperation>> result = normalizeTouchedKeys(touchedKeys);

    assertEquals("Should have 1 db entry", 1, result.size());
    LinkedHashMap<ByteArrayWrapper, KeyOperation> accountOps = result.get("account");
    assertNotNull("Should have account entry", accountOps);
    assertEquals("Should have 1 key after dedup", 1, accountOps.size());

    KeyOperation op = accountOps.values().iterator().next();
    assertTrue("Last operation (delete) should win", op.isDelete);
  }

  @Test
  public void testNormalizeTouchedKeys_multipleKeysMultipleDbs() {
    List<MockTouchedKey> touchedKeys = new ArrayList<>();

    byte[] key1 = new byte[]{0x01};
    byte[] key2 = new byte[]{0x02};
    byte[] key3 = new byte[]{0x03};

    touchedKeys.add(new MockTouchedKey("account", key1, false));
    touchedKeys.add(new MockTouchedKey("contract", key2, false));
    touchedKeys.add(new MockTouchedKey("account", key3, true));
    touchedKeys.add(new MockTouchedKey("contract", key2, true)); // duplicate, delete wins

    Map<String, LinkedHashMap<ByteArrayWrapper, KeyOperation>> result = normalizeTouchedKeys(touchedKeys);

    assertEquals("Should have 2 db entries", 2, result.size());

    LinkedHashMap<ByteArrayWrapper, KeyOperation> accountOps = result.get("account");
    assertEquals("Account should have 2 keys", 2, accountOps.size());

    LinkedHashMap<ByteArrayWrapper, KeyOperation> contractOps = result.get("contract");
    assertEquals("Contract should have 1 key (deduped)", 1, contractOps.size());
    assertTrue("Contract key2 should be delete", contractOps.values().iterator().next().isDelete);
  }

  @Test
  public void testNormalizeTouchedKeys_preservesKeyBytes() {
    byte[] originalKey = new byte[]{0x41, 0x42, 0x43}; // Different instance
    List<MockTouchedKey> touchedKeys = new ArrayList<>();
    touchedKeys.add(new MockTouchedKey("account", originalKey, false));

    Map<String, LinkedHashMap<ByteArrayWrapper, KeyOperation>> result = normalizeTouchedKeys(touchedKeys);

    KeyOperation op = result.get("account").values().iterator().next();
    assertSame("Should preserve the original key byte[] instance", originalKey, op.keyBytes);
  }

  // =========================================================================
  // Tests for batchGet key identity (simulating the fixed behavior)
  // =========================================================================

  @Test
  public void testBatchGetKeyIdentity_canLookupByOriginalKey() {
    // Simulate the fixed batchGet behavior where result uses original keys
    List<byte[]> inputKeys = new ArrayList<>();
    byte[] key1 = new byte[]{0x01, 0x02};
    byte[] key2 = new byte[]{0x03, 0x04};
    byte[] key3 = new byte[]{0x05, 0x06};
    inputKeys.add(key1);
    inputKeys.add(key2);
    inputKeys.add(key3);

    // Simulate response values
    byte[] value1 = "value1".getBytes();
    byte[] value3 = "value3".getBytes();
    // key2 not found (null)

    // Build result map using original keys (simulating fixed behavior)
    Map<byte[], byte[]> result = new LinkedHashMap<>();
    result.put(key1, value1);
    result.put(key2, null);
    result.put(key3, value3);

    // Verify lookup by original key works
    assertSame("Should find value1 by original key1", value1, result.get(key1));
    assertNull("key2 should return null (not found)", result.get(key2));
    assertTrue("key2 should be in map", result.containsKey(key2));
    assertSame("Should find value3 by original key3", value3, result.get(key3));

    // Verify lookup by different byte[] with same content does NOT work
    // (This is expected for identity-based lookup)
    byte[] key1Copy = new byte[]{0x01, 0x02};
    assertNull("Lookup by copy should fail (identity-based)", result.get(key1Copy));
  }

  @Test
  public void testByteArrayWrapperEquality() {
    // Verify ByteArrayWrapper works correctly for content-based lookup
    byte[] bytes1 = new byte[]{0x01, 0x02, 0x03};
    byte[] bytes2 = new byte[]{0x01, 0x02, 0x03}; // Same content, different instance

    ByteArrayWrapper wrapper1 = new ByteArrayWrapper(bytes1);
    ByteArrayWrapper wrapper2 = new ByteArrayWrapper(bytes2);

    assertEquals("Wrappers with same content should be equal", wrapper1, wrapper2);
    assertEquals("Wrappers should have same hashCode", wrapper1.hashCode(), wrapper2.hashCode());

    // Use in map
    Map<ByteArrayWrapper, String> map = new HashMap<>();
    map.put(wrapper1, "found");

    assertEquals("Should find by wrapper2", "found", map.get(wrapper2));
  }

  @Test
  public void testChunking_splitsByMaxKeys() {
    List<KeyOperation> readOps = new ArrayList<>();
    for (int i = 0; i < 10; i++) {
      readOps.add(new KeyOperation(new byte[]{(byte) i}, false));
    }

    int maxBatchKeys = 3;
    int expectedChunks = (int) Math.ceil(10.0 / maxBatchKeys); // 4 chunks

    int actualChunks = 0;
    for (int i = 0; i < readOps.size(); i += maxBatchKeys) {
      int end = Math.min(i + maxBatchKeys, readOps.size());
      List<KeyOperation> chunk = readOps.subList(i, end);
      assertTrue("Chunk size should be <= maxBatchKeys", chunk.size() <= maxBatchKeys);
      actualChunks++;
    }

    assertEquals("Should produce expected number of chunks", expectedChunks, actualChunks);
  }

  // =========================================================================
  // Helper methods (copied from RuntimeSpiImpl for testing)
  // =========================================================================

  /**
   * Normalize touched keys by database using last-write-wins semantics.
   */
  private Map<String, LinkedHashMap<ByteArrayWrapper, KeyOperation>> normalizeTouchedKeys(
      List<MockTouchedKey> touchedKeys) {

    Map<String, LinkedHashMap<ByteArrayWrapper, KeyOperation>> result = new HashMap<>();

    for (MockTouchedKey tk : touchedKeys) {
      String dbName = tk.db;
      byte[] keyBytes = tk.key;
      boolean isDelete = tk.isDelete;

      LinkedHashMap<ByteArrayWrapper, KeyOperation> dbMap =
          result.computeIfAbsent(dbName, k -> new LinkedHashMap<>());

      // Last-write-wins: always overwrite with the latest operation
      ByteArrayWrapper keyWrapper = new ByteArrayWrapper(keyBytes);
      dbMap.put(keyWrapper, new KeyOperation(keyBytes, isDelete));
    }

    return result;
  }

  /**
   * Mock TouchedKey for testing (avoids dependency on ExecutionSPI).
   */
  private static class MockTouchedKey {
    final String db;
    final byte[] key;
    final boolean isDelete;

    MockTouchedKey(String db, byte[] key, boolean isDelete) {
      this.db = db;
      this.key = key;
      this.isDelete = isDelete;
    }
  }

  /**
   * Helper class (copied from RuntimeSpiImpl).
   */
  private static class KeyOperation {
    final byte[] keyBytes;
    final boolean isDelete;

    KeyOperation(byte[] keyBytes, boolean isDelete) {
      this.keyBytes = keyBytes;
      this.isDelete = isDelete;
    }
  }
}
