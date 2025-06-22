use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: String,
    pub default_engine: String,
    pub rocksdb: RocksDbConfig,
    pub metrics: MetricsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RocksDbConfig {
    pub max_open_files: i32,
    pub block_cache_size: u64,
    pub enable_statistics: bool,
    pub level_compaction_dynamic_level_bytes: bool,
    pub max_background_compactions: i32,
    pub target_file_size_base: u64,
    pub max_bytes_for_level_base: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub port: u16,
    pub path: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            host: "127.0.0.1".to_string(),
            port: 50051,
            data_dir: "./data".to_string(),
            default_engine: "ROCKSDB".to_string(),
            rocksdb: RocksDbConfig::default(),
            metrics: MetricsConfig::default(),
        }
    }
}

impl Default for RocksDbConfig {
    fn default() -> Self {
        RocksDbConfig {
            max_open_files: 5000,
            block_cache_size: 1024 * 1024 * 1024, // 1GB
            enable_statistics: true,
            level_compaction_dynamic_level_bytes: true,
            max_background_compactions: 4,
            target_file_size_base: 64 * 1024 * 1024, // 64MB
            max_bytes_for_level_base: 512 * 1024 * 1024, // 512MB
        }
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        MetricsConfig {
            enabled: true,
            port: 9090,
            path: "/metrics".to_string(),
        }
    }
}

pub fn load_config() -> Result<Config> {
    let mut settings = config::Config::default();
    
    // Start with default values
    settings.set_default("host", "127.0.0.1")?;
    settings.set_default("port", 50051)?;
    settings.set_default("data_dir", "./data")?;
    settings.set_default("default_engine", "ROCKSDB")?;
    
    // RocksDB defaults
    settings.set_default("rocksdb.max_open_files", 5000)?;
    settings.set_default("rocksdb.block_cache_size", 1024 * 1024 * 1024i64)?;
    settings.set_default("rocksdb.enable_statistics", true)?;
    settings.set_default("rocksdb.level_compaction_dynamic_level_bytes", true)?;
    settings.set_default("rocksdb.max_background_compactions", 4)?;
    settings.set_default("rocksdb.target_file_size_base", 64 * 1024 * 1024i64)?;
    settings.set_default("rocksdb.max_bytes_for_level_base", 512 * 1024 * 1024i64)?;
    
    // Metrics defaults
    settings.set_default("metrics.enabled", true)?;
    settings.set_default("metrics.port", 9090)?;
    settings.set_default("metrics.path", "/metrics")?;

    // Try to load from file if it exists
    if let Ok(_) = settings.merge(config::File::with_name("config")) {
        // Config file loaded successfully
    }

    // Override with environment variables
    settings.merge(config::Environment::with_prefix("TRON_STORAGE"))?;

    Ok(settings.try_deserialize()?)
} 