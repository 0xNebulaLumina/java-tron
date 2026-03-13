use anyhow::{anyhow, Result};
use rocksdb::{DB, Options, WriteBatch, IteratorMode, Direction};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use tracing::{info, warn};

#[derive(Clone)]
pub struct StorageEngine {
    databases: Arc<RwLock<HashMap<String, Arc<DB>>>>,
    transactions: Arc<RwLock<HashMap<String, TransactionInfo>>>,
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

struct SnapshotInfo {
    db_name: String,
    // Note: In a real implementation, you'd need to handle snapshot lifetimes more carefully
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
        info!("Auto-initialized database: {} with default configuration", database);

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

        info!("Initialized database: {} with custom configuration", database);
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

    pub fn has(&self, database: &str, key: &[u8]) -> Result<bool> {
        let db = self.get_or_init_db(database)?;
        Ok(db.get(key)?.is_some())
    }

    pub fn batch_write(&self, database: &str, operations: &[WriteOperation]) -> Result<()> {
        let db = self.get_or_init_db(database)?;

        let mut batch = WriteBatch::default();

        for op in operations {
            match op.r#type {
                0 => { // PUT
                    batch.put(&op.key, &op.value);
                }
                1 => { // DELETE
                    batch.delete(&op.key);
                }
                _ => return Err(anyhow!("Unknown operation type: {}", op.r#type)),
            }
        }

        db.write(batch)?;
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

    pub fn get_keys_next(&self, database: &str, start_key: &[u8], limit: i32) -> Result<Vec<Vec<u8>>> {
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

    pub fn get_values_next(&self, database: &str, start_key: &[u8], limit: i32) -> Result<Vec<Vec<u8>>> {
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
            transactions.remove(transaction_id)
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
        transactions.remove(transaction_id)
            .ok_or_else(|| anyhow!("Transaction {} not found", transaction_id))?;
        
        Ok(())
    }

    pub fn create_snapshot(&self, database: &str) -> Result<String> {
        let snapshot_id = Uuid::new_v4().to_string();
        
        let snapshot_info = SnapshotInfo {
            db_name: database.to_string(),
        };
        
        let mut snapshots = self.snapshots.write().unwrap();
        snapshots.insert(snapshot_id.clone(), snapshot_info);
        
        info!("Created snapshot {} for database {}", snapshot_id, database);
        Ok(snapshot_id)
    }

    pub fn delete_snapshot(&self, snapshot_id: &str) -> Result<()> {
        let mut snapshots = self.snapshots.write().unwrap();
        snapshots.remove(snapshot_id)
            .ok_or_else(|| anyhow!("Snapshot {} not found", snapshot_id))?;
        
        info!("Deleted snapshot {}", snapshot_id);
        Ok(())
    }

    pub fn get_from_snapshot(&self, snapshot_id: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let db_name = {
            let snapshots = self.snapshots.read().unwrap();
            snapshots.get(snapshot_id)
                .ok_or_else(|| anyhow!("Snapshot {} not found", snapshot_id))?
                .db_name.clone()
        };

        // For simplicity, we just read from the current database
        // In a real implementation, you'd need to maintain actual snapshots
        self.get(&db_name, key)
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

    pub fn compact_range(&self, database: &str, start_key: Option<&[u8]>, end_key: Option<&[u8]>) -> Result<()> {
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