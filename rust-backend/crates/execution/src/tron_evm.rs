use anyhow::{Result, anyhow};
use revm::{
    primitives::{
        ExecutionResult, Output,
    },
    Evm, Database, DatabaseCommit,
};

use tron_backend_common::ExecutionConfig;
use crate::precompiles::TronPrecompiles;
use crate::storage_adapter::{StorageAdapterDatabase, StorageAdapter};

// Tron-specific transaction and execution types
#[derive(Debug, Clone)]
pub struct TronTransaction {
    pub from: revm::primitives::Address,
    pub to: Option<revm::primitives::Address>,
    pub value: revm::primitives::U256,
    pub data: revm::primitives::Bytes,
    pub gas_limit: u64,
    pub gas_price: revm::primitives::U256,
    pub nonce: u64,
}

#[derive(Debug, Clone)]
pub struct TronExecutionContext {
    pub block_number: u64,
    pub block_timestamp: u64,
    pub block_coinbase: revm::primitives::Address,
    pub block_difficulty: revm::primitives::U256,
    pub block_gas_limit: u64,
    pub chain_id: u64,
    pub energy_price: u64,
    pub bandwidth_price: u64,
}

#[derive(Debug, Clone)]
pub enum TronStateChange {
    /// Storage slot change within a contract
    StorageChange {
        address: revm::primitives::Address,
        key: revm::primitives::U256,
        old_value: revm::primitives::U256,
        new_value: revm::primitives::U256,
    },
    /// Account-level change (balance, nonce, code, etc.)
    AccountChange {
        address: revm::primitives::Address,
        old_account: Option<revm::primitives::AccountInfo>,
        new_account: Option<revm::primitives::AccountInfo>,
    },
}

#[derive(Debug, Clone)]
pub struct TronExecutionResult {
    pub success: bool,
    pub return_data: revm::primitives::Bytes,
    pub energy_used: u64,
    pub bandwidth_used: u64,
    pub logs: Vec<revm::primitives::Log>,
    pub state_changes: Vec<TronStateChange>,
    pub error: Option<String>,
}

/// TronEVM wrapper around REVM with Tron-specific configurations
pub struct TronEvm<DB: Database + DatabaseCommit + Send + Sync + 'static> {
    evm: Evm<'static, (), DB>,
    config: ExecutionConfig,
    precompiles: TronPrecompiles,
    energy_accounting: EnergyAccounting,
    bandwidth_accounting: BandwidthAccounting,
    // Track state changes during execution
    state_changes: Vec<TronStateChange>,
}

impl<DB: Database + DatabaseCommit + Send + Sync + 'static> TronEvm<DB> 
where 
    DB::Error: std::fmt::Debug,
{
    pub fn new(database: DB, config: &ExecutionConfig) -> Result<Self> {
        let mut evm = Evm::builder()
            .with_db(database)
            .build();

        // Configure for Tron - access through context
        evm.context.evm.inner.env.cfg.chain_id = 0x2b6653dc; // Tron mainnet chain ID
        
        // Note: spec_id is not directly accessible in this version of REVM
        // The spec is typically set through the handler configuration
        
        evm.context.evm.inner.env.cfg.limit_contract_code_size = Some(24576); // 24KB limit

        let precompiles = TronPrecompiles::new();
        let energy_accounting = EnergyAccounting::new(config.energy_limit);
        let bandwidth_accounting = BandwidthAccounting::new(config.bandwidth_limit);

        Ok(Self {
            evm,
            config: config.clone(),
            precompiles,
            energy_accounting,
            bandwidth_accounting,
            state_changes: Vec::new(),
        })
    }

    /// Call a contract without modifying state
    pub fn call_contract(
        &mut self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        self.setup_environment(tx, context);

        let result = self.evm.transact().map_err(|e| anyhow!("Contract call failed: {:?}", e))?;
        self.process_call_result(result.result, context)
    }

    /// Estimate energy usage for a transaction
    pub fn estimate_energy(
        &mut self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<u64> {
        self.setup_environment(tx, context);

        let result = self.evm.transact().map_err(|e| anyhow!("Energy estimation failed: {:?}", e))?;

        Ok(self.calculate_energy_usage(&result.result, tx))
    }

    fn setup_environment(&mut self, tx: &TronTransaction, context: &TronExecutionContext) {
        // Set transaction environment
        self.evm.context.evm.inner.env.tx.caller = tx.from;
        self.evm.context.evm.inner.env.tx.transact_to = match tx.to {
            Some(to) => revm::primitives::TransactTo::Call(to),
            None => revm::primitives::TransactTo::Create,
        };
        self.evm.context.evm.inner.env.tx.value = tx.value;
        self.evm.context.evm.inner.env.tx.data = tx.data.clone();
        self.evm.context.evm.inner.env.tx.gas_limit = tx.gas_limit;
        self.evm.context.evm.inner.env.tx.gas_price = tx.gas_price;
        self.evm.context.evm.inner.env.tx.nonce = Some(tx.nonce);

        // Set block environment
        self.evm.context.evm.inner.env.block.number = revm::primitives::U256::from(context.block_number);
        self.evm.context.evm.inner.env.block.timestamp = revm::primitives::U256::from(context.block_timestamp);
        self.evm.context.evm.inner.env.block.coinbase = context.block_coinbase;
        self.evm.context.evm.inner.env.block.difficulty = context.block_difficulty;
        self.evm.context.evm.inner.env.block.gas_limit = revm::primitives::U256::from(context.block_gas_limit);
        
        // TRON Parity Fix: Set basefee = 0 to prevent EIP-1559 base fee burns
        // Keep coinbase set for COINBASE opcode correctness, but ensure no fee distribution
        self.evm.context.evm.inner.env.block.basefee = revm::primitives::U256::ZERO;
        
        tracing::debug!("TRON environment setup - gas_price: {}, basefee: 0 (TRON mode)", tx.gas_price);

        // Set Tron-specific configurations
        self.energy_accounting.reset();
        self.bandwidth_accounting.reset();
    }

    fn process_execution_result(
        &mut self,
        result: ExecutionResult,
        tx: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let energy_used = self.calculate_energy_usage(&result, tx);
        let bandwidth_used = self.calculate_bandwidth_usage(tx);

        match result {
            ExecutionResult::Success { reason: _, gas_used: _, gas_refunded: _, logs, output } => {
                let return_data = match output {
                    Output::Call(data) => data,
                    Output::Create(data, _) => data,
                };

                Ok(TronExecutionResult {
                    success: true,
                    return_data,
                    energy_used,
                    bandwidth_used,
                    logs,
                    state_changes: vec![], // Will be populated by caller
                    error: None,
                })
            }
            ExecutionResult::Revert { gas_used: _, output } => {
                Ok(TronExecutionResult {
                    success: false,
                    return_data: output,
                    energy_used,
                    bandwidth_used,
                    logs: vec![],
                    state_changes: vec![],
                    error: Some("Transaction reverted".to_string()),
                })
            }
            ExecutionResult::Halt { reason, gas_used: _ } => {
                Ok(TronExecutionResult {
                    success: false,
                    return_data: revm::primitives::Bytes::new(),
                    energy_used,
                    bandwidth_used,
                    logs: vec![],
                    state_changes: vec![],
                    error: Some(format!("Transaction halted: {:?}", reason)),
                })
            }
        }
    }

    fn process_call_result(
        &mut self,
        result: ExecutionResult,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        match result {
            ExecutionResult::Success { reason: _, gas_used, gas_refunded: _, logs, output } => {
                let return_data = match output {
                    Output::Call(data) => data,
                    Output::Create(data, _) => data,
                };

                Ok(TronExecutionResult {
                    success: true,
                    return_data,
                    energy_used: gas_used,
                    bandwidth_used: 0, // Call doesn't use bandwidth
                    logs,
                    state_changes: vec![], // Calls don't modify state
                    error: None,
                })
            }
            ExecutionResult::Revert { gas_used, output } => {
                Ok(TronExecutionResult {
                    success: false,
                    return_data: output,
                    energy_used: gas_used,
                    bandwidth_used: 0,
                    logs: vec![],
                    state_changes: vec![],
                    error: Some("Call reverted".to_string()),
                })
            }
            ExecutionResult::Halt { reason, gas_used } => {
                Ok(TronExecutionResult {
                    success: false,
                    return_data: revm::primitives::Bytes::new(),
                    energy_used: gas_used,
                    bandwidth_used: 0,
                    logs: vec![],
                    state_changes: vec![],
                    error: Some(format!("Call halted: {:?}", reason)),
                })
            }
        }
    }

    fn calculate_energy_usage(&self, result: &ExecutionResult, _tx: &TronTransaction) -> u64 {
        match result {
            ExecutionResult::Success { gas_used, .. } => *gas_used,
            ExecutionResult::Revert { gas_used, .. } => *gas_used,
            ExecutionResult::Halt { gas_used, .. } => *gas_used,
        }
    }

    fn calculate_bandwidth_usage(&self, tx: &TronTransaction) -> u64 {
        // Simple bandwidth calculation based on transaction size
        let base_size = 32; // Basic transaction overhead
        let data_size = tx.data.len() as u64;
        base_size + data_size
    }
}

// Specialized implementation for StorageAdapterDatabase
impl<S: StorageAdapter + Send + Sync + 'static> TronEvm<StorageAdapterDatabase<S>> {
    /// Extract state changes from StorageAdapterDatabase after execution
    pub fn extract_state_changes_from_db(&mut self) -> Vec<TronStateChange> {
        let db = &mut self.evm.context.evm.db;
        let state_records = db.get_state_change_records();
        
        tracing::info!("Extracting {} state change records from database", state_records.len());
        
        let mut state_changes: Vec<TronStateChange> = state_records.iter().map(|record| {
            match record {
                crate::storage_adapter::StateChangeRecord::StorageChange { 
                    address, key, old_value, new_value 
                } => TronStateChange::StorageChange {
                    address: *address,
                    key: *key,
                    old_value: *old_value,
                    new_value: *new_value,
                },
                crate::storage_adapter::StateChangeRecord::AccountChange { 
                    address, old_account, new_account 
                } => TronStateChange::AccountChange {
                    address: *address,
                    old_account: old_account.clone(),
                    new_account: new_account.clone(),
                },
            }
        }).collect();
        
        // TRON Parity Fix: Sort state changes deterministically for consistent digest calculation
        state_changes.sort_by(|a, b| {
            match (a, b) {
                // AccountChange comes before StorageChange for same address
                (TronStateChange::AccountChange { address: addr_a, .. }, 
                 TronStateChange::StorageChange { address: addr_b, .. }) => {
                    let cmp = addr_a.cmp(addr_b);
                    if cmp == std::cmp::Ordering::Equal {
                        std::cmp::Ordering::Less // AccountChange before StorageChange
                    } else {
                        cmp
                    }
                },
                (TronStateChange::StorageChange { address: addr_a, .. }, 
                 TronStateChange::AccountChange { address: addr_b, .. }) => {
                    let cmp = addr_a.cmp(addr_b);
                    if cmp == std::cmp::Ordering::Equal {
                        std::cmp::Ordering::Greater // StorageChange after AccountChange
                    } else {
                        cmp
                    }
                },
                // AccountChange: sort by address
                (TronStateChange::AccountChange { address: addr_a, .. }, 
                 TronStateChange::AccountChange { address: addr_b, .. }) => {
                    addr_a.cmp(addr_b)
                },
                // StorageChange: sort by (address, key)
                (TronStateChange::StorageChange { address: addr_a, key: key_a, .. }, 
                 TronStateChange::StorageChange { address: addr_b, key: key_b, .. }) => {
                    let addr_cmp = addr_a.cmp(addr_b);
                    if addr_cmp == std::cmp::Ordering::Equal {
                        key_a.cmp(key_b)
                    } else {
                        addr_cmp
                    }
                },
            }
        });
        
        // Clear the records after extracting them
        db.clear_state_change_records();
        
        tracing::info!("Extracted and sorted {} state changes for return", state_changes.len());
        for (i, change) in state_changes.iter().enumerate() {
            match change {
                TronStateChange::StorageChange { address, key, .. } => {
                    tracing::info!("  State change {}: StorageChange for address {:?}, key {:?}", i, address, key);
                },
                TronStateChange::AccountChange { address, old_account, new_account } => {
                    let old_exists = old_account.is_some();
                    let new_exists = new_account.is_some();
                    tracing::info!("  State change {}: AccountChange for address {:?}, old_exists: {}, new_exists: {}", 
                                  i, address, old_exists, new_exists);
                },
            }
        }
        
        state_changes
    }

    /// Execute a transaction and capture real state changes
    pub fn execute_transaction_with_state_tracking(
        &mut self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        // Clear previous state changes
        self.state_changes.clear();
        
        // Validate gas limits before execution
        if tx.gas_limit > context.block_gas_limit {
            return Err(anyhow!("Transaction gas limit ({}) exceeds block gas limit ({})", 
                              tx.gas_limit, context.block_gas_limit));
        }
        
        // TRON Parity Fix: Remove Ethereum 21000 gas minimum requirement
        // Only warn for unusually low gas limits to help with debugging
        if tx.gas_limit > 0 && tx.gas_limit < 21000 {
            tracing::warn!("Transaction has unusually low gas limit ({}), may be non-VM transaction", tx.gas_limit);
        }
        
        self.setup_environment(tx, context);

        // Use transact_commit() to execute and commit changes to the database
        let result = self.evm.transact_commit().map_err(|e| anyhow!("Transaction execution failed: {:?}", e))?;
        let mut execution_result = self.process_execution_result(result, tx, context)?;
        
        // Extract real state changes from the database
        execution_result.state_changes = self.extract_state_changes_from_db();
        
        Ok(execution_result)
    }
}

/// Energy accounting for Tron transactions
#[derive(Debug, Clone)]
pub struct EnergyAccounting {
    limit: u64,
    used: u64,
}

impl EnergyAccounting {
    pub fn new(limit: u64) -> Self {
        Self { limit, used: 0 }
    }

    pub fn use_energy(&mut self, amount: u64) -> Result<()> {
        if self.used + amount > self.limit {
            return Err(anyhow!("Energy limit exceeded"));
        }
        self.used += amount;
        Ok(())
    }

    pub fn reset(&mut self) {
        self.used = 0;
    }

    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.used)
    }
}

/// Bandwidth accounting for Tron transactions
#[derive(Debug, Clone)]
pub struct BandwidthAccounting {
    limit: u64,
    used: u64,
}

impl BandwidthAccounting {
    pub fn new(limit: u64) -> Self {
        Self { limit, used: 0 }
    }

    pub fn use_bandwidth(&mut self, amount: u64) -> Result<()> {
        if self.used + amount > self.limit {
            return Err(anyhow!("Bandwidth limit exceeded"));
        }
        self.used += amount;
        Ok(())
    }

    pub fn reset(&mut self) {
        self.used = 0;
    }

    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.used)
    }
} 