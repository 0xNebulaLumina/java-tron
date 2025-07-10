use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub execution: ExecutionConfig,
    pub modules: HashMap<String, ModuleConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub max_connections: usize,
    pub keepalive_timeout: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub data_dir: String,
    pub max_open_files: i32,
    pub cache_size: usize,
    pub write_buffer_size: usize,
    pub max_write_buffer_number: i32,
    pub compression: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    pub max_call_depth: usize,
    pub max_code_size: usize,
    pub max_init_code_size: usize,
    pub enable_london_fork: bool,
    pub enable_berlin_fork: bool,
    pub enable_istanbul_fork: bool,
    // Tron-specific
    pub energy_limit: u64,
    pub bandwidth_limit: u64,
    pub max_cpu_time_of_one_tx: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleConfig {
    pub enabled: bool,
    pub settings: HashMap<String, serde_json::Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            storage: StorageConfig::default(),
            execution: ExecutionConfig::default(),
            modules: HashMap::new(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 50012, // Different from storage service (50011)
            max_connections: 1000,
            keepalive_timeout: 60,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: "./data".to_string(),
            max_open_files: 1000,
            cache_size: 128 * 1024 * 1024, // 128MB
            write_buffer_size: 64 * 1024 * 1024, // 64MB
            max_write_buffer_number: 3,
            compression: "lz4".to_string(),
        }
    }
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            max_call_depth: 1024,
            max_code_size: 24576,
            max_init_code_size: 49152,
            enable_london_fork: true,
            enable_berlin_fork: true,
            enable_istanbul_fork: true,
            // Tron defaults
            energy_limit: 100_000_000,
            bandwidth_limit: 5000,
            max_cpu_time_of_one_tx: 80,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, config::ConfigError> {
        let mut builder = config::Config::builder()
            .add_source(config::File::with_name("config").required(false))
            .add_source(config::Environment::with_prefix("TRON_BACKEND").separator("__"));

        // Add default values
        builder = builder.set_default("server.host", "127.0.0.1")?;
        builder = builder.set_default("server.port", 50012)?;
        builder = builder.set_default("server.max_connections", 1000)?;
        builder = builder.set_default("server.keepalive_timeout", 60)?;

        builder = builder.set_default("storage.data_dir", "./data")?;
        builder = builder.set_default("storage.max_open_files", 1000)?;
        builder = builder.set_default("storage.cache_size", 128 * 1024 * 1024)?;
        builder = builder.set_default("storage.write_buffer_size", 64 * 1024 * 1024)?;
        builder = builder.set_default("storage.max_write_buffer_number", 3)?;
        builder = builder.set_default("storage.compression", "lz4")?;

        builder = builder.set_default("execution.max_call_depth", 1024)?;
        builder = builder.set_default("execution.max_code_size", 24576)?;
        builder = builder.set_default("execution.max_init_code_size", 49152)?;
        builder = builder.set_default("execution.enable_london_fork", true)?;
        builder = builder.set_default("execution.enable_berlin_fork", true)?;
        builder = builder.set_default("execution.enable_istanbul_fork", true)?;
        builder = builder.set_default("execution.energy_limit", 100_000_000u64)?;
        builder = builder.set_default("execution.bandwidth_limit", 5000u64)?;
        builder = builder.set_default("execution.max_cpu_time_of_one_tx", 80u64)?;

        let config = builder.build()?;
        config.try_deserialize()
    }
} 