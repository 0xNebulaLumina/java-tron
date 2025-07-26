use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use tracing::info;
use async_trait::async_trait;

use tron_backend_common::{Module, ModuleHealth, ExecutionConfig};

// Re-export key types for external use
pub use tron_evm::{TronEvm, TronTransaction, TronExecutionContext, TronExecutionResult};
pub use precompiles::TronPrecompiles;
pub use storage_adapter::{StorageAdapter, InMemoryStorageAdapter, StorageAdapterDatabase};

mod tron_evm;
mod precompiles;
mod storage_adapter;

pub struct ExecutionModule {
    config: ExecutionConfig,
    initialized: bool,
}

impl ExecutionModule {
    pub fn new(config: ExecutionConfig) -> Self {
        Self {
            config,
            initialized: false,
        }
    }

    /// Execute a transaction using the provided storage adapter
    pub fn execute_transaction_with_storage<S: StorageAdapter + 'static>(
        &self,
        storage: S,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let database = StorageAdapterDatabase::new(storage);
        let mut evm = TronEvm::new(database, &self.config)?;
        evm.execute_transaction(tx, context)
    }

    /// Call a contract without state changes
    pub fn call_contract_with_storage<S: StorageAdapter + 'static>(
        &self,
        storage: S,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let database = StorageAdapterDatabase::new(storage);
        let mut evm = TronEvm::new(database, &self.config)?;
        evm.call_contract(tx, context)
    }

    /// Estimate energy usage for a transaction
    pub fn estimate_energy_with_storage<S: StorageAdapter + 'static>(
        &self,
        storage: S,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<u64> {
        let database = StorageAdapterDatabase::new(storage);
        let mut evm = TronEvm::new(database, &self.config)?;
        evm.estimate_energy(tx, context)
    }

    /// Execute a transaction using in-memory storage (for testing)
    pub fn execute_transaction(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let storage = InMemoryStorageAdapter::new();
        self.execute_transaction_with_storage(storage, tx, context)
    }

    /// Call a contract using in-memory storage (for testing)
    pub fn call_contract(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let storage = InMemoryStorageAdapter::new();
        self.call_contract_with_storage(storage, tx, context)
    }

    /// Estimate energy using in-memory storage (for testing)
    pub fn estimate_energy(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<u64> {
        let storage = InMemoryStorageAdapter::new();
        self.estimate_energy_with_storage(storage, tx, context)
    }
}

/// ExecutionModule with a specific storage adapter type
pub struct ExecutionModuleWithStorage<S: StorageAdapter + 'static> {
    module: ExecutionModule,
    storage: Arc<RwLock<S>>,
}

impl<S: StorageAdapter + 'static> ExecutionModuleWithStorage<S> {
    pub fn new(config: ExecutionConfig, storage: S) -> Self {
        Self {
            module: ExecutionModule::new(config),
            storage: Arc::new(RwLock::new(storage)),
        }
    }

    pub fn execute_transaction(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        // For now, we'll create a new storage adapter for each transaction
        // In a real implementation, we'd need to handle concurrent access properly
        let storage = InMemoryStorageAdapter::new(); // Placeholder
        self.module.execute_transaction_with_storage(storage, tx, context)
    }

    pub fn call_contract(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let storage = InMemoryStorageAdapter::new(); // Placeholder
        self.module.call_contract_with_storage(storage, tx, context)
    }

    pub fn estimate_energy(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<u64> {
        let storage = InMemoryStorageAdapter::new(); // Placeholder
        self.module.estimate_energy_with_storage(storage, tx, context)
    }
}

#[async_trait]
impl Module for ExecutionModule {
    fn name(&self) -> &str {
        "execution"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn init(&mut self) -> Result<()> {
        info!("Initializing execution module");
        
        // Validate configuration
        if self.config.energy_limit == 0 {
            return Err(anyhow::anyhow!("Energy limit must be greater than 0"));
        }
        
        if self.config.bandwidth_limit == 0 {
            return Err(anyhow::anyhow!("Bandwidth limit must be greater than 0"));
        }
        
        self.initialized = true;
        info!("Execution module initialized successfully");
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Starting execution module");
        if !self.initialized {
            return Err(anyhow::anyhow!("Module not initialized"));
        }
        
        // Test EVM creation with dummy storage
        let storage = InMemoryStorageAdapter::new();
        let database = StorageAdapterDatabase::new(storage);
        let _evm = TronEvm::new(database, &self.config)?;
        
        info!("Execution module started successfully");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping execution module");
        self.initialized = false;
        info!("Execution module stopped");
        Ok(())
    }

    async fn health(&self) -> ModuleHealth {
        if !self.initialized {
            return ModuleHealth::unhealthy("Module not initialized");
        }
        
        // Test EVM creation
        let storage = InMemoryStorageAdapter::new();
        let database = StorageAdapterDatabase::new(storage);
        match TronEvm::new(database, &self.config) {
            Ok(_) => ModuleHealth::healthy(),
            Err(e) => ModuleHealth::unhealthy(&format!("EVM creation failed: {}", e)),
        }
    }

    fn metrics(&self) -> HashMap<String, f64> {
        let mut metrics = HashMap::new();
        metrics.insert("initialized".to_string(), if self.initialized { 1.0 } else { 0.0 });
        metrics.insert("energy_limit".to_string(), self.config.energy_limit as f64);
        metrics.insert("bandwidth_limit".to_string(), self.config.bandwidth_limit as f64);
        metrics
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
} 