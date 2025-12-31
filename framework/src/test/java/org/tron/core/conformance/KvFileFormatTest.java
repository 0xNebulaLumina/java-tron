package org.tron.core.conformance;

import java.io.ByteArrayOutputStream;
import java.io.DataOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.util.IdentityHashMap;
import java.util.SortedMap;
import java.util.TreeMap;
import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.TemporaryFolder;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

public class KvFileFormatTest {

  @Rule
  public TemporaryFolder temporaryFolder = new TemporaryFolder();

  @Test
  public void testWriteReadRoundTrip() throws Exception {
    File file = temporaryFolder.newFile("round_trip.kv");

    SortedMap<byte[], byte[]> data = new TreeMap<>(KvFileFormat.BYTE_ARRAY_COMPARATOR);
    data.put(new byte[] {0x01, 0x02}, new byte[] {(byte) 0xAA, (byte) 0xBB, (byte) 0xCC});
    data.put(new byte[] {0x03}, new byte[] {});

    KvFileFormat.write(file, data);

    SortedMap<KvFileFormat.ByteArrayWrapper, byte[]> loaded = KvFileFormat.read(file);
    assertEquals(2, loaded.size());
    assertArrayEquals(
        new byte[] {(byte) 0xAA, (byte) 0xBB, (byte) 0xCC},
        loaded.get(new KvFileFormat.ByteArrayWrapper(new byte[] {0x01, 0x02})));
    assertArrayEquals(
        new byte[] {},
        loaded.get(new KvFileFormat.ByteArrayWrapper(new byte[] {0x03})));
  }

  @Test
  public void testReadRejectsEntryCountTooLarge() throws Exception {
    File file = temporaryFolder.newFile("bad_count.kv");

    try (DataOutputStream out = new DataOutputStream(new FileOutputStream(file))) {
      out.write(new byte[] {'K', 'V', 'D', 'B'});
      out.writeInt(1);
      out.writeInt(-1); // 0xFFFFFFFF
    }

    try {
      KvFileFormat.read(file);
      fail("Expected IOException");
    } catch (IOException e) {
      assertTrue(e.getMessage().contains("entry count"));
    }
  }

  @Test
  public void testReadRejectsKeyLengthTooLarge() throws Exception {
    File file = temporaryFolder.newFile("bad_key_len.kv");

    try (DataOutputStream out = new DataOutputStream(new FileOutputStream(file))) {
      out.write(new byte[] {'K', 'V', 'D', 'B'});
      out.writeInt(1);
      out.writeInt(1);
      out.writeInt(-1); // key length 0xFFFFFFFF
    }

    try {
      KvFileFormat.read(file);
      fail("Expected IOException");
    } catch (IOException e) {
      assertTrue(e.getMessage().contains("key length"));
    }
  }

  @Test
  public void testReadRejectsTruncatedValue() throws Exception {
    File file = temporaryFolder.newFile("bad_value.kv");

    try (DataOutputStream out = new DataOutputStream(new FileOutputStream(file))) {
      out.write(new byte[] {'K', 'V', 'D', 'B'});
      out.writeInt(1);
      out.writeInt(1);
      out.writeInt(1);
      out.writeByte(0x01);
      out.writeInt(2);
      out.writeByte(0x02);
    }

    try {
      KvFileFormat.read(file);
      fail("Expected IOException");
    } catch (IOException e) {
      assertTrue(e.getMessage().contains("value extends past end of file"));
    }
  }

  @Test
  public void testReadRejectsUnsortedKeys() throws Exception {
    File file = temporaryFolder.newFile("unsorted_keys.kv");

    try (DataOutputStream out = new DataOutputStream(new FileOutputStream(file))) {
      out.write(new byte[] {'K', 'V', 'D', 'B'});
      out.writeInt(1);
      out.writeInt(2);

      out.writeInt(1);
      out.writeByte(0x02);
      out.writeInt(0);

      out.writeInt(1);
      out.writeByte(0x01);
      out.writeInt(0);
    }

    try {
      KvFileFormat.read(file);
      fail("Expected IOException");
    } catch (IOException e) {
      assertTrue(e.getMessage().contains("sorted"));
    }
  }

  @Test
  public void testWriteRejectsDuplicateKeyBytes() throws Exception {
    File file = temporaryFolder.newFile("duplicate_keys.kv");

    IdentityHashMap<byte[], Integer> ids = new IdentityHashMap<>();
    SortedMap<byte[], byte[]> data = new TreeMap<>((a, b) -> {
      if (a == b) {
        return 0;
      }
      Integer idA = ids.get(a);
      Integer idB = ids.get(b);
      if (idA == null || idB == null) {
        throw new IllegalStateException("Missing identity id");
      }
      return idA.compareTo(idB);
    });

    byte[] key1 = new byte[] {0x01};
    byte[] key2 = new byte[] {0x01}; // Same bytes, different instance.
    ids.put(key1, 1);
    ids.put(key2, 2);

    data.put(key1, new byte[] {0x0A});
    data.put(key2, new byte[] {0x0B});

    try {
      KvFileFormat.write(file, data);
      fail("Expected IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      assertTrue(e.getMessage().contains("Duplicate keys"));
    }
  }

  @Test
  public void testReadRejectsUnexpectedEndOfFile() throws Exception {
    File file = temporaryFolder.newFile("truncated_header.kv");

    byte[] bytes = new byte[] {'K', 'V', 'D', 'B', 0, 0, 0};
    try (FileOutputStream out = new FileOutputStream(file)) {
      out.write(bytes);
    }

    try {
      KvFileFormat.read(file);
      fail("Expected IOException");
    } catch (IOException e) {
      assertTrue(e.getMessage().contains("file too small"));
    }
  }

  @Test
  public void testReadRejectsBadMagic() throws Exception {
    File file = temporaryFolder.newFile("bad_magic.kv");

    try (DataOutputStream out = new DataOutputStream(new FileOutputStream(file))) {
      out.write(new byte[] {'B', 'A', 'D', '!'});
      out.writeInt(1);
      out.writeInt(0);
    }

    try {
      KvFileFormat.read(file);
      fail("Expected IOException");
    } catch (IOException e) {
      assertTrue(e.getMessage().contains("bad magic"));
    }
  }

  @Test
  public void testReadRejectsUnsupportedVersion() throws Exception {
    File file = temporaryFolder.newFile("bad_version.kv");

    try (DataOutputStream out = new DataOutputStream(new FileOutputStream(file))) {
      out.write(new byte[] {'K', 'V', 'D', 'B'});
      out.writeInt(2);
      out.writeInt(0);
    }

    try {
      KvFileFormat.read(file);
      fail("Expected IOException");
    } catch (IOException e) {
      assertTrue(e.getMessage().contains("Unsupported KV file version"));
    }
  }

  @Test
  public void testReadRejectsValueLengthTooLarge() throws Exception {
    File file = temporaryFolder.newFile("bad_val_len.kv");

    try (DataOutputStream out = new DataOutputStream(new FileOutputStream(file))) {
      out.write(new byte[] {'K', 'V', 'D', 'B'});
      out.writeInt(1);
      out.writeInt(1);
      out.writeInt(1);
      out.writeByte(0x01);
      out.writeInt(-1); // value length 0xFFFFFFFF
    }

    try {
      KvFileFormat.read(file);
      fail("Expected IOException");
    } catch (IOException e) {
      assertTrue(e.getMessage().contains("value length"));
    }
  }

  @Test
  public void testReadRejectsKeyExtendsPastEndOfFile() throws Exception {
    File file = temporaryFolder.newFile("key_past_eof.kv");

    try (DataOutputStream out = new DataOutputStream(new FileOutputStream(file))) {
      out.write(new byte[] {'K', 'V', 'D', 'B'});
      out.writeInt(1);
      out.writeInt(1);
      out.writeInt(2);
      out.writeByte(0x01);
    }

    try {
      KvFileFormat.read(file);
      fail("Expected IOException");
    } catch (IOException e) {
      assertTrue(e.getMessage().contains("key extends past end of file"));
    }
  }

  @Test
  public void testReadRejectsCountPastEndOfFile() throws Exception {
    File file = temporaryFolder.newFile("count_past_eof.kv");

    ByteArrayOutputStream baos = new ByteArrayOutputStream();
    try (DataOutputStream out = new DataOutputStream(baos)) {
      out.write(new byte[] {'K', 'V', 'D', 'B'});
      out.writeInt(1);
      out.writeInt(1);
    }

    // Truncate after count; missing entry data.
    try (FileOutputStream out = new FileOutputStream(file)) {
      out.write(baos.toByteArray());
    }

    try {
      KvFileFormat.read(file);
      fail("Expected IOException");
    } catch (IOException e) {
      assertTrue(e.getMessage().contains("unexpected end of file"));
    }
  }
}

