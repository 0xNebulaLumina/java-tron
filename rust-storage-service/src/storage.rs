use anyhow::{anyhow, Result};
use rocksdb::{DB, Options, WriteBatch, IteratorMode, Direction};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use tracing::{info, warn};

// Re-export generated protobuf code
pub mod storage_proto {
    tonic::include_proto!("storage");
}
pub use storage_proto::*;
pub use storage_proto::storage_service_server;

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
    fn get_or_init_db(&self, db_name: &str) -> Result<Arc<DB>> {
        // First try to get the database with a read lock
        {
            let databases = self.databases.read().unwrap();
            if let Some(db) = databases.get(db_name) {
                return Ok(db.clone());
            }
        } // Read lock is released here

        // Database doesn't exist, need to initialize it
        // Use write lock with double-check pattern to avoid race conditions
        let mut databases = self.databases.write().unwrap();

        // Double-check: another thread might have initialized it while we were waiting for write lock
        if let Some(db) = databases.get(db_name) {
            return Ok(db.clone());
        }

        // Auto-initialize with default configuration
        let db_path = format!("{}/{}", self.base_path, db_name);

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

        databases.insert(db_name.to_string(), db_arc.clone());
        info!("Auto-initialized database: {} with default configuration", db_name);

        Ok(db_arc)
    }

    pub fn init_db(&self, db_name: &str, config: &StorageConfig) -> Result<()> {
        // Check if database is already initialized and open
        {
            let databases = self.databases.read().unwrap();
            if let Some(existing_db) = databases.get(db_name) {
                // Database already exists and is open - check if it's still valid
                if let Ok(_) = existing_db.get(b"__health_check__") {
                    info!("Database {} already initialized and healthy, reusing existing instance", db_name);
                    return Ok(());
                } else {
                    // Database exists but may be corrupted or closed, remove it
                    warn!("Database {} exists but appears unhealthy, will reinitialize", db_name);
                }
            }
        }

        let db_path = format!("{}/{}", self.base_path, db_name);
        
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_max_open_files(config.max_open_files);
        
        if config.block_cache_size > 0 {
            let cache = rocksdb::Cache::new_lru_cache(config.block_cache_size as usize);
            let mut block_opts = rocksdb::BlockBasedOptions::default();
            block_opts.set_block_cache(&cache);
            opts.set_block_based_table_factory(&block_opts);
        }

        // Apply engine-specific options
        for (key, value) in &config.engine_options {
            match key.as_str() {
                "write_buffer_size" => {
                    if let Ok(size) = value.parse::<usize>() {
                        opts.set_write_buffer_size(size);
                    }
                }
                "max_write_buffer_number" => {
                    if let Ok(num) = value.parse::<i32>() {
                        opts.set_max_write_buffer_number(num);
                    }
                }
                "compression_type" => {
                    match value.as_str() {
                        "none" => opts.set_compression_type(rocksdb::DBCompressionType::None),
                        "snappy" => opts.set_compression_type(rocksdb::DBCompressionType::Snappy),
                        "lz4" => opts.set_compression_type(rocksdb::DBCompressionType::Lz4),
                        _ => {}
                    }
                }
                _ => {
                    warn!("Unknown engine option: {}", key);
                }
            }
        }

        // Try to open the database
        match DB::open(&opts, &db_path) {
            Ok(db) => {
                let db_arc = Arc::new(db);
                
                // Update the database in our map
                self.databases.write().unwrap().insert(db_name.to_string(), db_arc);
                info!("Successfully initialized database: {} with custom configuration", db_name);
                Ok(())
            }
            Err(e) => {
                // Check if this is a lock error
                let error_msg = e.to_string();
                if error_msg.contains("lock") || error_msg.contains("LOCK") {
                    // This database is already open, which is actually fine in our case
                    // since we're the same process. Let's try to reuse the existing instance.
                    warn!("Database {} appears to be already open (lock error), checking for existing instance", db_name);
                    
                    // Double-check our internal map
                    let databases = self.databases.read().unwrap();
                    if databases.contains_key(db_name) {
                        info!("Database {} already exists in our map, initialization considered successful", db_name);
                        return Ok(());
                    } else {
                        // This shouldn't happen, but let's handle it gracefully
                        return Err(anyhow!("Database {} has lock conflict but not in our map: {}", db_name, e));
                    }
                } else {
                    // Some other error occurred
                    return Err(anyhow!("Failed to initialize database {}: {}", db_name, e));
                }
            }
        }
    }

    pub fn get(&self, db_name: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let db = self.get_or_init_db(db_name)?;

        match db.get(key)? {
            Some(value) => Ok(Some(value)),
            None => Ok(None),
        }
    }

    pub fn put(&self, db_name: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let db = self.get_or_init_db(db_name)?;

        db.put(key, value)?;
        Ok(())
    }

    pub fn delete(&self, db_name: &str, key: &[u8]) -> Result<()> {
        let db = self.get_or_init_db(db_name)?;

        db.delete(key)?;
        Ok(())
    }

    pub fn has(&self, db_name: &str, key: &[u8]) -> Result<bool> {
        let db = self.get_or_init_db(db_name)?;

        Ok(db.get(key)?.is_some())
    }

    pub fn batch_write(&self, db_name: &str, operations: &[BatchOperation]) -> Result<()> {
        let db = self.get_or_init_db(db_name)?;

        let mut batch = WriteBatch::default();

        for op in operations {
            match batch_operation::Type::try_from(op.r#type).unwrap_or(batch_operation::Type::Put) {
                batch_operation::Type::Put => {
                    batch.put(&op.key, &op.value);
                }
                batch_operation::Type::Delete => {
                    batch.delete(&op.key);
                }
            }
        }

        db.write(batch)?;
        Ok(())
    }

    pub fn batch_get(&self, db_name: &str, keys: &[Vec<u8>]) -> Result<Vec<KeyValue>> {
        let db = self.get_or_init_db(db_name)?;

        let mut results = Vec::new();

        for key in keys {
            match db.get(key)? {
                Some(value) => {
                    results.push(KeyValue {
                        key: key.clone(),
                        value,
                        found: true,
                    });
                }
                None => {
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

    pub fn get_keys_next(&self, db_name: &str, start_key: &[u8], limit: i32) -> Result<Vec<Vec<u8>>> {
        let db = self.get_or_init_db(db_name)?;

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

    pub fn get_values_next(&self, db_name: &str, start_key: &[u8], limit: i32) -> Result<Vec<Vec<u8>>> {
        let db = self.get_or_init_db(db_name)?;

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

    pub fn get_next(&self, db_name: &str, start_key: &[u8], limit: i32) -> Result<Vec<KeyValue>> {
        let db = self.get_or_init_db(db_name)?;

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

    pub fn prefix_query(&self, db_name: &str, prefix: &[u8]) -> Result<Vec<KeyValue>> {
        let db = self.get_or_init_db(db_name)?;

        let mut pairs = Vec::new();
        let iter = db.iterator(IteratorMode::From(prefix, Direction::Forward));

        for item in iter {
            let (key, value) = item?;
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

    pub fn close_db(&self, db_name: &str) -> Result<()> {
        let mut databases = self.databases.write().unwrap();
        databases.remove(db_name);
        info!("Closed database: {}", db_name);
        Ok(())
    }

    pub fn reset_db(&self, db_name: &str) -> Result<()> {
        self.close_db(db_name)?;
        
        let db_path = format!("{}/{}", self.base_path, db_name);
        if Path::new(&db_path).exists() {
            std::fs::remove_dir_all(&db_path)?;
        }
        
        info!("Reset database: {}", db_name);
        Ok(())
    }

    pub fn is_alive(&self, db_name: &str) -> bool {
        self.databases.read().unwrap().contains_key(db_name)
    }

    pub fn size(&self, db_name: &str) -> Result<i64> {
        let db = self.get_or_init_db(db_name)?;

        // Approximate count using RocksDB property
        let count_str = db.property_value("rocksdb.estimate-num-keys")?
            .unwrap_or_else(|| "0".to_string());

        Ok(count_str.parse::<i64>().unwrap_or(0))
    }

    pub fn is_empty(&self, db_name: &str) -> Result<bool> {
        Ok(self.size(db_name)? == 0)
    }

    pub fn begin_transaction(&self, db_name: &str) -> Result<String> {
        // Ensure database is initialized before creating transaction
        let _db = self.get_or_init_db(db_name)?;

        let transaction_id = Uuid::new_v4().to_string();

        let transaction = TransactionInfo {
            db_name: db_name.to_string(),
            operations: Vec::new(),
        };

        self.transactions.write().unwrap().insert(transaction_id.clone(), transaction);
        Ok(transaction_id)
    }

    pub fn commit_transaction(&self, transaction_id: &str) -> Result<()> {
        let transaction = {
            let mut transactions = self.transactions.write().unwrap();
            transactions.remove(transaction_id)
                .ok_or_else(|| anyhow!("Transaction not found: {}", transaction_id))?
        };
        
        let databases = self.databases.read().unwrap();
        let db = databases.get(&transaction.db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", transaction.db_name))?;
        
        // Apply all operations in the transaction
        let mut batch = WriteBatch::default();
        for op in transaction.operations {
            match op {
                BatchOp::Put { key, value } => batch.put(&key, &value),
                BatchOp::Delete { key } => batch.delete(&key),
            }
        }
        
        db.write(batch)?;
        Ok(())
    }

    pub fn rollback_transaction(&self, transaction_id: &str) -> Result<()> {
        let mut transactions = self.transactions.write().unwrap();
        transactions.remove(transaction_id)
            .ok_or_else(|| anyhow!("Transaction not found: {}", transaction_id))?;
        Ok(())
    }

    pub fn create_snapshot(&self, db_name: &str) -> Result<String> {
        // Ensure database is initialized before creating snapshot
        let _db = self.get_or_init_db(db_name)?;

        let snapshot_id = Uuid::new_v4().to_string();

        let snapshot_info = SnapshotInfo {
            db_name: db_name.to_string(),
        };

        self.snapshots.write().unwrap().insert(snapshot_id.clone(), snapshot_info);
        Ok(snapshot_id)
    }

    pub fn delete_snapshot(&self, snapshot_id: &str) -> Result<()> {
        let mut snapshots = self.snapshots.write().unwrap();
        snapshots.remove(snapshot_id)
            .ok_or_else(|| anyhow!("Snapshot not found: {}", snapshot_id))?;
        Ok(())
    }

    pub fn get_from_snapshot(&self, snapshot_id: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let snapshots = self.snapshots.read().unwrap();
        let snapshot_info = snapshots.get(snapshot_id)
            .ok_or_else(|| anyhow!("Snapshot not found: {}", snapshot_id))?;
        
        // For now, just delegate to regular get - in a real implementation,
        // you'd use actual RocksDB snapshots
        self.get(&snapshot_info.db_name, key)
    }

    pub fn get_stats(&self, db_name: &str) -> Result<StorageStats> {
        let db = self.get_or_init_db(db_name)?;

        let total_keys = db.property_value("rocksdb.estimate-num-keys")?
            .unwrap_or_else(|| "0".to_string())
            .parse::<i64>().unwrap_or(0);

        let total_size = db.property_value("rocksdb.total-sst-files-size")?
            .unwrap_or_else(|| "0".to_string())
            .parse::<i64>().unwrap_or(0);

        let mut engine_stats = HashMap::new();

        // Collect various RocksDB statistics
        if let Some(stats) = db.property_value("rocksdb.stats")? {
            engine_stats.insert("rocksdb.stats".to_string(), stats);
        }

        Ok(StorageStats {
            total_keys,
            total_size,
            engine_stats,
            last_modified: chrono::Utc::now().timestamp(),
        })
    }

    pub fn list_databases(&self) -> Vec<String> {
        self.databases.read().unwrap().keys().cloned().collect()
    }

    pub fn health_check(&self) -> HealthStatus {
        // Simple health check - could be more sophisticated
        if self.databases.read().unwrap().is_empty() {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_engine() -> (StorageEngine, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let engine = StorageEngine::new(temp_dir.path()).unwrap();
        (engine, temp_dir)
    }

    #[test]
    fn test_get_or_init_db_creates_new_database() {
        let (engine, _temp_dir) = create_test_engine();

        // Database should not exist initially
        assert!(!engine.is_alive("test_db"));

        // get_or_init_db should create the database
        let db = engine.get_or_init_db("test_db").unwrap();
        assert!(db.get(b"nonexistent_key").unwrap().is_none());

        // Database should now exist
        assert!(engine.is_alive("test_db"));
    }

    #[test]
    fn test_get_or_init_db_returns_existing_database() {
        let (engine, _temp_dir) = create_test_engine();

        // Create database first
        let db1 = engine.get_or_init_db("test_db").unwrap();
        db1.put(b"test_key", b"test_value").unwrap();

        // get_or_init_db should return the same database
        let db2 = engine.get_or_init_db("test_db").unwrap();
        let value = db2.get(b"test_key").unwrap().unwrap();
        assert_eq!(value, b"test_value");

        // Both should be the same Arc instance
        assert!(Arc::ptr_eq(&db1, &db2));
    }

    #[test]
    fn test_optimized_operations_work_correctly() {
        let (engine, _temp_dir) = create_test_engine();

        // Test put and get operations
        engine.put("test_db", b"key1", b"value1").unwrap();
        let result = engine.get("test_db", b"key1").unwrap();
        assert_eq!(result, Some(b"value1".to_vec()));

        // Test has operation
        assert!(engine.has("test_db", b"key1").unwrap());
        assert!(!engine.has("test_db", b"nonexistent").unwrap());

        // Test delete operation
        engine.delete("test_db", b"key1").unwrap();
        assert!(!engine.has("test_db", b"key1").unwrap());

        // Test size operation
        engine.put("test_db", b"key2", b"value2").unwrap();
        engine.put("test_db", b"key3", b"value3").unwrap();
        let size = engine.size("test_db").unwrap();
        assert!(size >= 0); // Size should be non-negative
    }

    #[test]
    fn test_batch_operations() {
        let (engine, _temp_dir) = create_test_engine();

        // Test batch write
        let operations = vec![
            BatchOperation {
                r#type: batch_operation::Type::Put as i32,
                key: b"batch_key1".to_vec(),
                value: b"batch_value1".to_vec(),
            },
            BatchOperation {
                r#type: batch_operation::Type::Put as i32,
                key: b"batch_key2".to_vec(),
                value: b"batch_value2".to_vec(),
            },
        ];

        engine.batch_write("test_db", &operations).unwrap();

        // Test batch get
        let keys = vec![b"batch_key1".to_vec(), b"batch_key2".to_vec(), b"nonexistent".to_vec()];
        let results = engine.batch_get("test_db", &keys).unwrap();

        assert_eq!(results.len(), 3);
        assert!(results[0].found);
        assert_eq!(results[0].value, b"batch_value1");
        assert!(results[1].found);
        assert_eq!(results[1].value, b"batch_value2");
        assert!(!results[2].found);
    }
}