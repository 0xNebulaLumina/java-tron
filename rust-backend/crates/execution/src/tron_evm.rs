use anyhow::{Result, anyhow};
use revm::{
    primitives::{
        ExecutionResult, Output,
    },
    Evm, Database, DatabaseCommit,
};

use tron_backend_common::ExecutionConfig;
use crate::precompiles::TronPrecompiles;
use crate::storage_adapter::{EvmStateDatabase, EvmStateStore};

// Tron-specific transaction and execution types

/// TRON Contract Type enumeration - matches protobuf ContractType
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum TronContractType {
    AccountCreateContract = 0,
    TransferContract = 1,
    TransferAssetContract = 2,
    VoteAssetContract = 3,
    VoteWitnessContract = 4,
    WitnessCreateContract = 5,
    AssetIssueContract = 6,
    WitnessUpdateContract = 8,
    ParticipateAssetIssueContract = 9,
    AccountUpdateContract = 10,
    FreezeBalanceContract = 11,
    UnfreezeBalanceContract = 12,
    WithdrawBalanceContract = 13,
    UnfreezeAssetContract = 14,
    UpdateAssetContract = 15,
    ProposalCreateContract = 16,
    ProposalApproveContract = 17,
    ProposalDeleteContract = 18,
    SetAccountIdContract = 19,
    CustomContract = 20,
    CreateSmartContract = 30,
    TriggerSmartContract = 31,
    GetContract = 32,
    UpdateSettingContract = 33,
    ExchangeCreateContract = 41,
    ExchangeInjectContract = 42,
    ExchangeWithdrawContract = 43,
    ExchangeTransactionContract = 44,
    UpdateEnergyLimitContract = 45,
    AccountPermissionUpdateContract = 46,
    ClearAbiContract = 48,
    UpdateBrokerageContract = 49,
    ShieldContract = 51,
    MarketSellAssetContract = 52,
    MarketCancelOrderContract = 53,
    FreezeBalanceV2Contract = 54,
    UnfreezeBalanceV2Contract = 55,
    WithdrawExpireUnfreezeContract = 56,
    DelegateResourceContract = 57,
    UndelegateResourceContract = 58,
    CancelAllUnfreezeV2Contract = 59,
}

impl TryFrom<i32> for TronContractType {
    type Error = String;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(TronContractType::AccountCreateContract),
            1 => Ok(TronContractType::TransferContract),
            2 => Ok(TronContractType::TransferAssetContract),
            3 => Ok(TronContractType::VoteAssetContract),
            4 => Ok(TronContractType::VoteWitnessContract),
            5 => Ok(TronContractType::WitnessCreateContract),
            6 => Ok(TronContractType::AssetIssueContract),
            8 => Ok(TronContractType::WitnessUpdateContract),
            9 => Ok(TronContractType::ParticipateAssetIssueContract),
            10 => Ok(TronContractType::AccountUpdateContract),
            11 => Ok(TronContractType::FreezeBalanceContract),
            12 => Ok(TronContractType::UnfreezeBalanceContract),
            13 => Ok(TronContractType::WithdrawBalanceContract),
            14 => Ok(TronContractType::UnfreezeAssetContract),
            15 => Ok(TronContractType::UpdateAssetContract),
            16 => Ok(TronContractType::ProposalCreateContract),
            17 => Ok(TronContractType::ProposalApproveContract),
            18 => Ok(TronContractType::ProposalDeleteContract),
            19 => Ok(TronContractType::SetAccountIdContract),
            20 => Ok(TronContractType::CustomContract),
            30 => Ok(TronContractType::CreateSmartContract),
            31 => Ok(TronContractType::TriggerSmartContract),
            32 => Ok(TronContractType::GetContract),
            33 => Ok(TronContractType::UpdateSettingContract),
            41 => Ok(TronContractType::ExchangeCreateContract),
            42 => Ok(TronContractType::ExchangeInjectContract),
            43 => Ok(TronContractType::ExchangeWithdrawContract),
            44 => Ok(TronContractType::ExchangeTransactionContract),
            45 => Ok(TronContractType::UpdateEnergyLimitContract),
            46 => Ok(TronContractType::AccountPermissionUpdateContract),
            48 => Ok(TronContractType::ClearAbiContract),
            49 => Ok(TronContractType::UpdateBrokerageContract),
            51 => Ok(TronContractType::ShieldContract),
            52 => Ok(TronContractType::MarketSellAssetContract),
            53 => Ok(TronContractType::MarketCancelOrderContract),
            54 => Ok(TronContractType::FreezeBalanceV2Contract),
            55 => Ok(TronContractType::UnfreezeBalanceV2Contract),
            56 => Ok(TronContractType::WithdrawExpireUnfreezeContract),
            57 => Ok(TronContractType::DelegateResourceContract),
            58 => Ok(TronContractType::UndelegateResourceContract),
            59 => Ok(TronContractType::CancelAllUnfreezeV2Contract),
            _ => Err(format!("Invalid contract type: {}", value)),
        }
    }
}

/// Transaction metadata for TRON system contracts
#[derive(Debug, Clone)]
pub struct TxMetadata {
    pub contract_type: Option<TronContractType>,
    pub asset_id: Option<Vec<u8>>,  // For TRC-10 transfers
}

impl Default for TxMetadata {
    fn default() -> Self {
        Self {
            contract_type: None,
            asset_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TronTransaction {
    pub from: revm::primitives::Address,
    pub to: Option<revm::primitives::Address>,
    pub value: revm::primitives::U256,
    pub data: revm::primitives::Bytes,
    pub gas_limit: u64,
    pub gas_price: revm::primitives::U256,
    pub nonce: u64,
    pub metadata: TxMetadata,  // Added metadata for contract type and asset ID
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

/// Freeze/resource ledger change for Phase 2 emission
/// Describes a single freeze or unfreeze operation affecting an owner's resource balance
#[derive(Debug, Clone)]
pub struct FreezeLedgerChange {
    pub owner_address: revm::primitives::Address,
    pub resource: FreezeLedgerResource,
    pub amount: i64,          // Absolute value after operation (for idempotency)
    pub expiration_ms: i64,   // Expiration timestamp in milliseconds since epoch
    pub v2_model: bool,       // true for V2 model, false for legacy V1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezeLedgerResource {
    Bandwidth = 0,
    Energy = 1,
    TronPower = 2,
}

/// Global resource totals snapshot for Phase 2 emission
/// Sent to Java to update DynamicPropertiesStore TOTAL_NET_WEIGHT/TOTAL_NET_LIMIT/etc
/// immediately after freeze/unfreeze operations, fixing FREE_NET vs ACCOUNT_NET divergence
#[derive(Debug, Clone)]
pub struct GlobalResourceTotalsChange {
    pub total_net_weight: i64,      // Sum of all BANDWIDTH freezes / TRX_PRECISION
    pub total_net_limit: i64,       // Current network bandwidth limit (from dynamic props)
    pub total_energy_weight: i64,   // Sum of all ENERGY freezes / TRX_PRECISION
    pub total_energy_limit: i64,    // Current energy limit (from dynamic props, or 0 if N/A)
}

/// TRC-10 Asset Issued (Phase 2: full TRC-10 ledger semantics)
/// Describes a new TRC-10 asset issuance operation for Java-side persistence
#[derive(Debug, Clone)]
pub struct Trc10AssetIssued {
    pub owner_address: revm::primitives::Address,
    pub name: Vec<u8>,
    pub abbr: Vec<u8>,
    pub total_supply: i64,
    pub trx_num: i32,
    pub precision: i32,
    pub num: i32,
    pub start_time: i64,
    pub end_time: i64,
    pub description: Vec<u8>,
    pub url: Vec<u8>,
    pub free_asset_net_limit: i64,
    pub public_free_asset_net_limit: i64,
    pub public_free_asset_net_usage: i64,
    pub public_latest_free_net_time: i64,
    pub token_id: Option<String>,  // Optional; if None, Java computes via TOKEN_ID_NUM
}

/// TRC-10 Asset Transferred (Phase 2: TRC-10 transfer operation)
/// Describes a TRC-10 transfer for Java-side persistence of asset balance changes
#[derive(Debug, Clone)]
pub struct Trc10AssetTransferred {
    pub owner_address: revm::primitives::Address,  // Sender address (20-byte EVM format)
    pub to_address: revm::primitives::Address,     // Recipient address (20-byte EVM format)
    pub asset_name: Vec<u8>,                       // V1 path: asset name bytes
    pub token_id: Option<String>,                  // V2 path: token ID if parsable from asset_name
    pub amount: i64,                               // Transfer amount
}

/// TRC-10 Change (union type for different TRC-10 operations)
/// Phase 2: AssetIssued and AssetTransferred
/// Future: add Trc10Participated, Trc10Updated variants
#[derive(Debug, Clone)]
pub enum Trc10Change {
    AssetIssued(Trc10AssetIssued),
    AssetTransferred(Trc10AssetTransferred),
}

/// Vote entry for VoteChange - represents a single vote for a witness
#[derive(Debug, Clone)]
pub struct VoteEntry {
    pub vote_address: revm::primitives::Address,  // Witness address (20-byte EVM format)
    pub vote_count: u64,
}

/// VoteChange carries updated votes for an account after VoteWitness execution.
/// Java should apply this to Account.votes to maintain parity with embedded mode.
/// This ensures correct old_votes seeding on subsequent votes in the same or later epochs.
#[derive(Debug, Clone)]
pub struct VoteChange {
    pub owner_address: revm::primitives::Address,  // Voter address (20-byte EVM format)
    pub votes: Vec<VoteEntry>,  // New votes list (replaces Account.votes)
}

/// WithdrawChange carries withdrawal info for applying allowance and latestWithdrawTime updates.
/// Used for WithdrawBalanceContract remote execution - Java applies this to Account fields.
/// Balance delta is already handled by AccountChange; this sidecar handles the allowance/time reset.
#[derive(Debug, Clone)]
pub struct WithdrawChange {
    pub owner_address: revm::primitives::Address,  // Witness address (20-byte EVM format)
    pub amount: i64,                               // The withdrawn amount (= Account.allowance before operation)
    pub latest_withdraw_time: i64,                 // Timestamp to set as Account.latestWithdrawTime (block time)
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
    /// AEXT sidecar: per-address resource tracking (before, after) for tracked mode
    /// Key: address, Value: (AextBefore, AextAfter)
    pub aext_map: std::collections::HashMap<revm::primitives::Address, (crate::storage_adapter::AccountAext, crate::storage_adapter::AccountAext)>,
    /// Freeze/resource ledger changes (Phase 2: emit_freeze_ledger_changes)
    /// Emitted when config flag is enabled, for Java-side application
    pub freeze_changes: Vec<FreezeLedgerChange>,
    /// Global resource totals changes (Phase 2: emit_global_resource_changes)
    /// Emitted when flag is enabled, for Java to update DynamicPropertiesStore totals
    /// Fixes FREE_NET vs ACCOUNT_NET divergence by ensuring totalNetWeight/totalNetLimit
    /// are current before next tx in same block
    pub global_resource_changes: Vec<GlobalResourceTotalsChange>,
    /// TRC-10 semantic changes (Phase 2: full TRC-10 ledger persistence)
    /// Rust emits high-level TRC-10 operations; Java applies them to existing stores
    pub trc10_changes: Vec<Trc10Change>,
    /// Vote changes (Phase 2: Account.votes update after VoteWitness)
    /// Rust emits the new votes list; Java applies it to Account.votes
    /// This ensures correct old_votes seeding for subsequent epochs
    pub vote_changes: Vec<VoteChange>,
    /// Withdraw changes (WithdrawBalanceContract: allowance/latestWithdrawTime sidecar)
    /// Rust emits the withdrawal info; Java applies allowance=0 and latestWithdrawTime update
    pub withdraw_changes: Vec<WithdrawChange>,
    /// Phase 0.4: Receipt passthrough - serialized Protocol.Transaction.Result bytes
    /// Contains system contract-specific fields like exchange_id, withdraw_amount,
    /// withdraw_expire_amount, cancel_unfreezeV2_amount, orderId, orderDetails, etc.
    /// Java deserializes this to TransactionResultCapsule and sets on ProgramResult.ret
    pub tron_transaction_result: Option<Vec<u8>>,
    /// Phase 2.I L2: Contract address for CreateSmartContract transactions
    /// Set when EVM creates a new contract; used for:
    /// 1. Persisting SmartContract metadata to ContractStore
    /// 2. Returning in receipt for Java ProgramResult.contractAddress
    pub contract_address: Option<revm::primitives::Address>,
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
                // Phase 2.I L2: Extract return_data and contract_address from output
                let (return_data, contract_address) = match output {
                    Output::Call(data) => (data, None),
                    Output::Create(data, addr) => {
                        // addr is Option<Address> - the created contract's address
                        if let Some(created_addr) = addr {
                            tracing::info!("Contract created at address: {:?}", created_addr);
                            (data, Some(created_addr))
                        } else {
                            tracing::warn!("Contract creation succeeded but no address returned");
                            (data, None)
                        }
                    },
                };

                Ok(TronExecutionResult {
                    success: true,
                    return_data,
                    energy_used,
                    bandwidth_used,
                    logs,
                    state_changes: vec![], // Will be populated by caller
                    error: None,
                    aext_map: std::collections::HashMap::new(), // Will be populated by caller for tracked mode
                    freeze_changes: vec![], // Will be populated by contract handlers
                    global_resource_changes: vec![], // Will be populated by contract handlers
                    trc10_changes: vec![], // Will be populated by TRC-10 contract handlers
                    vote_changes: vec![], // Will be populated by vote contract handlers
                    withdraw_changes: vec![], // Will be populated by withdraw contract handler
                    tron_transaction_result: None, // Phase 0.4: Populated by system contract handlers when needed
                    contract_address, // Phase 2.I L2: Set for CreateSmartContract
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
                    aext_map: std::collections::HashMap::new(),
                    freeze_changes: vec![],
                    global_resource_changes: vec![],
                    trc10_changes: vec![],
                    vote_changes: vec![],
                    withdraw_changes: vec![],
                    tron_transaction_result: None,
                    contract_address: None,
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
                    aext_map: std::collections::HashMap::new(),
                    freeze_changes: vec![],
                    global_resource_changes: vec![],
                    trc10_changes: vec![],
                    vote_changes: vec![],
                    withdraw_changes: vec![],
                    tron_transaction_result: None,
                    contract_address: None,
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
                    Output::Create(data, _) => data, // Call shouldn't create, but handle for completeness
                };

                Ok(TronExecutionResult {
                    success: true,
                    return_data,
                    energy_used: gas_used,
                    bandwidth_used: 0, // Call doesn't use bandwidth
                    logs,
                    state_changes: vec![], // Calls don't modify state
                    error: None,
                    aext_map: std::collections::HashMap::new(),
                    freeze_changes: vec![],
                    global_resource_changes: vec![],
                    trc10_changes: vec![],
                    vote_changes: vec![],
                    withdraw_changes: vec![],
                    tron_transaction_result: None,
                    contract_address: None, // Calls don't create contracts
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
                    aext_map: std::collections::HashMap::new(),
                    freeze_changes: vec![],
                    global_resource_changes: vec![],
                    trc10_changes: vec![],
                    vote_changes: vec![],
                    withdraw_changes: vec![],
                    tron_transaction_result: None,
                    contract_address: None,
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
                    aext_map: std::collections::HashMap::new(),
                    freeze_changes: vec![],
                    global_resource_changes: vec![],
                    trc10_changes: vec![],
                    vote_changes: vec![],
                    withdraw_changes: vec![],
                    tron_transaction_result: None,
                    contract_address: None,
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

// Specialized implementation for EvmStateDatabase
impl<S: EvmStateStore + Send + Sync + 'static> TronEvm<EvmStateDatabase<S>> {
    /// Extract state changes from EvmStateDatabase after execution
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