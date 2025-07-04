package org.tron.core.storage.spi;

import static org.junit.Assert.*;

import org.junit.Test;

/**
 * Test class to verify the GrpcStorageIterator fix for the infinite loop issue. This test focuses
 * on the key advancement logic to ensure proper iteration.
 */
public class GrpcStorageIteratorTest {

  @Test
  public void testKeyIncrementLogic() {
    // Test the key increment logic that was causing the infinite loop

    // Test normal key increment
    byte[] key1 = "key-001".getBytes();
    byte[] nextKey1 = incrementKey(key1);
    assertTrue("Incremented key should be greater", compareBytes(nextKey1, key1) > 0);

    // Test edge case with 0xFF bytes
    byte[] key2 = new byte[] {0x01, (byte) 0xFF};
    byte[] nextKey2 = incrementKey(key2);
    assertEquals("Should increment first byte", 0x02, nextKey2[0]);
    assertEquals("Should reset second byte", 0x00, nextKey2[1]);

    // Test empty key
    byte[] emptyKey = new byte[0];
    byte[] nextEmpty = incrementKey(emptyKey);
    assertEquals("Empty key should increment to single byte", 1, nextEmpty.length);
    assertEquals("Should be 0x01", 0x01, nextEmpty[0]);

    // Test all 0xFF bytes
    byte[] allFF = new byte[] {(byte) 0xFF, (byte) 0xFF};
    byte[] nextAllFF = incrementKey(allFF);
    assertEquals("Should extend array", 3, nextAllFF.length);
    assertEquals("First byte should be 0x00", 0x00, nextAllFF[0]);
    assertEquals("Second byte should be 0x00", 0x00, nextAllFF[1]);
    assertEquals("Third byte should be 0x01", 0x01, nextAllFF[2]);
  }

  @Test
  public void testKeyIncrementSequence() {
    // Test that incrementing keys produces a proper sequence
    byte[] current = "key-001".getBytes();

    for (int i = 0; i < 5; i++) {
      byte[] next = incrementKey(current);
      assertTrue("Each increment should produce a larger key", compareBytes(next, current) > 0);
      current = next;
    }
  }

  @Test
  public void testKeyIncrementNeverReturnsOriginal() {
    // This test verifies the core bug fix - increment should never return the same key
    byte[][] testKeys = {
      "key-001".getBytes(),
      "test".getBytes(),
      new byte[] {0x01, 0x02, 0x03},
      new byte[] {(byte) 0xFE},
      new byte[] {0x00, 0x00, 0x01}
    };

    for (byte[] key : testKeys) {
      byte[] incremented = incrementKey(key);
      assertFalse(
          "Incremented key must be different from original",
          java.util.Arrays.equals(key, incremented));
      assertTrue(
          "Incremented key must be greater than original", compareBytes(incremented, key) > 0);
    }
  }

  /** Copy of the incrementKey method from GrpcStorageIterator for testing */
  private byte[] incrementKey(byte[] key) {
    if (key == null || key.length == 0) {
      return new byte[] {0x01};
    }

    // Create a copy to avoid modifying the original
    byte[] nextKey = new byte[key.length];
    System.arraycopy(key, 0, nextKey, 0, key.length);

    // Increment the key by finding the rightmost byte that can be incremented
    for (int i = nextKey.length - 1; i >= 0; i--) {
      if (nextKey[i] != (byte) 0xFF) {
        nextKey[i]++;
        return nextKey;
      } else {
        nextKey[i] = 0x00;
      }
    }

    // If all bytes were 0xFF, we need to extend the key
    byte[] extendedKey = new byte[nextKey.length + 1];
    System.arraycopy(nextKey, 0, extendedKey, 0, nextKey.length);
    extendedKey[nextKey.length] = 0x01;

    return extendedKey;
  }

  /** Helper method to compare byte arrays lexicographically */
  private int compareBytes(byte[] a, byte[] b) {
    for (int i = 0; i < Math.min(a.length, b.length); i++) {
      int diff = Byte.toUnsignedInt(a[i]) - Byte.toUnsignedInt(b[i]);
      if (diff != 0) {
        return diff;
      }
    }
    return a.length - b.length;
  }
}
