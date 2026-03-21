//! ExecutionWriteBuffer: Atomic write buffer for transaction execution.
//!
//! ## Purpose
//!
//! This module provides a write buffer that accumulates all storage operations
//! during transaction execution and only commits them atomically on success.
//! This is essential for Phase B conformance testing where:
//! - `validate_fail` fixtures require **zero writes** to post_db
//! - Success cases require all writes to commit atomically
//!
//! ## Design
//!
//! The buffer follows a two-phase approach:
//! 1. **Accumulation Phase**: All puts/deletes are recorded in memory
//! 2. **Commit Phase**: On success, all operations are batch-written to storage
//!
//! On failure, the buffer is simply dropped (no writes occur).
//!
//! ## Usage
//!
//! ```ignore
//! let mut buffer = ExecutionWriteBuffer::new();
//!
//! // During execution, record operations
//! buffer.put("account", key, value);
//! buffer.delete("votes", key);
//!
//! // On success, commit all operations atomically
//! buffer.commit(storage_engine)?;
//!
//! // On failure, just drop the buffer (no writes)
//! drop(buffer);
//! ```
//!
//! ## Integration Points
//!
//! This buffer should be used by:
//! - `EvmStateDatabase::commit()` for EVM state changes
//! - System contract handlers in `rust-backend/crates/core/src/service/contracts/`
//! - Conformance runner for fixture execution

use anyhow::Result;
use std::collections::{BTreeMap, HashMap};
use tracing::{debug, trace};
use tron_backend_storage::StorageEngine;

use super::db_names;

/// A single write operation (put or delete).
#[derive(Debug, Clone)]
pub enum WriteOp {
    /// Put a key-value pair
    Put(Vec<u8>),
    /// Delete a key
    Delete,
}

/// A touched key record for B-镜像 (B-mirror) support.
/// Includes the database name, key, and whether it was a delete operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchedKey {
    /// Database name (canonical, from db_names module)
    pub db: String,
    /// The key that was touched
    pub key: Vec<u8>,
    /// True if this was a delete operation
    pub is_delete: bool,
}

/// Execution write buffer that accumulates writes and commits atomically.
///
/// All write operations are accumulated in memory during transaction execution.
/// On success, `commit()` writes all operations to storage in batches per database.
/// On failure or validation error, the buffer is simply dropped with no writes.
#[derive(Debug, Default)]
pub struct ExecutionWriteBuffer {
    /// Operations grouped by database name.
    /// Using BTreeMap for deterministic ordering (important for testing).
    /// Inner map: key -> operation (put or delete)
    operations: BTreeMap<String, HashMap<Vec<u8>, WriteOp>>,

    /// Track the order of touched keys for B-镜像 support.
    /// This maintains insertion order for returning touched_keys to Java.
    touched_keys_order: Vec<TouchedKey>,
}

impl ExecutionWriteBuffer {
    /// Create a new empty write buffer.
    pub fn new() -> Self {
        Self {
            operations: BTreeMap::new(),
            touched_keys_order: Vec::new(),
        }
    }

    /// Record a put operation.
    ///
    /// # Arguments
    /// * `db` - Database name (should use constants from `db_names` module)
    /// * `key` - The key to write
    /// * `value` - The value to write
    pub fn put(&mut self, db: &str, key: Vec<u8>, value: Vec<u8>) {
        trace!(
            "Buffer put: db={}, key_len={}, value_len={}",
            db,
            key.len(),
            value.len()
        );

        let db_ops = self.operations.entry(db.to_string()).or_default();

        // Check if this key is already tracked
        let key_exists = db_ops.contains_key(&key);

        db_ops.insert(key.clone(), WriteOp::Put(value));

        // Only add to touched_keys if this is a new key
        if !key_exists {
            self.touched_keys_order.push(TouchedKey {
                db: db.to_string(),
                key,
                is_delete: false,
            });
        } else {
            // Update existing touched key to reflect latest operation type
            if let Some(tk) = self
                .touched_keys_order
                .iter_mut()
                .find(|tk| tk.db == db && tk.key == key)
            {
                tk.is_delete = false;
            }
        }
    }

    /// Record a delete operation.
    ///
    /// # Arguments
    /// * `db` - Database name (should use constants from `db_names` module)
    /// * `key` - The key to delete
    pub fn delete(&mut self, db: &str, key: Vec<u8>) {
        trace!("Buffer delete: db={}, key_len={}", db, key.len());

        let db_ops = self.operations.entry(db.to_string()).or_default();

        // Check if this key is already tracked
        let key_exists = db_ops.contains_key(&key);

        db_ops.insert(key.clone(), WriteOp::Delete);

        // Only add to touched_keys if this is a new key
        if !key_exists {
            self.touched_keys_order.push(TouchedKey {
                db: db.to_string(),
                key,
                is_delete: true,
            });
        } else {
            // Update existing touched key to reflect latest operation type
            if let Some(tk) = self
                .touched_keys_order
                .iter_mut()
                .find(|tk| tk.db == db && tk.key == key)
            {
                tk.is_delete = true;
            }
        }
    }

    /// Get all touched keys in order of first touch.
    ///
    /// This is used for B-镜像 (B-mirror) support where Java needs to know
    /// which keys were modified so it can refresh its local revoking head.
    pub fn touched_keys(&self) -> &[TouchedKey] {
        &self.touched_keys_order
    }

    /// Get the number of databases with pending operations.
    pub fn database_count(&self) -> usize {
        self.operations.len()
    }

    /// Get the total number of operations across all databases.
    pub fn operation_count(&self) -> usize {
        self.operations.values().map(|ops| ops.len()).sum()
    }

    /// Check if the buffer is empty (no pending operations).
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Get operations for a specific database (for testing/debugging).
    pub fn get_operations(&self, db: &str) -> Option<&HashMap<Vec<u8>, WriteOp>> {
        self.operations.get(db)
    }

    /// Commit all buffered operations to storage atomically (per database).
    ///
    /// Each database's operations are written as a single batch, ensuring
    /// atomicity within each database. Cross-database atomicity would require
    /// WAL support (future enhancement for fullnode/reorg support).
    ///
    /// # Arguments
    /// * `engine` - The storage engine to commit to
    ///
    /// # Returns
    /// * `Ok(())` - All operations committed successfully
    /// * `Err(...)` - Commit failed (partial writes may have occurred)
    ///
    /// # Note
    /// After calling `commit()`, the buffer is cleared.
    pub fn commit(&mut self, engine: &StorageEngine) -> Result<()> {
        if self.operations.is_empty() {
            debug!("ExecutionWriteBuffer: nothing to commit");
            return Ok(());
        }

        debug!(
            "ExecutionWriteBuffer: committing {} operations across {} databases",
            self.operation_count(),
            self.database_count()
        );

        // Commit each database's operations as a batch
        for (db_name, ops) in &self.operations {
            if ops.is_empty() {
                continue;
            }

            // Convert to storage engine WriteOperation format
            let write_ops: Vec<tron_backend_storage::WriteOperation> = ops
                .iter()
                .map(|(key, op)| match op {
                    WriteOp::Put(value) => tron_backend_storage::WriteOperation {
                        r#type: 0, // PUT
                        key: key.clone(),
                        value: value.clone(),
                    },
                    WriteOp::Delete => tron_backend_storage::WriteOperation {
                        r#type: 1, // DELETE
                        key: key.clone(),
                        value: Vec::new(),
                    },
                })
                .collect();

            debug!(
                "ExecutionWriteBuffer: batch_write to {} with {} operations",
                db_name,
                write_ops.len()
            );

            engine.batch_write(db_name, &write_ops)?;
        }

        // Clear the buffer after successful commit
        self.clear();

        Ok(())
    }

    /// Clear all buffered operations without committing.
    ///
    /// This is automatically called on `commit()` success, but can be called
    /// explicitly to discard pending operations (e.g., on validation failure).
    pub fn clear(&mut self) {
        self.operations.clear();
        self.touched_keys_order.clear();
    }

    /// Merge another buffer into this one.
    ///
    /// This is useful when combining writes from multiple execution phases
    /// (e.g., EVM execution + post-processing).
    pub fn merge(&mut self, other: ExecutionWriteBuffer) {
        for (db, ops) in other.operations {
            let db_ops = self.operations.entry(db.clone()).or_default();
            for (key, op) in ops {
                // Check if key already exists
                let key_exists = db_ops.contains_key(&key);
                let is_delete = matches!(op, WriteOp::Delete);

                db_ops.insert(key.clone(), op);

                if !key_exists {
                    self.touched_keys_order.push(TouchedKey {
                        db: db.clone(),
                        key,
                        is_delete,
                    });
                } else {
                    // Update existing touched key
                    if let Some(tk) = self
                        .touched_keys_order
                        .iter_mut()
                        .find(|tk| tk.db == db && tk.key == key)
                    {
                        tk.is_delete = is_delete;
                    }
                }
            }
        }
    }
}

/// Builder pattern for creating an ExecutionWriteBuffer with common patterns.
pub struct WriteBufferBuilder {
    buffer: ExecutionWriteBuffer,
}

impl WriteBufferBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            buffer: ExecutionWriteBuffer::new(),
        }
    }

    /// Add an account write.
    pub fn account(mut self, key: Vec<u8>, value: Vec<u8>) -> Self {
        self.buffer.put(db_names::account::ACCOUNT, key, value);
        self
    }

    /// Add a storage row write.
    pub fn storage_row(mut self, key: Vec<u8>, value: Vec<u8>) -> Self {
        self.buffer.put(db_names::storage::STORAGE_ROW, key, value);
        self
    }

    /// Add a code write.
    pub fn code(mut self, key: Vec<u8>, value: Vec<u8>) -> Self {
        self.buffer.put(db_names::contract::CODE, key, value);
        self
    }

    /// Add a properties write.
    pub fn properties(mut self, key: Vec<u8>, value: Vec<u8>) -> Self {
        self.buffer.put(db_names::system::PROPERTIES, key, value);
        self
    }

    /// Add a witness write.
    pub fn witness(mut self, key: Vec<u8>, value: Vec<u8>) -> Self {
        self.buffer.put(db_names::governance::WITNESS, key, value);
        self
    }

    /// Add a votes write.
    pub fn votes(mut self, key: Vec<u8>, value: Vec<u8>) -> Self {
        self.buffer.put(db_names::governance::VOTES, key, value);
        self
    }

    /// Build the buffer.
    pub fn build(self) -> ExecutionWriteBuffer {
        self.buffer
    }
}

impl Default for WriteBufferBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_buffer() {
        let buffer = ExecutionWriteBuffer::new();
        assert!(buffer.is_empty());
        assert_eq!(buffer.operation_count(), 0);
        assert_eq!(buffer.database_count(), 0);
        assert!(buffer.touched_keys().is_empty());
    }

    #[test]
    fn test_put_operation() {
        let mut buffer = ExecutionWriteBuffer::new();
        buffer.put("account", vec![1, 2, 3], vec![4, 5, 6]);

        assert!(!buffer.is_empty());
        assert_eq!(buffer.operation_count(), 1);
        assert_eq!(buffer.database_count(), 1);
        assert_eq!(buffer.touched_keys().len(), 1);

        let tk = &buffer.touched_keys()[0];
        assert_eq!(tk.db, "account");
        assert_eq!(tk.key, vec![1, 2, 3]);
        assert!(!tk.is_delete);
    }

    #[test]
    fn test_delete_operation() {
        let mut buffer = ExecutionWriteBuffer::new();
        buffer.delete("votes", vec![1, 2, 3]);

        assert!(!buffer.is_empty());
        assert_eq!(buffer.operation_count(), 1);
        assert_eq!(buffer.touched_keys().len(), 1);

        let tk = &buffer.touched_keys()[0];
        assert_eq!(tk.db, "votes");
        assert!(!tk.key.is_empty());
        assert!(tk.is_delete);
    }

    #[test]
    fn test_multiple_databases() {
        let mut buffer = ExecutionWriteBuffer::new();
        buffer.put("account", vec![1], vec![2]);
        buffer.put("votes", vec![3], vec![4]);
        buffer.delete("witness", vec![5]);

        assert_eq!(buffer.database_count(), 3);
        assert_eq!(buffer.operation_count(), 3);
        assert_eq!(buffer.touched_keys().len(), 3);
    }

    #[test]
    fn test_overwrite_same_key() {
        let mut buffer = ExecutionWriteBuffer::new();
        buffer.put("account", vec![1, 2, 3], vec![1]);
        buffer.put("account", vec![1, 2, 3], vec![2]); // Overwrite

        // Should still be one operation (overwritten)
        assert_eq!(buffer.operation_count(), 1);
        // Should still be one touched key (same key)
        assert_eq!(buffer.touched_keys().len(), 1);

        // Verify the value is the latest
        let ops = buffer.get_operations("account").unwrap();
        if let WriteOp::Put(value) = &ops[&vec![1, 2, 3]] {
            assert_eq!(value, &vec![2]);
        } else {
            panic!("Expected Put operation");
        }
    }

    #[test]
    fn test_put_then_delete_same_key() {
        let mut buffer = ExecutionWriteBuffer::new();
        buffer.put("account", vec![1, 2, 3], vec![4, 5, 6]);
        buffer.delete("account", vec![1, 2, 3]); // Now delete it

        assert_eq!(buffer.operation_count(), 1);
        assert_eq!(buffer.touched_keys().len(), 1);

        // Verify it's now a delete
        let tk = &buffer.touched_keys()[0];
        assert!(tk.is_delete);

        let ops = buffer.get_operations("account").unwrap();
        assert!(matches!(ops.get(&vec![1, 2, 3]), Some(WriteOp::Delete)));
    }

    #[test]
    fn test_clear() {
        let mut buffer = ExecutionWriteBuffer::new();
        buffer.put("account", vec![1], vec![2]);
        buffer.put("votes", vec![3], vec![4]);

        assert!(!buffer.is_empty());

        buffer.clear();

        assert!(buffer.is_empty());
        assert!(buffer.touched_keys().is_empty());
    }

    #[test]
    fn test_merge() {
        let mut buffer1 = ExecutionWriteBuffer::new();
        buffer1.put("account", vec![1], vec![2]);

        let mut buffer2 = ExecutionWriteBuffer::new();
        buffer2.put("votes", vec![3], vec![4]);
        buffer2.delete("witness", vec![5]);

        buffer1.merge(buffer2);

        assert_eq!(buffer1.database_count(), 3);
        assert_eq!(buffer1.operation_count(), 3);
        assert_eq!(buffer1.touched_keys().len(), 3);
    }

    #[test]
    fn test_builder() {
        let buffer = WriteBufferBuilder::new()
            .account(vec![1], vec![2])
            .storage_row(vec![3], vec![4])
            .code(vec![5], vec![6])
            .build();

        assert_eq!(buffer.database_count(), 3);
        assert_eq!(buffer.operation_count(), 3);
    }
}
