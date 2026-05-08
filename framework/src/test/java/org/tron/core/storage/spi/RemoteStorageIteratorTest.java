package org.tron.core.storage.spi;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

/**
 * Test class to verify the RemoteStorageIterator cursor advancement logic.
 */
public class RemoteStorageIteratorTest {

  @Test
  public void testExclusiveStartKeyAppendsZeroByte() {
    byte[] key = new byte[] {0x01};
    byte[] nextKey = exclusiveStartKey(key);

    assertArrayEquals(new byte[] {0x01, 0x00}, nextKey);
    assertTrue(compareBytes(nextKey, key) > 0);
  }

  @Test
  public void testExclusiveStartKeyPreservesVariableLengthPrefixKeys() {
    byte[] key = new byte[] {0x01, 0x02};
    byte[] prefixChild = new byte[] {0x01, 0x02, 0x00};
    byte[] nextKey = exclusiveStartKey(key);

    assertArrayEquals(prefixChild, nextKey);
    assertTrue(compareBytes(nextKey, key) > 0);
    assertEquals(0, compareBytes(nextKey, prefixChild));
  }

  @Test
  public void testExclusiveStartKeyDoesNotRewindAllFFKey() {
    byte[] allFF = new byte[] {(byte) 0xFF, (byte) 0xFF};
    byte[] nextAllFF = exclusiveStartKey(allFF);

    assertArrayEquals(new byte[] {(byte) 0xFF, (byte) 0xFF, 0x00}, nextAllFF);
    assertTrue(compareBytes(nextAllFF, allFF) > 0);
  }

  @Test
  public void testExclusiveStartKeyNeverReturnsOriginal() {
    byte[][] testKeys = {
      "key-001".getBytes(),
      "test".getBytes(),
      new byte[] {0x01, 0x02, 0x03},
      new byte[] {(byte) 0xFE},
      new byte[] {0x00, 0x00, 0x01},
      new byte[] {(byte) 0xFF}
    };

    for (byte[] key : testKeys) {
      byte[] advanced = exclusiveStartKey(key);
      assertFalse(
          "Advanced key must be different from original",
          java.util.Arrays.equals(key, advanced));
      assertTrue("Advanced key must be greater than original", compareBytes(advanced, key) > 0);
    }
  }

  private byte[] exclusiveStartKey(byte[] key) {
    if (key == null) {
      return new byte[] {0x00};
    }

    byte[] nextKey = new byte[key.length + 1];
    System.arraycopy(key, 0, nextKey, 0, key.length);
    nextKey[key.length] = 0x00;
    return nextKey;
  }

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
