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
    /// For TRON parity: suppress EVM-style coinbase/miner payouts (default: false for parity)
    pub evm_eth_coinbase_compat: bool,
    /// TRON fee handling configuration
    pub fees: ExecutionFeeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionFeeConfig {
    /// Fee handling mode: "burn", "blackhole", or "none"
    /// - "burn": No state delta for fees (supply reduction handled elsewhere)
    /// - "blackhole": Credit fees to designated blackhole address
    /// - "none": No fee handling (useful for testing)
    pub mode: String,
    
    /// Whether black hole optimization is supported (matches java-tron's supportBlackHoleOptimization)
    pub support_black_hole_optimization: bool,
    
    /// Base58-encoded TRON address for blackhole (required if mode = "blackhole")
    pub blackhole_address_base58: String,
    
    /// Experimental: emit synthetic VM blackhole credits (default: false)
    /// When enabled, VM transactions will emit estimated fee credits to blackhole
    /// This is an approximation and should remain off by default
    pub experimental_vm_blackhole_credit: bool,
    
    /// Optional flat fee for non-VM transactions in SUN (when not reading from dynamic properties)
    /// If None, no fee deltas are emitted for non-VM transactions
    pub non_vm_blackhole_credit_flat: Option<u64>,
    
    /// Whether to enable Rust-side bandwidth/fee semantics reading from dynamic properties
    /// When true, Rust becomes authoritative for non-VM TRX transfer fees
    /// Default: false (Java handles fees until Phase 1 rollout)
    pub use_dynamic_properties: bool,
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
            port: 50011,
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
            evm_eth_coinbase_compat: false, // Default off for TRON parity
            fees: ExecutionFeeConfig::default(),
        }
    }
}

impl Default for ExecutionFeeConfig {
    fn default() -> Self {
        Self {
            mode: "burn".to_string(), // Default to burn mode for TRON parity
            support_black_hole_optimization: true, // Match java-tron default
            blackhole_address_base58: String::new(), // Empty by default, required if mode = "blackhole"
            experimental_vm_blackhole_credit: false, // Default off to avoid double-counting
            non_vm_blackhole_credit_flat: None, // No flat fee emission by default
            use_dynamic_properties: false, // Default off until Phase 1 rollout
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
        builder = builder.set_default("server.port", 50011)?;
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
        builder = builder.set_default("execution.evm_eth_coinbase_compat", false)?;
        
        // Fee configuration defaults
        builder = builder.set_default("execution.fees.mode", "burn")?;
        builder = builder.set_default("execution.fees.support_black_hole_optimization", true)?;
        builder = builder.set_default("execution.fees.blackhole_address_base58", "")?;
        builder = builder.set_default("execution.fees.experimental_vm_blackhole_credit", false)?;
        builder = builder.set_default("execution.fees.use_dynamic_properties", false)?;
        // non_vm_blackhole_credit_flat is Option<u64>, leave unset for None default

        let config = builder.build()?;
        config.try_deserialize()
    }
} 