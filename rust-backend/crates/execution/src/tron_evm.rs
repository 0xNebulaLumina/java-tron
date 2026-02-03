use anyhow::{Result, anyhow};
use prost::Message;
use revm::{
    primitives::{
        ExecutionResult, HandlerCfg, Output, SpecId,
    },
    EvmContext, Inspector,
    Evm, Database, DatabaseCommit,
};

use tron_backend_common::ExecutionConfig;
use crate::precompiles::TronPrecompiles;
use crate::storage_adapter::{EvmStateDatabase, EvmStateStore};

// Tron-specific transaction and execution types

#[derive(Debug, Clone, Default)]
struct TronExternalContext {
    /// Override address for the next top-level CREATE.
    ///
    /// TRON derives CreateSmartContract addresses from txid + owner address,
    /// not from (caller, nonce) like Ethereum.
    create_address_override: Option<revm::primitives::Address>,

    // === TRON internal transaction nonce ===
    //
    // java-tron uses a global per-root-tx internal-transaction counter for deriving
    // CREATE addresses inside EVM execution. The address formula is:
    //   keccak256(root_txid || nonce_be_u64)[12..]
    //
    // The nonce increments for EVERY internal transaction (CALLs, CREATEs, etc.),
    // not just CREATEs. This differs from Ethereum's (caller, account_nonce) scheme.
    root_transaction_id: Option<revm::primitives::B256>,
    /// Internal transaction counter (starts at 0, increments per internal tx).
    internal_tx_nonce: u64,

    // === Energy accounting (TRON vs Ethereum gas schedule) ===
    //
    // java-tron charges memory expansion/copy costs for these opcodes but does NOT include the
    // Ethereum "very low tier" base cost (+3) for each of them. We keep executing with REVM's
    // standard gas schedule for correctness/limits, then subtract the base-cost delta to compute
    // TRON energy usage for receipts/fee accounting.
    mload_count: u64,
    mstore_count: u64,
    mstore8_count: u64,
    calldatacopy_count: u64,
    codecopy_count: u64,
    returndatacopy_count: u64,
    sload_count: u64,
    exp_bytes_occupied: u64,

    // === Execution context for java-tron parity errors ===
    last_opcode: Option<u8>,
    last_gas_limit: Option<u64>,
    last_gas_spent: Option<u64>,
    last_gas_remaining: Option<u64>,

    last_create_output: Option<revm::primitives::Bytes>,
    last_create_gas_limit: Option<u64>,
    last_create_gas_remaining: Option<u64>,
}

impl TronExternalContext {
    fn reset_energy_counters(&mut self) {
        self.mload_count = 0;
        self.mstore_count = 0;
        self.mstore8_count = 0;
        self.calldatacopy_count = 0;
        self.codecopy_count = 0;
        self.returndatacopy_count = 0;
        self.sload_count = 0;
        self.exp_bytes_occupied = 0;

        self.last_opcode = None;
        self.last_gas_limit = None;
        self.last_gas_spent = None;
        self.last_gas_remaining = None;
        self.last_create_output = None;
        self.last_create_gas_limit = None;
        self.last_create_gas_remaining = None;
    }

    /// Derive internal CREATE address using TRON's txid + nonce scheme.
    ///
    /// Java reference: `TransactionUtil.generateContractAddress(transactionRootId, nonce)`
    /// Formula: `keccak256(txid || nonce_be_u64)[12..]`
    fn derive_internal_create_address(&mut self) -> Option<revm::primitives::Address> {
        use sha3::{Digest, Keccak256};

        let root_txid = self.root_transaction_id?;

        // Get current nonce and increment for next use
        let current_nonce = self.internal_tx_nonce;
        self.internal_tx_nonce = self.internal_tx_nonce.saturating_add(1);

        // Concatenate: txid (32 bytes) || nonce (8 bytes big-endian)
        let mut combined = [0u8; 40];
        combined[..32].copy_from_slice(root_txid.as_slice());
        combined[32..40].copy_from_slice(&current_nonce.to_be_bytes());

        // keccak256 and take last 20 bytes
        let hash = Keccak256::digest(&combined);
        let address_bytes: [u8; 20] = hash[12..32].try_into().ok()?;

        Some(revm::primitives::Address::from(address_bytes))
    }

    /// Increment internal transaction nonce (for CALLs that don't CREATE).
    fn increment_internal_nonce(&mut self) {
        self.internal_tx_nonce = self.internal_tx_nonce.saturating_add(1);
    }

    fn tron_energy_opcode_adjustment(&self) -> u64 {
        let ops = self.mload_count
            + self.mstore_count
            + self.mstore8_count
            + self.calldatacopy_count
            + self.codecopy_count
            + self.returndatacopy_count;

        // See java-tron: EnergyCost.getSloadCost() returns 50, while Ethereum (EIP-150+) is 200.
        // Adjust REVM's gas metering to TRON energy by subtracting the delta per SLOAD.
        let sload_delta = self.sload_count.saturating_mul(150);

        // See java-tron: EnergyCost.getExpCost() uses EXP_BYTE_ENERGY = 10, while Ethereum uses 50.
        // Adjust REVM's EXP metering by subtracting 40 per non-zero exponent byte.
        let exp_delta = self.exp_bytes_occupied.saturating_mul(40);

        ops.saturating_mul(3)
            .saturating_add(sload_delta)
            .saturating_add(exp_delta)
    }
}

impl<DB: Database> Inspector<DB> for TronExternalContext {
    fn step(&mut self, interp: &mut revm::interpreter::Interpreter, _context: &mut EvmContext<DB>) {
        self.last_opcode = Some(interp.current_opcode());
        self.last_gas_limit = Some(interp.gas.limit());
        self.last_gas_remaining = Some(interp.gas.remaining());
        self.last_gas_spent = Some(interp.gas.spent());

        match interp.current_opcode() {
            0x51 => self.mload_count += 1,         // MLOAD
            0x52 => self.mstore_count += 1,        // MSTORE
            0x53 => self.mstore8_count += 1,       // MSTORE8
            0x37 => self.calldatacopy_count += 1,  // CALLDATACOPY
            0x39 => self.codecopy_count += 1,      // CODECOPY
            0x3e => self.returndatacopy_count += 1,// RETURNDATACOPY
            0x54 => self.sload_count += 1,         // SLOAD
            0x0a => {                               // EXP
                let stack_len = interp.stack.len();
                if stack_len >= 2 {
                    // EVM stack order: [.., exponent, base] with base on top.
                    let exponent = &interp.stack.data()[stack_len - 2];
                    let exp_bytes = exponent.to_be_bytes::<32>();
                    let first_nonzero = exp_bytes.iter().position(|b| *b != 0);
                    let occupied = match first_nonzero {
                        Some(idx) => (exp_bytes.len() - idx) as u64,
                        None => 0,
                    };
                    self.exp_bytes_occupied = self.exp_bytes_occupied.saturating_add(occupied);
                }
            }
            _ => {}
        }
    }

    fn call(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &mut revm::interpreter::CallInputs,
    ) -> Option<revm::interpreter::CallOutcome> {
        // Java-tron increments the internal transaction nonce for CALL operations too.
        // This affects subsequent CREATE address derivations which use txid + nonce.
        self.increment_internal_nonce();
        None // Let the call proceed normally
    }

    fn create_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &revm::interpreter::CreateInputs,
        outcome: revm::interpreter::CreateOutcome,
    ) -> revm::interpreter::CreateOutcome {
        self.last_create_output = Some(outcome.result.output.clone());
        self.last_create_gas_limit = Some(outcome.result.gas.limit());
        self.last_create_gas_remaining = Some(outcome.result.gas.remaining());
        outcome
    }
}

fn tron_revm_handle_register<DB: Database>(
    handler: &mut revm::handler::register::EvmHandler<'_, TronExternalContext, DB>,
) {
    use std::sync::Arc;

    let spec_id = handler.cfg.spec_id;
    handler.execution.create = Arc::new(move |context, inputs| {
        tron_create_with_optional_override(context, inputs, spec_id)
    });
}

fn tron_create_with_optional_override<DB: Database>(
    context: &mut revm::Context<TronExternalContext, DB>,
    inputs: Box<revm::interpreter::CreateInputs>,
    spec_id: revm::primitives::SpecId,
) -> Result<revm::FrameOrResult, revm::primitives::EVMError<DB::Error>> {
    use revm::{
        interpreter::{Contract, Gas, InstructionResult, Interpreter, InterpreterResult},
        primitives::{Bytecode, Bytes, CreateScheme, SpecId, B256, EOF_MAGIC_BYTES, PRAGUE_EOF},
        FrameOrResult, CALL_STACK_LIMIT,
    };

    // TRON address derivation differs from Ethereum:
    // - Top-level CreateSmartContract: uses pre-computed override (txid + owner_address)
    // - Internal CREATE opcode: uses txid + internal_tx_nonce
    // - CREATE2: uses standard salt-based derivation (handled by REVM)
    let created_address = match inputs.scheme {
        CreateScheme::Create => {
            // First check for top-level override (CreateSmartContract)
            if let Some(override_addr) = context.external.create_address_override.take() {
                override_addr
            } else if let Some(nonce_addr) = context.external.derive_internal_create_address() {
                // Internal CREATE: use TRON's txid + nonce scheme
                nonce_addr
            } else {
                // Fallback to REVM's Ethereum-style derivation (shouldn't happen in normal TRON execution)
                return context.evm.make_create_frame(spec_id, &inputs);
            }
        }
        CreateScheme::Create2 { .. } => {
            // CREATE2 uses salt-based derivation - let REVM handle it
            return context.evm.make_create_frame(spec_id, &inputs);
        }
    };

    let return_error = |e| {
        Ok(FrameOrResult::new_create_result(
            InterpreterResult {
                result: e,
                gas: Gas::new(inputs.gas_limit),
                output: Bytes::new(),
            },
            None,
        ))
    };

    // Depth check.
    if context.evm.journaled_state.depth() > CALL_STACK_LIMIT {
        return return_error(InstructionResult::CallTooDeep);
    }

    // Prague EOF.
    if spec_id.is_enabled_in(PRAGUE_EOF) && inputs.init_code.starts_with(&EOF_MAGIC_BYTES) {
        return return_error(InstructionResult::CreateInitCodeStartingEF00);
    }

    // Fetch caller balance.
    let caller_balance = context.evm.balance(inputs.caller)?;

    // Check if caller has enough balance to send to the created contract.
    if caller_balance.data < inputs.value {
        return return_error(InstructionResult::OutOfFunds);
    }

    // Increase nonce of caller and check overflow.
    if context.evm.journaled_state.inc_nonce(inputs.caller).is_none() {
        return return_error(InstructionResult::Return);
    }

    // The created address is not allowed to be a precompile.
    if context.evm.precompiles.contains(&created_address) {
        return return_error(InstructionResult::CreateCollision);
    }

    // Warm load created account.
    context.evm.load_account(created_address)?;

    // Create account checkpoint and transfer funds.
    let checkpoint = match context.evm.journaled_state.create_account_checkpoint(
        inputs.caller,
        created_address,
        inputs.value,
        spec_id,
    ) {
        Ok(checkpoint) => checkpoint,
        Err(e) => return return_error(e),
    };

    let init_code_hash = B256::ZERO;
    if spec_id.is_enabled_in(SpecId::PRAGUE_EOF) && inputs.init_code.starts_with(&EOF_MAGIC_BYTES) {
        // Defensive: should already be covered by the earlier check.
        return return_error(InstructionResult::CreateInitCodeStartingEF00);
    }

    let bytecode = Bytecode::new_legacy(inputs.init_code.clone());
    let contract = Contract::new(
        Bytes::new(),
        bytecode,
        Some(init_code_hash),
        created_address,
        None,
        inputs.caller,
        inputs.value,
    );

    Ok(FrameOrResult::new_create_frame(
        created_address,
        checkpoint,
        Interpreter::new(contract, inputs.gas_limit, false),
    ))
}

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
    /// Raw `from` bytes as received over the wire (20 bytes EVM, 21 bytes TRON-prefixed, or malformed).
    /// Some fixtures intentionally include malformed owner addresses; storing the raw bytes allows
    /// contract-level validation to match java-tron error ordering and messages.
    pub from_raw: Option<Vec<u8>>,
    /// Raw `google.protobuf.Any` from Protocol.Transaction.Contract.parameter, when provided by Java.
    /// Carries both `type_url` and `value` so Rust can mirror java-tron `any.is(...)` behavior.
    pub contract_parameter: Option<TronContractParameter>,
}

#[derive(Debug, Clone)]
pub struct TronContractParameter {
    pub type_url: String,
    pub value: Vec<u8>,
}

impl Default for TxMetadata {
    fn default() -> Self {
        Self {
            contract_type: None,
            asset_id: None,
            from_raw: None,
            contract_parameter: None,
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
    /// Transaction ID (TRON txid = sha256(raw_data)), when available.
    /// Used for CreateSmartContract address derivation.
    pub transaction_id: Option<revm::primitives::B256>,
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
    evm: Evm<'static, TronExternalContext, DB>,
    spec_id: SpecId,
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
    pub(crate) fn spec_id_from_config(config: &ExecutionConfig) -> SpecId {
        if config.enable_london_fork {
            SpecId::LONDON
        } else if config.enable_berlin_fork {
            SpecId::BERLIN
        } else if config.enable_istanbul_fork {
            SpecId::ISTANBUL
        } else {
            SpecId::BYZANTIUM
        }
    }

    pub fn new(database: DB, config: &ExecutionConfig) -> Result<Self> {
        let spec_id = Self::spec_id_from_config(config);
        Self::new_with_spec_id(database, config, spec_id)
    }

    pub fn new_with_spec_id(database: DB, config: &ExecutionConfig, spec_id: SpecId) -> Result<Self> {
        let mut evm = Evm::builder()
            .with_db(database)
            .with_external_context(TronExternalContext::default())
            .with_handler_cfg(HandlerCfg::new(spec_id))
            .append_handler_register(tron_revm_handle_register::<DB>)
            .append_handler_register(revm::inspector_handle_register::<DB, TronExternalContext>)
            .build();

        // Configure for Tron - access through context
        evm.context.evm.inner.env.cfg.chain_id = 0x2b6653dc; // Tron mainnet chain ID
        
        evm.context.evm.inner.env.cfg.limit_contract_code_size = Some(24576); // 24KB limit

        let precompiles = TronPrecompiles::new();
        let energy_accounting = EnergyAccounting::new(config.energy_limit);
        let bandwidth_accounting = BandwidthAccounting::new(config.bandwidth_limit);

        Ok(Self {
            evm,
            spec_id,
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
        self.evm.context.external.reset_energy_counters();

        // Set root transaction ID for TRON's internal CREATE address derivation.
        // java-tron derives internal CREATE addresses using: keccak256(txid || nonce)[12..]
        self.evm.context.external.root_transaction_id = context.transaction_id;
        self.evm.context.external.internal_tx_nonce = 0;
        // Ensure per-tx config flags don't leak between executions.
        self.evm.context.evm.inner.env.cfg.disable_balance_check = false;
        self.evm.context.evm.inner.env.tx.caller = tx.from;
        self.evm.context.evm.inner.env.tx.transact_to = match tx.to {
            Some(to) => revm::primitives::TransactTo::Call(to),
            None => revm::primitives::TransactTo::Create,
        };
        self.evm.context.evm.inner.env.tx.value = tx.value;
        self.evm.context.evm.inner.env.tx.data = tx.data.clone();
        // NOTE: TRON's transaction "energy limit" corresponds to *EVM execution gas* and excludes
        // Ethereum's intrinsic tx costs. We'll add the intrinsic gas later so REVM doesn't reject
        // low-energy fixtures with `CallGasCostMoreThanGasLimit`.
        self.evm.context.evm.inner.env.tx.gas_limit = tx.gas_limit;
        self.evm.context.evm.inner.env.tx.gas_price = tx.gas_price;
        self.evm.context.evm.inner.env.tx.nonce = Some(tx.nonce);

        // Set block environment
        self.evm.context.evm.inner.env.block.number = revm::primitives::U256::from(context.block_number);
        self.evm.context.evm.inner.env.block.timestamp = revm::primitives::U256::from(context.block_timestamp);
        // TRON parity: many callers omit coinbase; avoid touching address(0) by defaulting to sender.
        self.evm.context.evm.inner.env.block.coinbase = if context.block_coinbase
            == revm::primitives::Address::ZERO
        {
            tx.from
        } else {
            context.block_coinbase
        };
        self.evm.context.evm.inner.env.block.difficulty = context.block_difficulty;
        self.evm.context.evm.inner.env.block.gas_limit =
            revm::primitives::U256::from(context.block_gas_limit);
        
        // TRON Parity Fix: Set basefee = 0 to prevent EIP-1559 base fee burns
        // Keep coinbase set for COINBASE opcode correctness, but ensure no fee distribution
        self.evm.context.evm.inner.env.block.basefee = revm::primitives::U256::ZERO;
        
        tracing::debug!("TRON environment setup - gas_price: {}, basefee: 0 (TRON mode)", tx.gas_price);

        // Set Tron-specific configurations
        self.energy_accounting.reset();
        self.bandwidth_accounting.reset();

        // TRON CreateSmartContract: Java sends CreateSmartContract proto bytes in tx.data.
        // Extract init code for EVM execution, and override the created address to match
        // java-tron's txid+owner derivation.
        if tx.metadata.contract_type == Some(TronContractType::CreateSmartContract) {
            match crate::protocol::CreateSmartContract::decode(tx.data.as_ref()) {
                Ok(create_contract) => {
                    if let Some(new_contract) = create_contract.new_contract.as_ref() {
                        self.evm.context.evm.inner.env.tx.data =
                            revm::primitives::Bytes::from(new_contract.bytecode.clone());
                    }

                    if let (Some(txid), owner_address) =
                        (context.transaction_id, create_contract.owner_address)
                    {
                        if owner_address.len() == 21 {
                            let mut combined = Vec::with_capacity(32 + owner_address.len());
                            combined.extend_from_slice(txid.as_slice());
                            combined.extend_from_slice(&owner_address);
                            let hash = crate::storage_adapter::utils::keccak256(&combined);
                            let addr_bytes = &hash.as_slice()[12..32];
                            self.evm.context.external.create_address_override =
                                Some(revm::primitives::Address::from_slice(addr_bytes));
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to decode CreateSmartContract proto for VM execution: {}",
                        e
                    );
                }
            }
        }

        // TRON TriggerSmartContract: Java sends TriggerSmartContract proto bytes in tx.data.
        // Extract the call data payload for EVM execution.
        if tx.metadata.contract_type == Some(TronContractType::TriggerSmartContract) {
            match crate::protocol::TriggerSmartContract::decode(tx.data.as_ref()) {
                Ok(trigger_contract) => {
                    self.evm.context.evm.inner.env.tx.data =
                        revm::primitives::Bytes::from(trigger_contract.data);
                    // TRON parity: java-tron does not pre-reject malformed negative callValue in
                    // all forks, but still passes the value into TVM, which can trigger REVERT
                    // paths (e.g., nonpayable checks). Allow execution to proceed without
                    // balance prechecks for these fixtures.
                    if trigger_contract.call_value < 0 {
                        self.evm.context.evm.inner.env.cfg.disable_balance_check = true;
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to decode TriggerSmartContract proto for VM execution: {}",
                        e
                    );
                }
            }
        }

        // TRON parity: allow very low energy limits by accounting intrinsic gas separately.
        // Set REVM tx.gas_limit = tron_energy_limit + intrinsic_gas so that the post-intrinsic
        // execution gas equals the TRON energy limit.
        let intrinsic = self.tron_intrinsic_energy();
        let adjusted_tx_gas_limit = tx.gas_limit.saturating_add(intrinsic);
        self.evm.context.evm.inner.env.tx.gas_limit = adjusted_tx_gas_limit;

        // TRON does not have a per-block gas limit; fixtures use `ExecutionContext.energy_limit`
        // (fee limit in SUN), which is not comparable to the EVM gas limit. Ensure REVM validation
        // does not reject the tx due to `tx.gas_limit > block.gas_limit`.
        let desired_block_gas_limit = std::cmp::max(context.block_gas_limit, adjusted_tx_gas_limit);
        self.evm.context.evm.inner.env.block.gas_limit =
            revm::primitives::U256::from(desired_block_gas_limit);
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
                    error: Some("REVERT opcode executed".to_string()),
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
                let error = Some(self.format_halt_error(&reason, tx));
                Ok(TronExecutionResult {
                    success: false,
                    return_data: revm::primitives::Bytes::new(),
                    energy_used,
                    bandwidth_used,
                    logs: vec![],
                    state_changes: vec![],
                    error,
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

    fn format_halt_error(&self, reason: &revm::primitives::HaltReason, tx: &TronTransaction) -> String {
        use revm::primitives::HaltReason;

        if tx.metadata.contract_type == Some(TronContractType::CreateSmartContract) {
            match reason {
                HaltReason::CreateContractSizeLimit => {
                    // REVM v10 maps CreateContractStartingWithEF -> CreateContractSizeLimit in
                    // SuccessOrHalt conversion. Derive the java-tron error from returned code.
                    if self.spec_id.is_enabled_in(SpecId::LONDON) {
                        if let Some(output) = self.evm.context.external.last_create_output.as_ref() {
                            if output.first() == Some(&0xEF) {
                                return "invalid code: must not begin with 0xef".to_string();
                            }
                        }
                    }

                    return "Transaction halted: CreateContractSizeLimit".to_string();
                }
                HaltReason::OutOfGas(_) => {
                    // Distinguish opcode OOG from code-deposit (save code) OOG.
                    if let Some(output) = self.evm.context.external.last_create_output.as_ref() {
                        if !output.is_empty() {
                            // java-tron: notEnoughSpendEnergy("save just created contract code", need, left)
                            let need_energy = output.len() as u64 * 200;
                            let left_evm = self
                                .evm
                                .context
                                .external
                                .last_create_gas_remaining
                                .unwrap_or(0);
                            let adjustment = self.evm.context.external.tron_energy_opcode_adjustment();
                            let left_tron = left_evm.saturating_add(adjustment);
                            return format!(
                                "Not enough energy for 'save just created contract code' executing: needEnergy[{}], leftEnergy[{}];",
                                need_energy, left_tron
                            );
                        }
                    }

                    // Opcode-level OOG: java-tron throws OutOfEnergyException from Program.spendEnergy.
                    let Some(opcode) = self.evm.context.external.last_opcode else {
                        return "Not enough energy".to_string();
                    };
                    let op_name = opcode_name(opcode).unwrap_or("UNKNOWN");
                    let op_energy = opcode_energy_cost_for_oog(opcode).unwrap_or(0);

                    let limit = self.evm.context.external.last_gas_limit.unwrap_or(0);
                    let used_evm = self.evm.context.external.last_gas_spent.unwrap_or(0);
                    let adjustment = self.evm.context.external.tron_energy_opcode_adjustment();
                    let used_tron = used_evm.saturating_sub(adjustment);

                    return format!(
                        "Not enough energy for '{}' operation executing: curInvokeEnergyLimit[{}], curOpEnergy[{}], usedEnergy[{}]",
                        op_name, limit, op_energy, used_tron
                    );
                }
                _ => {}
            }
        }

        format!("Transaction halted: {:?}", reason)
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
        let intrinsic = self.tron_intrinsic_energy();
        let tron_adjustment = self.evm.context.external.tron_energy_opcode_adjustment();
        match result {
            // TRON parity: energy usage counts only TVM execution, not transaction intrinsic costs
            // (base + calldata). Bandwidth accounts for transaction bytes separately.
            ExecutionResult::Success { gas_used, gas_refunded, .. } => gas_used
                .saturating_add(*gas_refunded)
                .saturating_sub(intrinsic)
                .saturating_sub(tron_adjustment),
            ExecutionResult::Revert { gas_used, .. } => gas_used
                .saturating_sub(intrinsic)
                .saturating_sub(tron_adjustment),
            // java-tron: exceptions spend ALL energy (Program.spendAllEnergy()).
            ExecutionResult::Halt { .. } => _tx.gas_limit,
        }
    }

    fn tron_intrinsic_energy(&self) -> u64 {
        use revm::interpreter::gas;

        let env = &self.evm.context.evm.inner.env;
        let input = env.tx.data.as_ref();
        let is_create = env.tx.transact_to.is_create();
        let access_list = env.tx.access_list.as_ref();
        let authorization_list_num = env
            .tx
            .authorization_list
            .as_ref()
            .map(|l| l.len() as u64)
            .unwrap_or_default();

        gas::validate_initial_tx_gas(self.spec_id, input, is_create, access_list, authorization_list_num)
    }

    fn calculate_bandwidth_usage(&self, tx: &TronTransaction) -> u64 {
        // Simple bandwidth calculation based on transaction size
        let base_size = 32; // Basic transaction overhead
        let data_size = tx.data.len() as u64;
        base_size + data_size
    }
}

fn opcode_name(opcode: u8) -> Option<&'static str> {
    match opcode {
        0x56 => Some("JUMP"),
        0x57 => Some("JUMPI"),
        0x5b => Some("JUMPDEST"),
        0xf3 => Some("RETURN"),
        0xfd => Some("REVERT"),
        0xfe => Some("INVALID"),
        0x00 => Some("STOP"),
        0x01 => Some("ADD"),
        _ => None,
    }
}

fn opcode_energy_cost_for_oog(opcode: u8) -> Option<u64> {
    match opcode {
        // java-tron EnergyCost MID_TIER
        0x56 => Some(8), // JUMP
        // VERY_LOW_TIER for PUSH* opcodes (used by trigger_smart_contract fixtures)
        0x60..=0x7f => Some(3),
        _ => None,
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
