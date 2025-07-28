use std::collections::HashMap;
use anyhow::{Result, anyhow};
use revm::{
    primitives::{
        ExecutionResult, Output, Address, U256, Bytes, AccountInfo, Bytecode, B256,
    },
    Evm, Database, DatabaseCommit,
};

use tron_backend_common::ExecutionConfig;
use crate::precompiles::TronPrecompiles;

// State change tracking types
#[derive(Debug, Clone)]
pub enum StateChangeType {
    AccountBalance { old_value: U256, new_value: U256 },
    AccountNonce { old_value: u64, new_value: u64 },
    AccountCode { old_value: Option<Bytes>, new_value: Option<Bytes> },
    StorageSlot { key: U256, old_value: U256, new_value: U256 },
    AccountCreated,
    AccountDeleted,
}

#[derive(Debug, Clone)]
pub struct StateChange {
    pub address: Address,
    pub change_type: StateChangeType,
}

#[derive(Debug, Clone, Default)]
pub struct StateChangeTracker {
    pub changes: Vec<StateChange>,
    // Track account states before modification for comparison
    pub account_snapshots: HashMap<Address, AccountInfo>,
    pub storage_snapshots: HashMap<Address, HashMap<U256, U256>>,
}

impl StateChangeTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn track_account_change(&mut self, address: Address, old_info: Option<AccountInfo>, new_info: AccountInfo) {
        if let Some(old) = old_info {
            // Track balance changes
            if old.balance != new_info.balance {
                self.changes.push(StateChange {
                    address,
                    change_type: StateChangeType::AccountBalance {
                        old_value: old.balance,
                        new_value: new_info.balance,
                    },
                });
            }

            // Track nonce changes
            if old.nonce != new_info.nonce {
                self.changes.push(StateChange {
                    address,
                    change_type: StateChangeType::AccountNonce {
                        old_value: old.nonce,
                        new_value: new_info.nonce,
                    },
                });
            }

            // Track code changes
            if old.code_hash != new_info.code_hash {
                self.changes.push(StateChange {
                    address,
                    change_type: StateChangeType::AccountCode {
                        old_value: old.code.map(|c| c.bytes()),
                        new_value: new_info.code.map(|c| c.bytes()),
                    },
                });
            }
        } else {
            // New account created
            self.changes.push(StateChange {
                address,
                change_type: StateChangeType::AccountCreated,
            });
        }
    }

    pub fn track_storage_change(&mut self, address: Address, key: U256, old_value: U256, new_value: U256) {
        if old_value != new_value {
            self.changes.push(StateChange {
                address,
                change_type: StateChangeType::StorageSlot {
                    key,
                    old_value,
                    new_value,
                },
            });
        }
    }

    pub fn track_account_deletion(&mut self, address: Address) {
        self.changes.push(StateChange {
            address,
            change_type: StateChangeType::AccountDeleted,
        });
    }
}

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
pub struct TronExecutionResult {
    pub success: bool,
    pub return_data: revm::primitives::Bytes,
    pub energy_used: u64,
    pub bandwidth_used: u64,
    pub logs: Vec<revm::primitives::Log>,
    pub error: Option<String>,
    pub state_changes: Vec<StateChange>,
}

/// TronEVM wrapper around REVM with Tron-specific configurations
pub struct TronEvm<DB: Database + DatabaseCommit + Send + Sync + 'static> {
    evm: Evm<'static, (), DB>,
    config: ExecutionConfig,
    precompiles: TronPrecompiles,
    energy_accounting: EnergyAccounting,
    bandwidth_accounting: BandwidthAccounting,
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
        })
    }

    /// Get mutable access to the database
    pub fn database_mut(&mut self) -> &mut DB {
        &mut self.evm.context.evm.db
    }

    /// Execute a transaction and modify state
    pub fn execute_transaction(
        &mut self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        self.setup_environment(tx, context);

        let result = self.evm.transact().map_err(|e| anyhow!("Transaction execution failed: {:?}", e))?;
        self.process_execution_result(result.result, tx, context)
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

        // Set Tron-specific configurations
        self.energy_accounting.reset();
        self.bandwidth_accounting.reset();
    }

    fn process_execution_result(
        &mut self,
        result: ExecutionResult,
        tx: &TronTransaction,
        context: &TronExecutionContext,
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
                    error: None,
                    state_changes: vec![], // TODO: Implement state change tracking
                })
            }
            ExecutionResult::Revert { gas_used: _, output } => {
                Ok(TronExecutionResult {
                    success: false,
                    return_data: output,
                    energy_used,
                    bandwidth_used,
                    logs: vec![],
                    error: Some("Transaction reverted".to_string()),
                    state_changes: vec![], // TODO: Implement state change tracking
                })
            }
            ExecutionResult::Halt { reason, gas_used: _ } => {
                Ok(TronExecutionResult {
                    success: false,
                    return_data: revm::primitives::Bytes::new(),
                    energy_used,
                    bandwidth_used,
                    logs: vec![],
                    error: Some(format!("Transaction halted: {:?}", reason)),
                    state_changes: vec![], // TODO: Implement state change tracking
                })
            }
        }
    }

    fn process_call_result(
        &mut self,
        result: ExecutionResult,
        context: &TronExecutionContext,
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
                    error: None,
                    state_changes: vec![], // TODO: Implement state change tracking
                })
            }
            ExecutionResult::Revert { gas_used, output } => {
                Ok(TronExecutionResult {
                    success: false,
                    return_data: output,
                    energy_used: gas_used,
                    bandwidth_used: 0,
                    logs: vec![],
                    error: Some("Call reverted".to_string()),
                    state_changes: vec![], // TODO: Implement state change tracking
                })
            }
            ExecutionResult::Halt { reason, gas_used } => {
                Ok(TronExecutionResult {
                    success: false,
                    return_data: revm::primitives::Bytes::new(),
                    energy_used: gas_used,
                    bandwidth_used: 0,
                    logs: vec![],
                    error: Some(format!("Call halted: {:?}", reason)),
                    state_changes: vec![], // TODO: Implement state change tracking
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