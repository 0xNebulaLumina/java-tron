use std::collections::HashMap;
use std::path::Path;

use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, debug, error};

use tron_backend_common::{Module, ModuleHealth, HealthStatus, StorageConfig};

mod engine;
pub use engine::{StorageEngine, StorageConfig as EngineStorageConfig};

pub struct StorageModule {
    config: StorageConfig,
    engine: Option<StorageEngine>,
}

impl StorageModule {
    pub fn new(config: &StorageConfig) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            engine: None,
        })
    }

    pub fn get_engine(&self) -> Option<&StorageEngine> {
        self.engine.as_ref()
    }

    // Storage operations that can be called from the core service
    pub fn storage_get(&self, db_name: &str, key: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        if let Some(engine) = &self.engine {
            engine.get(db_name, key)
        } else {
            Err(anyhow::anyhow!("Storage engine not initialized"))
        }
    }

    pub fn storage_put(&self, db_name: &str, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        if let Some(engine) = &self.engine {
            engine.put(db_name, key, value)
        } else {
            Err(anyhow::anyhow!("Storage engine not initialized"))
        }
    }

    pub fn storage_delete(&self, db_name: &str, key: &[u8]) -> anyhow::Result<()> {
        if let Some(engine) = &self.engine {
            engine.delete(db_name, key)
        } else {
            Err(anyhow::anyhow!("Storage engine not initialized"))
        }
    }

    pub fn storage_has(&self, db_name: &str, key: &[u8]) -> anyhow::Result<bool> {
        if let Some(engine) = &self.engine {
            engine.has(db_name, key)
        } else {
            Err(anyhow::anyhow!("Storage engine not initialized"))
        }
    }

    pub fn storage_batch_write(&self, db_name: &str, operations: &[engine::WriteOperation]) -> anyhow::Result<()> {
        if let Some(engine) = &self.engine {
            engine.batch_write(db_name, operations)
        } else {
            Err(anyhow::anyhow!("Storage engine not initialized"))
        }
    }

    pub fn storage_batch_get(&self, db_name: &str, keys: &[Vec<u8>]) -> anyhow::Result<Vec<engine::KeyValue>> {
        if let Some(engine) = &self.engine {
            engine.batch_get(db_name, keys)
        } else {
            Err(anyhow::anyhow!("Storage engine not initialized"))
        }
    }

    pub fn storage_list_databases(&self) -> Vec<String> {
        if let Some(engine) = &self.engine {
            engine.list_databases()
        } else {
            vec![]
        }
    }

    pub fn storage_get_stats(&self, db_name: &str) -> anyhow::Result<std::collections::HashMap<String, String>> {
        if let Some(engine) = &self.engine {
            engine.get_stats(db_name)
        } else {
            Err(anyhow::anyhow!("Storage engine not initialized"))
        }
    }

    pub fn storage_init_db(&self, db_name: &str, config: &EngineStorageConfig) -> anyhow::Result<()> {
        if let Some(engine) = &self.engine {
            engine.init_db(db_name, config)
        } else {
            Err(anyhow::anyhow!("Storage engine not initialized"))
        }
    }

    pub fn storage_is_alive(&self, db_name: &str) -> bool {
        if let Some(engine) = &self.engine {
            engine.is_alive(db_name)
        } else {
            false
        }
    }

    pub fn storage_size(&self, db_name: &str) -> anyhow::Result<i64> {
        if let Some(engine) = &self.engine {
            engine.size(db_name)
        } else {
            Err(anyhow::anyhow!("Storage engine not initialized"))
        }
    }

    pub fn storage_is_empty(&self, db_name: &str) -> anyhow::Result<bool> {
        if let Some(engine) = &self.engine {
            engine.is_empty(db_name)
        } else {
            Err(anyhow::anyhow!("Storage engine not initialized"))
        }
    }
}

#[async_trait]
impl Module for StorageModule {
    fn name(&self) -> &str {
        "storage"
    }
    
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
    
    async fn init(&mut self) -> Result<()> {
        info!("Initializing storage module");

        // Create data directory if it doesn't exist
        let data_dir = Path::new(&self.config.data_dir);
        if !data_dir.exists() {
            std::fs::create_dir_all(data_dir)?;
            info!("Created data directory: {}", self.config.data_dir);
        }

        // Initialize the storage engine
        let engine = StorageEngine::new(&self.config.data_dir)?;
        self.engine = Some(engine);

        info!("Storage engine initialized successfully");
        Ok(())
    }
    
    async fn start(&mut self) -> Result<()> {
        info!("Starting storage module");
        
        // TODO: Start any background tasks needed for storage
        
        Ok(())
    }
    
    async fn stop(&mut self) -> Result<()> {
        info!("Stopping storage module");
        
        // TODO: Cleanup storage resources
        
        Ok(())
    }
    
    async fn health(&self) -> ModuleHealth {
        if let Some(engine) = &self.engine {
            let status = engine.health_check();
            match status {
                engine::ProtoHealthStatus::Healthy => ModuleHealth::healthy(),
                engine::ProtoHealthStatus::Degraded => ModuleHealth::degraded("Some databases may be unavailable"),
                engine::ProtoHealthStatus::Unhealthy => ModuleHealth::unhealthy("Storage engine is unhealthy"),
            }
        } else {
            ModuleHealth::unhealthy("Storage engine not initialized")
        }
    }

    fn metrics(&self) -> HashMap<String, f64> {
        let mut metrics = HashMap::new();

        if let Some(engine) = &self.engine {
            let databases = engine.list_databases();
            metrics.insert("databases_count".to_string(), databases.len() as f64);

            // Collect metrics from all databases
            let mut total_keys = 0i64;
            let mut total_size = 0i64;

            for db_name in databases {
                if let Ok(stats) = engine.get_stats(&db_name) {
                    if let Some(keys_str) = stats.get("total_keys") {
                        if let Ok(keys) = keys_str.parse::<i64>() {
                            total_keys += keys;
                        }
                    }
                    if let Some(size_str) = stats.get("total_size") {
                        if let Ok(size) = size_str.parse::<i64>() {
                            total_size += size;
                        }
                    }
                }
            }

            metrics.insert("total_keys".to_string(), total_keys as f64);
            metrics.insert("total_size_bytes".to_string(), total_size as f64);
        }

        metrics
    }
} 