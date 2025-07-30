use anyhow::{Result, anyhow};
use revm::{
    primitives::{
        ExecutionResult, Output,
    },
    Evm, Database, DatabaseCommit,
};

use tron_backend_common::ExecutionConfig;
use crate::precompiles::TronPrecompiles;

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
pub struct TronStateChange {
    pub address: revm::primitives::Address,
    pub key: revm::primitives::U256,
    pub old_value: revm::primitives::U256,
    pub new_value: revm::primitives::U256,
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

    /// Execute a transaction and modify state
    pub fn execute_transaction(
        &mut self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        // Clear previous state changes
        self.state_changes.clear();
        
        self.setup_environment(tx, context);

        let result = self.evm.transact().map_err(|e| anyhow!("Transaction execution failed: {:?}", e))?;
        let mut execution_result = self.process_execution_result(result.result, tx, context)?;
        
        // Extract state changes from the database
        execution_result.state_changes = self.extract_state_changes();
        
        Ok(execution_result)
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

    /// Extract state changes from the database after execution
    fn extract_state_changes(&mut self) -> Vec<TronStateChange> {
        // For now, return the tracked state changes
        // In a full implementation, this would extract changes from REVM's state
        // after transaction execution. This requires deeper integration with REVM's
        // internal state tracking mechanisms.
        
        // TODO: Implement proper state change extraction
        // This would involve:
        // 1. Hooking into REVM's state change notifications
        // 2. Tracking storage slot changes during execution
        // 3. Capturing old values before changes occur
        
        self.state_changes.clone()
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