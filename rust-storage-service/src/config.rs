use anyhow::Result;
use serde::{Deserialize, Serialize};

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
            port: 50011,
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
    let settings = config::Config::builder()
        // Start with default values
        .set_default("host", "127.0.0.1")?
        .set_default("port", 50011)?
        .set_default("data_dir", "./data")?
        .set_default("default_engine", "ROCKSDB")?
        
        // RocksDB defaults
        .set_default("rocksdb.max_open_files", 5000)?
        .set_default("rocksdb.block_cache_size", 1024 * 1024 * 1024i64)?
        .set_default("rocksdb.enable_statistics", true)?
        .set_default("rocksdb.level_compaction_dynamic_level_bytes", true)?
        .set_default("rocksdb.max_background_compactions", 4)?
        .set_default("rocksdb.target_file_size_base", 64 * 1024 * 1024i64)?
        .set_default("rocksdb.max_bytes_for_level_base", 512 * 1024 * 1024i64)?
        
        // Metrics defaults
        .set_default("metrics.enabled", true)?
        .set_default("metrics.port", 9090)?
        .set_default("metrics.path", "/metrics")?
        
        // Try to load from file if it exists
        .add_source(config::File::with_name("config").required(false))
        
        // Override with environment variables
        .add_source(config::Environment::with_prefix("TRON_STORAGE"))
        
        .build()?;

    Ok(settings.try_deserialize()?)
} 