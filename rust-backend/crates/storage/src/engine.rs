//! Storage engine for the Rust backend.
//!
//! Transaction and snapshot semantics for this engine are **locked** by the
//! close_loop Phase 1 planning notes:
//!
//! - Transactions: `planning/close_loop.storage_transactions.md`
//!   - Scope: per-DB only. Cross-DB transactions are out of scope.
//!   - `begin_transaction` allocates an opaque id + per-tx write buffer.
//!   - `commit_transaction` applies the buffer atomically via `WriteBatch`.
//!   - `rollback_transaction` discards the buffer.
//!   - Writes with an empty `transaction_id` go directly to the base DB.
//!   - Writes with an unknown `transaction_id` MUST return an error (no
//!     silent fallback to direct-write).
//!   - Read-your-writes is optional and currently not implemented for
//!     `get` / `has` / `batch_get`. Iterators never see uncommitted writes.
//!
//! - Snapshots: `planning/close_loop.snapshot.md`
//!   - Real point-in-time semantics are the goal; fake "reads current DB"
//!     behavior is an acceptance blocker and must be replaced with either
//!     a real RocksDB snapshot handle or an explicit unsupported error.
//!
//! Notable anti-goals: cross-DB transactions, transactional iterators,
//! savepoints, MVCC reads, "generic DB product" semantics. Do not relax
//! any of these without updating the planning notes first.

use anyhow::{anyhow, Result};
use rocksdb::{Direction, IteratorMode, Options, WriteBatch, DB};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct StorageEngine {
    databases: Arc<RwLock<HashMap<String, Arc<DB>>>>,
    transactions: Arc<RwLock<HashMap<String, TransactionInfo>>>,
    // The snapshot storage map is intentionally retained as an empty
    // placeholder so that re-introducing real snapshot support in a later
    // phase does not have to restructure `StorageEngine`. Phase 1 returns
    // an explicit unsupported error from every snapshot-related method —
    // see `planning/close_loop.snapshot.md`. Do not start populating this
    // map without first updating that planning note and providing a real
    // RocksDB point-in-time backing for the entries.
    #[allow(dead_code)]
    snapshots: Arc<RwLock<HashMap<String, SnapshotInfo>>>,
    base_path: String,
}

// Simplified transaction info that doesn't hold WriteBatch directly
struct TransactionInfo {
    db_name: String,
    operations: Vec<BatchOp>,
}

#[derive(Clone)]
#[allow(dead_code)] // Used in transaction operations
enum BatchOp {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

#[allow(dead_code)] // Reserved for the future real-snapshot implementation; see snapshots field comment.
struct SnapshotInfo {
    db_name: String,
}

impl StorageEngine {
    pub fn new<P: AsRef<Path>>(base_path: P) -> Result<Self> {
        let base_path = base_path.as_ref().to_string_lossy().to_string();
        std::fs::create_dir_all(&base_path)?;

        Ok(StorageEngine {
            databases: Arc::new(RwLock::new(HashMap::new())),
            transactions: Arc::new(RwLock::new(HashMap::new())),
            snapshots: Arc::new(RwLock::new(HashMap::new())),
            base_path,
        })
    }

    // Optimized function that combines initialization checking with database retrieval
    fn get_or_init_db(&self, database: &str) -> Result<Arc<DB>> {
        // First try to get the database with a read lock
        {
            let databases = self.databases.read().unwrap();
            if let Some(db) = databases.get(database) {
                return Ok(db.clone());
            }
        } // Read lock is released here

        // Database doesn't exist, need to initialize it
        // Use write lock with double-check pattern to avoid race conditions
        let mut databases = self.databases.write().unwrap();

        // Double-check: another thread might have initialized it while we were waiting for write lock
        if let Some(db) = databases.get(database) {
            return Ok(db.clone());
        }

        // Auto-initialize with default configuration
        let db_path = format!("{}/{}", self.base_path, database);

        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_max_open_files(1000); // Default value

        // Set default block cache
        let cache = rocksdb::Cache::new_lru_cache(8 * 1024 * 1024); // 8MB default
        let mut block_opts = rocksdb::BlockBasedOptions::default();
        block_opts.set_block_cache(&cache);
        opts.set_block_based_table_factory(&block_opts);

        let db = DB::open(&opts, &db_path)?;
        let db_arc = Arc::new(db);

        databases.insert(database.to_string(), db_arc.clone());
        info!(
            "Auto-initialized database: {} with default configuration",
            database
        );

        Ok(db_arc)
    }

    pub fn init_db(&self, database: &str, config: &StorageConfig) -> Result<()> {
        // Check if database is already initialized and open
        {
            let databases = self.databases.read().unwrap();
            if let Some(_existing_db) = databases.get(database) {
                warn!("Database {} is already initialized", database);
                return Ok(());
            }
        }

        let db_path = format!("{}/{}", self.base_path, database);

        let mut opts = Options::default();
        opts.create_if_missing(true);

        // Apply configuration from StorageConfig
        if let Some(max_files) = config.engine_options.get("max_open_files") {
            if let Ok(value) = max_files.parse::<i32>() {
                opts.set_max_open_files(value);
            }
        }

        if let Some(cache_size) = config.engine_options.get("block_cache_size") {
            if let Ok(value) = cache_size.parse::<usize>() {
                let cache = rocksdb::Cache::new_lru_cache(value);
                let mut block_opts = rocksdb::BlockBasedOptions::default();
                block_opts.set_block_cache(&cache);
                opts.set_block_based_table_factory(&block_opts);
            }
        }

        if config.enable_statistics {
            opts.enable_statistics();
        }

        let db = DB::open(&opts, &db_path)?;
        let db_arc = Arc::new(db);

        let mut databases = self.databases.write().unwrap();
        databases.insert(database.to_string(), db_arc);

        info!(
            "Initialized database: {} with custom configuration",
            database
        );
        Ok(())
    }

    pub fn close_db(&self, database: &str) -> Result<()> {
        let mut databases = self.databases.write().unwrap();

        if let Some(_db) = databases.remove(database) {
            info!("Closed database: {}", database);
            Ok(())
        } else {
            Err(anyhow!("Database {} not found", database))
        }
    }

    pub fn reset_db(&self, database: &str) -> Result<()> {
        // Close the database first
        self.close_db(database)?;

        // Remove the database directory
        let db_path = format!("{}/{}", self.base_path, database);
        if std::path::Path::new(&db_path).exists() {
            std::fs::remove_dir_all(&db_path)?;
            info!("Reset database: {} (removed directory)", database);
        }

        Ok(())
    }

    pub fn is_alive(&self, database: &str) -> Result<bool> {
        let databases = self.databases.read().unwrap();
        Ok(databases.contains_key(database))
    }

    pub fn size(&self, database: &str) -> Result<i64> {
        let db = self.get_or_init_db(database)?;

        let mut count = 0i64;
        let iter = db.iterator(IteratorMode::Start);
        for _item in iter {
            count += 1;
        }

        Ok(count)
    }

    pub fn is_empty(&self, database: &str) -> Result<bool> {
        let db = self.get_or_init_db(database)?;

        let mut iter = db.iterator(IteratorMode::Start);
        Ok(iter.next().is_none())
    }

    pub fn get(&self, database: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let db = self.get_or_init_db(database)?;
        Ok(db.get(key)?)
    }

    pub fn put(&self, database: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let db = self.get_or_init_db(database)?;
        db.put(key, value)?;
        Ok(())
    }

    pub fn delete(&self, database: &str, key: &[u8]) -> Result<()> {
        let db = self.get_or_init_db(database)?;
        db.delete(key)?;
        Ok(())
    }

    /// Transactional `put`. Buffers the write into the transaction's
    /// per-tx operation list; the actual RocksDB write happens at
    /// `commit_transaction(transaction_id)` time.
    ///
    /// Phase 1 contract (see `planning/close_loop.storage_transactions.md`):
    /// - Per-DB scope: the `database` argument must match the `database`
    ///   the transaction was opened against. Cross-DB writes inside one
    ///   transaction are out of scope and return an error.
    /// - Read-your-writes is NOT provided. The buffered write is invisible
    ///   to `get` / `has` / `batch_get` until `commit_transaction` runs.
    /// - Unknown `transaction_id` returns an explicit error — there is
    ///   no silent fallback to a direct write.
    pub fn put_in_tx(
        &self,
        transaction_id: &str,
        database: &str,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        let mut transactions = self.transactions.write().unwrap();
        let tx_info = transactions.get_mut(transaction_id).ok_or_else(|| {
            anyhow!(
                "transaction {} not found (close_loop Phase 1: unknown \
                 transaction_id is rejected, no silent fallback to direct write)",
                transaction_id
            )
        })?;
        if tx_info.db_name != database {
            return Err(anyhow!(
                "cross-database write in transaction {} is not supported in \
                 close_loop Phase 1 (transaction was opened against `{}`, \
                 but write targets `{}`); see planning/close_loop.storage_transactions.md",
                transaction_id,
                tx_info.db_name,
                database
            ));
        }
        tx_info.operations.push(BatchOp::Put {
            key: key.to_vec(),
            value: value.to_vec(),
        });
        Ok(())
    }

    /// Transactional `delete`. See [`StorageEngine::put_in_tx`] for the
    /// per-DB / no-read-your-writes / unknown-tx-id contract. The buffered
    /// delete is applied atomically with the rest of the transaction at
    /// commit time.
    pub fn delete_in_tx(
        &self,
        transaction_id: &str,
        database: &str,
        key: &[u8],
    ) -> Result<()> {
        let mut transactions = self.transactions.write().unwrap();
        let tx_info = transactions.get_mut(transaction_id).ok_or_else(|| {
            anyhow!(
                "transaction {} not found (close_loop Phase 1: unknown \
                 transaction_id is rejected, no silent fallback to direct delete)",
                transaction_id
            )
        })?;
        if tx_info.db_name != database {
            return Err(anyhow!(
                "cross-database delete in transaction {} is not supported in \
                 close_loop Phase 1 (transaction was opened against `{}`, \
                 but delete targets `{}`); see planning/close_loop.storage_transactions.md",
                transaction_id,
                tx_info.db_name,
                database
            ));
        }
        tx_info.operations.push(BatchOp::Delete { key: key.to_vec() });
        Ok(())
    }

    pub fn has(&self, database: &str, key: &[u8]) -> Result<bool> {
        let db = self.get_or_init_db(database)?;
        Ok(db.get(key)?.is_some())
    }

    pub fn batch_write(&self, database: &str, operations: &[WriteOperation]) -> Result<()> {
        let db = self.get_or_init_db(database)?;

        let mut batch = WriteBatch::default();

        for op in operations {
            match op.r#type {
                0 => {
                    // PUT
                    batch.put(&op.key, &op.value);
                }
                1 => {
                    // DELETE
                    batch.delete(&op.key);
                }
                _ => return Err(anyhow!("Unknown operation type: {}", op.r#type)),
            }
        }

        db.write(batch)?;
        Ok(())
    }

    /// Transactional `batch_write`. See [`StorageEngine::put_in_tx`] for
    /// the contract. Each operation in the batch is appended to the
    /// transaction's buffer; the entire batch is applied atomically with
    /// the rest of the transaction at commit time. Unknown operation
    /// type codes are rejected before any operation is buffered, so a
    /// malformed batch never partially-mutates the buffer.
    pub fn batch_write_in_tx(
        &self,
        transaction_id: &str,
        database: &str,
        operations: &[WriteOperation],
    ) -> Result<()> {
        // Validate every op before touching the buffer so we never leave
        // a half-buffered state on a malformed input.
        for op in operations {
            if op.r#type != 0 && op.r#type != 1 {
                return Err(anyhow!("Unknown operation type: {}", op.r#type));
            }
        }

        let mut transactions = self.transactions.write().unwrap();
        let tx_info = transactions.get_mut(transaction_id).ok_or_else(|| {
            anyhow!(
                "transaction {} not found (close_loop Phase 1: unknown \
                 transaction_id is rejected, no silent fallback to direct batch_write)",
                transaction_id
            )
        })?;
        if tx_info.db_name != database {
            return Err(anyhow!(
                "cross-database batch_write in transaction {} is not supported in \
                 close_loop Phase 1 (transaction was opened against `{}`, \
                 but batch_write targets `{}`); see planning/close_loop.storage_transactions.md",
                transaction_id,
                tx_info.db_name,
                database
            ));
        }
        for op in operations {
            match op.r#type {
                0 => tx_info.operations.push(BatchOp::Put {
                    key: op.key.clone(),
                    value: op.value.clone(),
                }),
                1 => tx_info.operations.push(BatchOp::Delete {
                    key: op.key.clone(),
                }),
                // Unreachable due to pre-validation above.
                other => return Err(anyhow!("Unknown operation type: {}", other)),
            }
        }
        Ok(())
    }

    /// Batch get operation using RocksDB's native multi_get for improved performance.
    ///
    /// This implementation uses `multi_get` which batches all key lookups into a single
    /// RocksDB operation, reducing per-key overhead compared to individual `get()` calls.
    ///
    /// # Performance
    /// - Before: O(#keys) individual RocksDB get() calls
    /// - After: O(1) batched multi_get() call (internally optimized by RocksDB)
    ///
    /// # Response semantics
    /// - Results are returned in the same order as input keys
    /// - Each key has a corresponding (key, value, found) tuple
    /// - found=true with value for existing keys
    /// - found=false with empty value for missing keys
    pub fn batch_get(&self, database: &str, keys: &[Vec<u8>]) -> Result<Vec<KeyValue>> {
        let db = self.get_or_init_db(database)?;

        if keys.is_empty() {
            return Ok(Vec::new());
        }

        // Use RocksDB's multi_get for batched lookup (more efficient than per-key get)
        // multi_get returns Vec<Result<Option<Vec<u8>>, Error>> in input key order
        let key_refs: Vec<&[u8]> = keys.iter().map(|k| k.as_slice()).collect();
        let multi_results = db.multi_get(&key_refs);

        // Build response in input order, preserving key identity
        let mut results = Vec::with_capacity(keys.len());

        for (key, result) in keys.iter().zip(multi_results.into_iter()) {
            match result {
                Ok(Some(value)) => {
                    results.push(KeyValue {
                        key: key.clone(),
                        value,
                        found: true,
                    });
                }
                Ok(None) => {
                    results.push(KeyValue {
                        key: key.clone(),
                        value: Vec::new(),
                        found: false,
                    });
                }
                Err(e) => {
                    // Log error but continue processing other keys
                    // This maintains partial success semantics
                    warn!("multi_get error for key in db={}: {}", database, e);
                    results.push(KeyValue {
                        key: key.clone(),
                        value: Vec::new(),
                        found: false,
                    });
                }
            }
        }

        Ok(results)
    }

    pub fn get_keys_next(
        &self,
        database: &str,
        start_key: &[u8],
        limit: i32,
    ) -> Result<Vec<Vec<u8>>> {
        let db = self.get_or_init_db(database)?;

        let mut keys = Vec::new();
        let iter = db.iterator(IteratorMode::From(start_key, Direction::Forward));

        for (i, item) in iter.enumerate() {
            if i >= limit as usize {
                break;
            }

            let (key, _) = item?;
            keys.push(key.to_vec());
        }

        Ok(keys)
    }

    pub fn get_values_next(
        &self,
        database: &str,
        start_key: &[u8],
        limit: i32,
    ) -> Result<Vec<Vec<u8>>> {
        let db = self.get_or_init_db(database)?;

        let mut values = Vec::new();
        let iter = db.iterator(IteratorMode::From(start_key, Direction::Forward));

        for (i, item) in iter.enumerate() {
            if i >= limit as usize {
                break;
            }

            let (_, value) = item?;
            values.push(value.to_vec());
        }

        Ok(values)
    }

    pub fn get_next(&self, database: &str, start_key: &[u8], limit: i32) -> Result<Vec<KeyValue>> {
        let db = self.get_or_init_db(database)?;

        let mut pairs = Vec::new();
        let iter = db.iterator(IteratorMode::From(start_key, Direction::Forward));

        for (i, item) in iter.enumerate() {
            if i >= limit as usize {
                break;
            }

            let (key, value) = item?;
            pairs.push(KeyValue {
                key: key.to_vec(),
                value: value.to_vec(),
                found: true,
            });
        }

        Ok(pairs)
    }

    pub fn prefix_query(&self, database: &str, prefix: &[u8]) -> Result<Vec<KeyValue>> {
        let db = self.get_or_init_db(database)?;

        let mut pairs = Vec::new();
        let iter = db.iterator(IteratorMode::From(prefix, Direction::Forward));

        for item in iter {
            let (key, value) = item?;

            // Check if key still has the prefix
            if !key.starts_with(prefix) {
                break;
            }

            pairs.push(KeyValue {
                key: key.to_vec(),
                value: value.to_vec(),
                found: true,
            });
        }

        Ok(pairs)
    }

    pub fn begin_transaction(&self, database: &str) -> Result<String> {
        let transaction_id = Uuid::new_v4().to_string();

        let transaction_info = TransactionInfo {
            db_name: database.to_string(),
            operations: Vec::new(),
        };

        let mut transactions = self.transactions.write().unwrap();
        transactions.insert(transaction_id.clone(), transaction_info);

        Ok(transaction_id)
    }

    pub fn commit_transaction(&self, transaction_id: &str) -> Result<()> {
        let transaction_info = {
            let mut transactions = self.transactions.write().unwrap();
            transactions
                .remove(transaction_id)
                .ok_or_else(|| anyhow!("Transaction {} not found", transaction_id))?
        };

        let db = self.get_or_init_db(&transaction_info.db_name)?;
        let mut batch = WriteBatch::default();

        for op in transaction_info.operations {
            match op {
                BatchOp::Put { key, value } => {
                    batch.put(&key, &value);
                }
                BatchOp::Delete { key } => {
                    batch.delete(&key);
                }
            }
        }

        db.write(batch)?;
        Ok(())
    }

    pub fn rollback_transaction(&self, transaction_id: &str) -> Result<()> {
        let mut transactions = self.transactions.write().unwrap();
        transactions
            .remove(transaction_id)
            .ok_or_else(|| anyhow!("Transaction {} not found", transaction_id))?;

        Ok(())
    }

    /// Phase 1: snapshot creation is explicitly unsupported.
    ///
    /// See `planning/close_loop.snapshot.md`. The previous implementation
    /// allocated a UUID, recorded the database name, and let
    /// `get_from_snapshot` silently read the live DB — which gave callers
    /// fake point-in-time semantics. That has been replaced with an
    /// explicit error so that callers cannot accidentally rely on the
    /// missing isolation. Do NOT restore the fake-success behavior; if a
    /// real snapshot implementation is needed, update the planning note
    /// first and back the snapshot with a real RocksDB snapshot handle.
    pub fn create_snapshot(&self, _database: &str) -> Result<String> {
        Err(anyhow!(
            "storage snapshot is not supported in close_loop Phase 1 \
             (see planning/close_loop.snapshot.md). The previous behavior \
             returned a placeholder snapshot id and silently read from the \
             live database, which has been removed. Use direct reads, or \
             open a follow-up to add real point-in-time snapshot support."
        ))
    }

    /// Phase 1: snapshot deletion is explicitly unsupported.
    ///
    /// See `create_snapshot` above. Always returns an error so that an
    /// accidental "delete a snapshot we never really created" call does
    /// not look like success.
    pub fn delete_snapshot(&self, _snapshot_id: &str) -> Result<()> {
        Err(anyhow!(
            "storage snapshot is not supported in close_loop Phase 1 \
             (see planning/close_loop.snapshot.md). delete_snapshot is a \
             no-op error rather than a fake success."
        ))
    }

    /// Phase 1: snapshot reads are explicitly unsupported.
    ///
    /// Previously this returned the live-DB value, masquerading as a
    /// point-in-time read. That has been removed. Callers MUST NOT
    /// rely on snapshot isolation in Phase 1 — see
    /// `planning/close_loop.snapshot.md`.
    pub fn get_from_snapshot(&self, _snapshot_id: &str, _key: &[u8]) -> Result<Option<Vec<u8>>> {
        Err(anyhow!(
            "storage snapshot is not supported in close_loop Phase 1 \
             (see planning/close_loop.snapshot.md). get_from_snapshot \
             previously fell through to a live-DB read, which has been \
             removed to prevent silent isolation bugs."
        ))
    }

    pub fn list_databases(&self) -> Result<Vec<String>> {
        let databases = self.databases.read().unwrap();
        Ok(databases.keys().cloned().collect())
    }

    pub fn get_stats(&self, database: &str) -> Result<StorageStats> {
        let db = self.get_or_init_db(database)?;

        // Get basic stats
        let total_keys = self.size(database)?;

        // Get RocksDB specific stats
        let mut engine_stats = HashMap::new();

        if let Ok(Some(stats)) = db.property_value("rocksdb.stats") {
            engine_stats.insert("rocksdb.stats".to_string(), stats);
        }

        if let Ok(Some(mem_usage)) = db.property_value("rocksdb.estimate-table-readers-mem") {
            engine_stats.insert("rocksdb.table_readers_mem".to_string(), mem_usage);
        }

        Ok(StorageStats {
            total_keys,
            total_size: 0, // Would need to calculate actual size
            engine_stats,
            last_modified: chrono::Utc::now().timestamp(),
        })
    }

    pub fn compact_range(
        &self,
        database: &str,
        start_key: Option<&[u8]>,
        end_key: Option<&[u8]>,
    ) -> Result<()> {
        let db = self.get_or_init_db(database)?;
        db.compact_range(start_key, end_key);
        Ok(())
    }

    pub fn get_property(&self, database: &str, property: &str) -> Result<Option<String>> {
        let db = self.get_or_init_db(database)?;
        Ok(db.property_value(property)?)
    }
}

// Define the types that match our protobuf definitions
#[derive(Clone)]
pub struct WriteOperation {
    pub r#type: i32,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Clone)]
pub struct KeyValue {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub found: bool,
}

#[derive(Clone)]
pub struct StorageConfig {
    pub engine: String,
    pub engine_options: HashMap<String, String>,
    pub enable_statistics: bool,
    pub max_open_files: i32,
    pub block_cache_size: i64,
}

#[derive(Clone)]
pub struct StorageStats {
    pub total_keys: i64,
    pub total_size: i64,
    pub engine_stats: HashMap<String, String>,
    pub last_modified: i64,
}

// =============================================================================
// Tests (close_loop Phase 1 — Section 3.4)
// =============================================================================
//
// These tests exercise the storage engine directly. They cover:
//   - basic CRUD via direct (non-transactional) put/get/delete/has
//   - batch_write happy path and unknown-op-type rejection
//   - transactional commit and rollback semantics
//   - read-isolation: writes inside a transaction are NOT visible to
//     direct `get` until commit (matches the no-read-your-writes contract
//     in close_loop.storage_transactions.md)
//   - rollback discards buffered writes
//   - unknown transaction_id is rejected with an explicit error
//   - cross-database writes inside one transaction are rejected
//   - concurrent transaction ids are isolated
//   - snapshot APIs return explicit unsupported errors
//
// Each test builds its own `StorageEngine` rooted in a fresh `tempfile::TempDir`
// so they do not collide on disk and can be run with `--test-threads=1` or
// the default parallel runner. RocksDB instances are dropped (and the temp
// directory is cleaned up) when the test scope exits.
//
// Phase 1 contracts referenced here:
//   planning/close_loop.storage_transactions.md
//   planning/close_loop.snapshot.md
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Build a fresh engine rooted in a temporary directory. The TempDir is
    /// returned so the caller can keep it alive for the duration of the test;
    /// dropping it removes the on-disk state.
    fn fresh_engine() -> (StorageEngine, TempDir) {
        let dir = TempDir::new().expect("temp dir");
        let engine = StorageEngine::new(dir.path()).expect("engine new");
        (engine, dir)
    }

    fn put_op(key: &[u8], value: &[u8]) -> WriteOperation {
        WriteOperation {
            r#type: 0,
            key: key.to_vec(),
            value: value.to_vec(),
        }
    }

    fn delete_op(key: &[u8]) -> WriteOperation {
        WriteOperation {
            r#type: 1,
            key: key.to_vec(),
            value: Vec::new(),
        }
    }

    // ---- Direct (non-transactional) CRUD ------------------------------------

    #[test]
    fn direct_put_and_get_round_trips() {
        let (engine, _dir) = fresh_engine();
        engine.put("db", b"k", b"v").unwrap();
        assert_eq!(engine.get("db", b"k").unwrap().as_deref(), Some(&b"v"[..]));
        assert!(engine.has("db", b"k").unwrap());
    }

    #[test]
    fn direct_delete_removes_key() {
        let (engine, _dir) = fresh_engine();
        engine.put("db", b"k", b"v").unwrap();
        engine.delete("db", b"k").unwrap();
        assert!(engine.get("db", b"k").unwrap().is_none());
        assert!(!engine.has("db", b"k").unwrap());
    }

    #[test]
    fn get_missing_key_returns_none() {
        let (engine, _dir) = fresh_engine();
        assert!(engine.get("db", b"missing").unwrap().is_none());
        assert!(!engine.has("db", b"missing").unwrap());
    }

    // ---- Batch writes -------------------------------------------------------

    #[test]
    fn batch_write_applies_puts_and_deletes_atomically() {
        let (engine, _dir) = fresh_engine();
        // Seed one key so we can verify batch delete works alongside puts.
        engine.put("db", b"old", b"x").unwrap();

        let ops = vec![
            put_op(b"a", b"1"),
            put_op(b"b", b"2"),
            delete_op(b"old"),
        ];
        engine.batch_write("db", &ops).unwrap();

        assert_eq!(engine.get("db", b"a").unwrap().as_deref(), Some(&b"1"[..]));
        assert_eq!(engine.get("db", b"b").unwrap().as_deref(), Some(&b"2"[..]));
        assert!(engine.get("db", b"old").unwrap().is_none());
    }

    #[test]
    fn batch_write_rejects_unknown_op_type() {
        let (engine, _dir) = fresh_engine();
        let ops = vec![WriteOperation {
            r#type: 99,
            key: b"k".to_vec(),
            value: b"v".to_vec(),
        }];
        let err = engine.batch_write("db", &ops).unwrap_err();
        assert!(err.to_string().contains("Unknown operation type"));
    }

    // ---- Transaction commit -------------------------------------------------

    #[test]
    fn transactional_commit_applies_buffered_writes() {
        let (engine, _dir) = fresh_engine();
        let tx = engine.begin_transaction("db").unwrap();
        engine.put_in_tx(&tx, "db", b"k1", b"v1").unwrap();
        engine.put_in_tx(&tx, "db", b"k2", b"v2").unwrap();

        // Read isolation: writes are NOT visible via direct `get` before commit.
        // (see planning/close_loop.storage_transactions.md — no read-your-writes)
        assert!(engine.get("db", b"k1").unwrap().is_none());
        assert!(engine.get("db", b"k2").unwrap().is_none());

        engine.commit_transaction(&tx).unwrap();

        // After commit the writes are visible.
        assert_eq!(engine.get("db", b"k1").unwrap().as_deref(), Some(&b"v1"[..]));
        assert_eq!(engine.get("db", b"k2").unwrap().as_deref(), Some(&b"v2"[..]));
    }

    #[test]
    fn transactional_commit_applies_deletes() {
        let (engine, _dir) = fresh_engine();
        engine.put("db", b"old", b"x").unwrap();

        let tx = engine.begin_transaction("db").unwrap();
        engine.delete_in_tx(&tx, "db", b"old").unwrap();
        // Still visible via direct read until commit.
        assert!(engine.has("db", b"old").unwrap());
        engine.commit_transaction(&tx).unwrap();
        assert!(!engine.has("db", b"old").unwrap());
    }

    #[test]
    fn batch_write_in_tx_rejects_mixed_batch_before_buffering_anything() {
        // A batch containing one valid op and one malformed op must leave
        // the buffer untouched — a later commit should persist nothing
        // from this batch. This guards the "pre-validate before touching
        // the buffer" invariant in `batch_write_in_tx`.
        let (engine, _dir) = fresh_engine();
        let tx = engine.begin_transaction("db").unwrap();

        let ops = vec![
            put_op(b"ok_key", b"ok_val"),
            WriteOperation {
                r#type: 99, // malformed
                key: b"bad_key".to_vec(),
                value: b"bad_val".to_vec(),
            },
        ];
        let err = engine.batch_write_in_tx(&tx, "db", &ops).unwrap_err();
        assert!(
            err.to_string().contains("Unknown operation type"),
            "expected pre-validation error, got: {err}"
        );

        // Commit the transaction: if the pre-validation had partially
        // buffered the valid op, commit would persist it here. It must
        // not — neither key should be present after commit.
        engine.commit_transaction(&tx).unwrap();
        assert!(engine.get("db", b"ok_key").unwrap().is_none());
        assert!(engine.get("db", b"bad_key").unwrap().is_none());
    }

    #[test]
    fn transactional_batch_write_commit() {
        let (engine, _dir) = fresh_engine();
        engine.put("db", b"old", b"x").unwrap();

        let tx = engine.begin_transaction("db").unwrap();
        let ops = vec![
            put_op(b"a", b"1"),
            put_op(b"b", b"2"),
            delete_op(b"old"),
        ];
        engine.batch_write_in_tx(&tx, "db", &ops).unwrap();
        engine.commit_transaction(&tx).unwrap();

        assert_eq!(engine.get("db", b"a").unwrap().as_deref(), Some(&b"1"[..]));
        assert_eq!(engine.get("db", b"b").unwrap().as_deref(), Some(&b"2"[..]));
        assert!(engine.get("db", b"old").unwrap().is_none());
    }

    // ---- Transaction rollback ----------------------------------------------

    #[test]
    fn transactional_rollback_discards_buffered_writes() {
        let (engine, _dir) = fresh_engine();
        let tx = engine.begin_transaction("db").unwrap();
        engine.put_in_tx(&tx, "db", b"k1", b"v1").unwrap();
        engine.put_in_tx(&tx, "db", b"k2", b"v2").unwrap();

        engine.rollback_transaction(&tx).unwrap();

        assert!(engine.get("db", b"k1").unwrap().is_none());
        assert!(engine.get("db", b"k2").unwrap().is_none());
    }

    #[test]
    fn rollback_then_commit_same_id_fails() {
        // After rollback, the transaction id is gone — a follow-up commit
        // must report the unknown id rather than silently succeeding.
        let (engine, _dir) = fresh_engine();
        let tx = engine.begin_transaction("db").unwrap();
        engine.rollback_transaction(&tx).unwrap();
        let err = engine.commit_transaction(&tx).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    // ---- Unknown transaction id --------------------------------------------

    #[test]
    fn put_in_tx_unknown_id_is_rejected() {
        let (engine, _dir) = fresh_engine();
        let err = engine
            .put_in_tx("ghost-id", "db", b"k", b"v")
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
        // And the direct path is untouched.
        assert!(engine.get("db", b"k").unwrap().is_none());
    }

    #[test]
    fn delete_in_tx_unknown_id_is_rejected() {
        let (engine, _dir) = fresh_engine();
        let err = engine.delete_in_tx("ghost-id", "db", b"k").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn batch_write_in_tx_unknown_id_is_rejected() {
        let (engine, _dir) = fresh_engine();
        let ops = vec![put_op(b"k", b"v")];
        let err = engine
            .batch_write_in_tx("ghost-id", "db", &ops)
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn commit_unknown_id_is_rejected() {
        let (engine, _dir) = fresh_engine();
        let err = engine.commit_transaction("ghost-id").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn rollback_unknown_id_is_rejected() {
        let (engine, _dir) = fresh_engine();
        let err = engine.rollback_transaction("ghost-id").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    // ---- Cross-database transactional writes are out of scope --------------

    #[test]
    fn put_in_tx_rejects_cross_db_write() {
        let (engine, _dir) = fresh_engine();
        let tx = engine.begin_transaction("db_a").unwrap();
        let err = engine.put_in_tx(&tx, "db_b", b"k", b"v").unwrap_err();
        assert!(err.to_string().contains("cross-database"));
    }

    #[test]
    fn batch_write_in_tx_rejects_cross_db_write() {
        let (engine, _dir) = fresh_engine();
        let tx = engine.begin_transaction("db_a").unwrap();
        let ops = vec![put_op(b"k", b"v")];
        let err = engine
            .batch_write_in_tx(&tx, "db_b", &ops)
            .unwrap_err();
        assert!(err.to_string().contains("cross-database"));
    }

    // ---- Concurrent transaction ids are isolated ---------------------------

    #[test]
    fn concurrent_transactions_are_isolated() {
        let (engine, _dir) = fresh_engine();
        let tx_a = engine.begin_transaction("db").unwrap();
        let tx_b = engine.begin_transaction("db").unwrap();
        assert_ne!(tx_a, tx_b, "transaction ids must be unique");

        engine.put_in_tx(&tx_a, "db", b"key", b"from_a").unwrap();
        engine.put_in_tx(&tx_b, "db", b"key", b"from_b").unwrap();

        // Neither write is visible via direct read yet.
        assert!(engine.get("db", b"key").unwrap().is_none());

        // Commit B first, then commit A. The "last writer wins" outcome is
        // whoever commits last — A in this case. This documents that the
        // engine does NOT implement optimistic concurrency control; the
        // application is responsible for serializing conflicting writes.
        engine.commit_transaction(&tx_b).unwrap();
        assert_eq!(
            engine.get("db", b"key").unwrap().as_deref(),
            Some(&b"from_b"[..])
        );
        engine.commit_transaction(&tx_a).unwrap();
        assert_eq!(
            engine.get("db", b"key").unwrap().as_deref(),
            Some(&b"from_a"[..])
        );
    }

    #[test]
    fn rollback_does_not_affect_other_concurrent_transaction() {
        let (engine, _dir) = fresh_engine();
        let tx_a = engine.begin_transaction("db").unwrap();
        let tx_b = engine.begin_transaction("db").unwrap();

        engine.put_in_tx(&tx_a, "db", b"a_key", b"a_val").unwrap();
        engine.put_in_tx(&tx_b, "db", b"b_key", b"b_val").unwrap();

        engine.rollback_transaction(&tx_a).unwrap();
        engine.commit_transaction(&tx_b).unwrap();

        assert!(engine.get("db", b"a_key").unwrap().is_none());
        assert_eq!(
            engine.get("db", b"b_key").unwrap().as_deref(),
            Some(&b"b_val"[..])
        );
    }

    // ---- Snapshot APIs are explicitly unsupported --------------------------

    #[test]
    fn create_snapshot_returns_explicit_error() {
        let (engine, _dir) = fresh_engine();
        let err = engine.create_snapshot("db").unwrap_err();
        assert!(
            err.to_string().contains("snapshot is not supported"),
            "expected explicit unsupported error, got: {err}"
        );
    }

    #[test]
    fn delete_snapshot_returns_explicit_error() {
        let (engine, _dir) = fresh_engine();
        let err = engine.delete_snapshot("any-id").unwrap_err();
        assert!(err.to_string().contains("snapshot is not supported"));
    }

    #[test]
    fn get_from_snapshot_returns_explicit_error() {
        let (engine, _dir) = fresh_engine();
        let err = engine.get_from_snapshot("any-id", b"k").unwrap_err();
        assert!(err.to_string().contains("snapshot is not supported"));
    }
}
