package org.tron.core.conformance;

import java.io.BufferedOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/**
 * Binary format for storing database key-value pairs in conformance fixtures.
 *
 * <p>Format specification:
 * - Header: 4-byte magic "KVDB" + 4-byte version (big-endian uint32)
 * - Entry count: 4-byte big-endian uint32
 * - Entries (sorted by key lexicographically):
 *   - Key length: 4-byte big-endian uint32
 *   - Key bytes
 *   - Value length: 4-byte big-endian uint32 (0 for deletion marker)
 *   - Value bytes (omitted if length is 0)
 */
public class KvFileFormat {

  private static final byte[] MAGIC = {'K', 'V', 'D', 'B'};
  private static final int VERSION = 1;

  /** Comparator for lexicographic byte array comparison. */
  public static final Comparator<byte[]> BYTE_ARRAY_COMPARATOR = (a, b) -> {
    int minLen = Math.min(a.length, b.length);
    for (int i = 0; i < minLen; i++) {
      int cmp = (a[i] & 0xFF) - (b[i] & 0xFF);
      if (cmp != 0) {
        return cmp;
      }
    }
    return a.length - b.length;
  };

  /**
   * Write key-value pairs to a .kv file.
   *
   * @param file Output file
   * @param data Key-value pairs to write
   * @throws IOException If writing fails
   */
  public static void write(File file, Map<byte[], byte[]> data) throws IOException {
    // Sort keys
    List<byte[]> sortedKeys = new ArrayList<>(data.keySet());
    sortedKeys.sort(BYTE_ARRAY_COMPARATOR);

    try (DataOutputStream out = new DataOutputStream(
        new BufferedOutputStream(new FileOutputStream(file)))) {
      // Write header
      out.write(MAGIC);
      out.writeInt(VERSION);

      // Write entry count
      out.writeInt(sortedKeys.size());

      // Write entries
      for (byte[] key : sortedKeys) {
        byte[] value = data.get(key);
        out.writeInt(key.length);
        out.write(key);
        if (value != null) {
          out.writeInt(value.length);
          if (value.length > 0) {
            out.write(value);
          }
        } else {
          out.writeInt(0); // Null value treated as deletion marker
        }
      }
    }
  }

  /**
   * Read key-value pairs from a .kv file.
   *
   * @param file Input file
   * @return TreeMap with byte array keys in lexicographic order
   * @throws IOException If reading fails or format is invalid
   */
  public static TreeMap<ByteArrayWrapper, byte[]> read(File file) throws IOException {
    TreeMap<ByteArrayWrapper, byte[]> result = new TreeMap<>();

    try (DataInputStream in = new DataInputStream(new FileInputStream(file))) {
      // Verify magic
      byte[] magic = new byte[4];
      in.readFully(magic);
      if (!Arrays.equals(magic, MAGIC)) {
        throw new IOException("Invalid KV file: bad magic bytes");
      }

      // Verify version
      int version = in.readInt();
      if (version != VERSION) {
        throw new IOException("Unsupported KV file version: " + version);
      }

      // Read entry count
      int count = in.readInt();

      // Read entries
      for (int i = 0; i < count; i++) {
        int keyLen = in.readInt();
        byte[] key = new byte[keyLen];
        in.readFully(key);

        int valLen = in.readInt();
        byte[] value;
        if (valLen > 0) {
          value = new byte[valLen];
          in.readFully(value);
        } else {
          value = new byte[0]; // Empty value (or deletion marker)
        }

        result.put(new ByteArrayWrapper(key), value);
      }
    }

    return result;
  }

  /**
   * Wrapper class for byte arrays to use as map keys.
   */
  public static class ByteArrayWrapper implements Comparable<ByteArrayWrapper> {
    private final byte[] data;

    public ByteArrayWrapper(byte[] data) {
      this.data = data;
    }

    public byte[] getData() {
      return data;
    }

    @Override
    public boolean equals(Object o) {
      if (this == o) {
        return true;
      }
      if (o == null || getClass() != o.getClass()) {
        return false;
      }
      ByteArrayWrapper that = (ByteArrayWrapper) o;
      return Arrays.equals(data, that.data);
    }

    @Override
    public int hashCode() {
      return Arrays.hashCode(data);
    }

    @Override
    public int compareTo(ByteArrayWrapper other) {
      return BYTE_ARRAY_COMPARATOR.compare(this.data, other.data);
    }

    @Override
    public String toString() {
      return org.tron.common.utils.ByteArray.toHexString(data);
    }
  }

  /**
   * Compare two KV files for equality.
   *
   * @param file1 First file
   * @param file2 Second file
   * @return true if contents are identical
   * @throws IOException If reading fails
   */
  public static boolean filesEqual(File file1, File file2) throws IOException {
    TreeMap<ByteArrayWrapper, byte[]> data1 = read(file1);
    TreeMap<ByteArrayWrapper, byte[]> data2 = read(file2);

    if (data1.size() != data2.size()) {
      return false;
    }

    for (Map.Entry<ByteArrayWrapper, byte[]> entry : data1.entrySet()) {
      byte[] value2 = data2.get(entry.getKey());
      if (value2 == null || !Arrays.equals(entry.getValue(), value2)) {
        return false;
      }
    }

    return true;
  }

  /**
   * Generate a diff between two KV files.
   *
   * @param file1 First file (expected)
   * @param file2 Second file (actual)
   * @return Human-readable diff string
   * @throws IOException If reading fails
   */
  public static String diff(File file1, File file2) throws IOException {
    TreeMap<ByteArrayWrapper, byte[]> expected = read(file1);
    TreeMap<ByteArrayWrapper, byte[]> actual = read(file2);

    StringBuilder sb = new StringBuilder();
    int added = 0, removed = 0, modified = 0;

    // Find removed and modified keys
    for (Map.Entry<ByteArrayWrapper, byte[]> entry : expected.entrySet()) {
      ByteArrayWrapper key = entry.getKey();
      byte[] actualValue = actual.get(key);

      if (actualValue == null) {
        sb.append("- ").append(key).append(": ").append(toHex(entry.getValue())).append("\n");
        removed++;
      } else if (!Arrays.equals(entry.getValue(), actualValue)) {
        sb.append("~ ").append(key).append(":\n");
        sb.append("  expected: ").append(toHex(entry.getValue())).append("\n");
        sb.append("  actual:   ").append(toHex(actualValue)).append("\n");
        modified++;
      }
    }

    // Find added keys
    for (Map.Entry<ByteArrayWrapper, byte[]> entry : actual.entrySet()) {
      if (!expected.containsKey(entry.getKey())) {
        sb.append("+ ").append(entry.getKey()).append(": ").append(toHex(entry.getValue())).append("\n");
        added++;
      }
    }

    if (added == 0 && removed == 0 && modified == 0) {
      return "Files are identical";
    }

    sb.insert(0, String.format("Diff: +%d, -%d, ~%d\n", added, removed, modified));
    return sb.toString();
  }

  private static String toHex(byte[] bytes) {
    if (bytes == null) {
      return "null";
    }
    if (bytes.length == 0) {
      return "(empty)";
    }
    if (bytes.length > 64) {
      return org.tron.common.utils.ByteArray.toHexString(Arrays.copyOf(bytes, 64)) + "... (" + bytes.length + " bytes)";
    }
    return org.tron.common.utils.ByteArray.toHexString(bytes);
  }
}
