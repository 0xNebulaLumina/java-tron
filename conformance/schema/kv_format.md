# KV File Format Specification

Version: 1.0

## Overview

The `.kv` file format is a binary format for storing database key-value pairs in a deterministic, cross-platform manner. It is used to capture pre-execution and post-execution database state for conformance testing.

## File Structure

```
+------------------+
| Header (8 bytes) |
+------------------+
| Entry Count (4)  |
+------------------+
| Entry 0          |
+------------------+
| Entry 1          |
+------------------+
| ...              |
+------------------+
| Entry N-1        |
+------------------+
```

## Header Format (8 bytes)

| Offset | Size | Description |
|--------|------|-------------|
| 0 | 4 | Magic bytes: `KVDB` (0x4B 0x56 0x44 0x42) |
| 4 | 4 | Version number (big-endian uint32), currently `1` |

## Entry Count (4 bytes)

| Offset | Size | Description |
|--------|------|-------------|
| 8 | 4 | Number of entries (big-endian uint32) |

## Entry Format

Each entry consists of:

| Field | Size | Description |
|-------|------|-------------|
| Key Length | 4 | Length of key in bytes (big-endian uint32) |
| Key Data | variable | Raw key bytes |
| Value Length | 4 | Length of value in bytes (big-endian uint32, 0 for deletion marker) |
| Value Data | variable | Raw value bytes (omitted if length is 0) |

## Ordering

**CRITICAL**: Entries MUST be sorted lexicographically by key bytes. This ensures:
- Deterministic file generation across platforms
- Byte-for-byte comparison of files
- Efficient merge/diff operations

Lexicographic ordering means comparing keys byte-by-byte as unsigned integers (0x00 < 0x01 < ... < 0xFF).

## Value Semantics

- **Value Length = 0**: Represents a key that exists in the database with an empty value, or a deletion marker (context-dependent)
- **Key not present**: The key does not exist in the database

## Example

A database with two entries:
- Key: `0x01 0x02` → Value: `0xAA 0xBB 0xCC`
- Key: `0x03` → Value: `0xDD`

Binary representation (hex):
```
4B 56 44 42  # Magic "KVDB"
00 00 00 01  # Version 1
00 00 00 02  # 2 entries

# Entry 0
00 00 00 02  # Key length: 2
01 02        # Key data
00 00 00 03  # Value length: 3
AA BB CC     # Value data

# Entry 1
00 00 00 01  # Key length: 1
03           # Key data
00 00 00 01  # Value length: 1
DD           # Value data
```

Total size: 8 (header) + 4 (count) + 4+2+4+3 (entry 0) + 4+1+4+1 (entry 1) = 35 bytes

## Implementation Notes

### Java Writer

```java
public void writeKvFile(File file, Map<byte[], byte[]> data) throws IOException {
    // Sort keys
    List<byte[]> sortedKeys = new ArrayList<>(data.keySet());
    sortedKeys.sort(ByteArrayComparator.INSTANCE);

    try (DataOutputStream out = new DataOutputStream(
            new BufferedOutputStream(new FileOutputStream(file)))) {
        // Header
        out.write(new byte[] {0x4B, 0x56, 0x44, 0x42}); // KVDB
        out.writeInt(1); // Version

        // Entry count
        out.writeInt(sortedKeys.size());

        // Entries
        for (byte[] key : sortedKeys) {
            byte[] value = data.get(key);
            out.writeInt(key.length);
            out.write(key);
            out.writeInt(value != null ? value.length : 0);
            if (value != null && value.length > 0) {
                out.write(value);
            }
        }
    }
}
```

### Rust Reader

```rust
pub fn read_kv_file(path: &Path) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
    let mut file = File::open(path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    // Verify header
    if buf.len() < 12 || &buf[0..4] != b"KVDB" {
        return Err(Error::InvalidFormat);
    }

    let version = u32::from_be_bytes(buf[4..8].try_into()?);
    if version != 1 {
        return Err(Error::UnsupportedVersion(version));
    }

    let count = u32::from_be_bytes(buf[8..12].try_into()?) as usize;
    let mut result = BTreeMap::new();
    let mut offset = 12;

    for _ in 0..count {
        let key_len = u32::from_be_bytes(buf[offset..offset+4].try_into()?) as usize;
        offset += 4;
        let key = buf[offset..offset+key_len].to_vec();
        offset += key_len;

        let val_len = u32::from_be_bytes(buf[offset..offset+4].try_into()?) as usize;
        offset += 4;
        let value = if val_len > 0 {
            buf[offset..offset+val_len].to_vec()
        } else {
            Vec::new()
        };
        offset += val_len;

        result.insert(key, value);
    }

    Ok(result)
}
```

## Comparison Algorithm

To compare two KV files for equality:

1. **Fast path**: If file sizes differ, they are not equal
2. **Byte comparison**: Compare files byte-by-byte (since ordering is deterministic)

For detailed diff:

1. Parse both files into sorted key-value maps
2. Walk keys in merged order
3. Report: added keys, removed keys, modified values
