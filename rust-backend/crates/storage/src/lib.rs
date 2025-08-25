use std::collections::HashMap;
use std::path::Path;

use async_trait::async_trait;
use anyhow::{anyhow, Result};
use tracing::info;

use tron_backend_common::{Module, ModuleHealth, StorageConfig as CommonStorageConfig};

mod engine;
pub use engine::*;

pub struct StorageModule {
    config: CommonStorageConfig,
    engine: Option<StorageEngine>,
}

impl StorageModule {
    pub fn new(config: &CommonStorageConfig) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            engine: None,
        })
    }
    
    pub fn engine(&self) -> Result<&StorageEngine> {
        self.engine.as_ref().ok_or_else(|| anyhow!("Storage engine not initialized"))
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
        
        // Initialize storage engine
        let engine = StorageEngine::new(&self.config.data_dir)?;
        self.engine = Some(engine);
        
        info!("Storage module initialized successfully");
        Ok(())
    }
    
    async fn start(&mut self) -> Result<()> {
        info!("Starting storage module");
        
        // Engine is already initialized, nothing else to start
        
        Ok(())
    }
    
    async fn stop(&mut self) -> Result<()> {
        info!("Stopping storage module");
        
        // RocksDB handles cleanup automatically when dropped
        self.engine = None;
        
        Ok(())
    }
    
    async fn health(&self) -> ModuleHealth {
        if let Some(engine) = &self.engine {
            // Check if we can access the databases
            match engine.list_databases() {
                Ok(databases) => {
                    let mut details = HashMap::new();
                    details.insert("databases_count".to_string(), databases.len().to_string());
                    
                    // Try to perform a basic operation on each database
                    let mut healthy_dbs = 0;
                    for db_name in &databases {
                        if engine.is_alive(db_name).unwrap_or(false) {
                            healthy_dbs += 1;
                        }
                    }
                    
                    details.insert("healthy_databases".to_string(), healthy_dbs.to_string());
                    
                    if healthy_dbs == databases.len() {
                        ModuleHealth::healthy().with_details(details)
                    } else {
                        ModuleHealth::degraded("Some databases are not responding").with_details(details)
                    }
                }
                Err(e) => {
                    ModuleHealth::unhealthy(&format!("Cannot access databases: {}", e))
                }
            }
        } else {
            ModuleHealth::unhealthy("Storage engine not initialized")
        }
    }
    
    fn metrics(&self) -> HashMap<String, f64> {
        let mut metrics = HashMap::new();
        
        if let Some(engine) = &self.engine {
            if let Ok(databases) = engine.list_databases() {
                metrics.insert("storage.databases.count".to_string(), databases.len() as f64);
                
                for db_name in databases {
                    if let Ok(size) = engine.size(&db_name) {
                        metrics.insert(format!("storage.database.{}.size", db_name), size as f64);
                    }
                }
            }
        }
        
        metrics
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
} 