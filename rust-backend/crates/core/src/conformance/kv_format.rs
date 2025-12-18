//! Binary KV file format reader for conformance testing.
//!
//! Format specification:
//! - Header: 4-byte magic "KVDB" + 4-byte version (big-endian u32)
//! - Entry count: 4-byte big-endian u32
//! - Entries (sorted by key lexicographically):
//!   - Key length: 4-byte big-endian u32
//!   - Key bytes
//!   - Value length: 4-byte big-endian u32 (0 for deletion marker)
//!   - Value bytes (omitted if length is 0)

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Write, BufWriter};
use std::path::Path;

const MAGIC: &[u8; 4] = b"KVDB";
const VERSION: u32 = 1;

/// Error type for KV file operations
#[derive(Debug)]
pub enum KvError {
    Io(io::Error),
    InvalidMagic,
    UnsupportedVersion(u32),
    InvalidFormat(String),
}

impl From<io::Error> for KvError {
    fn from(err: io::Error) -> Self {
        KvError::Io(err)
    }
}

impl std::fmt::Display for KvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KvError::Io(e) => write!(f, "IO error: {}", e),
            KvError::InvalidMagic => write!(f, "Invalid KV file: bad magic bytes"),
            KvError::UnsupportedVersion(v) => write!(f, "Unsupported KV file version: {}", v),
            KvError::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
        }
    }
}

impl std::error::Error for KvError {}

/// Read a KV file into a BTreeMap (sorted by key)
pub fn read_kv_file(path: &Path) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, KvError> {
    let mut file = File::open(path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    if buf.len() < 12 {
        return Err(KvError::InvalidFormat("File too small".to_string()));
    }

    // Verify magic
    if &buf[0..4] != MAGIC {
        return Err(KvError::InvalidMagic);
    }

    // Verify version
    let version = u32::from_be_bytes(buf[4..8].try_into().unwrap());
    if version != VERSION {
        return Err(KvError::UnsupportedVersion(version));
    }

    // Read entry count
    let count = u32::from_be_bytes(buf[8..12].try_into().unwrap()) as usize;
    let mut result = BTreeMap::new();
    let mut offset = 12;

    for _ in 0..count {
        if offset + 4 > buf.len() {
            return Err(KvError::InvalidFormat("Unexpected end of file".to_string()));
        }

        let key_len = u32::from_be_bytes(buf[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        if offset + key_len > buf.len() {
            return Err(KvError::InvalidFormat("Key extends past end of file".to_string()));
        }

        let key = buf[offset..offset + key_len].to_vec();
        offset += key_len;

        if offset + 4 > buf.len() {
            return Err(KvError::InvalidFormat("Unexpected end of file".to_string()));
        }

        let val_len = u32::from_be_bytes(buf[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        let value = if val_len > 0 {
            if offset + val_len > buf.len() {
                return Err(KvError::InvalidFormat("Value extends past end of file".to_string()));
            }
            let v = buf[offset..offset + val_len].to_vec();
            offset += val_len;
            v
        } else {
            Vec::new()
        };

        result.insert(key, value);
    }

    Ok(result)
}

/// Write a BTreeMap to a KV file
pub fn write_kv_file(path: &Path, data: &BTreeMap<Vec<u8>, Vec<u8>>) -> Result<(), KvError> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);

    // Write header
    writer.write_all(MAGIC)?;
    writer.write_all(&VERSION.to_be_bytes())?;

    // Write entry count
    writer.write_all(&(data.len() as u32).to_be_bytes())?;

    // Write entries (already sorted by BTreeMap)
    for (key, value) in data {
        writer.write_all(&(key.len() as u32).to_be_bytes())?;
        writer.write_all(key)?;
        writer.write_all(&(value.len() as u32).to_be_bytes())?;
        if !value.is_empty() {
            writer.write_all(value)?;
        }
    }

    Ok(())
}

/// Compare two KV file contents
pub fn compare_kv_data(
    expected: &BTreeMap<Vec<u8>, Vec<u8>>,
    actual: &BTreeMap<Vec<u8>, Vec<u8>>,
) -> KvDiff {
    let mut diff = KvDiff::default();

    // Find removed and modified keys
    for (key, expected_value) in expected {
        match actual.get(key) {
            None => {
                diff.removed.push(key.clone());
            }
            Some(actual_value) => {
                if expected_value != actual_value {
                    diff.modified.push(KeyDiff {
                        key: key.clone(),
                        expected: expected_value.clone(),
                        actual: actual_value.clone(),
                    });
                }
            }
        }
    }

    // Find added keys
    for key in actual.keys() {
        if !expected.contains_key(key) {
            diff.added.push(key.clone());
        }
    }

    diff
}

/// Difference between two KV datasets
#[derive(Default, Debug)]
pub struct KvDiff {
    pub added: Vec<Vec<u8>>,
    pub removed: Vec<Vec<u8>>,
    pub modified: Vec<KeyDiff>,
}

impl KvDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "+{} -{} ~{}",
            self.added.len(),
            self.removed.len(),
            self.modified.len()
        )
    }
}

#[derive(Debug)]
pub struct KeyDiff {
    pub key: Vec<u8>,
    pub expected: Vec<u8>,
    pub actual: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.kv");

        let mut data = BTreeMap::new();
        data.insert(vec![0x01, 0x02], vec![0xAA, 0xBB, 0xCC]);
        data.insert(vec![0x03], vec![0xDD]);
        data.insert(vec![0x04, 0x05, 0x06], vec![]); // Empty value

        write_kv_file(&path, &data).unwrap();
        let loaded = read_kv_file(&path).unwrap();

        assert_eq!(data, loaded);
    }

    #[test]
    fn test_compare_identical() {
        let mut data1 = BTreeMap::new();
        data1.insert(vec![0x01], vec![0xAA]);

        let data2 = data1.clone();
        let diff = compare_kv_data(&data1, &data2);

        assert!(diff.is_empty());
    }

    #[test]
    fn test_compare_added() {
        let data1 = BTreeMap::new();
        let mut data2 = BTreeMap::new();
        data2.insert(vec![0x01], vec![0xAA]);

        let diff = compare_kv_data(&data1, &data2);

        assert_eq!(diff.added.len(), 1);
        assert!(diff.removed.is_empty());
        assert!(diff.modified.is_empty());
    }

    #[test]
    fn test_compare_removed() {
        let mut data1 = BTreeMap::new();
        data1.insert(vec![0x01], vec![0xAA]);

        let data2 = BTreeMap::new();
        let diff = compare_kv_data(&data1, &data2);

        assert!(diff.added.is_empty());
        assert_eq!(diff.removed.len(), 1);
        assert!(diff.modified.is_empty());
    }

    #[test]
    fn test_compare_modified() {
        let mut data1 = BTreeMap::new();
        data1.insert(vec![0x01], vec![0xAA]);

        let mut data2 = BTreeMap::new();
        data2.insert(vec![0x01], vec![0xBB]);

        let diff = compare_kv_data(&data1, &data2);

        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert_eq!(diff.modified.len(), 1);
        assert_eq!(diff.modified[0].expected, vec![0xAA]);
        assert_eq!(diff.modified[0].actual, vec![0xBB]);
    }
}
