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

    pub fn init_db(&self, db_name: &str, config: &StorageConfig) -> Result<()> {
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

        let db = DB::open(&opts, &db_path)?;
        let db_arc = Arc::new(db);
        
        self.databases.write().unwrap().insert(db_name.to_string(), db_arc);
        info!("Initialized database: {}", db_name);
        
        Ok(())
    }

    pub fn get(&self, db_name: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
        match db.get(key)? {
            Some(value) => Ok(Some(value)),
            None => Ok(None),
        }
    }

    pub fn put(&self, db_name: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
        db.put(key, value)?;
        Ok(())
    }

    pub fn delete(&self, db_name: &str, key: &[u8]) -> Result<()> {
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
        db.delete(key)?;
        Ok(())
    }

    pub fn has(&self, db_name: &str, key: &[u8]) -> Result<bool> {
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
        Ok(db.get(key)?.is_some())
    }

    pub fn batch_write(&self, db_name: &str, operations: &[BatchOperation]) -> Result<()> {
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
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
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
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
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
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
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
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
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
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
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
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
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
        // Approximate count using RocksDB property
        let count_str = db.property_value("rocksdb.estimate-num-keys")?
            .unwrap_or_else(|| "0".to_string());
        
        Ok(count_str.parse::<i64>().unwrap_or(0))
    }

    pub fn is_empty(&self, db_name: &str) -> Result<bool> {
        Ok(self.size(db_name)? == 0)
    }

    pub fn begin_transaction(&self, db_name: &str) -> Result<String> {
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
        let databases = self.databases.read().unwrap();
        let _db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
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
        let databases = self.databases.read().unwrap();
        let db = databases.get(db_name)
            .ok_or_else(|| anyhow!("Database not found: {}", db_name))?;
        
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