package org.tron.core.conformance;

import java.io.BufferedOutputStream;
import java.io.DataOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.BufferUnderflowException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.SortedMap;
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
   *   - Value length: 4-byte big-endian uint32 (0 for empty value)
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
   * <p>Keys must be unique by bytes, and values must be non-null. To represent a deletion, omit
   * the key entirely.
   *
   * @param file Output file
   * @param data Key-value pairs to write
   * @throws IOException If writing fails
   */
  public static void write(File file, SortedMap<byte[], byte[]> data) throws IOException {
    // Sort keys (enforce canonical lexicographic ordering regardless of input map ordering).
    List<byte[]> sortedKeys = new ArrayList<>(data.keySet());
    sortedKeys.sort(BYTE_ARRAY_COMPARATOR);

    // Prevent non-canonical output when duplicate keys compare equal byte-wise.
    for (int i = 1; i < sortedKeys.size(); i++) {
      byte[] prev = sortedKeys.get(i - 1);
      byte[] curr = sortedKeys.get(i);
      if (BYTE_ARRAY_COMPARATOR.compare(prev, curr) == 0) {
        throw new IllegalArgumentException(
            "Duplicate keys with identical bytes are not supported: "
                + org.tron.common.utils.ByteArray.toHexString(curr));
      }
    }

    try (DataOutputStream out = new DataOutputStream(
        new BufferedOutputStream(new FileOutputStream(file)))) {
      // Write header
      out.write(MAGIC);
      out.writeInt(VERSION);

      // Write entry count
      out.writeInt(sortedKeys.size());

      // Write entries
      for (byte[] key : sortedKeys) {
        if (key == null) {
          throw new IllegalArgumentException("Null keys are not supported");
        }
        byte[] value = data.get(key);
        if (value == null) {
          throw new IllegalArgumentException(
              "Null values are not supported; omit the key to represent deletion");
        }
        out.writeInt(key.length);
        out.write(key);
        out.writeInt(value.length);
        if (value.length > 0) {
          out.write(value);
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

    byte[] bytes = Files.readAllBytes(file.toPath());
    if (bytes.length < 12) {
      throw new IOException("Invalid KV file: file too small (" + bytes.length + " bytes)");
    }

    ByteBuffer buffer = ByteBuffer.wrap(bytes).order(ByteOrder.BIG_ENDIAN);

    // Verify magic
    byte[] magic = new byte[4];
    buffer.get(magic);
    if (!Arrays.equals(magic, MAGIC)) {
      throw new IOException("Invalid KV file: bad magic bytes");
    }

    // Verify version
    long version = readUint32(buffer, "version");
    if (version != VERSION) {
      throw new IOException("Unsupported KV file version: " + version);
    }

    // Read entry count
    long countLong = readUint32(buffer, "entry count");
    if (countLong > Integer.MAX_VALUE) {
      throw new IOException("Invalid KV file: entry count too large: " + countLong);
    }
    int count = (int) countLong;

    // Read entries
    byte[] previousKey = null;
    for (int i = 0; i < count; i++) {
      long keyLenLong = readUint32(buffer, "key length (entry " + i + ")");
      if (keyLenLong > Integer.MAX_VALUE) {
        throw new IOException(
            "Invalid KV file: key length too large in entry " + i + ": " + keyLenLong);
      }
      int keyLen = (int) keyLenLong;
      if (keyLen > buffer.remaining()) {
        throw new IOException("Invalid KV file: key extends past end of file in entry " + i);
      }
      byte[] key = new byte[keyLen];
      buffer.get(key);

      if (previousKey != null && BYTE_ARRAY_COMPARATOR.compare(previousKey, key) > 0) {
        throw new IOException("Invalid KV file: keys are not sorted lexicographically");
      }
      previousKey = key;

      long valLenLong = readUint32(buffer, "value length (entry " + i + ")");
      if (valLenLong > Integer.MAX_VALUE) {
        throw new IOException(
            "Invalid KV file: value length too large in entry " + i + ": " + valLenLong);
      }
      int valLen = (int) valLenLong;
      if (valLen > buffer.remaining()) {
        throw new IOException("Invalid KV file: value extends past end of file in entry " + i);
      }
      byte[] value = new byte[valLen];
      buffer.get(value);

      result.put(new ByteArrayWrapper(key, false), value);
    }

    return result;
  }

  private static long readUint32(ByteBuffer buffer, String fieldName) throws IOException {
    try {
      return Integer.toUnsignedLong(buffer.getInt());
    } catch (BufferUnderflowException e) {
      throw new IOException(
          "Invalid KV file: unexpected end of file while reading " + fieldName, e);
    }
  }

  /**
   * Wrapper class for byte arrays to use as map keys.
   */
  public static class ByteArrayWrapper implements Comparable<ByteArrayWrapper> {
    private final byte[] data;

    public ByteArrayWrapper(byte[] data) {
      this(data, true);
    }

    private ByteArrayWrapper(byte[] data, boolean copy) {
      if (data == null) {
        throw new IllegalArgumentException("data is null");
      }
      if (copy) {
        this.data = Arrays.copyOf(data, data.length);
      } else {
        this.data = data;
      }
    }

    public byte[] getData() {
      return Arrays.copyOf(data, data.length);
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
        sb.append("+ ")
            .append(entry.getKey())
            .append(": ")
            .append(toHex(entry.getValue()))
            .append("\n");
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
      String prefix = org.tron.common.utils.ByteArray.toHexString(Arrays.copyOf(bytes, 64));
      return prefix + "... (" + bytes.length + " bytes)";
    }
    return org.tron.common.utils.ByteArray.toHexString(bytes);
  }
}
