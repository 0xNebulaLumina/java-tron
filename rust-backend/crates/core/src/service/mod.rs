use std::collections::HashMap;
use std::time::SystemTime;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};

use crate::backend::*;
use num_bigint::{BigInt, Sign};
use revm_primitives::hex;
use revm_primitives::AccountInfo;
use tron_backend_common::{from_tron_address, HealthStatus, ModuleManager};
use tron_backend_execution::{
    EvmStateStore, ExecutionModule, TronExecutionContext, TronExecutionResult, TronStateChange,
    TronTransaction,
};

// Module declarations
pub mod contracts;
pub mod grpc;

// Import utilities from submodules
use contracts::proto::read_varint;
use contracts::proto::{
    read_length_delimited_typed, read_tag_typed, read_varint_typed, skip_protobuf_field_checked,
    MarketOrderDetail, ProtobufError, TransactionResultBuilder,
};
use grpc::address::{
    add_tron_address_prefix, add_tron_address_prefix_with, validate_tron_address_prefix,
};

/// Vote witness contract constants
const MAX_VOTE_NUMBER: usize = 30;
const TRX_PRECISION: u64 = 1_000_000; // 1 TRX = 1,000,000 SUN

pub struct BackendService {
    module_manager: ModuleManager,
    start_time: SystemTime,
}

impl BackendService {
    fn any_type_url_matches(type_url: &str, expected_full_name: &str) -> bool {
        if type_url == expected_full_name {
            return true;
        }
        match type_url.rsplit_once('/') {
            Some((_, tail)) => tail == expected_full_name,
            None => false,
        }
    }

    /// Java error constant: `ActuatorConstant.CONTRACT_NOT_EXIST = "No contract!"`
    const CONTRACT_NOT_EXIST: &'static str = "No contract!";

    /// Require the `contract_parameter` (protobuf `Any`) to be present.
    ///
    /// Returns the `Any` on success, or `missing_error` (typically
    /// [`Self::CONTRACT_NOT_EXIST`]) when the field is absent.
    ///
    /// Java equivalent: the `if (this.any == null)` guard at the top of every
    /// actuator's `validate()` method.
    fn require_contract_any<'a>(
        transaction: &'a TronTransaction,
        missing_error: &str,
    ) -> Result<&'a tron_backend_execution::TronContractParameter, String> {
        transaction
            .metadata
            .contract_parameter
            .as_ref()
            .ok_or_else(|| missing_error.to_string())
    }

    /// Require the `Any`'s `type_url` to match `expected_proto_type`.
    ///
    /// Returns the raw `value` bytes on success, or `type_mismatch_error` when
    /// the URL is empty or does not match.
    ///
    /// Java equivalent: the `if (!any.is(ExpectedContract.class))` check that
    /// follows the null guard.
    fn require_contract_type<'a>(
        any: &'a tron_backend_execution::TronContractParameter,
        expected_proto_type: &str,
        type_mismatch_error: &str,
    ) -> Result<&'a [u8], String> {
        if any.type_url.is_empty()
            || !Self::any_type_url_matches(&any.type_url, expected_proto_type)
        {
            return Err(type_mismatch_error.to_string());
        }
        Ok(&any.value)
    }

    /// Convenience: require `contract_parameter` present AND matching type.
    ///
    /// Preserves Java ordering: missing Any → `missing_error` first,
    /// wrong type → `type_mismatch_error` second.
    fn require_contract_parameter<'a>(
        transaction: &'a TronTransaction,
        expected_proto_type: &str,
        missing_error: &str,
        type_mismatch_error: &str,
    ) -> Result<&'a [u8], String> {
        let any = Self::require_contract_any(transaction, missing_error)?;
        Self::require_contract_type(any, expected_proto_type, type_mismatch_error)
    }

    /// Resolve the owner address from the parsed contract field, falling back
    /// to `from_raw` when the contract field is empty.
    fn resolve_owner_address<'a>(
        owner_from_contract: &'a [u8],
        from_raw: Option<&'a [u8]>,
    ) -> &'a [u8] {
        if !owner_from_contract.is_empty() {
            owner_from_contract
        } else {
            from_raw.unwrap_or(&[])
        }
    }

    /// Parse `WitnessCreateContract` protobuf to extract the URL.
    ///
    /// ```protobuf
    /// message WitnessCreateContract {
    ///   bytes owner_address = 1;  // wire type 2
    ///   bytes url           = 2;  // wire type 2
    /// }
    /// ```
    fn parse_witness_create_url(data: &[u8]) -> Result<Vec<u8>, String> {
        let mut url: Option<Vec<u8>> = None;
        let mut pos = 0;
        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;
            match (field_number, wire_type) {
                (2, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    url = Some(payload.to_vec());
                    pos += total_len;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }
        // Proto3 default: missing field → empty bytes
        Ok(url.unwrap_or_default())
    }

    /// Parse `WitnessUpdateContract` protobuf to extract the update URL and
    /// validate decodability (Java parity: `any.unpack()` fails on malformed
    /// protobuf).
    ///
    /// ```protobuf
    /// message WitnessUpdateContract {
    ///   bytes owner_address = 1;   // wire type 2
    ///   bytes update_url   = 12;   // wire type 2
    /// }
    /// ```
    fn parse_witness_update_url(data: &[u8]) -> Result<Vec<u8>, String> {
        let mut update_url: Option<Vec<u8>> = None;
        let mut pos = 0;
        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;
            match (field_number, wire_type) {
                (12, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    update_url = Some(payload.to_vec());
                    pos += total_len;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }
        Ok(update_url.unwrap_or_default())
    }

    /// Parse `asset_name` (field 1, length-delimited) from a serialized
    /// `TransferAssetContract` protobuf message.
    ///
    /// Proto definition (asset_issue_contract.proto):
    ///   bytes asset_name = 1;
    ///   bytes owner_address = 2;
    ///   bytes to_address = 3;
    ///   int64 amount = 4;
    ///
    /// Wire format: tag = (field_number << 3) | wire_type
    /// Field 1, wire type 2 (length-delimited) → tag byte = 0x0a
    fn parse_asset_name_from_transfer_asset_contract(data: &[u8]) -> Option<Vec<u8>> {
        let mut pos = 0;
        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..]).ok()?;
            pos += tag_len;

            match wire_type {
                0 => {
                    // Varint: skip
                    let (_, vr) = read_varint_typed(&data[pos..]).ok()?;
                    pos += vr;
                }
                2 => {
                    // Length-delimited
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..]).ok()?;
                    if field_number == 1 {
                        // asset_name
                        return Some(payload.to_vec());
                    }
                    pos += total_len;
                }
                5 => {
                    // 32-bit fixed
                    pos += 4;
                }
                1 => {
                    // 64-bit fixed
                    pos += 8;
                }
                _ => return None,
            }
        }
        None
    }

    pub fn new(module_manager: ModuleManager) -> Self {
        Self {
            module_manager,
            start_time: SystemTime::now(),
        }
    }

    fn get_storage_module(&self) -> Result<&Box<dyn tron_backend_common::Module>, Status> {
        self.module_manager
            .get("storage")
            .ok_or_else(|| Status::unavailable("Storage module not available"))
    }

    fn get_execution_module(&self) -> Result<&Box<dyn tron_backend_common::Module>, Status> {
        self.module_manager
            .get("execution")
            .ok_or_else(|| Status::unavailable("Execution module not available"))
    }

    fn get_storage_engine(&self) -> Result<&tron_backend_storage::StorageEngine, Status> {
        let storage_module = self.get_storage_module()?;

        // Downcast to the concrete storage module type
        let storage_module = storage_module
            .as_any()
            .downcast_ref::<tron_backend_storage::StorageModule>()
            .ok_or_else(|| Status::internal("Failed to downcast storage module"))?;

        storage_module
            .engine()
            .map_err(|e| Status::internal(format!("Storage engine not available: {}", e)))
    }

    fn get_execution_config(&self) -> Result<&tron_backend_common::ExecutionConfig, String> {
        let execution_module = self
            .get_execution_module()
            .map_err(|e| format!("Failed to get execution module: {}", e))?;

        // Downcast to the concrete execution module type
        let execution_module = execution_module
            .as_any()
            .downcast_ref::<ExecutionModule>()
            .ok_or_else(|| "Failed to downcast execution module".to_string())?;

        // Access the config field (we need to add a getter method)
        execution_module.get_config()
    }

    /// Detect if a transaction is likely a non-VM transaction based on heuristics
    fn is_likely_non_vm_transaction(
        &self,
        tx: &TronTransaction,
        storage_adapter: &tron_backend_execution::EngineBackedEvmStateStore,
    ) -> bool {
        // Non-VM heuristic: empty data AND to address has no code
        if !tx.data.is_empty() {
            return false; // Has data, likely VM transaction
        }

        if tx.to.is_none() {
            return false; // Contract creation, definitely VM transaction
        }

        let to_address = tx.to.unwrap();

        // Check if the 'to' address has code (making it a contract)
        // We'll query the 'code' database to see if there's code at this address
        match storage_adapter.get_code(&to_address) {
            Ok(Some(code)) => {
                if code.is_empty() {
                    // No code = EOA (Externally Owned Account), likely non-VM transaction
                    true
                } else {
                    // Has code = Contract, VM transaction
                    false
                }
            }
            Ok(None) => {
                // No code entry = new or EOA account, likely non-VM transaction
                true
            }
            Err(_) => {
                // Error accessing code, be conservative and assume VM transaction
                false
            }
        }
    }

    /// Apply TRON fee policy post-processing to execution results
    fn apply_fee_post_processing(
        &self,
        result: &mut TronExecutionResult,
        _tx: &TronTransaction,
        context: &TronExecutionContext,
        is_non_vm: bool,
    ) -> Result<(), String> {
        let execution_config = self.get_execution_config()?;
        let fee_config = &execution_config.fees;

        // Only apply fee post-processing in blackhole mode
        if fee_config.mode != "blackhole" {
            debug!(
                "Fee mode is '{}', skipping fee post-processing",
                fee_config.mode
            );
            return Ok(());
        }

        // Validate blackhole address if required
        if fee_config.blackhole_address_base58.is_empty() {
            warn!("Fee mode is 'blackhole' but no blackhole address configured, skipping fee emission");
            return Ok(());
        }

        // Parse blackhole address
        let blackhole_evm_address = match from_tron_address(&fee_config.blackhole_address_base58) {
            Ok(addr) => revm_primitives::Address::from_slice(&addr),
            Err(e) => {
                warn!(
                    "Invalid blackhole address '{}': {}, skipping fee emission",
                    fee_config.blackhole_address_base58, e
                );
                return Ok(());
            }
        };

        let mut fee_amount = 0u64;
        let mut should_emit = false;

        if is_non_vm {
            // Non-VM transaction fee handling
            if let Some(flat_fee) = fee_config.non_vm_blackhole_credit_flat {
                fee_amount = flat_fee;
                should_emit = true;
                debug!(
                    "Non-VM transaction: will emit flat fee {} SUN to blackhole",
                    flat_fee
                );
            } else {
                debug!("Non-VM transaction: no flat fee configured, skipping fee emission");
            }
        } else {
            // VM transaction fee handling
            if fee_config.experimental_vm_blackhole_credit {
                // Approximate fee calculation: energy_used * energy_price
                fee_amount = result.energy_used * context.energy_price;
                should_emit = true;
                debug!(
                    "VM transaction: will emit estimated fee {} SUN ({}*{}) to blackhole",
                    fee_amount, result.energy_used, context.energy_price
                );
            } else {
                debug!("VM transaction: experimental_vm_blackhole_credit disabled, skipping fee emission");
            }
        }

        if should_emit && fee_amount > 0 {
            // Create synthetic AccountChange for blackhole credit
            let blackhole_change = TronStateChange::AccountChange {
                address: blackhole_evm_address,
                old_account: None, // We don't know the old state, this is a synthetic credit
                new_account: Some(revm_primitives::AccountInfo {
                    balance: revm_primitives::U256::from(fee_amount), // This would be added to existing balance
                    nonce: 0,
                    code_hash: revm_primitives::B256::ZERO,
                    code: None,
                }),
            };

            result.state_changes.push(blackhole_change);
            debug!(
                "Added synthetic blackhole fee credit: {} SUN to address {:?}",
                fee_amount, blackhole_evm_address
            );

            // Log warning about approximation
            warn!("Emitted synthetic fee credit to blackhole (approximation until Phase 3)");
        }

        Ok(())
    }

    /// Phase 2.I L2: Persist SmartContract metadata after successful EVM contract creation
    /// Parses CreateSmartContract proto from transaction data and stores SmartContract to ContractStore
    pub(crate) fn persist_smart_contract_metadata(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
        created_address: &revm_primitives::Address,
    ) -> Result<(), String> {
        use prost::Message;
        use tron_backend_execution::protocol::{
            AccountType as ProtoAccountType, CreateSmartContract,
        };

        info!(
            "Phase 2.I L2: Persisting SmartContract metadata for contract at {:?}",
            created_address
        );

        // Parse CreateSmartContract proto from transaction data
        let create_contract = CreateSmartContract::decode(transaction.data.as_ref())
            .map_err(|e| format!("Failed to parse CreateSmartContract proto: {}", e))?;

        let new_contract = create_contract
            .new_contract
            .ok_or_else(|| "CreateSmartContract.new_contract is missing".to_string())?;

        // Build the SmartContract proto with all metadata
        let mut smart_contract = new_contract.clone();

        // Java VMActuator.create() forces version = 1 for newly created contracts
        // when ALLOW_TVM_COMPATIBLE_EVM is enabled. This affects downstream behavior
        // (e.g., TransferContract rejects transfers to version=1 contracts).
        let allow_tvm_compatible_evm = storage_adapter
            .get_allow_tvm_compatible_evm()
            .map_err(|e| format!("Failed to get ALLOW_TVM_COMPATIBLE_EVM: {}", e))?;
        if allow_tvm_compatible_evm == 1 {
            smart_contract.version = 1;
            debug!("Setting SmartContract.version = 1 (ALLOW_TVM_COMPATIBLE_EVM enabled)");
        }

        // Set the contract_address to the EVM-created address (21-byte TRON format)
        let tron_address = storage_adapter.to_tron_address_21(created_address).to_vec();
        smart_contract.contract_address = tron_address.clone();

        // Set origin_address from the owner (21-byte TRON format)
        if create_contract.owner_address.len() == 20 {
            let origin_evm = revm_primitives::Address::from_slice(&create_contract.owner_address);
            smart_contract.origin_address =
                storage_adapter.to_tron_address_21(&origin_evm).to_vec();
        } else {
            // Already 21-byte format
            smart_contract.origin_address = create_contract.owner_address.clone();
        }

        // Compute code_hash if not set
        if smart_contract.code_hash.is_empty() {
            use sha3::{Digest, Keccak256};

            // java-tron stores SmartContract.code_hash as keccak256(runtime_code) (CodeStore),
            // not keccak256(deployment_bytecode) (SmartContract.bytecode).
            let runtime_code = storage_adapter
                .get_code(created_address)
                .map_err(|e| format!("Failed to read contract runtime code: {}", e))?
                .map(|code| code.original_byte_slice().to_vec())
                .unwrap_or_default();

            let mut hasher = Keccak256::new();
            hasher.update(&runtime_code);
            smart_contract.code_hash = hasher.finalize().to_vec();
        }

        // ContractStore stores SmartContract metadata WITHOUT ABI; ABI is stored separately in AbiStore.
        let abi_to_store = smart_contract.abi.clone();
        smart_contract.abi = None;

        // Persist to ContractStore
        storage_adapter
            .put_smart_contract(&smart_contract)
            .map_err(|e| format!("Failed to persist SmartContract to ContractStore: {}", e))?;

        info!("Successfully persisted SmartContract: name='{}', version={}, origin_energy_limit={}, consume_user_resource_percent={}",
              smart_contract.name, smart_contract.version, smart_contract.origin_energy_limit, smart_contract.consume_user_resource_percent);

        // Ensure AccountStore entry for the contract has the correct type/name (Contract account).
        let mut contract_account = storage_adapter
            .get_account_proto(created_address)
            .map_err(|e| format!("Failed to load contract Account proto: {}", e))?
            .unwrap_or_default();
        contract_account.address = tron_address.clone();
        contract_account.account_name = smart_contract.name.as_bytes().to_vec();
        contract_account.r#type = ProtoAccountType::Contract as i32;
        storage_adapter
            .put_account_proto(created_address, &contract_account)
            .map_err(|e| format!("Failed to persist contract Account proto: {}", e))?;

        // Persist ABI if present
        if let Some(ref abi) = abi_to_store {
            // java-tron stores an ABI key even when the ABI message is empty (serializes to 0 bytes).
            storage_adapter
                .put_abi(&tron_address, abi)
                .map_err(|e| format!("Failed to persist ABI: {}", e))?;
            info!("Persisted ABI with {} entries", abi.entrys.len());
        }

        Ok(())
    }

    /// Extract TRC-10 call_token_value transfer for CreateSmartContract
    ///
    /// When ALLOW_TVM_TRANSFER_TRC10 is enabled and call_token_value > 0,
    /// Java's VMActuator transfers TRC-10 tokens from owner to contract before execution.
    /// This function returns the corresponding Trc10Change to be emitted on success.
    ///
    /// Returns: Some(Trc10Change) if TRC-10 transfer is needed, None otherwise.
    pub(crate) fn extract_create_contract_trc10_transfer(
        &self,
        storage_adapter: &tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        created_address: &revm_primitives::Address,
    ) -> Result<Option<tron_backend_execution::Trc10Change>, String> {
        use prost::Message;
        use tron_backend_execution::protocol::CreateSmartContract;

        // Check if TRC-10 transfers are enabled
        let allow_tvm_transfer_trc10 = storage_adapter
            .tron_dynamic_property_i64(b"ALLOW_TVM_TRANSFER_TRC10")
            .map_err(|e| format!("Failed to get ALLOW_TVM_TRANSFER_TRC10: {}", e))?
            .unwrap_or(0);

        if allow_tvm_transfer_trc10 == 0 {
            return Ok(None);
        }

        // Parse CreateSmartContract proto
        let create_contract = CreateSmartContract::decode(transaction.data.as_ref())
            .map_err(|e| format!("Failed to parse CreateSmartContract proto: {}", e))?;

        let call_token_value = create_contract.call_token_value;
        let token_id = create_contract.token_id;

        // No transfer needed if token_value is 0 or negative
        if call_token_value <= 0 {
            return Ok(None);
        }

        // Build the TRC-10 transfer change
        let owner_address = if create_contract.owner_address.len() == 21 {
            // TRON 21-byte format - strip the 0x41 prefix
            revm_primitives::Address::from_slice(&create_contract.owner_address[1..])
        } else if create_contract.owner_address.len() == 20 {
            revm_primitives::Address::from_slice(&create_contract.owner_address)
        } else {
            return Err(format!(
                "Invalid owner_address length: {}",
                create_contract.owner_address.len()
            ));
        };

        let token_id_str = token_id.to_string();

        let trc10_change = tron_backend_execution::Trc10Change::AssetTransferred(
            tron_backend_execution::Trc10AssetTransferred {
                owner_address,
                to_address: *created_address,
                asset_name: token_id_str.as_bytes().to_vec(),
                token_id: Some(token_id_str),
                amount: call_token_value,
            },
        );

        info!(
            "CreateSmartContract TRC-10 transfer: owner={:?} -> contract={:?}, token_id={}, amount={}",
            owner_address, created_address, token_id, call_token_value
        );

        Ok(Some(trc10_change))
    }

    /// Execute a non-VM transaction with contract type dispatch
    /// Routes to specific handlers based on TRON contract type
    pub(crate) fn execute_non_vm_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        // Legacy compatibility: unwrap Any wrapper from tx.data if present.
        // All handlers now parse from contract_parameter.value directly, so this
        // is only relevant for backward-compatible test paths.
        let mut tx = transaction.clone();
        if let Ok(inner) = Self::unwrap_any_value_if_present(tx.data.as_ref()) {
            tx.data = revm_primitives::Bytes::from(inner);
        }
        let transaction = &tx;

        debug!(
            "Executing non-VM contract: from={:?}, to={:?}, value={}, contract_type={:?}",
            transaction.from, transaction.to, transaction.value, transaction.metadata.contract_type
        );

        // Get execution configuration to check feature flags
        let execution_config = self.get_execution_config()?;
        let remote_config = &execution_config.remote;

        // Check global remote system contract enablement
        if !remote_config.system_enabled {
            return Err("Remote system contract execution is disabled".to_string());
        }

        // Branch execution based on contract type with feature flag checks
        match transaction.metadata.contract_type {
            Some(tron_backend_execution::TronContractType::TransferContract) => {
                debug!("Executing TRANSFER_CONTRACT");
                self.execute_transfer_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::WitnessCreateContract) => {
                if !remote_config.witness_create_enabled {
                    return Err(
                        "WITNESS_CREATE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing WITNESS_CREATE_CONTRACT");
                self.execute_witness_create_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::WitnessUpdateContract) => {
                if !remote_config.witness_update_enabled {
                    return Err(
                        "WITNESS_UPDATE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing WITNESS_UPDATE_CONTRACT");
                self.execute_witness_update_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::VoteWitnessContract) => {
                if !remote_config.vote_witness_enabled {
                    return Err(
                        "VOTE_WITNESS_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing VOTE_WITNESS_CONTRACT");
                self.execute_vote_witness_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::TransferAssetContract) => {
                if !remote_config.trc10_enabled {
                    return Err("TRC-10 transfers are disabled - falling back to Java".to_string());
                }
                debug!("Executing TRANSFER_ASSET_CONTRACT (TRC-10)");
                self.execute_trc10_transfer_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::AssetIssueContract) => {
                if !remote_config.trc10_enabled {
                    return Err(
                        "ASSET_ISSUE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing ASSET_ISSUE_CONTRACT");
                self.execute_asset_issue_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::AccountUpdateContract) => {
                debug!("Executing ACCOUNT_UPDATE_CONTRACT");
                self.execute_account_update_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::FreezeBalanceContract) => {
                if !remote_config.freeze_balance_enabled {
                    return Err(
                        "FREEZE_BALANCE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing FREEZE_BALANCE_CONTRACT");
                self.execute_freeze_balance_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::UnfreezeBalanceContract) => {
                if !remote_config.unfreeze_balance_enabled {
                    return Err(
                        "UNFREEZE_BALANCE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing UNFREEZE_BALANCE_CONTRACT");
                self.execute_unfreeze_balance_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::FreezeBalanceV2Contract) => {
                if !remote_config.freeze_balance_v2_enabled {
                    return Err(
                        "FREEZE_BALANCE_V2_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing FREEZE_BALANCE_V2_CONTRACT");
                self.execute_freeze_balance_v2_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract) => {
                if !remote_config.unfreeze_balance_v2_enabled {
                    return Err(
                        "UNFREEZE_BALANCE_V2_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing UNFREEZE_BALANCE_V2_CONTRACT");
                self.execute_unfreeze_balance_v2_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::WithdrawBalanceContract) => {
                if !remote_config.withdraw_balance_enabled {
                    return Err(
                        "WITHDRAW_BALANCE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing WITHDRAW_BALANCE_CONTRACT");
                self.execute_withdraw_balance_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::AccountCreateContract) => {
                if !remote_config.account_create_enabled {
                    return Err(
                        "ACCOUNT_CREATE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing ACCOUNT_CREATE_CONTRACT");
                self.execute_account_create_contract(storage_adapter, transaction, context)
            }
            // Phase 2.A: Proposal contracts (16/17/18)
            Some(tron_backend_execution::TronContractType::ProposalCreateContract) => {
                if !remote_config.proposal_create_enabled {
                    return Err(
                        "PROPOSAL_CREATE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing PROPOSAL_CREATE_CONTRACT");
                self.execute_proposal_create_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::ProposalApproveContract) => {
                if !remote_config.proposal_approve_enabled {
                    return Err(
                        "PROPOSAL_APPROVE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing PROPOSAL_APPROVE_CONTRACT");
                self.execute_proposal_approve_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::ProposalDeleteContract) => {
                if !remote_config.proposal_delete_enabled {
                    return Err(
                        "PROPOSAL_DELETE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing PROPOSAL_DELETE_CONTRACT");
                self.execute_proposal_delete_contract(storage_adapter, transaction, context)
            }
            // Phase 2.B: Account management contracts (19/46)
            Some(tron_backend_execution::TronContractType::SetAccountIdContract) => {
                if !remote_config.set_account_id_enabled {
                    return Err(
                        "SET_ACCOUNT_ID_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing SET_ACCOUNT_ID_CONTRACT");
                self.execute_set_account_id_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::AccountPermissionUpdateContract) => {
                if !remote_config.account_permission_update_enabled {
                    return Err("ACCOUNT_PERMISSION_UPDATE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing ACCOUNT_PERMISSION_UPDATE_CONTRACT");
                self.execute_account_permission_update_contract(
                    storage_adapter,
                    transaction,
                    context,
                )
            }
            // Phase 2.C: Contract metadata contracts (33/45/48)
            Some(tron_backend_execution::TronContractType::UpdateSettingContract) => {
                if !remote_config.update_setting_enabled {
                    return Err(
                        "UPDATE_SETTING_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing UPDATE_SETTING_CONTRACT");
                self.execute_update_setting_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::UpdateEnergyLimitContract) => {
                if !remote_config.update_energy_limit_enabled {
                    return Err(
                        "UPDATE_ENERGY_LIMIT_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing UPDATE_ENERGY_LIMIT_CONTRACT");
                self.execute_update_energy_limit_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::ClearAbiContract) => {
                if !remote_config.clear_abi_enabled {
                    return Err(
                        "CLEAR_ABI_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing CLEAR_ABI_CONTRACT");
                self.execute_clear_abi_contract(storage_adapter, transaction, context)
            }
            // Phase 2.C2: UpdateBrokerage contract (49)
            Some(tron_backend_execution::TronContractType::UpdateBrokerageContract) => {
                if !remote_config.update_brokerage_enabled {
                    return Err(
                        "UPDATE_BROKERAGE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing UPDATE_BROKERAGE_CONTRACT");
                self.execute_update_brokerage_contract(storage_adapter, transaction, context)
            }
            // Phase 2.D: Resource/Freeze/Delegation contracts (56/57/58/59)
            Some(tron_backend_execution::TronContractType::WithdrawExpireUnfreezeContract) => {
                if !remote_config.withdraw_expire_unfreeze_enabled {
                    return Err("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing WITHDRAW_EXPIRE_UNFREEZE_CONTRACT");
                self.execute_withdraw_expire_unfreeze_contract(
                    storage_adapter,
                    transaction,
                    context,
                )
            }
            Some(tron_backend_execution::TronContractType::DelegateResourceContract) => {
                if !remote_config.delegate_resource_enabled {
                    return Err(
                        "DELEGATE_RESOURCE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing DELEGATE_RESOURCE_CONTRACT");
                self.execute_delegate_resource_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::UndelegateResourceContract) => {
                if !remote_config.undelegate_resource_enabled {
                    return Err(
                        "UNDELEGATE_RESOURCE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing UNDELEGATE_RESOURCE_CONTRACT");
                self.execute_undelegate_resource_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract) => {
                if !remote_config.cancel_all_unfreeze_v2_enabled {
                    return Err("CANCEL_ALL_UNFREEZE_V2_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing CANCEL_ALL_UNFREEZE_V2_CONTRACT");
                self.execute_cancel_all_unfreeze_v2_contract(storage_adapter, transaction, context)
            }
            // Phase 2.E: TRC-10 Extension contracts (9/14/15)
            Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract) => {
                if !remote_config.participate_asset_issue_enabled {
                    return Err("PARTICIPATE_ASSET_ISSUE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing PARTICIPATE_ASSET_ISSUE_CONTRACT");
                self.execute_participate_asset_issue_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::UnfreezeAssetContract) => {
                if !remote_config.unfreeze_asset_enabled {
                    return Err(
                        "UNFREEZE_ASSET_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing UNFREEZE_ASSET_CONTRACT");
                self.execute_unfreeze_asset_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::UpdateAssetContract) => {
                if !remote_config.update_asset_enabled {
                    return Err(
                        "UPDATE_ASSET_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing UPDATE_ASSET_CONTRACT");
                self.execute_update_asset_contract(storage_adapter, transaction, context)
            }
            // Phase 2.F: Exchange contracts (41/42/43/44)
            Some(tron_backend_execution::TronContractType::ExchangeCreateContract) => {
                if !remote_config.exchange_create_enabled {
                    return Err(
                        "EXCHANGE_CREATE_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing EXCHANGE_CREATE_CONTRACT");
                self.execute_exchange_create_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::ExchangeInjectContract) => {
                if !remote_config.exchange_inject_enabled {
                    return Err(
                        "EXCHANGE_INJECT_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing EXCHANGE_INJECT_CONTRACT");
                self.execute_exchange_inject_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::ExchangeWithdrawContract) => {
                if !remote_config.exchange_withdraw_enabled {
                    return Err(
                        "EXCHANGE_WITHDRAW_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing EXCHANGE_WITHDRAW_CONTRACT");
                self.execute_exchange_withdraw_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::ExchangeTransactionContract) => {
                if !remote_config.exchange_transaction_enabled {
                    return Err("EXCHANGE_TRANSACTION_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing EXCHANGE_TRANSACTION_CONTRACT");
                self.execute_exchange_transaction_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::MarketSellAssetContract) => {
                if !remote_config.market_sell_asset_enabled {
                    return Err(
                        "MARKET_SELL_ASSET_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing MARKET_SELL_ASSET_CONTRACT");
                self.execute_market_sell_asset_contract(storage_adapter, transaction, context)
            }
            Some(tron_backend_execution::TronContractType::MarketCancelOrderContract) => {
                if !remote_config.market_cancel_order_enabled {
                    return Err(
                        "MARKET_CANCEL_ORDER_CONTRACT execution is disabled - falling back to Java"
                            .to_string(),
                    );
                }
                debug!("Executing MARKET_CANCEL_ORDER_CONTRACT");
                self.execute_market_cancel_order_contract(storage_adapter, transaction, context)
            }
            Some(contract_type) => {
                // Other contract types not yet implemented - return error to fall back to Java
                Err(format!(
                    "Contract type {:?} not yet implemented in Rust backend",
                    contract_type
                ))
            }
            None => {
                // No contract type specified - use legacy transfer logic for backward compatibility
                debug!("No contract type specified, using legacy transfer logic");
                self.execute_transfer_contract(storage_adapter, transaction, context)
            }
        }
    }

    /// If `data` is a `google.protobuf.Any` wrapper, extract and return the inner `value` bytes.
    ///
    /// Fixture generation (and the Java runtime) serialize `Transaction.Contract.parameter`
    /// directly, which is an Any. For convenience, we accept either Any-wrapped bytes or raw
    /// contract bytes.
    fn unwrap_any_value_if_present(data: &[u8]) -> Result<Vec<u8>, String> {
        // Fast-path: `Any` always starts with field 1 (type_url) as a string.
        // We detect it by checking for the "type.googleapis.com/" prefix.
        let mut pos = 0;
        let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
            .map_err(|e| e.to_java_message().to_string())?;
        pos += tag_len;
        if field_number != 1 || wire_type != 2 {
            return Err("Not an Any wrapper".to_string());
        }

        let (payload, total_len) = read_length_delimited_typed(&data[pos..])
            .map_err(|e| e.to_java_message().to_string())?;
        if !payload.starts_with(b"type.googleapis.com/") {
            return Err("Not an Any wrapper".to_string());
        }
        // (don't advance pos — we restart full parse below)

        // Full parse to find field 2 (value).
        let mut pos = 0;
        let mut value: Option<Vec<u8>> = None;
        while pos < data.len() {
            let (fn_num, wt, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (fn_num, wt) {
                (1, 2) => {
                    // type_url
                    let (_payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += total_len;
                }
                (2, 2) => {
                    // value
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    value = Some(payload.to_vec());
                    pos += total_len;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wt)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        value.ok_or_else(|| "Missing Any.value".to_string())
    }

    /// Execute a TRANSFER_CONTRACT (legacy non-VM transaction)
    /// Handles TRON value transfer with proper fee accounting
    fn execute_transfer_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        debug!(
            "Executing TRANSFER_CONTRACT: from={:?}, to={:?}, value={}",
            transaction.from, transaction.to, transaction.value
        );

        // Validate contract parameter presence and type (strict Java parity)
        let _contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.TransferContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [TransferContract], real type [class com.google.protobuf.Any]",
        )?;

        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();

        // TransferContract amount is a signed long; fixtures encode it into the low 8 bytes of
        // `value` (big-endian) and conformance conversion preserves the raw bits in U256.
        let amount_i64 = {
            let limbs = transaction.value.as_limbs();
            if limbs[1] != 0 || limbs[2] != 0 || limbs[3] != 0 {
                return Err("long overflow".to_string());
            }
            limbs[0] as i64
        };

        // Validation parity with java-tron TransferActuator.validate()
        // Java checks: DecodeUtil.addressValid(ownerAddress) then DecodeUtil.addressValid(toAddress)
        // addressValid requires: non-null, exactly 21 bytes, prefix == configured prefix byte.
        // Error ordering: ownerAddress validation MUST happen before toAddress validation.
        let prefix = storage_adapter.address_prefix();
        if let Some(owner_raw) = transaction.metadata.from_raw.as_deref() {
            if owner_raw.len() != 21 || owner_raw[0] != prefix {
                return Err("Invalid ownerAddress!".to_string());
            }
        }

        // Validate to_raw using the same rules as Java's DecodeUtil.addressValid(toAddress).
        // We derive the 20-byte EVM address directly from validated to_raw bytes rather than
        // relying on the pre-converted transaction.to (which may be None for malformed addresses
        // that failed during gRPC conversion).
        let to_address = match transaction.metadata.to_raw.as_deref() {
            Some(raw) if raw.len() == 21 && raw[0] == prefix => {
                revm_primitives::Address::from_slice(&raw[1..])
            }
            _ => {
                return Err("Invalid toAddress!".to_string());
            }
        };

        if to_address == transaction.from {
            return Err("Cannot transfer TRX to yourself.".to_string());
        }

        // Calculate bandwidth used based on transaction payload size
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Start with empty state changes
        let mut state_changes = Vec::new();

        // Load sender account
        let sender_opt = storage_adapter
            .get_account(&transaction.from)
            .map_err(|e| format!("Failed to load sender account: {}", e))?;
        let sender_account = sender_opt
            .clone()
            .ok_or_else(|| "Validate TransferContract error, no OwnerAccount.".to_string())?;

        if amount_i64 <= 0 {
            return Err("Amount must be greater than 0.".to_string());
        }
        let amount_u256 = revm_primitives::U256::from(amount_i64 as u64);

        // Load recipient account (track existence)
        let recipient_opt = storage_adapter
            .get_account(&to_address)
            .map_err(|e| format!("Failed to load recipient account: {}", e))?;
        let recipient_account = recipient_opt.clone().unwrap_or_default();

        // TransferContract charges CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT when the recipient
        // account does not exist.
        let create_account_fee = if recipient_opt.is_none() {
            storage_adapter
                .get_create_new_account_fee_in_system_contract()
                .map_err(|e| {
                    format!(
                        "Failed to get CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT: {}",
                        e
                    )
                })?
        } else {
            0
        };

        // Strict actuator parity: Java's TransferActuator.calcFee() returns TRANSFER_FEE = 0.
        // The only fee for TransferContract is the create-account-fee when recipient is new.
        // No additional flat fee is applied at the actuator level.
        let fee_amount: u64 = 0;

        // Contract-type restrictions (java: ForbidTransferToContract, AllowTvmCompatibleEvm).
        // Apply after fee adjustment for existence, but before balance checks.
        let recipient_proto_opt = if recipient_opt.is_some() {
            storage_adapter
                .get_account_proto(&to_address)
                .map_err(|e| format!("Failed to load recipient account proto: {}", e))?
        } else {
            None
        };

        let recipient_is_contract = matches!(
            recipient_proto_opt.as_ref(),
            Some(acct) if acct.r#type == tron_backend_execution::protocol::AccountType::Contract as i32
        );

        let forbid_transfer_to_contract = storage_adapter
            .get_forbid_transfer_to_contract()
            .map_err(|e| format!("Failed to get FORBID_TRANSFER_TO_CONTRACT: {}", e))?;
        if forbid_transfer_to_contract == 1 && recipient_is_contract {
            return Err("Cannot transfer TRX to a smartContract.".to_string());
        }

        let allow_tvm_compatible_evm = storage_adapter
            .get_allow_tvm_compatible_evm()
            .map_err(|e| format!("Failed to get ALLOW_TVM_COMPATIBLE_EVM: {}", e))?;
        if allow_tvm_compatible_evm == 1 && recipient_is_contract {
            let to_tron_21 = storage_adapter.to_tron_address_21(&to_address);
            let contract = storage_adapter
                .get_smart_contract(&to_tron_21)
                .map_err(|e| format!("Failed to load smart contract: {}", e))?;
            match contract {
                None => {
                    return Err(
                        "Account type is Contract, but it is not exist in contract store."
                            .to_string(),
                    );
                }
                Some(c) => {
                    if c.version == 1 {
                        return Err("Cannot transfer TRX to a smartContract which version is one. Instead please use TriggerSmartContract "
                            .to_string());
                    }
                }
            }
        }

        // Validate sender has enough balance for amount + fees (java: addExact(amount, fee)).
        // fee = create_account_fee (TRANSFER_FEE is always 0 in Java's TransferActuator).
        let fee_i64 = i64::try_from(create_account_fee).map_err(|_| "long overflow".to_string())?;
        let total_cost_i64 = amount_i64
            .checked_add(fee_i64)
            .ok_or_else(|| "long overflow".to_string())?;

        let sender_balance_i64 = sender_account.balance.as_limbs()[0] as i64;
        if sender_balance_i64 < total_cost_i64 {
            return Err("Validate TransferContract error, balance is not sufficient.".to_string());
        }

        // Recipient balance overflow check (java: addExact(toBalance, amount) when toAccount != null).
        if recipient_opt.is_some() {
            let recipient_balance_i64 = recipient_account.balance.as_limbs()[0] as i64;
            recipient_balance_i64
                .checked_add(amount_i64)
                .ok_or_else(|| "long overflow".to_string())?;
        }

        let total_cost = revm_primitives::U256::from(total_cost_i64 as u64);

        // Track AEXT for bandwidth if in tracked mode (after validation to ensure validate_fail has 0 writes)
        let mut aext_map = std::collections::HashMap::new();
        if aext_mode == "tracked" {
            use tron_backend_execution::{AccountAext, BandwidthParams, ResourceTracker};

            // Get current AEXT for sender (or initialize with defaults)
            let current_aext = storage_adapter
                .get_account_aext(&transaction.from)
                .map_err(|e| format!("Failed to get account AEXT: {}", e))?
                .unwrap_or_else(AccountAext::with_defaults);

            // Get dynamic properties for bandwidth tracking
            let free_net_limit = storage_adapter
                .get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;
            let public_net_limit = storage_adapter
                .get_public_net_limit()
                .map_err(|e| format!("Failed to get PUBLIC_NET_LIMIT: {}", e))?;
            let public_net_usage = storage_adapter
                .get_public_net_usage()
                .map_err(|e| format!("Failed to get PUBLIC_NET_USAGE: {}", e))?;
            let public_net_time = storage_adapter
                .get_public_net_time()
                .map_err(|e| format!("Failed to get PUBLIC_NET_TIME: {}", e))?;
            let transaction_fee = storage_adapter
                .get_transaction_fee()
                .map_err(|e| format!("Failed to get TRANSACTION_FEE: {}", e))?;
            let create_account_bandwidth_rate = storage_adapter
                .get_create_new_account_bandwidth_rate()
                .map_err(|e| format!("Failed to get CREATE_NEW_ACCOUNT_BANDWIDTH_RATE: {}", e))?;

            // Compute account_net_limit from frozen bandwidth
            let account_net_limit = {
                let freeze_record = storage_adapter
                    .get_freeze_record(&transaction.from, 0) // 0 = BANDWIDTH
                    .map_err(|e| format!("Failed to get freeze record: {}", e))?;
                let total_net_weight = storage_adapter
                    .get_total_net_weight()
                    .map_err(|e| format!("Failed to get TOTAL_NET_WEIGHT: {}", e))?;
                let total_net_limit = storage_adapter
                    .get_total_net_limit()
                    .map_err(|e| format!("Failed to get TOTAL_NET_LIMIT: {}", e))?;
                if let Some(record) = freeze_record {
                    if total_net_weight > 0 {
                        // Java: calculateGlobalNetLimit = (frozenBalance / TRX_PRECISION) * (totalNetLimit / totalNetWeight)
                        let frozen_trx = record.frozen_amount as i64 / TRX_PRECISION as i64;
                        frozen_trx.saturating_mul(total_net_limit) / total_net_weight
                    } else {
                        0
                    }
                } else {
                    0
                }
            };

            // Java uses headSlot = (latestBlockTimestamp - genesisTimestamp) / 3000
            let genesis_ts = execution_config.remote.genesis_block_timestamp;
            let now_slot = (context.block_timestamp as i64 - genesis_ts) / 3000;

            let creates_new_account = recipient_opt.is_none();

            let bw_params = BandwidthParams {
                bytes_used: bandwidth_used as i64,
                now: now_slot,
                current_aext,
                account_net_limit,
                free_net_limit,
                public_net_limit,
                public_net_usage,
                public_net_time,
                creates_new_account,
                create_account_bandwidth_rate,
                transaction_fee,
            };

            let bw_result = ResourceTracker::track_bandwidth_v2(&bw_params)
                .map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist after AEXT to storage
            storage_adapter
                .set_account_aext(&transaction.from, &bw_result.after_aext)
                .map_err(|e| format!("Failed to persist account AEXT: {}", e))?;
            storage_adapter
                .apply_bandwidth_aext_to_account_proto(&transaction.from, &bw_result.after_aext)
                .map_err(|e| {
                    format!("Failed to persist bandwidth usage to account proto: {}", e)
                })?;

            // Persist global PUBLIC_NET changes if FREE_NET path was used
            if let Some(new_pub_usage) = bw_result.new_public_net_usage {
                storage_adapter
                    .set_public_net_usage(new_pub_usage)
                    .map_err(|e| format!("Failed to persist PUBLIC_NET_USAGE: {}", e))?;
            }
            if let Some(new_pub_time) = bw_result.new_public_net_time {
                storage_adapter
                    .set_public_net_time(new_pub_time)
                    .map_err(|e| format!("Failed to persist PUBLIC_NET_TIME: {}", e))?;
            }

            // Add to aext_map
            aext_map.insert(
                transaction.from,
                (bw_result.before_aext.clone(), bw_result.after_aext.clone()),
            );

            debug!(
                "AEXT tracked for transfer: owner={:?}, path={:?}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}",
                transaction.from,
                bw_result.path,
                bw_result.before_aext.net_usage,
                bw_result.after_aext.net_usage,
                bw_result.before_aext.free_net_usage,
                bw_result.after_aext.free_net_usage
            );
        }

        // Update sender account: balance -= (value + fee)
        let new_sender_balance = sender_account.balance - total_cost;
        let new_sender_account = revm_primitives::AccountInfo {
            balance: new_sender_balance,
            nonce: sender_account.nonce, // Phase 3 Fix: Do NOT increment nonce for non-VM TRX transfers
            code_hash: sender_account.code_hash,
            code: sender_account.code.clone(),
        };

        // Add sender account change
        state_changes.push(TronStateChange::AccountChange {
            address: transaction.from,
            old_account: Some(sender_account),
            new_account: Some(new_sender_account.clone()),
        });
        // Persist sender account update
        storage_adapter
            .set_account(transaction.from, new_sender_account.clone())
            .map_err(|e| format!("Failed to persist sender account: {}", e))?;

        // Update recipient account: balance += value
        let new_recipient_balance = recipient_account
            .balance
            .checked_add(amount_u256)
            .ok_or("Recipient balance overflow")?;
        let new_recipient_account = revm_primitives::AccountInfo {
            balance: new_recipient_balance,
            nonce: recipient_account.nonce,
            code_hash: recipient_account.code_hash,
            code: recipient_account.code.clone(),
        };

        // Add recipient account change
        // Creation should be based on true storage absence, not zero balance
        let old_recipient_account = if recipient_opt.is_none() {
            None // Account creation
        } else {
            Some(recipient_account)
        };

        state_changes.push(TronStateChange::AccountChange {
            address: to_address,
            old_account: old_recipient_account,
            new_account: Some(new_recipient_account.clone()),
        });

        // Persist recipient account update (create_time for newly-created accounts)
        if recipient_opt.is_none() {
            use tron_backend_execution::protocol::permission::PermissionType;
            use tron_backend_execution::protocol::{Account as ProtoAccount, Key, Permission};

            // java-tron uses DynamicPropertiesStore.latest_block_header_timestamp as "now"
            // for account creation timestamps.
            let create_time = storage_adapter
                .get_latest_block_header_timestamp()
                .map_err(|e| format!("Failed to get LATEST_BLOCK_HEADER_TIMESTAMP: {}", e))?;
            let recipient_address_bytes = storage_adapter.to_tron_address_21(&to_address).to_vec();

            let mut recipient_proto = ProtoAccount {
                address: recipient_address_bytes.clone(),
                balance: new_recipient_account.balance.as_limbs()[0] as i64,
                create_time,
                ..Default::default()
            };

            let allow_multi_sign = storage_adapter
                .get_allow_multi_sign()
                .map_err(|e| format!("Failed to get ALLOW_MULTI_SIGN: {}", e))?;
            if allow_multi_sign {
                let active_default_operations = storage_adapter
                    .get_active_default_operations()
                    .map_err(|e| format!("Failed to get ACTIVE_DEFAULT_OPERATIONS: {}", e))?;

                let default_key = Key {
                    address: recipient_address_bytes.clone(),
                    weight: 1,
                };

                recipient_proto.owner_permission = Some(Permission {
                    r#type: PermissionType::Owner as i32,
                    id: 0,
                    permission_name: "owner".to_string(),
                    threshold: 1,
                    parent_id: 0,
                    operations: Vec::new(),
                    keys: vec![default_key.clone()],
                });

                recipient_proto.active_permission = vec![Permission {
                    r#type: PermissionType::Active as i32,
                    id: 2,
                    permission_name: "active".to_string(),
                    threshold: 1,
                    parent_id: 0,
                    operations: active_default_operations,
                    keys: vec![default_key],
                }];
            }
            storage_adapter
                .put_account_proto(&to_address, &recipient_proto)
                .map_err(|e| format!("Failed to persist recipient Account proto: {}", e))?;
        } else {
            storage_adapter
                .set_account(to_address, new_recipient_account.clone())
                .map_err(|e| format!("Failed to persist recipient account: {}", e))?;
        }

        // Handle create-account-fee (burn or credit blackhole based on dynamic properties)
        if create_account_fee > 0 {
            let support_blackhole = storage_adapter
                .support_black_hole_optimization()
                .map_err(|e| format!("Failed to get SupportBlackHoleOptimization: {}", e))?;
            if support_blackhole {
                debug!(
                    "Burning create-account-fee {} SUN (blackhole optimization)",
                    create_account_fee
                );
                storage_adapter
                    .burn_trx(create_account_fee)
                    .map_err(|e| format!("Failed to burn TRX: {}", e))?;
            } else if let Some(blackhole_addr) = storage_adapter
                .get_blackhole_address()
                .map_err(|e| format!("Failed to get blackhole address: {}", e))?
            {
                let blackhole_account = storage_adapter
                    .get_account(&blackhole_addr)
                    .map_err(|e| format!("Failed to load blackhole account: {}", e))?
                    .unwrap_or_default();

                let fee_u256 = revm_primitives::U256::from(create_account_fee);
                let new_blackhole_balance = blackhole_account
                    .balance
                    .checked_add(fee_u256)
                    .ok_or("Blackhole balance overflow")?;
                let new_blackhole_account = revm_primitives::AccountInfo {
                    balance: new_blackhole_balance,
                    nonce: blackhole_account.nonce,
                    code_hash: blackhole_account.code_hash,
                    code: blackhole_account.code.clone(),
                };

                state_changes.push(TronStateChange::AccountChange {
                    address: blackhole_addr,
                    old_account: Some(blackhole_account),
                    new_account: Some(new_blackhole_account.clone()),
                });

                storage_adapter
                    .set_account(blackhole_addr, new_blackhole_account)
                    .map_err(|e| format!("Failed to persist blackhole account: {}", e))?;
            }
        }

        // fee_amount is always 0 for TransferContract (strict actuator parity).
        // Only create-account-fee is charged (handled above).
        debug_assert_eq!(
            fee_amount, 0,
            "TransferContract fee_amount must be 0 for actuator parity"
        );

        // Sort state changes deterministically for digest parity
        state_changes.sort_by(|a, b| {
            match (a, b) {
                (
                    TronStateChange::AccountChange {
                        address: addr_a, ..
                    },
                    TronStateChange::AccountChange {
                        address: addr_b, ..
                    },
                ) => addr_a.cmp(addr_b),
                (
                    TronStateChange::StorageChange {
                        address: addr_a,
                        key: key_a,
                        ..
                    },
                    TronStateChange::StorageChange {
                        address: addr_b,
                        key: key_b,
                        ..
                    },
                ) => addr_a.cmp(addr_b).then(key_a.cmp(key_b)),
                // Account changes before storage changes for same address
                (
                    TronStateChange::AccountChange {
                        address: addr_a, ..
                    },
                    TronStateChange::StorageChange {
                        address: addr_b, ..
                    },
                ) => addr_a.cmp(addr_b).then(std::cmp::Ordering::Less),
                (
                    TronStateChange::StorageChange {
                        address: addr_a, ..
                    },
                    TronStateChange::AccountChange {
                        address: addr_b, ..
                    },
                ) => addr_a.cmp(addr_b).then(std::cmp::Ordering::Greater),
            }
        });

        debug!("Non-VM transaction executed successfully - bandwidth_used: {}, create_account_fee: {}, state_changes: {}",
               bandwidth_used, create_account_fee, state_changes.len());

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(), // No return data for value transfers
            energy_used: 0,                             // Non-VM transactions use 0 energy
            bandwidth_used,
            state_changes,
            logs: Vec::new(), // No logs for value transfers
            error: None,
            aext_map,               // Populated with tracked AEXT if mode is "tracked"
            freeze_changes: vec![], // Will be populated by freeze-related contracts
            global_resource_changes: vec![], // Not applicable for value transfers
            trc10_changes: vec![],  // Not applicable for value transfers
            vote_changes: vec![],   // Not applicable for value transfers
            withdraw_changes: vec![], // Not applicable for value transfers
            tron_transaction_result: None,
            contract_address: None,
        })
    }

    /// Execute a WITNESS_CREATE_CONTRACT
    /// Creates a new witness account with proper validation and state changes
    fn execute_witness_create_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        debug!(
            "Executing WITNESS_CREATE_CONTRACT for address {:?}",
            transaction.from
        );

        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();

        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.WitnessCreateContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [WitnessCreateContract],real type[class com.google.protobuf.Any]",
        )?;

        // 1. Validate owner address (java-tron: DecodeUtil.addressValid)
        let prefix = storage_adapter.address_prefix();
        let owner_from_field = transaction.metadata.from_raw.as_deref().unwrap_or(&[]);
        if owner_from_field.len() != 21 || owner_from_field[0] != prefix {
            return Err("Invalid address".to_string());
        }

        let readable_owner = hex::encode(owner_from_field);

        // 2. Parse URL from contract_parameter.value (WitnessCreateContract protobuf)
        let url_bytes = Self::parse_witness_create_url(contract_bytes)?;
        // Validate URL format (java-tron TransactionUtil.validUrl with allowEmpty=false)
        if url_bytes.is_empty() || url_bytes.len() > 256 {
            return Err("Invalid url".to_string());
        }

        // Java uses ByteString#toStringUtf8(); accept non-UTF-8 bytes lossily for parity.
        let url = String::from_utf8_lossy(&url_bytes).to_string();

        debug!("WitnessCreate URL: {}", url);

        // 3. Load owner account
        let owner_account = storage_adapter
            .get_account(&transaction.from)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or_else(|| format!("account[{}] not exists", readable_owner))?;

        // 4. Check if owner is already a witness
        if storage_adapter
            .is_witness(&transaction.from)
            .map_err(|e| format!("Failed to check witness status: {}", e))?
        {
            return Err(format!("Witness[{}] has existed", readable_owner));
        }

        // 5. Get dynamic properties
        let account_upgrade_cost = storage_adapter
            .get_account_upgrade_cost()
            .map_err(|e| format!("Failed to get AccountUpgradeCost: {}", e))?;
        let allow_multi_sign = storage_adapter
            .get_allow_multi_sign()
            .map_err(|e| format!("Failed to get AllowMultiSign: {}", e))?;
        let support_blackhole = storage_adapter
            .support_black_hole_optimization()
            .map_err(|e| format!("Failed to get SupportBlackHoleOptimization: {}", e))?;

        info!(
            "WitnessCreate flags: upgrade_cost={} SUN, allow_multi_sign={}, support_blackhole={}",
            account_upgrade_cost, allow_multi_sign, support_blackhole
        );

        // 6. Validate sufficient balance
        if owner_account.balance < revm_primitives::U256::from(account_upgrade_cost) {
            return Err("balance < AccountUpgradeCost".to_string());
        }

        // 7. Prepare state changes
        let mut state_changes = Vec::new();

        // 8. Create witness entry
        let witness_info = tron_backend_execution::WitnessInfo::new(
            transaction.from,
            url,
            0, // Initial vote count is 0
        );

        storage_adapter
            .put_witness(&witness_info)
            .map_err(|e| format!("Failed to create witness: {}", e))?;

        debug!("Created witness entry for address {:?}", transaction.from);

        // 9. Mark owner account as witness (Account.is_witness = true)
        //    and set default witness permissions if ALLOW_MULTI_SIGN == 1
        //    (Java: AccountCapsule.setDefaultWitnessPermission)
        let mut owner_account_proto = storage_adapter
            .get_account_proto(&transaction.from)
            .map_err(|e| format!("Failed to load owner account proto: {}", e))?
            .ok_or_else(|| format!("account[{}] not exists", readable_owner))?;
        owner_account_proto.is_witness = true;

        if allow_multi_sign {
            use tron_backend_execution::protocol::{permission::PermissionType, Key, Permission};

            // Java: setDefaultWitnessPermission(dynamicStore)
            // Always set witness_permission (id=1, type=Witness)
            owner_account_proto.witness_permission = Some(Permission {
                r#type: PermissionType::Witness as i32,
                id: 1,
                permission_name: "witness".to_string(),
                threshold: 1,
                parent_id: 0,
                operations: vec![],
                keys: vec![Key {
                    address: owner_from_field.to_vec(),
                    weight: 1,
                }],
            });

            // If owner_permission is not set, initialize default (id=0, type=Owner)
            if owner_account_proto.owner_permission.is_none() {
                owner_account_proto.owner_permission = Some(Permission {
                    r#type: PermissionType::Owner as i32,
                    id: 0,
                    permission_name: "owner".to_string(),
                    threshold: 1,
                    parent_id: 0,
                    operations: vec![],
                    keys: vec![Key {
                        address: owner_from_field.to_vec(),
                        weight: 1,
                    }],
                });
            }

            // If active_permission is empty, add default (id=2, type=Active)
            if owner_account_proto.active_permission.is_empty() {
                let active_ops = storage_adapter
                    .get_active_default_operations()
                    .map_err(|e| format!("Failed to get ACTIVE_DEFAULT_OPERATIONS: {}", e))?;

                owner_account_proto.active_permission.push(Permission {
                    r#type: PermissionType::Active as i32,
                    id: 2,
                    permission_name: "active".to_string(),
                    threshold: 1,
                    parent_id: 0,
                    operations: active_ops,
                    keys: vec![Key {
                        address: owner_from_field.to_vec(),
                        weight: 1,
                    }],
                });
            }

            debug!(
                "Set default witness permissions for account {}",
                readable_owner
            );
        }

        storage_adapter
            .put_account_proto(&transaction.from, &owner_account_proto)
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;

        // 10. Update owner account - deduct cost
        let new_owner_account = revm_primitives::AccountInfo {
            balance: owner_account.balance - revm_primitives::U256::from(account_upgrade_cost),
            nonce: owner_account.nonce,
            code_hash: owner_account.code_hash,
            code: owner_account.code.clone(),
        };

        // Emit account change for owner (balance decreased)
        state_changes.push(TronStateChange::AccountChange {
            address: transaction.from,
            old_account: Some(owner_account),
            new_account: Some(new_owner_account.clone()),
        });
        // Persist owner account update
        storage_adapter
            .set_account(transaction.from, new_owner_account.clone())
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;

        // 10. Handle fee burning/crediting
        let fee_destination: String;
        if support_blackhole {
            // Burn mode - no additional account change needed
            info!(
                "Burning {} SUN (blackhole optimization)",
                account_upgrade_cost
            );
            storage_adapter
                .burn_trx(account_upgrade_cost)
                .map_err(|e| format!("Failed to burn TRX: {}", e))?;
            fee_destination = String::from("burn");
        } else {
            // Credit blackhole account
            if let Some(blackhole_addr) = storage_adapter
                .get_blackhole_address()
                .map_err(|e| format!("Failed to get blackhole address: {}", e))?
            {
                let blackhole_account = storage_adapter
                    .get_account(&blackhole_addr)
                    .map_err(|e| format!("Failed to load blackhole account: {}", e))?
                    .unwrap_or_default();

                let new_blackhole_account = revm_primitives::AccountInfo {
                    balance: blackhole_account.balance
                        + revm_primitives::U256::from(account_upgrade_cost),
                    nonce: blackhole_account.nonce,
                    code_hash: blackhole_account.code_hash,
                    code: blackhole_account.code.clone(),
                };

                // Emit account change for blackhole
                state_changes.push(TronStateChange::AccountChange {
                    address: blackhole_addr,
                    old_account: Some(blackhole_account),
                    new_account: Some(new_blackhole_account.clone()),
                });

                // Persist blackhole account update
                storage_adapter
                    .set_account(blackhole_addr, new_blackhole_account.clone())
                    .map_err(|e| format!("Failed to persist blackhole account: {}", e))?;

                let bh_tron =
                    revm_primitives::hex::encode(add_tron_address_prefix(&blackhole_addr));
                info!(
                    "Credited {} SUN to blackhole address {}",
                    account_upgrade_cost, bh_tron
                );
                fee_destination = format!("blackhole:{}", bh_tron);
            } else {
                warn!(
                    "No blackhole address configured, burning {} SUN",
                    account_upgrade_cost
                );
                fee_destination = String::from("burn(no_addr)");
            }
        }

        // 11. Update dynamic properties (java: addTotalCreateWitnessCost)
        storage_adapter
            .add_total_create_witness_cost(account_upgrade_cost)
            .map_err(|e| format!("Failed to update TOTAL_CREATE_WITNESS_FEE: {}", e))?;

        // 12. Sort state changes deterministically for CSV parity
        state_changes.sort_by(|a, b| match (a, b) {
            (
                TronStateChange::AccountChange {
                    address: addr_a, ..
                },
                TronStateChange::AccountChange {
                    address: addr_b, ..
                },
            ) => addr_a.cmp(addr_b),
            _ => std::cmp::Ordering::Equal,
        });

        // 13. Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Track AEXT for bandwidth if in tracked mode
        let mut aext_map = std::collections::HashMap::new();
        if aext_mode == "tracked" {
            use tron_backend_execution::{AccountAext, ResourceTracker};

            // Get current AEXT for owner (or initialize with defaults)
            let current_aext = storage_adapter
                .get_account_aext(&transaction.from)
                .map_err(|e| format!("Failed to get account AEXT: {}", e))?
                .unwrap_or_else(|| AccountAext::with_defaults());

            // Get FREE_NET_LIMIT from dynamic properties
            let free_net_limit = storage_adapter
                .get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;

            // Java uses headSlot = block_timestamp_ms / 3000 for resource windows.
            let now_slot = (context.block_timestamp / 3000) as i64;

            // Track bandwidth usage (returns path, before_aext, after_aext)
            let (path, before_aext, after_aext) = ResourceTracker::track_bandwidth(
                &transaction.from,
                bandwidth_used as i64,
                now_slot,
                &current_aext,
                free_net_limit,
            )
            .map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist after AEXT to storage
            storage_adapter
                .set_account_aext(&transaction.from, &after_aext)
                .map_err(|e| format!("Failed to persist account AEXT: {}", e))?;
            storage_adapter
                .apply_bandwidth_aext_to_account_proto(&transaction.from, &after_aext)
                .map_err(|e| {
                    format!("Failed to persist bandwidth usage to account proto: {}", e)
                })?;

            // Add to aext_map
            aext_map.insert(transaction.from, (before_aext.clone(), after_aext.clone()));

            debug!("AEXT tracked for witness_create: owner={:?}, path={:?}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}",
                   transaction.from, path, before_aext.net_usage, after_aext.net_usage,
                   before_aext.free_net_usage, after_aext.free_net_usage);
        }

        let owner_tron = revm_primitives::hex::encode(add_tron_address_prefix(&transaction.from));
        info!(
            "WitnessCreate completed: cost={} SUN, state_changes={}, owner={}, fee_dest={}",
            account_upgrade_cost,
            state_changes.len(),
            owner_tron,
            fee_destination
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0, // System contracts use 0 energy
            bandwidth_used,
            logs: Vec::new(), // No logs for witness creation
            state_changes,
            error: None,
            aext_map,               // Populated with tracked AEXT if mode is "tracked"
            freeze_changes: vec![], // Will be populated by freeze-related contracts
            global_resource_changes: vec![], // Not applicable for witness creation
            trc10_changes: vec![],  // Not applicable for witness creation
            vote_changes: vec![],   // Not applicable for witness creation
            withdraw_changes: vec![], // Not applicable for witness creation
            tron_transaction_result: None,
            contract_address: None,
        })
    }

    /// Execute a WITNESS_UPDATE_CONTRACT
    /// Updates an existing witness's URL. No balance/energy changes, no logs, energy_used=0.
    /// Parity with Java WitnessUpdateActuator: validates owner account, witness existence, URL format.
    fn execute_witness_update_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::{TronExecutionResult, TronStateChange};

        // 0. Validate contract parameter presence, type, and decodability (strict Java parity).
        //    Java: any.unpack(WitnessUpdateContract.class) runs before account checks,
        //    so malformed protobuf must fail here, not be masked by later errors.
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.WitnessUpdateContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [WitnessUpdateContract],real type[class com.google.protobuf.Any]",
        )?;
        let url_bytes = Self::parse_witness_update_url(contract_bytes)?;

        // 1. Validate owner address (java-tron: DecodeUtil.addressValid)
        let prefix = storage_adapter.address_prefix();
        let owner_from_field = transaction.metadata.from_raw.as_deref().unwrap_or(&[]);
        if owner_from_field.len() != 21 || owner_from_field[0] != prefix {
            return Err("Invalid address".to_string());
        }

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        debug!("Executing WITNESS_UPDATE_CONTRACT for owner {}", owner_tron);

        // 2. Load owner account (required)
        let _owner_account = storage_adapter
            .get_account(&owner)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or_else(|| {
                warn!(
                    "WITNESS_UPDATE_CONTRACT: account does not exist for {}",
                    owner_tron
                );
                "account does not exist".to_string()
            })?;

        // 3. Validate URL format
        if url_bytes.is_empty() || url_bytes.len() > 256 {
            warn!(
                "WITNESS_UPDATE_CONTRACT: Invalid url (empty={}, len={})",
                url_bytes.is_empty(),
                url_bytes.len()
            );
            return Err("Invalid url".to_string());
        }

        // Java uses ByteString#toStringUtf8(); accept non-UTF-8 bytes lossily for parity.
        let new_url = String::from_utf8_lossy(&url_bytes).to_string();

        debug!("WitnessUpdate: new URL = {}", new_url);

        // 4. Load existing witness (required) — validates witness existence
        let _existing_witness = storage_adapter
            .get_witness(&owner)
            .map_err(|e| format!("Failed to load witness: {}", e))?
            .ok_or_else(|| {
                warn!(
                    "WITNESS_UPDATE_CONTRACT: Witness does not exist for {}",
                    owner_tron
                );
                "Witness does not exist".to_string()
            })?;

        // 5. Update only the URL field, preserving all other protocol.Witness fields
        //    (pub_key, total_produced, total_missed, latest_block_num, latest_slot_num, is_jobs).
        //    Java always calls witnessStore.put(...) even when URL is unchanged, so we do the same.
        storage_adapter
            .update_witness_url(&owner, &new_url)
            .map_err(|e| format!("Failed to update witness: {}", e))?;
        info!(
            "Updated witness URL: owner={}, new_url='{}'",
            owner_tron, new_url
        );

        // 7. Do not emit state changes for WitnessUpdateContract to match embedded semantics
        // (Java embedded CSV logs 0 state changes and empty digest for witness updates)
        let state_changes: Vec<TronStateChange> = Vec::new();

        // 8. Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // 9. Handle AEXT tracking if enabled
        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();
        let mut aext_map = std::collections::HashMap::new();

        if aext_mode == "tracked" {
            debug!("AEXT tracking enabled for WITNESS_UPDATE_CONTRACT");

            // Load current AEXT or default
            let current_aext = storage_adapter
                .get_account_aext(&owner)
                .map_err(|e| format!("Failed to load AEXT: {}", e))?
                .unwrap_or_default();

            // Load FREE_NET_LIMIT from dynamic properties
            let free_net_limit = storage_adapter
                .get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;

            // Java uses headSlot = block_timestamp_ms / 3000 for resource windows.
            let now_slot = (context.block_timestamp / 3000) as i64;

            // Track bandwidth usage (returns path, before_aext, after_aext)
            let (_path, before_aext, after_aext) =
                tron_backend_execution::ResourceTracker::track_bandwidth(
                    &owner,
                    bandwidth_used as i64,
                    now_slot,
                    &current_aext,
                    free_net_limit,
                )
                .map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist updated AEXT
            storage_adapter
                .set_account_aext(&owner, &after_aext)
                .map_err(|e| format!("Failed to persist AEXT: {}", e))?;
            storage_adapter
                .apply_bandwidth_aext_to_account_proto(&owner, &after_aext)
                .map_err(|e| {
                    format!("Failed to persist bandwidth usage to account proto: {}", e)
                })?;

            // Populate aext_map
            aext_map.insert(owner, (before_aext, after_aext));

            debug!(
                "AEXT tracked for owner {}: bandwidth_used={}",
                owner_tron, bandwidth_used
            );
        }

        // 10. Return success result
        debug!(
            "WITNESS_UPDATE_CONTRACT completed successfully for {}",
            owner_tron
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0, // Witness update uses zero energy
            bandwidth_used,
            logs: Vec::new(), // No logs for witness update
            state_changes,
            error: None,
            aext_map,
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],     // Not applicable for witness update
            withdraw_changes: vec![], // Not applicable for witness update
            tron_transaction_result: None,
            contract_address: None,
        })
    }

    /// Parse VoteWitnessContract from protobuf bytes
    /// message VoteWitnessContract {
    ///   bytes owner_address = 1;     // field 1 (validated; used as owner)
    ///   repeated Vote votes = 2;     // field 2
    ///   bool support = 3;            // field 3 (not used)
    /// }
    /// message Vote {
    ///   bytes vote_address = 1;      // field 1
    ///   int64 vote_count = 2;        // field 2
    /// }
    fn parse_vote_witness_contract(data: &[u8]) -> Result<(Vec<u8>, Vec<(Vec<u8>, i64)>), String> {
        let mut owner_address = Vec::new();
        let mut votes = Vec::new();
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address = payload.to_vec();
                    pos += total_len;
                }
                (2, 2) => {
                    let (vote_data, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += total_len;
                    let (vote_address, vote_count) = Self::parse_vote(vote_data)?;
                    votes.push((vote_address, vote_count));
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        Ok((owner_address, votes))
    }

    /// Parse a single Vote message from protobuf bytes
    fn parse_vote(data: &[u8]) -> Result<(Vec<u8>, i64), String> {
        let mut vote_address = Vec::new();
        let mut vote_count: Option<i64> = None;
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    vote_address = payload.to_vec();
                    pos += total_len;
                }
                (2, 0) => {
                    let (count, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    vote_count = Some(count as i64);
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        Ok((vote_address, vote_count.unwrap_or(0)))
    }

    /// Skip a protobuf field based on wire type
    fn skip_protobuf_field(data: &[u8], wire_type: u64) -> Result<usize, String> {
        match wire_type {
            0 => {
                // Varint
                let (_, bytes_read) = read_varint(data)?;
                Ok(bytes_read)
            }
            1 => {
                // 64-bit
                Ok(8)
            }
            2 => {
                // Length-delimited
                let (length, bytes_read) = read_varint(data)?;
                Ok(bytes_read + length as usize)
            }
            5 => {
                // 32-bit
                Ok(4)
            }
            _ => Err(format!("Unknown wire type: {}", wire_type)),
        }
    }

    /// Execute a VOTE_WITNESS_CONTRACT
    /// Handles witness voting with tally updates
    fn execute_vote_witness_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use revm_primitives::Address;
        use tron_backend_execution::{TronExecutionResult, TronStateChange, VotesRecord};

        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.VoteWitnessContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [VoteWitnessContract],real type[class com.google.protobuf.Any]",
        )?;

        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();

        let prefix = storage_adapter.address_prefix();

        // 1. Parse VoteWitnessContract from contract_parameter.value
        let (owner_address_raw, votes_raw) =
            Self::parse_vote_witness_contract(contract_bytes)
                .map_err(|e| format!("Failed to parse VoteWitnessContract: {}", e))?;

        // 2. Validate owner address (java-tron: DecodeUtil.addressValid)
        if owner_address_raw.len() != 21 || owner_address_raw[0] != prefix {
            return Err("Invalid address".to_string());
        }
        let owner = Address::from_slice(&owner_address_raw[1..]);

        let owner_tron = tron_backend_common::to_tron_address(&owner);
        let readable_owner_address = hex::encode(&owner_address_raw);

        info!("VoteWitness owner={} vote_count=?", owner_tron);
        info!("Parsed {} votes from VoteWitnessContract", votes_raw.len());

        // 3. Validate votes count
        if votes_raw.is_empty() {
            warn!("VoteNumber must more than 0");
            return Err("VoteNumber must more than 0".to_string());
        }

        if votes_raw.len() > MAX_VOTE_NUMBER {
            warn!("VoteNumber more than maxVoteNumber {}", MAX_VOTE_NUMBER);
            return Err(format!(
                "VoteNumber more than maxVoteNumber {}",
                MAX_VOTE_NUMBER
            ));
        }

        // 4. Validate each vote and compute total
        let mut sum_trx: u64 = 0;
        let mut votes: Vec<(Address, u64)> = Vec::with_capacity(votes_raw.len());
        for (vote_address_raw, vote_count) in &votes_raw {
            // Validate vote_address (java-tron: DecodeUtil.addressValid)
            if vote_address_raw.len() != 21 || vote_address_raw[0] != prefix {
                return Err("Invalid vote address!".to_string());
            }
            let vote_address = Address::from_slice(&vote_address_raw[1..]);

            // Validate vote_count > 0
            if *vote_count <= 0 {
                warn!("vote count must be greater than 0");
                return Err("vote count must be greater than 0".to_string());
            }
            let vote_count_u64: u64 = (*vote_count)
                .try_into()
                .map_err(|_| "vote count overflow".to_string())?;

            let readable_vote_address = hex::encode(vote_address_raw);

            // Validate witness exists
            let account_exists = storage_adapter
                .get_account_proto(&vote_address)
                .map_err(|e| format!("Failed to get account: {}", e))?
                .is_some();
            if !account_exists {
                warn!("Account {} not exists", readable_vote_address);
                return Err(format!("Account[{}] not exists", readable_vote_address));
            }

            let is_witness = storage_adapter
                .is_witness(&vote_address)
                .map_err(|e| format!("Failed to check witness status: {}", e))?;
            if !is_witness {
                warn!("Witness {} not exists", readable_vote_address);
                return Err(format!("Witness[{}] not exists", readable_vote_address));
            }

            // Add to sum
            sum_trx = sum_trx
                .checked_add(vote_count_u64)
                .ok_or_else(|| "Vote count overflow".to_string())?;
            votes.push((vote_address, vote_count_u64));
        }

        // 4.5 Validate owner exists
        let owner_exists = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .is_some();
        if !owner_exists {
            warn!("Account {} not exists", readable_owner_address);
            return Err(format!("Account[{}] not exists", readable_owner_address));
        }

        // 4. Convert sum to SUN and check against tron power
        let sum_sun = sum_trx
            .checked_mul(TRX_PRECISION)
            .ok_or_else(|| "Vote sum overflow when converting to SUN".to_string())?;

        // Get resource model flag
        let new_model = storage_adapter
            .support_allow_new_resource_model()
            .map_err(|e| format!("Failed to get resource model flag: {}", e))?;

        // Get tron power (using preferred method name)
        let tron_power_sun = storage_adapter
            .compute_tron_power_in_sun(&owner, new_model)
            .map_err(|e| format!("Failed to compute tron power: {}", e))?;

        info!(
            "VoteWitness owner={} sum={} TRX ({} SUN), tronPower={} SUN, new_model={}",
            owner_tron, sum_trx, sum_sun, tron_power_sun, new_model
        );

        if sum_sun > tron_power_sun {
            warn!(
                "The total number of votes[{}] is greater than the tronPower[{}]",
                sum_sun, tron_power_sun
            );
            return Err(format!(
                "The total number of votes[{}] is greater than the tronPower[{}]",
                sum_sun, tron_power_sun
            ));
        }

        // 5. Withdraw delegation rewards (Java: mortgageService.withdrawReward at start of countVoteAccount)
        // Must be called BEFORE mutating votes, since reward computation uses the old vote set.
        let reward = contracts::delegation::withdraw_reward(storage_adapter, &owner)
            .map_err(|e| format!("Failed to withdraw reward: {}", e))?;
        if reward > 0 {
            info!(
                "VoteWitness withdrawReward: owner={} reward={} SUN",
                owner_tron, reward
            );
        }

        // 5.5 Load owner account proto (after withdrawReward, matching Java ordering)
        // Java: accountCapsule = accountStore.get(ownerAddress) after withdrawReward
        let mut owner_account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("Account[{}] not exists", readable_owner_address))?;

        // 5.6 Apply reward to allowance (Java: MortgageService.adjustAllowance)
        // Java skips if amount <= 0
        if reward > 0 {
            owner_account.allowance = owner_account
                .allowance
                .checked_add(reward)
                .ok_or_else(|| "Allowance overflow".to_string())?;
            info!(
                "VoteWitness adjustAllowance: owner={} new_allowance={}",
                owner_tron, owner_account.allowance
            );
        }

        // 5.7 Initialize oldTronPower (Java: accountCapsule.initializeOldTronPower)
        // Java: if (supportAllowNewResourceModel() && oldTronPowerIsNotInitialized())
        //   initializeOldTronPower() → value = getTronPower(); if (value == 0) value = -1
        if new_model && owner_account.old_tron_power == 0 {
            let tron_power = storage_adapter
                .compute_tron_power_in_sun(&owner, false)
                .map_err(|e| format!("Failed to compute tron power for oldTronPower: {}", e))?;
            owner_account.old_tron_power = if tron_power == 0 {
                -1
            } else {
                tron_power as i64
            };
            info!(
                "VoteWitness: initialized oldTronPower to {} for {}",
                owner_account.old_tron_power, owner_tron
            );
        }

        // 6. Load or create VotesRecord
        // java-tron semantics:
        // - VoteWitness updates VotesCapsule.newVotes only
        // - VotesCapsule.oldVotes is updated at maintenance boundaries (not on every vote)
        // When creating a new record (no existing VotesRecord), seed old_votes from Account.votes
        // to match embedded behavior. This ensures correct delta computation in maintenance.
        let seed_from_account = execution_config.remote.vote_witness_seed_old_from_account;

        let mut votes_record = match storage_adapter.get_votes(&owner) {
            Ok(Some(record)) => {
                info!(
                    "Found existing votes for {}: old_votes={}, new_votes={}",
                    owner_tron,
                    record.old_votes.len(),
                    record.new_votes.len()
                );
                // Preserve old_votes (epoch baseline) and overwrite new_votes below.
                record
            }
            Ok(None) => {
                // No existing VotesRecord - this is the first vote for this account in this epoch
                if seed_from_account {
                    // Seed old_votes from Account.votes field (matches embedded behavior)
                    let prior_votes_tuples = storage_adapter
                        .get_account_votes_list(&owner)
                        .map_err(|e| format!("Failed to get account votes list: {}", e))?;

                    if prior_votes_tuples.is_empty() {
                        info!("No existing votes for {} and no Account.votes, creating empty record (seed_enabled=true)",
                              owner_tron);
                        VotesRecord::empty(owner)
                    } else {
                        info!("Seeding old_votes from Account.votes for {}: {} entries (seed_enabled=true)",
                              owner_tron, prior_votes_tuples.len());
                        // Convert (Address, u64) tuples to Vote structs
                        use tron_backend_execution::Vote;
                        let prior_votes: Vec<Vote> = prior_votes_tuples
                            .into_iter()
                            .map(|(addr, count)| Vote::new(addr, count))
                            .collect();
                        VotesRecord::new(owner, prior_votes, Vec::new())
                    }
                } else {
                    // Legacy behavior: empty old_votes
                    info!("No existing votes for {}, creating new record with empty old_votes (seed_enabled=false)",
                          owner_tron);
                    VotesRecord::empty(owner)
                }
            }
            Err(e) => {
                error!("Failed to get votes for {}: {}", owner_tron, e);
                return Err(format!("Failed to get votes: {}", e));
            }
        };

        // 7. Clear new_votes and add new votes
        votes_record.clear_new_votes();
        for (vote_address, vote_count) in &votes {
            votes_record.add_new_vote(*vote_address, *vote_count);
        }

        // 8. Persist votes record
        storage_adapter
            .set_votes(owner, &votes_record)
            .map_err(|e| format!("Failed to set votes: {}", e))?;

        info!(
            "Successfully stored votes for {}: old_votes={}, new_votes={}",
            owner_tron,
            votes_record.old_votes.len(),
            votes_record.new_votes.len()
        );

        // 8.5 Update Account.votes list to match embedded semantics.
        // java-tron clears the existing votes and appends the new ones on every vote.
        // Reuse owner_account loaded at step 5.5 (already has allowance + oldTronPower updates).
        owner_account.votes.clear();
        for (vote_address, vote_count) in &votes {
            let vote_count_i64: i64 = (*vote_count)
                .try_into()
                .map_err(|_| "vote count overflow when converting to i64".to_string())?;
            let vote_address_bytes = storage_adapter.to_tron_address_21(vote_address).to_vec();
            owner_account
                .votes
                .push(tron_backend_execution::protocol::Vote {
                    vote_address: vote_address_bytes,
                    vote_count: vote_count_i64,
                });
        }

        // Persist all account changes: allowance (from withdrawReward), oldTronPower, and votes
        storage_adapter
            .put_account_proto(&owner, &owner_account)
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;

        // 9. Build result with CSV parity
        // Get owner account for state change
        let old_account = storage_adapter
            .get_account(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?;

        // Create state changes (exactly one AccountChange for owner, old==new for CSV parity)
        let mut state_changes = Vec::new();
        state_changes.push(TronStateChange::AccountChange {
            address: owner,
            old_account: old_account.clone(),
            new_account: old_account,
        });

        // Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Track AEXT for bandwidth if in tracked mode
        let mut aext_map = std::collections::HashMap::new();
        if aext_mode == "tracked" {
            use tron_backend_execution::{AccountAext, ResourceTracker};

            // Get current AEXT for owner (or initialize with defaults)
            let current_aext = storage_adapter
                .get_account_aext(&owner)
                .map_err(|e| format!("Failed to get account AEXT: {}", e))?
                .unwrap_or_else(|| AccountAext::with_defaults());

            // Get FREE_NET_LIMIT from dynamic properties
            let free_net_limit = storage_adapter
                .get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;

            // Java uses headSlot = block_timestamp_ms / 3000 for resource windows.
            let now_slot = (context.block_timestamp / 3000) as i64;

            // Track bandwidth usage (returns path, before_aext, after_aext)
            let (path, before_aext, after_aext) = ResourceTracker::track_bandwidth(
                &owner,
                bandwidth_used as i64,
                now_slot,
                &current_aext,
                free_net_limit,
            )
            .map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist after AEXT to storage
            storage_adapter
                .set_account_aext(&owner, &after_aext)
                .map_err(|e| format!("Failed to persist account AEXT: {}", e))?;
            storage_adapter
                .apply_bandwidth_aext_to_account_proto(&owner, &after_aext)
                .map_err(|e| {
                    format!("Failed to persist bandwidth usage to account proto: {}", e)
                })?;

            // Add to aext_map
            aext_map.insert(owner, (before_aext.clone(), after_aext.clone()));

            debug!("AEXT tracked for vote_witness: owner={:?}, path={:?}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}",
                   owner, path, before_aext.net_usage, after_aext.net_usage,
                   before_aext.free_net_usage, after_aext.free_net_usage);
        }

        // Build VoteChange for Java to update Account.votes
        // This ensures correct old_votes seeding in subsequent epochs
        use tron_backend_execution::{VoteChange, VoteEntry};
        let vote_change = VoteChange {
            owner_address: owner,
            votes: votes_record
                .new_votes
                .iter()
                .map(|v| VoteEntry {
                    vote_address: v.vote_address.clone(),
                    vote_count: v.vote_count,
                })
                .collect(),
        };

        info!("VoteWitness completed: owner={}, votes={}, state_changes={}, bandwidth={}, vote_change_entries={}",
              owner_tron, votes_record.new_votes.len(), state_changes.len(), bandwidth_used, vote_change.votes.len());

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0, // System contracts use 0 energy
            bandwidth_used,
            logs: Vec::new(), // No logs for voting
            state_changes,
            error: None,
            aext_map,               // Populated with tracked AEXT if mode is "tracked"
            freeze_changes: vec![], // Will be populated by freeze-related contracts
            global_resource_changes: vec![], // Not applicable for vote witness
            trc10_changes: vec![],  // Not applicable for vote witness
            vote_changes: vec![vote_change], // VoteChange for Account.votes update
            withdraw_changes: vec![], // Not applicable for vote witness
            tron_transaction_result: None,
            contract_address: None,
        })
    }

    /// Execute an ACCOUNT_UPDATE_CONTRACT
    /// Updates the account name for a given address with proper validation and CSV parity
    ///
    /// End-to-end parity with Java UpdateAccountActuator:
    /// 1. Unpacks contract_parameter.value as AccountUpdateContract protobuf
    /// 2. Validates owner_address via DecodeUtil.addressValid (21 bytes, correct prefix)
    /// 3. Validates account_name via TransactionUtil.validAccountName (allows empty, max 200)
    /// 4. Validates consistency between decoded proto fields and transaction fields
    /// 5. Tracks bandwidth/AEXT usage for resource accounting
    fn execute_account_update_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use contracts::proto::parse_account_update_contract;
        use tron_backend_execution::{TronExecutionResult, TronStateChange};

        // 1. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.AccountUpdateContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [AccountUpdateContract], real type[class com.google.protobuf.Any]",
        )?;

        // Parse the protobuf value - mirrors Java's any.unpack()
        let parsed = parse_account_update_contract(contract_bytes)
            .map_err(|e| format!("Protocol buffer parse error: {}", e))?;

        let decoded_owner_address: Option<Vec<u8>> = Some(parsed.owner_address);

        // 2. Determine canonical source for name_bytes from contract_parameter.value
        let name_bytes: &[u8] = {
            // Validate consistency: decoded account_name should match transaction.data
            if transaction.data.as_ref() != parsed.account_name.as_slice() {
                warn!(
                    "AccountUpdate: decoded account_name differs from transaction.data: decoded={} tx_data={}",
                    parsed.account_name.len(),
                    transaction.data.len()
                );
            }
            &parsed.account_name
        };

        let owner_tron = tron_backend_common::to_tron_address(&transaction.from);

        info!(
            "AccountUpdate owner={} name_len={}",
            owner_tron,
            name_bytes.len()
        );

        // 3. Validation parity: TransactionUtil.validAccountName(bytes)
        // - allow empty
        // - max length = 200
        if name_bytes.len() > 200 {
            warn!(
                "Invalid accountName: len={} owner={}",
                name_bytes.len(),
                owner_tron
            );
            return Err("Invalid accountName".to_string());
        }

        // 4. Validation parity: DecodeUtil.addressValid(ownerAddress)
        // Java requires: len == 21 AND prefix == configured addressPreFixByte
        // This matches DecodeUtil.addressValid exactly:
        //   - Empty address → false (warn: "Address is empty")
        //   - len != 21 → false (warn: "Address length need 42 but...")
        //   - prefix != addressPreFixByte → false (warn: "Address need prefix with...")
        let from_raw = transaction
            .metadata
            .from_raw
            .as_deref()
            .ok_or_else(|| "Invalid ownerAddress".to_string())?;

        let expected_prefix = storage_adapter.address_prefix();
        let owner_address_valid = from_raw.len() == 21 && from_raw[0] == expected_prefix;
        if !owner_address_valid {
            return Err("Invalid ownerAddress".to_string());
        }

        // 5. Validate consistency: decoded owner_address should match from_raw
        if let Some(ref decoded_owner) = decoded_owner_address {
            if !decoded_owner.is_empty() && decoded_owner.as_slice() != from_raw {
                warn!(
                    "AccountUpdate: decoded owner_address differs from from_raw: decoded_len={} from_raw_len={}",
                    decoded_owner.len(),
                    from_raw.len()
                );
                // For strict parity, we could reject here, but Java uses decoded proto internally
                // and the request mapping ensures they match. Log warning but proceed.
            }
        }

        // 6. Validation: owner account must exist (java: "Account does not exist")
        let owner_account = storage_adapter
            .get_account(&transaction.from)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| "Account does not exist".to_string())?;

        // 7. Validation: only-set-once + duplicate name checks depend on ALLOW_UPDATE_ACCOUNT_NAME
        let owner_proto = storage_adapter
            .get_account_proto(&transaction.from)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| "Account does not exist".to_string())?;

        let allow_update_account_name = storage_adapter
            .get_allow_update_account_name()
            .map_err(|e| format!("Failed to get ALLOW_UPDATE_ACCOUNT_NAME: {}", e))?;

        if allow_update_account_name == 0 && !owner_proto.account_name.is_empty() {
            return Err("This account name is already existed".to_string());
        }

        if allow_update_account_name == 0
            && storage_adapter
                .account_index_has(name_bytes)
                .map_err(|e| format!("Failed to check account-index: {}", e))?
        {
            return Err("This name is existed".to_string());
        }

        // 8. Apply: persist account name and update account-index (name -> address).
        storage_adapter
            .set_account_name(transaction.from, name_bytes)
            .map_err(|e| format!("Failed to set account name: {}", e))?;

        // 9. State Changes: emit exactly one account-level change for CSV parity
        // old_account == new_account (no balance/nonce/code changes) to match embedded journaled no-op
        let state_changes = vec![TronStateChange::AccountChange {
            address: transaction.from,
            old_account: Some(owner_account.clone()),
            new_account: Some(owner_account), // Same account, name is metadata outside AccountInfo
        }];

        // 10. Calculate bandwidth based on transaction payload size
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // 11. AEXT tracking for end-to-end parity (bandwidth/resource accounting)
        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();

        let mut aext_map = std::collections::HashMap::new();
        if aext_mode == "tracked" {
            use tron_backend_execution::{AccountAext, ResourceTracker};

            // Get current AEXT for owner (or initialize with proper defaults including window size 28800)
            let current_aext = storage_adapter
                .get_account_aext(&transaction.from)
                .map_err(|e| format!("Failed to get owner AEXT: {}", e))?
                .unwrap_or_else(|| AccountAext::with_defaults());

            // Get free net limit from dynamic properties (default: 5000)
            let free_net_limit = storage_adapter
                .get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;

            // Java uses headSlot = block_timestamp_ms / 3000 for resource windows.
            let now_slot = (context.block_timestamp / 3000) as i64;

            // Track bandwidth usage
            let (_path, before_aext, after_aext) = ResourceTracker::track_bandwidth(
                &transaction.from,
                bandwidth_used as i64,
                now_slot,
                &current_aext,
                free_net_limit,
            )
            .map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist after AEXT to storage
            storage_adapter
                .set_account_aext(&transaction.from, &after_aext)
                .map_err(|e| format!("Failed to persist account AEXT: {}", e))?;
            storage_adapter
                .apply_bandwidth_aext_to_account_proto(&transaction.from, &after_aext)
                .map_err(|e| {
                    format!("Failed to persist bandwidth usage to account proto: {}", e)
                })?;

            // Add to aext_map
            aext_map.insert(transaction.from, (before_aext.clone(), after_aext.clone()));

            debug!(
                "AEXT tracked for account_update: owner={}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}",
                owner_tron,
                before_aext.net_usage,
                after_aext.net_usage,
                before_aext.free_net_usage,
                after_aext.free_net_usage
            );
        }

        // Result: success with energy_used=0, state change, and AEXT tracking
        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(), // No return data for account update
            energy_used: 0,                             // Account update uses zero energy
            bandwidth_used,                             // Compute bandwidth from payload size
            state_changes,                              // Exactly one account-level change
            logs: vec![],                               // No logs for account update
            error: None,
            aext_map,               // Populated with tracked AEXT if mode is "tracked"
            freeze_changes: vec![], // Not applicable for account update
            global_resource_changes: vec![], // Not applicable for account update
            trc10_changes: vec![],  // Not applicable for account update
            vote_changes: vec![],   // Not applicable for account update
            withdraw_changes: vec![], // Not applicable for account update
            tron_transaction_result: None,
            contract_address: None,
        })
    }

    /// Execute an ACCOUNT_CREATE_CONTRACT
    /// Creates a new account with proper fee charging and blackhole handling
    /// Parity with Java CreateAccountActuator:
    /// - Validates owner exists and has sufficient balance
    /// - Validates target account does not exist
    /// - Charges CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT
    /// - Creates new account with default values
    /// - Burns or credits blackhole based on dynamic property
    fn execute_account_create_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::{TronExecutionResult, TronStateChange};

        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.AccountCreateContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [AccountCreateContract],real type[class com.google.protobuf.Any]",
        )?;

        // 1. Parse AccountCreateContract
        // AccountCreateContract protobuf:
        //   bytes owner_address = 1;
        //   bytes account_address = 2; (target account to create)
        //   AccountType type = 3;      (account type enum)

        // Get expected address prefix for strict validation (matches Java DecodeUtil.addressValid)
        let expected_prefix = storage_adapter.address_prefix();
        let (owner, target_address, account_type) =
            self.parse_account_create_contract(contract_bytes, expected_prefix)?;
        let owner_tron = tron_backend_common::to_tron_address(&owner);
        let owner_tron_21 = storage_adapter.to_tron_address_21(&owner);
        let readable_owner_address = revm_primitives::hex::encode(owner_tron_21);

        info!("AccountCreate owner={}", owner_tron);
        let target_tron = tron_backend_common::to_tron_address(&target_address);

        info!(
            "AccountCreate: owner={}, target={}, type={}",
            owner_tron, target_tron, account_type
        );

        // 2. Validate owner account exists
        let owner_account = storage_adapter
            .get_account(&owner)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or_else(|| {
                let msg = format!("Account[{}] not exists", readable_owner_address);
                warn!("{}", msg);
                msg
            })?;

        // 3. Validate target account does NOT exist
        let target_exists = storage_adapter
            .get_account(&target_address)
            .map_err(|e| format!("Failed to check target account: {}", e))?
            .is_some();

        if target_exists {
            warn!("Account has existed: {}", target_tron);
            return Err("Account has existed".to_string());
        }

        // 4. Get fee from dynamic properties
        let execution_config_for_strict = self.get_execution_config()?;
        let strict = execution_config_for_strict
            .remote
            .strict_dynamic_properties;

        let fee = if strict {
            storage_adapter
                .get_create_new_account_fee_in_system_contract_strict()
                .map_err(|e| {
                    format!(
                        "Failed to get CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT: {}",
                        e
                    )
                })?
        } else {
            storage_adapter
                .get_create_new_account_fee_in_system_contract()
                .map_err(|e| {
                    format!(
                        "Failed to get CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT: {}",
                        e
                    )
                })?
        };

        info!("AccountCreate fee: {} SUN", fee);

        // 5. Validate sufficient balance
        let fee_u256 = revm_primitives::U256::from(fee);
        if owner_account.balance < fee_u256 {
            warn!(
                "Validate CreateAccountActuator error, insufficient fee. need={}, have={}",
                fee, owner_account.balance
            );
            return Err("Validate CreateAccountActuator error, insufficient fee.".to_string());
        }

        // 6. Get blackhole optimization flag
        let support_blackhole = if strict {
            storage_adapter
                .support_black_hole_optimization_strict()
                .map_err(|e| format!("Failed to get SupportBlackHoleOptimization: {}", e))?
        } else {
            storage_adapter
                .support_black_hole_optimization()
                .map_err(|e| format!("Failed to get SupportBlackHoleOptimization: {}", e))?
        };

        info!(
            "AccountCreate: fee={} SUN, support_blackhole={}",
            fee, support_blackhole
        );

        // 7. Prepare state changes
        let mut state_changes = Vec::new();

        // 8. Update owner account - deduct fee (only if fee > 0)
        if fee > 0 {
            let new_owner_account = revm_primitives::AccountInfo {
                balance: owner_account.balance - fee_u256,
                nonce: owner_account.nonce,
                code_hash: owner_account.code_hash,
                code: owner_account.code.clone(),
            };

            // Emit owner account change
            state_changes.push(TronStateChange::AccountChange {
                address: owner,
                old_account: Some(owner_account),
                new_account: Some(new_owner_account.clone()),
            });

            // Persist owner account update
            storage_adapter
                .set_account(owner, new_owner_account.clone())
                .map_err(|e| format!("Failed to persist owner account: {}", e))?;
        }

        // 9. Create new target account with default values
        let new_target_account = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::ZERO,
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        // Emit target account change (is_creation = true since old_account is None)
        state_changes.push(TronStateChange::AccountChange {
            address: target_address,
            old_account: None, // Account creation
            new_account: Some(new_target_account.clone()),
        });

        // Persist new account (include create_time and account_type for fixture parity).
        // Java's AccountCapsule(AccountCreateContract, ...) stores the contract's type field.
        use tron_backend_execution::protocol::permission::PermissionType;
        use tron_backend_execution::protocol::{Account as ProtoAccount, Key, Permission};
        let create_time = if strict {
            storage_adapter
                .get_latest_block_header_timestamp_strict()
                .map_err(|e| format!("Failed to get latest_block_header_timestamp: {}", e))?
        } else {
            storage_adapter
                .get_latest_block_header_timestamp()
                .map_err(|e| format!("Failed to get latest_block_header_timestamp: {}", e))?
        };
        let target_address_bytes = storage_adapter.to_tron_address_21(&target_address).to_vec();

        let allow_multi_sign = storage_adapter
            .get_allow_multi_sign()
            .map_err(|e| format!("Failed to get ALLOW_MULTI_SIGN: {}", e))?;

        let mut target_proto = ProtoAccount {
            address: target_address_bytes.clone(),
            create_time,
            r#type: account_type, // Respect contract's type field (matches Java AccountCapsule)
            ..Default::default()
        };

        if allow_multi_sign {
            let active_default_operations = storage_adapter
                .get_active_default_operations()
                .map_err(|e| format!("Failed to get ACTIVE_DEFAULT_OPERATIONS: {}", e))?;

            let default_key = Key {
                address: target_address_bytes.clone(),
                weight: 1,
            };

            target_proto.owner_permission = Some(Permission {
                r#type: PermissionType::Owner as i32,
                id: 0,
                permission_name: "owner".to_string(),
                threshold: 1,
                parent_id: 0,
                operations: Vec::new(),
                keys: vec![default_key.clone()],
            });

            target_proto.active_permission = vec![Permission {
                r#type: PermissionType::Active as i32,
                id: 2,
                permission_name: "active".to_string(),
                threshold: 1,
                parent_id: 0,
                operations: active_default_operations,
                keys: vec![default_key],
            }];
        }
        storage_adapter
            .put_account_proto(&target_address, &target_proto)
            .map_err(|e| format!("Failed to persist new account proto: {}", e))?;

        // 10. Handle fee burning/crediting (only if fee > 0)
        let fee_destination: String;
        if fee == 0 {
            // No fee to process
            fee_destination = String::from("none(fee=0)");
        } else if support_blackhole {
            // Burn mode - no additional account change needed
            info!("Burning {} SUN (blackhole optimization)", fee);
            storage_adapter
                .burn_trx(fee)
                .map_err(|e| format!("Failed to burn TRX: {}", e))?;
            fee_destination = String::from("burn");
        } else {
            // Credit blackhole account
            if let Some(blackhole_addr) = storage_adapter
                .get_blackhole_address()
                .map_err(|e| format!("Failed to get blackhole address: {}", e))?
            {
                let blackhole_account = storage_adapter
                    .get_account(&blackhole_addr)
                    .map_err(|e| format!("Failed to load blackhole account: {}", e))?
                    .unwrap_or_default();

                let new_blackhole_account = revm_primitives::AccountInfo {
                    balance: blackhole_account.balance + fee_u256,
                    nonce: blackhole_account.nonce,
                    code_hash: blackhole_account.code_hash,
                    code: blackhole_account.code.clone(),
                };

                // Emit account change for blackhole
                state_changes.push(TronStateChange::AccountChange {
                    address: blackhole_addr,
                    old_account: Some(blackhole_account),
                    new_account: Some(new_blackhole_account.clone()),
                });

                // Persist blackhole account update
                storage_adapter
                    .set_account(blackhole_addr, new_blackhole_account)
                    .map_err(|e| format!("Failed to persist blackhole account: {}", e))?;

                let bh_tron = tron_backend_common::to_tron_address(&blackhole_addr);
                info!("Credited {} SUN to blackhole address {}", fee, bh_tron);
                fee_destination = format!("blackhole:{}", bh_tron);
            } else {
                warn!("No blackhole address configured, burning {} SUN", fee);
                fee_destination = String::from("burn(no_addr)");
            }
        }

        // 11. Sort state changes deterministically for CSV parity
        state_changes.sort_by(|a, b| match (a, b) {
            (
                TronStateChange::AccountChange {
                    address: addr_a, ..
                },
                TronStateChange::AccountChange {
                    address: addr_b, ..
                },
            ) => addr_a.cmp(addr_b),
            _ => std::cmp::Ordering::Equal,
        });

        // 12. Calculate bandwidth usage with CREATE_NEW_ACCOUNT_BANDWIDTH_RATE multiplier
        // Java BandwidthProcessor: netCost = bytes * createNewAccountBandwidthRatio
        let raw_bandwidth_bytes = Self::calculate_bandwidth_usage(transaction);
        let bandwidth_rate = if strict {
            storage_adapter
                .get_create_new_account_bandwidth_rate_strict()
                .map_err(|e| format!("Failed to get CREATE_NEW_ACCOUNT_BANDWIDTH_RATE: {}", e))?
        } else {
            storage_adapter
                .get_create_new_account_bandwidth_rate()
                .map_err(|e| format!("Failed to get CREATE_NEW_ACCOUNT_BANDWIDTH_RATE: {}", e))?
        };
        let net_cost = (raw_bandwidth_bytes as i64).saturating_mul(bandwidth_rate);

        info!(
            "AccountCreate bandwidth: raw_bytes={}, rate={}, netCost={}",
            raw_bandwidth_bytes, bandwidth_rate, net_cost
        );

        // 13. Track AEXT for bandwidth if in tracked mode
        // Implements Java BandwidthProcessor create-account path selection:
        // 1. Try ACCOUNT_NET (if account has frozen bandwidth)
        // 2. Try FREE_NET (if public bandwidth available)
        // 3. Fall back to FEE (charge CREATE_ACCOUNT_FEE)
        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();
        let mut aext_map = std::collections::HashMap::new();
        let mut bandwidth_path_used = "none";
        let mut create_account_fee_charged: u64 = 0;

        if aext_mode == "tracked" {
            use tron_backend_execution::{AccountAext, BandwidthPath, ResourceTracker};

            // Get current AEXT for owner (or initialize with defaults)
            let current_aext = storage_adapter
                .get_account_aext(&owner)
                .map_err(|e| format!("Failed to get account AEXT: {}", e))?
                .unwrap_or_else(|| AccountAext::with_defaults());

            // Get FREE_NET_LIMIT from dynamic properties
            let free_net_limit = if strict {
                storage_adapter
                    .get_free_net_limit_strict()
                    .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?
            } else {
                storage_adapter
                    .get_free_net_limit()
                    .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?
            };

            // Java uses headSlot = block_timestamp_ms / 3000 for resource windows.
            let now_slot = (context.block_timestamp / 3000) as i64;

            // Track bandwidth usage with netCost (not raw bytes) - matches Java semantics
            let (path, before_aext, after_aext) = ResourceTracker::track_bandwidth(
                &owner,
                net_cost, // Use netCost (bytes * rate) for create-account
                now_slot,
                &current_aext,
                free_net_limit,
            )
            .map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            bandwidth_path_used = match path {
                BandwidthPath::AccountNet => "ACCOUNT_NET",
                BandwidthPath::CreateAccount => "CREATE_ACCOUNT",
                BandwidthPath::FreeNet => "FREE_NET",
                BandwidthPath::Fee => "FEE",
            };

            // If fee path is used, charge CREATE_ACCOUNT_FEE and update TOTAL_CREATE_ACCOUNT_COST
            // This is separate from CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT
            if path == BandwidthPath::Fee {
                create_account_fee_charged = if strict {
                    storage_adapter
                        .get_create_account_fee_strict()
                        .map_err(|e| format!("Failed to get CREATE_ACCOUNT_FEE: {}", e))?
                } else {
                    storage_adapter
                        .get_create_account_fee()
                        .map_err(|e| format!("Failed to get CREATE_ACCOUNT_FEE: {}", e))?
                };

                // Validate owner has sufficient balance for CREATE_ACCOUNT_FEE
                // This matches Java BandwidthProcessor.payFee() which checks balance before deduction
                // Get current owner balance (after actuator fee deduction if any)
                let current_owner_balance = storage_adapter
                    .get_account(&owner)
                    .map_err(|e| format!("Failed to reload owner account: {}", e))?
                    .map(|acc| acc.balance.as_limbs()[0]) // Get u64 balance
                    .unwrap_or(0);

                if current_owner_balance < create_account_fee_charged {
                    // Calculate available bandwidth for error message
                    let available_bandwidth =
                        free_net_limit.saturating_sub(after_aext.free_net_usage);
                    let error_msg = format!(
                        "account [{}] has insufficient bandwidth[{}] and balance[{}] to create new account",
                        owner_tron,
                        available_bandwidth,
                        current_owner_balance
                    );
                    warn!("{}", error_msg);
                    return Err(error_msg);
                }

                info!(
                    "AccountCreate bandwidth insufficient, using fee fallback: CREATE_ACCOUNT_FEE={} SUN",
                    create_account_fee_charged
                );

                // Update TOTAL_CREATE_ACCOUNT_COST dynamic property
                // In strict mode, verify the key exists before updating
                if strict {
                    let _ = storage_adapter
                        .get_total_create_account_cost_strict()
                        .map_err(|e| format!("Failed to get TOTAL_CREATE_ACCOUNT_COST: {}", e))?;
                }
                storage_adapter
                    .add_total_create_account_cost(create_account_fee_charged)
                    .map_err(|e| format!("Failed to update TOTAL_CREATE_ACCOUNT_COST: {}", e))?;

                // Note: The CREATE_ACCOUNT_FEE is already deducted by Java BandwidthProcessor
                // before the actuator runs. In remote mode, the actual fee deduction happens
                // on the Java side, so we just track the path here.
            }

            // Persist after AEXT to storage
            storage_adapter
                .set_account_aext(&owner, &after_aext)
                .map_err(|e| format!("Failed to persist account AEXT: {}", e))?;
            storage_adapter
                .apply_bandwidth_aext_to_account_proto(&owner, &after_aext)
                .map_err(|e| {
                    format!("Failed to persist bandwidth usage to account proto: {}", e)
                })?;

            // Add to aext_map
            aext_map.insert(owner, (before_aext.clone(), after_aext.clone()));

            debug!(
                "AEXT tracked for account_create: owner={:?}, path={}, netCost={}, before_net_usage={}, after_net_usage={}",
                owner, bandwidth_path_used, net_cost, before_aext.net_usage, after_aext.net_usage
            );
        }

        // 14. Build receipt passthrough (matches Java ret.setStatus(fee, SUCESS))
        let tron_transaction_result = TransactionResultBuilder::new().with_fee(fee as i64).build();

        info!(
            "AccountCreate completed: actuator_fee={} SUN, bandwidth_path={}, create_account_fee={}, state_changes={}, owner={}, target={}, fee_dest={}",
            fee, bandwidth_path_used, create_account_fee_charged, state_changes.len(), owner_tron, target_tron, fee_destination
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,                      // System contracts use 0 energy
            bandwidth_used: raw_bandwidth_bytes, // Report raw bytes for bandwidth_used field
            logs: Vec::new(),                    // No logs for account creation
            state_changes,
            error: None,
            aext_map,
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: Some(tron_transaction_result), // Receipt passthrough
            contract_address: None,
        })
    }

    /// Parse AccountCreateContract from protobuf bytes
    /// AccountCreateContract structure:
    ///   bytes owner_address = 1;   (field 1, length-delimited)
    ///   bytes account_address = 2; (field 2, length-delimited) - target account
    ///   AccountType type = 3;      (field 3, varint) - account type enum
    ///
    /// Returns (owner_address, account_address, account_type)
    /// account_type defaults to 0 (Normal) if not present
    fn parse_account_create_contract(
        &self,
        data: &[u8],
        expected_prefix: u8,
    ) -> Result<(revm::primitives::Address, revm::primitives::Address, i32), String> {
        let mut owner_address_bytes: Option<Vec<u8>> = None;
        let mut account_address_bytes: Option<Vec<u8>> = None;
        let mut account_type: i32 = 0; // Default: Normal
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address_bytes = Some(payload.to_vec());
                    pos += total_len;
                }
                (2, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    account_address_bytes = Some(payload.to_vec());
                    pos += total_len;
                }
                (3, 0) => {
                    let (type_val, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    account_type = type_val as i32;
                    pos += bytes_read;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        let owner_address_bytes =
            owner_address_bytes.ok_or_else(|| "Invalid ownerAddress".to_string())?;
        let account_address_bytes =
            account_address_bytes.ok_or_else(|| "Invalid account address".to_string())?;

        // Validate address with configured prefix (matches Java DecodeUtil.addressValid semantics)
        fn parse_tron_prefixed_address(
            bytes: &[u8],
            expected_prefix: u8,
        ) -> Result<revm::primitives::Address, &'static str> {
            if bytes.len() != 21 {
                return Err("length");
            }
            let prefix = bytes[0];
            if prefix != expected_prefix {
                return Err("prefix");
            }
            Ok(revm::primitives::Address::from_slice(&bytes[1..]))
        }

        let owner_address = parse_tron_prefixed_address(&owner_address_bytes, expected_prefix)
            .map_err(|_| "Invalid ownerAddress".to_string())?;
        let account_address = parse_tron_prefixed_address(&account_address_bytes, expected_prefix)
            .map_err(|_| "Invalid account address".to_string())?;

        Ok((owner_address, account_address, account_type))
    }

    // =========================================================================
    // Phase 2.A: Proposal Contracts (16/17/18)
    // =========================================================================
    // These contracts handle TRON governance proposals (parameter changes).
    // Java reference: ProposalCreateActuator, ProposalApproveActuator, ProposalDeleteActuator

    /// Check if a proposal parameter code is supported.
    /// Delegates to the proposal module's ProposalType enum.
    #[allow(dead_code)]
    fn is_supported_proposal_parameter_code(code: i64) -> bool {
        contracts::proposal::ProposalType::from_code(code).is_some()
    }

    /// Execute a PROPOSAL_CREATE_CONTRACT
    /// Creates a new proposal with specified parameters
    ///
    /// Java reference: ProposalCreateActuator.java
    fn execute_proposal_create_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::protocol::Proposal;
        use tron_backend_execution::TronExecutionResult;

        // Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.ProposalCreateContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [ProposalCreateContract],real type[class com.google.protobuf.Any]",
        )?;

        // 1. Parse ProposalCreateContract from contract_bytes
        // ProposalCreateContract:
        //   bytes owner_address = 1;
        //   map<int64, int64> parameters = 2;
        let (owner_address_bytes, parameters) =
            self.parse_proposal_create_contract(contract_bytes)?;
        let readable_owner_address = hex::encode(&owner_address_bytes);

        // 2. Validate owner address (java-tron: DecodeUtil.addressValid)
        if owner_address_bytes.len() != 21
            || owner_address_bytes[0] != storage_adapter.address_prefix()
        {
            return Err("Invalid address".to_string());
        }
        let owner = revm_primitives::Address::from_slice(&owner_address_bytes[1..]);
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("ProposalCreate owner={}", owner_tron);

        // 3. Validate owner exists and is a witness
        // Java: AccountStore.has(owner) then WitnessStore.has(owner)
        let account_exists = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .is_some();
        if !account_exists {
            warn!("Account {} does not exist", owner_tron);
            return Err(format!("Account[{}] not exists", readable_owner_address));
        }

        let is_witness = storage_adapter
            .is_witness(&owner)
            .map_err(|e| format!("Failed to check witness status: {}", e))?;
        if !is_witness {
            warn!("Witness {} does not exist", owner_tron);
            return Err(format!("Witness[{}] not exists", readable_owner_address));
        }

        if parameters.is_empty() {
            warn!("ProposalCreate: empty parameters");
            return Err("This proposal has no parameter.".to_string());
        }

        info!("ProposalCreate: {} parameters", parameters.len());

        // 4. Validate proposal parameter values (java-tron: ProposalUtil.validator)
        // Full parity with Java's ProposalUtil.validator() including fork gating,
        // prerequisites, and "already active" checks.
        for (&code, &value) in parameters.iter() {
            contracts::proposal::validate_proposal_parameter(storage_adapter, code, value)?;
        }

        // 5. Get next proposal ID
        let latest_proposal_num = storage_adapter
            .get_latest_proposal_num()
            .map_err(|e| format!("Failed to get LATEST_PROPOSAL_NUM: {}", e))?;
        let new_proposal_id = latest_proposal_num + 1;

        // 6. Calculate create/expiration time (java-tron parity)
        // Java reference: ProposalCreateActuator.execute()
        //   now = dynamicStore.getLatestBlockHeaderTimestamp()
        //   currentMaintenanceTime = dynamicStore.getNextMaintenanceTime()
        //   maintenanceTimeInterval = dynamicStore.getMaintenanceTimeInterval()
        //   now3 = now + CommonParameter.getInstance().getProposalExpireTime()
        //   round = (now3 - currentMaintenanceTime) / maintenanceTimeInterval
        //   expirationTime = currentMaintenanceTime + (round + 1) * maintenanceTimeInterval
        let execution_config = self.get_execution_config()?;
        let proposal_expire_time_ms = execution_config.remote.proposal_expire_time_ms as i64;

        let now = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest_block_header_timestamp: {}", e))?;
        let current_maintenance_time = storage_adapter
            .get_next_maintenance_time()
            .map_err(|e| format!("Failed to get NEXT_MAINTENANCE_TIME: {}", e))?;
        let maintenance_time_interval = storage_adapter
            .get_maintenance_time_interval()
            .map_err(|e| format!("Failed to get MAINTENANCE_TIME_INTERVAL: {}", e))?;

        let now3 = now + proposal_expire_time_ms;
        let round = (now3 - current_maintenance_time) / maintenance_time_interval;
        let expiration_time = current_maintenance_time + (round + 1) * maintenance_time_interval;

        // 7. Create new Proposal
        let proposal = Proposal {
            proposal_id: new_proposal_id,
            proposer_address: owner_address_bytes,
            parameters,
            expiration_time,
            create_time: now,
            approvals: Vec::new(),
            state: 0, // PENDING
        };

        // 8. Persist proposal
        storage_adapter
            .put_proposal(&proposal)
            .map_err(|e| format!("Failed to persist proposal: {}", e))?;

        // 9. Update LATEST_PROPOSAL_NUM
        storage_adapter
            .set_latest_proposal_num(new_proposal_id)
            .map_err(|e| format!("Failed to update LATEST_PROPOSAL_NUM: {}", e))?;

        info!(
            "ProposalCreate completed: id={}, expiration={}, params={}",
            new_proposal_id,
            expiration_time,
            proposal.parameters.len()
        );

        // Calculate bandwidth
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            logs: Vec::new(),
            state_changes: Vec::new(), // Proposal changes are persisted directly
            error: None,
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

    /// Parse ProposalCreateContract from protobuf bytes
    /// ProposalCreateContract:
    ///   bytes owner_address = 1;
    ///   map<int64, int64> parameters = 2;
    fn parse_proposal_create_contract(
        &self,
        data: &[u8],
    ) -> Result<(Vec<u8>, std::collections::BTreeMap<i64, i64>), String> {
        let mut owner_address_bytes: Vec<u8> = Vec::new();
        let mut parameters = std::collections::BTreeMap::new();
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address_bytes = payload.to_vec();
                    pos += total_len;
                }
                (2, 2) => {
                    let (entry_data, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += total_len;
                    let (key, value) = self.parse_map_entry_i64_i64(entry_data)?;
                    parameters.insert(key, value);
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        Ok((owner_address_bytes, parameters))
    }

    /// Parse a map entry with int64 key and int64 value
    fn parse_map_entry_i64_i64(&self, data: &[u8]) -> Result<(i64, i64), String> {
        let mut key: Option<i64> = None;
        let mut value: Option<i64> = None;
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 0) => {
                    let (v, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    key = Some(v as i64);
                }
                (2, 0) => {
                    let (v, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    value = Some(v as i64);
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        let k = key.ok_or("Missing map key")?;
        let v = value.ok_or("Missing map value")?;
        Ok((k, v))
    }

    /// Execute a PROPOSAL_APPROVE_CONTRACT
    /// Adds or removes approval from a proposal
    ///
    /// Java reference: ProposalApproveActuator.java
    fn execute_proposal_approve_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::TronExecutionResult;

        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.ProposalApproveContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [ProposalApproveContract],real type[class com.google.protobuf.Any]",
        )?;
        let (owner_address_bytes, proposal_id, is_add_approval) =
            self.parse_proposal_approve_contract(contract_bytes)?;
        let readable_owner_address = hex::encode(&owner_address_bytes);

        // 2. Validate owner address (java-tron: DecodeUtil.addressValid)
        if owner_address_bytes.len() != 21
            || owner_address_bytes[0] != storage_adapter.address_prefix()
        {
            return Err("Invalid address".to_string());
        }

        let owner = revm_primitives::Address::from_slice(&owner_address_bytes[1..]);
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("ProposalApprove owner={}", owner_tron);

        // 3. Validate owner exists and is a witness (java-tron parity)
        let account_exists = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .is_some();
        if !account_exists {
            warn!("Account {} does not exist", owner_tron);
            return Err(format!("Account[{}] not exists", readable_owner_address));
        }

        let is_witness = storage_adapter
            .is_witness(&owner)
            .map_err(|e| format!("Failed to check witness status: {}", e))?;
        if !is_witness {
            warn!("Witness {} does not exist", owner_tron);
            return Err(format!("Witness[{}] not exists", readable_owner_address));
        }

        info!(
            "ProposalApprove: id={}, is_add={}",
            proposal_id, is_add_approval
        );

        // 3. Validate proposal exists (java-tron parity checks LATEST_PROPOSAL_NUM first)
        let latest_proposal_num = storage_adapter
            .get_latest_proposal_num()
            .map_err(|e| format!("Failed to get LATEST_PROPOSAL_NUM: {}", e))?;
        if proposal_id > latest_proposal_num {
            return Err(format!("Proposal[{}] not exists", proposal_id));
        }

        // Get proposal with raw bytes to preserve unknown protobuf fields (Java parity)
        // Java's toBuilder()...build() preserves unknown fields; we do the same via surgical update.
        let (mut proposal, raw_bytes) = storage_adapter
            .get_proposal_with_raw(proposal_id)
            .map_err(|e| format!("Failed to get proposal: {}", e))?
            .ok_or_else(|| format!("Proposal[{}] not exists", proposal_id))?;

        // 4. Validate expiration / canceled status
        let now = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest_block_header_timestamp: {}", e))?;
        if now >= proposal.expiration_time {
            return Err(format!("Proposal[{}] expired", proposal_id));
        }
        if proposal.state == 3 {
            return Err(format!("Proposal[{}] canceled", proposal_id));
        }

        // 5. Validate approval add/remove semantics
        if is_add_approval {
            if proposal.approvals.iter().any(|a| a == &owner_address_bytes) {
                return Err(format!(
                    "Witness[{}]has approved proposal[{}] before",
                    readable_owner_address, proposal_id
                ));
            }
        } else if !proposal.approvals.iter().any(|a| a == &owner_address_bytes) {
            return Err(format!(
                "Witness[{}]has not approved proposal[{}] before",
                readable_owner_address, proposal_id
            ));
        }

        // 6. Add or remove approval
        if is_add_approval {
            proposal.approvals.push(owner_address_bytes.clone());
        } else {
            // Java parity: removeApproval() removes only the FIRST matching entry
            // (using ArrayList.remove(Object) semantics), not all occurrences.
            // This matters if corrupted/non-canonical DB ever contains duplicates.
            if let Some(idx) = proposal
                .approvals
                .iter()
                .position(|a| a == &owner_address_bytes)
            {
                proposal.approvals.remove(idx);
            }
        }

        // 7. Persist updated proposal using surgical update to preserve unknown protobuf fields
        // Java's toBuilder()...build() preserves unknown fields when rebuilding the protobuf.
        // We achieve the same by surgically replacing only field 6 (approvals) in the raw bytes.
        let updated_bytes = storage_adapter
            .surgical_update_proposal_approvals(&raw_bytes, &proposal.approvals)
            .map_err(|e| format!("Failed to update proposal approvals: {}", e))?;
        storage_adapter
            .put_proposal_raw(proposal_id, updated_bytes)
            .map_err(|e| format!("Failed to persist proposal: {}", e))?;

        info!(
            "ProposalApprove completed: id={}, approvals={}",
            proposal_id,
            proposal.approvals.len()
        );

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            logs: Vec::new(),
            state_changes: Vec::new(),
            error: None,
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

    /// Parse ProposalApproveContract from protobuf bytes
    /// ProposalApproveContract:
    ///   bytes owner_address = 1;
    ///   int64 proposal_id = 2;
    ///   bool is_add_approval = 3;
    fn parse_proposal_approve_contract(&self, data: &[u8]) -> Result<(Vec<u8>, i64, bool), String> {
        let mut owner_address_bytes: Vec<u8> = Vec::new();
        let mut proposal_id: i64 = 0;
        let mut is_add_approval = false;
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address_bytes = payload.to_vec();
                    pos += total_len;
                }
                (2, 0) => {
                    let (v, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    proposal_id = v as i64;
                }
                (3, 0) => {
                    let (v, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    is_add_approval = v != 0;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        Ok((owner_address_bytes, proposal_id, is_add_approval))
    }

    /// Execute a PROPOSAL_DELETE_CONTRACT
    /// Cancels a proposal (only by the proposer)
    ///
    /// Java reference: ProposalDeleteActuator.java
    fn execute_proposal_delete_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::TronExecutionResult;

        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.ProposalDeleteContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [ProposalDeleteContract],real type[class com.google.protobuf.Any]",
        )?;

        // 1. Parse ProposalDeleteContract
        // ProposalDeleteContract:
        //   bytes owner_address = 1;
        //   int64 proposal_id = 2;
        let (owner_address_bytes, proposal_id) =
            self.parse_proposal_delete_contract(contract_bytes)?;
        let readable_owner_address = hex::encode(&owner_address_bytes);

        // 2. Validate owner address (java-tron: DecodeUtil.addressValid)
        let expected_prefix = storage_adapter.address_prefix();
        if owner_address_bytes.len() != 21 || owner_address_bytes[0] != expected_prefix {
            return Err("Invalid address".to_string());
        }

        let owner = revm_primitives::Address::from_slice(&owner_address_bytes[1..]);
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("ProposalDelete owner={}", owner_tron);

        // 3. Validate owner exists
        let account_exists = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .is_some();
        if !account_exists {
            warn!("Account {} does not exist", owner_tron);
            return Err(format!("Account[{}] not exists", readable_owner_address));
        }

        info!("ProposalDelete: id={}", proposal_id);

        // 4. Validate proposal exists (java-tron parity checks LATEST_PROPOSAL_NUM first)
        let latest_proposal_num = storage_adapter
            .get_latest_proposal_num()
            .map_err(|e| format!("Failed to get LATEST_PROPOSAL_NUM: {}", e))?;
        if proposal_id > latest_proposal_num {
            return Err(format!("Proposal[{}] not exists", proposal_id));
        }

        // Use get_proposal_with_raw to preserve original bytes for byte-level parity.
        // This avoids re-encoding the proposal (which would reorder BTreeMap parameters).
        let (proposal, raw_bytes) = storage_adapter
            .get_proposal_with_raw(proposal_id)
            .map_err(|e| format!("Failed to get proposal: {}", e))?
            .ok_or_else(|| format!("Proposal[{}] not exists", proposal_id))?;

        // 5. Validate owner is the proposer
        if proposal.proposer_address != owner_address_bytes {
            return Err(format!(
                "Proposal[{}] is not proposed by {}",
                proposal_id, readable_owner_address
            ));
        }

        // 6. Validate expiration / canceled status
        let now = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest_block_header_timestamp: {}", e))?;
        if now >= proposal.expiration_time {
            return Err(format!("Proposal[{}] expired", proposal_id));
        }
        if proposal.state == 3 {
            return Err(format!("Proposal[{}] canceled", proposal_id));
        }

        // 7. Surgically patch state to CANCELED (3) in raw bytes, preserving original
        //    parameter map ordering for byte-level parity with java-tron.
        let patched_bytes = storage_adapter
            .surgical_update_proposal_state(&raw_bytes, 3)
            .map_err(|e| format!("Failed to patch proposal state: {}", e))?;

        // 8. Persist updated proposal raw bytes
        storage_adapter
            .put_proposal_raw(proposal_id, patched_bytes)
            .map_err(|e| format!("Failed to persist proposal: {}", e))?;

        info!(
            "ProposalDelete completed: id={}, state=CANCELED",
            proposal_id
        );

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            logs: Vec::new(),
            state_changes: Vec::new(),
            error: None,
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

    /// Parse ProposalDeleteContract from protobuf bytes with Java-compatible error strings.
    /// ProposalDeleteContract:
    ///   bytes owner_address = 1;
    ///   int64 proposal_id = 2;
    ///
    /// Error handling matches protobuf-java 3.21.12 for exact parity:
    /// - Truncated input → PROTOBUF_TRUNCATED_MESSAGE
    /// - Malformed varint → PROTOBUF_MALFORMED_VARINT
    fn parse_proposal_delete_contract(&self, data: &[u8]) -> Result<(Vec<u8>, i64), String> {
        use contracts::proto::{
            read_varint_typed, skip_protobuf_field_checked, VarintError, PROTOBUF_MALFORMED_VARINT,
            PROTOBUF_TRUNCATED_MESSAGE,
        };

        let map_varint_error = |e: VarintError| -> String {
            match e {
                VarintError::Truncated => PROTOBUF_TRUNCATED_MESSAGE.to_string(),
                VarintError::TooLong => PROTOBUF_MALFORMED_VARINT.to_string(),
            }
        };

        let mut owner_address_bytes: Vec<u8> = Vec::new();
        let mut proposal_id: Option<i64> = None;
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) =
                read_varint_typed(&data[pos..]).map_err(&map_varint_error)?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = (field_header & 0x7) as u8;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address (bytes)
                    let (length, bytes_read) =
                        read_varint_typed(&data[pos..]).map_err(&map_varint_error)?;
                    pos += bytes_read;
                    if pos + length as usize > data.len() {
                        return Err(PROTOBUF_TRUNCATED_MESSAGE.to_string());
                    }
                    owner_address_bytes = data[pos..pos + length as usize].to_vec();
                    pos += length as usize;
                }
                (2, 0) => {
                    // proposal_id (int64, varint)
                    let (v, bytes_read) =
                        read_varint_typed(&data[pos..]).map_err(&map_varint_error)?;
                    pos += bytes_read;
                    proposal_id = Some(v as i64);
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message())?;
                    pos += skip_len;
                }
            }
        }

        // Proto3 default: absent proposal_id field defaults to 0, matching java-tron behavior.
        // Java's protobuf decoding returns 0 for absent int64 fields, so validation
        // proceeds to "Proposal[0] not exists" rather than a parse-level error.
        let id = proposal_id.unwrap_or(0);
        Ok((owner_address_bytes, id))
    }

    // ==========================================================================
    // Phase 2.B: Account Management Contracts (19/46)
    // ==========================================================================

    /// Execute a SET_ACCOUNT_ID_CONTRACT (type 19)
    /// Sets a unique, immutable account ID for an account
    ///
    /// Java reference: SetAccountIdActuator.java
    fn execute_set_account_id_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::TronExecutionResult;

        // Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.SetAccountIdContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [SetAccountIdContract],real type[class com.google.protobuf.Any]",
        )?;

        // 1. Parse SetAccountIdContract
        // SetAccountIdContract:
        //   bytes account_id = 1;
        //   bytes owner_address = 2;
        let (account_id, owner_address_bytes) =
            self.parse_set_account_id_contract(contract_bytes)?;

        // 2. Validate account ID format (java-tron: TransactionUtil.validAccountId)
        // Java validates accountId BEFORE ownerAddress.
        if !self.validate_account_id(&account_id) {
            return Err("Invalid accountId".to_string());
        }

        // 3. Validate owner address from contract bytes (java-tron: DecodeUtil.addressValid)
        // Java requires exactly 21 bytes with the correct TRON prefix (0x41).
        let expected_prefix = storage_adapter.address_prefix();
        if owner_address_bytes.len() != 21 || owner_address_bytes[0] != expected_prefix {
            return Err("Invalid ownerAddress".to_string());
        }

        // Derive the 20-byte EVM address from the parsed 21-byte TRON address
        let owner = revm_primitives::Address::from_slice(&owner_address_bytes[1..]);
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!(
            "SetAccountId: owner={}, account_id={:?}",
            owner_tron,
            String::from_utf8_lossy(&account_id)
        );

        // 4. Get owner account
        let mut account_proto = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or_else(|| "Account has not existed".to_string())?;

        // 5. Check if account already has an ID
        if !account_proto.account_id.is_empty() {
            return Err("This account id already set".to_string());
        }

        // 6. Check if account ID is already taken
        if storage_adapter
            .has_account_id(&account_id)
            .map_err(|e| format!("Failed to check account id: {}", e))?
        {
            return Err("This id has existed".to_string());
        }

        // 7. Set account ID
        account_proto.account_id = account_id.clone();

        // 8. Persist account
        storage_adapter
            .put_account_proto(&owner, &account_proto)
            .map_err(|e| format!("Failed to persist account: {}", e))?;

        // 9. Add to account ID index
        let owner_address_bytes = storage_adapter.to_tron_address_21(&owner).to_vec();

        storage_adapter
            .put_account_id_index(&account_id, &owner_address_bytes)
            .map_err(|e| format!("Failed to persist account id index: {}", e))?;

        info!(
            "SetAccountId completed: owner={}, account_id={:?}",
            owner_tron,
            String::from_utf8_lossy(&account_id)
        );

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            logs: Vec::new(),
            state_changes: Vec::new(),
            error: None,
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

    /// Parse SetAccountIdContract from protobuf bytes.
    /// Returns `(account_id, owner_address)`.
    ///
    /// SetAccountIdContract:
    ///   bytes account_id = 1;
    ///   bytes owner_address = 2;
    fn parse_set_account_id_contract(&self, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
        let mut account_id: Option<Vec<u8>> = None;
        let mut owner_address: Option<Vec<u8>> = None;
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    account_id = Some(payload.to_vec());
                    pos += total_len;
                }
                (2, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address = Some(payload.to_vec());
                    pos += total_len;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        // In proto3, empty bytes fields are omitted on the wire; treat missing as empty.
        Ok((
            account_id.unwrap_or_default(),
            owner_address.unwrap_or_default(),
        ))
    }

    /// Validate account ID format
    /// Java: TransactionUtil.validAccountId(accountId)
    /// Rules:
    /// - Length: 8-32 bytes
    /// - Readable ASCII characters only (from '!' to '~')
    fn validate_account_id(&self, account_id: &[u8]) -> bool {
        // validReadableBytes(accountId, 32) && accountId.length >= 8
        if account_id.len() < 8 || account_id.len() > 32 {
            return false;
        }

        // b must be readable: 0x21 = '!', 0x7E = '~'
        for &b in account_id {
            if b < 0x21 || b > 0x7E {
                return false;
            }
        }

        true
    }

    fn check_account_permission_update_permission(
        &self,
        storage_adapter: &tron_backend_execution::EngineBackedEvmStateStore,
        permission: &tron_backend_execution::protocol::Permission,
        address_prefix: u8,
    ) -> Result<(), String> {
        use std::collections::HashSet;
        use tron_backend_execution::protocol::permission::PermissionType;

        let total_sign_num = storage_adapter
            .get_total_sign_num()
            .map_err(|e| format!("Failed to get TOTAL_SIGN_NUM: {}", e))?;
        if permission.keys.len() as i64 > total_sign_num {
            return Err(format!(
                "number of keys in permission should not be greater than {}",
                total_sign_num
            ));
        }
        if permission.keys.is_empty() {
            return Err("key's count should be greater than 0".to_string());
        }

        let permission_type = PermissionType::from_i32(permission.r#type)
            .ok_or_else(|| "Invalid permission type".to_string())?;
        let permission_type_str = match permission_type {
            PermissionType::Owner => "Owner",
            PermissionType::Witness => "Witness",
            PermissionType::Active => "Active",
        };

        if permission_type == PermissionType::Witness && permission.keys.len() != 1 {
            return Err("Witness permission's key count should be 1".to_string());
        }
        if permission.threshold <= 0 {
            return Err("permission's threshold should be greater than 0".to_string());
        }
        if !permission.permission_name.is_empty() && permission.permission_name.len() > 32 {
            return Err("permission's name is too long".to_string());
        }
        if permission.parent_id != 0 {
            return Err("permission's parent should be owner".to_string());
        }

        let mut seen_addresses: HashSet<&[u8]> = HashSet::new();
        let mut weight_sum: i64 = 0;
        for key in &permission.keys {
            if !seen_addresses.insert(key.address.as_slice()) {
                return Err(format!(
                    "address should be distinct in permission {}",
                    permission_type_str
                ));
            }
            if key.address.len() != 21 || key.address[0] != address_prefix {
                return Err("key is not a validate address".to_string());
            }
            if key.weight <= 0 {
                return Err("key's weight should be greater than 0".to_string());
            }
            weight_sum = weight_sum
                .checked_add(key.weight)
                .ok_or_else(|| "long overflow".to_string())?;
        }
        if weight_sum < permission.threshold {
            return Err(format!(
                "sum of all key's weight should not be less than threshold in permission {}",
                permission_type_str
            ));
        }

        let operations = permission.operations.as_slice();
        if permission_type != PermissionType::Active {
            if !operations.is_empty() {
                return Err(format!(
                    "{} permission needn't operations",
                    permission_type_str
                ));
            }
            return Ok(());
        }

        if operations.is_empty() || operations.len() != 32 {
            return Err("operations size must 32".to_string());
        }

        // Check operations bits against AVAILABLE_CONTRACT_TYPE bitmap.
        // Java-tron requires this property to exist and be >= 32 bytes; missing/short is an error.
        let available_contract_type = storage_adapter
            .get_available_contract_type()
            .map_err(|e| format!("{}", e))?;
        let allowed_bitmap: &[u8] = if available_contract_type.len() >= 32 {
            &available_contract_type[..32]
        } else {
            return Err(format!(
                "AVAILABLE_CONTRACT_TYPE is too short: expected >= 32 bytes, got {}",
                available_contract_type.len()
            ));
        };

        for i in 0..256 {
            let byte_index = i / 8;
            let bit_mask = 1u8 << (i % 8);
            let op_enabled = (operations[byte_index] & bit_mask) != 0;
            let op_allowed = (allowed_bitmap[byte_index] & bit_mask) != 0;
            if op_enabled && !op_allowed {
                return Err(format!("{} isn't a validate ContractType", i));
            }
        }

        Ok(())
    }

    /// Execute an ACCOUNT_PERMISSION_UPDATE_CONTRACT (type 46)
    /// Updates owner/witness/active permissions for multi-sig functionality
    ///
    /// Java reference: AccountPermissionUpdateActuator.java
    fn execute_account_permission_update_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::TronExecutionResult;

        // Validate: multi-sign must be enabled
        let allow_multi_sign = storage_adapter
            .get_allow_multi_sign()
            .map_err(|e| format!("Failed to get allow_multi_sign: {}", e))?;
        if !allow_multi_sign {
            return Err(
                "multi sign is not allowed, need to be opened by the committee".to_string(),
            );
        }

        // Require contract_parameter with matching type_url (strict enforcement)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.AccountPermissionUpdateContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [AccountPermissionUpdateContract],real type[class com.google.protobuf.Any]",
        )?;
        let (owner_address_bytes, mut owner_permission, witness_permission, active_permissions) =
            self.parse_account_permission_update_contract(contract_bytes)?;

        // Validation: owner address must be present and valid (DecodeUtil.addressValid parity).
        let expected_prefix =
            storage_adapter.to_tron_address_21(&revm_primitives::Address::ZERO)[0];
        if owner_address_bytes.is_empty()
            || owner_address_bytes.len() != 21
            || owner_address_bytes[0] != expected_prefix
        {
            return Err("invalidate ownerAddress".to_string());
        }

        let owner = revm_primitives::Address::from_slice(&owner_address_bytes[1..]);
        let owner_tron = tron_backend_common::to_tron_address(&owner);
        info!("AccountPermissionUpdate owner={}", owner_tron);

        // Load owner account
        let mut account_proto = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or_else(|| "ownerAddress account does not exist".to_string())?;

        // Validation (match java-tron ordering)
        let address_prefix = storage_adapter.to_tron_address_21(&owner)[0];

        let mut owner_permission = owner_permission
            .take()
            .ok_or_else(|| "owner permission is missed".to_string())?;

        if account_proto.is_witness {
            if witness_permission.is_none() {
                return Err("witness permission is missed".to_string());
            }
        } else if witness_permission.is_some() {
            return Err("account isn't witness can't set witness permission".to_string());
        }

        if active_permissions.is_empty() {
            return Err("active permission is missed".to_string());
        }
        if active_permissions.len() > 8 {
            return Err("active permission is too many".to_string());
        }

        use tron_backend_execution::protocol::permission::PermissionType;
        if PermissionType::from_i32(owner_permission.r#type) != Some(PermissionType::Owner) {
            return Err("owner permission type is error".to_string());
        }
        self.check_account_permission_update_permission(
            storage_adapter,
            &owner_permission,
            address_prefix,
        )?;

        if account_proto.is_witness {
            let witness_perm = witness_permission
                .as_ref()
                .ok_or_else(|| "witness permission is missed".to_string())?;
            if PermissionType::from_i32(witness_perm.r#type) != Some(PermissionType::Witness) {
                return Err("witness permission type is error".to_string());
            }
            self.check_account_permission_update_permission(
                storage_adapter,
                witness_perm,
                address_prefix,
            )?;
        }

        for active_perm in &active_permissions {
            if PermissionType::from_i32(active_perm.r#type) != Some(PermissionType::Active) {
                return Err("active permission type is error".to_string());
            }
            self.check_account_permission_update_permission(
                storage_adapter,
                active_perm,
                address_prefix,
            )?;
        }

        let fee = storage_adapter
            .get_update_account_permission_fee()
            .map_err(|e| format!("Failed to get update_account_permission_fee: {}", e))?;
        info!("AccountPermissionUpdate: owner={}, fee={}", owner_tron, fee);

        if fee < 0 {
            return Err("Invalid update account permission fee".to_string());
        }

        // Java actuator order: persist permission updates, then charge the fee.
        // In java-tron this runs under the revoking store, so if fee payment fails
        // (BalanceInsufficientException) all writes are rolled back.

        // Update permissions in memory
        owner_permission.id = 0;
        account_proto.owner_permission = Some(owner_permission);

        if account_proto.is_witness {
            if let Some(mut witness_perm) = witness_permission {
                witness_perm.id = 1;
                account_proto.witness_permission = Some(witness_perm);
            }
        }

        account_proto.active_permission.clear();
        for (i, mut active_perm) in active_permissions.into_iter().enumerate() {
            active_perm.id = i as i32 + 2;
            account_proto.active_permission.push(active_perm);
        }

        // Persist permission changes first (before fee charging).
        storage_adapter
            .put_account_proto(&owner, &account_proto)
            .map_err(|e| format!("Failed to persist account permissions: {}", e))?;

        // Charge fee after permissions are persisted.
        if fee > 0 {
            let current_balance = account_proto.balance;
            if current_balance < fee {
                let owner_hex = hex::encode(storage_adapter.to_tron_address_21(&owner));
                return Err(format!(
                    "{} insufficient balance, balance: {}, amount: {}",
                    owner_hex, current_balance, fee
                ));
            }

            account_proto.balance -= fee;
        }

        // Persist fee deduction (if any) in a separate write.
        if fee > 0 {
            storage_adapter
                .put_account_proto(&owner, &account_proto)
                .map_err(|e| format!("Failed to persist fee deduction: {}", e))?;
        }

        // Handle fee destination: burn or credit to blackhole
        // Java: supportBlackHoleOptimization() ? burnTrx(fee) : credit blackhole account
        let support_blackhole_optimization = storage_adapter
            .support_black_hole_optimization()
            .map_err(|e| format!("Failed to get blackhole optimization flag: {}", e))?;

        if fee > 0 {
            if support_blackhole_optimization {
                // Burn mode: increment BURN_TRX_AMOUNT (matches Java's burnTrx())
                storage_adapter
                    .burn_trx(fee as u64)
                    .map_err(|e| format!("Failed to burn TRX: {}", e))?;
            } else {
                // Credit blackhole account (use get_blackhole_address for consistency)
                if let Some(blackhole_addr) = storage_adapter
                    .get_blackhole_address()
                    .map_err(|e| format!("Failed to get blackhole address: {}", e))?
                {
                    storage_adapter
                        .add_balance(&blackhole_addr, fee as u64)
                        .map_err(|e| format!("Failed to credit blackhole: {}", e))?;
                }
            }
        }

        info!(
            "AccountPermissionUpdate completed: owner={}, fee={}",
            owner_tron, fee
        );

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Build Transaction.Result with fee for receipt passthrough
        let tron_transaction_result = TransactionResultBuilder::new().with_fee(fee).build();

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            logs: Vec::new(),
            state_changes: Vec::new(),
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: Some(tron_transaction_result),
            contract_address: None,
        })
    }

    /// Parse AccountPermissionUpdateContract from protobuf bytes
    /// AccountPermissionUpdateContract:
    ///   bytes owner_address = 1;
    ///   Permission owner = 2;
    ///   Permission witness = 3;
    ///   repeated Permission actives = 4;
    ///
    /// Returns a tuple: (owner_address, owner_permission, witness_permission, active_permissions)
    fn parse_account_permission_update_contract(
        &self,
        data: &[u8],
    ) -> Result<
        (
            Vec<u8>,
            Option<tron_backend_execution::protocol::Permission>,
            Option<tron_backend_execution::protocol::Permission>,
            Vec<tron_backend_execution::protocol::Permission>,
        ),
        String,
    > {
        use contracts::proto::{
            read_length_delimited_typed, read_tag_typed, skip_protobuf_field_checked,
            ProtobufError,
        };
        use prost::Message;
        use tron_backend_execution::protocol::Permission;

        // Outer wire-level parsing (tags, length-delimited framing, unknown fields) uses
        // the unified ProtobufError taxonomy for protobuf-java 3.21.12 message parity.
        // Inner Permission::decode() uses prost, which has its own error surface — remapping
        // prost errors to Java-parity strings would require manual Permission parsing and is
        // out of scope for the wire-level taxonomy unification.
        let map_err = |e: ProtobufError| e.to_java_message();

        let mut owner_address: Option<Vec<u8>> = None;
        let mut owner_permission: Option<Permission> = None;
        let mut witness_permission: Option<Permission> = None;
        let mut active_permissions: Vec<Permission> = Vec::new();
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, bytes_read) =
                read_tag_typed(&data[pos..]).map_err(map_err)?;
            pos += bytes_read;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address (bytes)
                    let (payload, total) =
                        read_length_delimited_typed(&data[pos..]).map_err(map_err)?;
                    owner_address = Some(payload.to_vec());
                    pos += total;
                }
                (2, 2) => {
                    // owner permission (embedded message)
                    let (payload, total) =
                        read_length_delimited_typed(&data[pos..]).map_err(map_err)?;
                    owner_permission = Some(
                        Permission::decode(payload)
                            .map_err(|e| format!("Failed to decode owner permission: {}", e))?,
                    );
                    pos += total;
                }
                (3, 2) => {
                    // witness permission (embedded message)
                    let (payload, total) =
                        read_length_delimited_typed(&data[pos..]).map_err(map_err)?;
                    witness_permission = Some(
                        Permission::decode(payload)
                            .map_err(|e| format!("Failed to decode witness permission: {}", e))?,
                    );
                    pos += total;
                }
                (4, 2) => {
                    // active permission (repeated embedded message)
                    let (payload, total) =
                        read_length_delimited_typed(&data[pos..]).map_err(map_err)?;
                    let perm = Permission::decode(payload)
                        .map_err(|e| format!("Failed to decode active permission: {}", e))?;
                    active_permissions.push(perm);
                    pos += total;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(map_err)?;
                    pos += skip_len;
                }
            }
        }

        Ok((
            owner_address.unwrap_or_default(),
            owner_permission,
            witness_permission,
            active_permissions,
        ))
    }

    /// Execute an ASSET_ISSUE_CONTRACT (TRC-10 token issuance)
    /// Phase 1: Charge asset issue fee, emit fee deltas, bandwidth, and AEXT tracking
    /// Phase 2 (future): Full TRC-10 persistence via proto extension
    /// Execute a TRANSFER_ASSET_CONTRACT (TRC-10 transfer, non-VM)
    ///
    /// This handler processes TRC-10 token transfers. Unlike TRX transfers:
    /// - No TRX balance changes (unless fee is configured)
    /// - Emits a Trc10Change::AssetTransferred for Java to apply to TRC-10 stores
    /// - energy_used = 0, bandwidth_used > 0
    /// - AEXT tracking for bandwidth consumption
    fn execute_trc10_transfer_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use revm_primitives::Address;
        use tron_backend_execution::{TronExecutionResult, TronStateChange};

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        debug!("Executing TRANSFER_ASSET_CONTRACT for owner {}", owner_tron);

        // Validate contract parameter presence and type (strict Java parity)
        let _contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.TransferAssetContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [TransferAssetContract], real type[class com.google.protobuf.Any]",
        )?;

        // Validation parity: DecodeUtil.addressValid(ownerAddress)
        // Java requires exactly 21 bytes and prefix == configured network prefix.
        // Missing or empty from_raw is invalid (mirrors Java's ArrayUtils.isEmpty check).
        let prefix = storage_adapter.address_prefix();
        match transaction.metadata.from_raw.as_deref() {
            None | Some(&[]) => {
                return Err("Invalid ownerAddress".to_string());
            }
            Some(from_raw) => {
                if from_raw.len() != 21 || from_raw[0] != prefix {
                    return Err("Invalid ownerAddress".to_string());
                }
            }
        }

        // Validation parity: DecodeUtil.addressValid(toAddress)
        // Java checks toAddress is non-empty, 21 bytes, and has the correct prefix.
        match transaction.metadata.to_raw.as_deref() {
            None | Some(&[]) => {
                return Err("Invalid toAddress".to_string());
            }
            Some(to_raw) => {
                if to_raw.len() != 21 || to_raw[0] != prefix {
                    return Err("Invalid toAddress".to_string());
                }
            }
        }

        // 1. Extract required fields from transaction
        let to_address = transaction
            .to
            .ok_or_else(|| "Invalid toAddress".to_string())?;
        let to_tron = tron_backend_common::to_tron_address(&to_address);

        // Get asset_id from metadata (Java passes it as metadata.asset_id).
        // If metadata.asset_id is absent (Java omits empty bytes), fall back to parsing
        // TransferAssetContract from contract_parameter.value to extract asset_name bytes.
        // Java's TransferAssetActuator treats empty asset_name as "No asset!" via store lookup.
        let asset_id = match transaction.metadata.asset_id.as_ref() {
            Some(id) if !id.is_empty() => id.clone(),
            _ => {
                // Try to parse asset_name from the raw TransferAssetContract protobuf
                let asset_name_from_proto = transaction
                    .metadata
                    .contract_parameter
                    .as_ref()
                    .and_then(|param| {
                        Self::parse_asset_name_from_transfer_asset_contract(&param.value)
                    });
                match asset_name_from_proto {
                    Some(name) if !name.is_empty() => name,
                    _ => {
                        // Java: empty asset_name → store.has() returns false → "No asset!"
                        return Err("No asset!".to_string());
                    }
                }
            }
        };

        // Java uses ByteArray.toStr(assetName) as the TRC-10 map key.
        let asset_key_str = String::from_utf8_lossy(&asset_id).to_string();

        // Convert value (U256) to i64 for TRC-10 amount
        // TransferAssetContract amounts are typically i64
        let amount_u64: u64 = transaction
            .value
            .try_into()
            .map_err(|_| "TransferAssetContract amount overflow: value too large for i64")?;
        let amount = amount_u64 as i64;

        if amount <= 0 {
            return Err("Amount must be greater than 0.".to_string());
        }

        if owner == to_address {
            return Err("Cannot transfer asset to yourself.".to_string());
        }

        // Java validate(): owner must exist, asset must exist, and owner must have balance.
        // Only after these validations do we write any state (validate_fail must produce 0 writes).
        let allow_same_token_name = storage_adapter
            .get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;

        let mut owner_account_proto = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or("No owner account!".to_string())?;

        let asset_issue = storage_adapter
            .get_asset_issue(&asset_id, allow_same_token_name)
            .map_err(|e| format!("Failed to get asset issue: {}", e))?
            .ok_or("No asset!".to_string())?;

        // Java semantics:
        // - allowSameTokenName == 0: tokenID comes from AssetIssueCapsule.getId() (may be empty for legacy assets)
        // - allowSameTokenName == 1: tokenID is ByteArray.toStr(assetName)
        let token_id_str = if allow_same_token_name == 0 {
            asset_issue.id.clone()
        } else {
            asset_key_str.clone()
        };

        let owner_asset_balance =
            Self::get_asset_balance_v2(&owner_account_proto, &asset_id, allow_same_token_name);
        if owner_asset_balance <= 0 {
            return Err("assetBalance must be greater than 0.".to_string());
        }
        if amount > owner_asset_balance {
            return Err("assetBalance is not sufficient.".to_string());
        }

        let recipient_proto_opt = storage_adapter
            .get_account_proto(&to_address)
            .map_err(|e| format!("Failed to get recipient account: {}", e))?;
        let recipient_was_missing = recipient_proto_opt.is_none();

        if let Some(ref recipient_proto) = recipient_proto_opt {
            // After ForbidTransferToContract proposal, sending TRC-10 to smart contracts is disallowed.
            let forbid_transfer_to_contract = storage_adapter
                .get_forbid_transfer_to_contract()
                .map_err(|e| format!("Failed to get FORBID_TRANSFER_TO_CONTRACT: {}", e))?;
            if forbid_transfer_to_contract == 1 {
                use tron_backend_execution::protocol::AccountType as ProtoAccountType;
                if recipient_proto.r#type == ProtoAccountType::Contract as i32 {
                    return Err("Cannot transfer asset to smartContract.".to_string());
                }
            }

            // Validation parity: addExact(recipientBalance, amount) overflow check.
            let existing_balance = if allow_same_token_name == 0 {
                recipient_proto.asset.get(&asset_key_str).copied()
            } else {
                recipient_proto.asset_v2.get(&asset_key_str).copied()
            };
            if let Some(balance) = existing_balance {
                balance
                    .checked_add(amount)
                    .ok_or_else(|| "long overflow".to_string())?;
            }
        }

        let create_account_fee = if recipient_proto_opt.is_none() {
            storage_adapter
                .get_create_new_account_fee_in_system_contract()
                .map_err(|e| {
                    format!(
                        "Failed to get CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT: {}",
                        e
                    )
                })?
        } else {
            0
        };

        if create_account_fee > 0 && owner_account_proto.balance < create_account_fee as i64 {
            return Err("Validate TransferAssetActuator error, insufficient fee.".to_string());
        }

        // Java execute(): create recipient when absent, update TRC-10 balances, and apply fee.
        // When creating a missing recipient, java-tron's AccountCapsule constructor initializes
        // default permissions if ALLOW_MULTI_SIGN == 1 (owner_permission id=0, active_permission id=2).
        let mut recipient_account_proto = match recipient_proto_opt {
            Some(acc) => acc,
            None => {
                use tron_backend_execution::protocol::permission::PermissionType;
                use tron_backend_execution::protocol::{
                    Account as ProtoAccount, AccountType as ProtoAccountType,
                };
                use tron_backend_execution::protocol::{Key, Permission};
                let create_time = storage_adapter
                    .get_latest_block_header_timestamp()
                    .map_err(|e| format!("Failed to get LATEST_BLOCK_HEADER_TIMESTAMP: {}", e))?;
                let recipient_address_bytes =
                    storage_adapter.to_tron_address_21(&to_address).to_vec();

                let mut new_account = ProtoAccount {
                    address: recipient_address_bytes.clone(),
                    create_time,
                    r#type: ProtoAccountType::Normal as i32,
                    ..Default::default()
                };

                // Mirror java-tron: withDefaultPermission = (ALLOW_MULTI_SIGN == 1)
                let allow_multi_sign = storage_adapter
                    .get_allow_multi_sign()
                    .map_err(|e| format!("Failed to get ALLOW_MULTI_SIGN: {}", e))?;

                if allow_multi_sign {
                    let active_default_operations = storage_adapter
                        .get_active_default_operations()
                        .map_err(|e| format!("Failed to get ACTIVE_DEFAULT_OPERATIONS: {}", e))?;

                    let default_key = Key {
                        address: recipient_address_bytes,
                        weight: 1,
                    };

                    new_account.owner_permission = Some(Permission {
                        r#type: PermissionType::Owner as i32,
                        id: 0,
                        permission_name: "owner".to_string(),
                        threshold: 1,
                        parent_id: 0,
                        operations: Vec::new(),
                        keys: vec![default_key.clone()],
                    });

                    new_account.active_permission = vec![Permission {
                        r#type: PermissionType::Active as i32,
                        id: 2,
                        permission_name: "active".to_string(),
                        threshold: 1,
                        parent_id: 0,
                        operations: active_default_operations,
                        keys: vec![default_key],
                    }];
                }

                new_account
            }
        };

        // Capture old_account snapshots BEFORE any mutations for state_changes parity.
        // Java's revoking session journals account deltas; we must replicate by snapshotting
        // pre-execution state and emitting real AccountChange entries.
        let old_owner_account = storage_adapter
            .get_account(&owner)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or("Owner account does not exist".to_string())?;
        let old_recipient_account = if !recipient_was_missing {
            storage_adapter
                .get_account(&to_address)
                .map_err(|e| format!("Failed to load recipient account: {}", e))?
        } else {
            None
        };

        Self::reduce_asset_amount_v2(
            &mut owner_account_proto,
            &asset_id,
            &token_id_str,
            amount,
            allow_same_token_name,
        )?;
        Self::add_asset_amount_v2(
            &mut recipient_account_proto,
            &asset_id,
            &token_id_str,
            amount,
            allow_same_token_name,
        )?;

        if create_account_fee > 0 {
            owner_account_proto.balance = owner_account_proto
                .balance
                .checked_sub(create_account_fee as i64)
                .ok_or("Validate TransferAssetActuator error, insufficient fee.".to_string())?;
        }

        storage_adapter
            .put_account_proto(&owner, &owner_account_proto)
            .map_err(|e| format!("Failed to persist owner Account proto: {}", e))?;
        storage_adapter
            .put_account_proto(&to_address, &recipient_account_proto)
            .map_err(|e| format!("Failed to persist recipient Account proto: {}", e))?;

        // Handle create-account fee at the EVM layer + blackhole crediting/burning.
        // Track blackhole change for state_changes emission.
        let mut blackhole_state_change: Option<(
            Address,
            Option<revm_primitives::AccountInfo>,
            revm_primitives::AccountInfo,
        )> = None;
        if create_account_fee > 0 {
            // Also deduct create-account fee from owner EVM AccountInfo balance
            // so state_changes reflect the real delta.
            let fee_u256 = revm_primitives::U256::from(create_account_fee);
            let new_owner_evm = revm_primitives::AccountInfo {
                balance: old_owner_account
                    .balance
                    .checked_sub(fee_u256)
                    .ok_or("Owner balance underflow for create-account fee".to_string())?,
                nonce: old_owner_account.nonce,
                code_hash: old_owner_account.code_hash,
                code: old_owner_account.code.clone(),
            };
            storage_adapter
                .set_account(owner, new_owner_evm)
                .map_err(|e| format!("Failed to persist owner EVM account: {}", e))?;

            let support_blackhole = storage_adapter
                .support_black_hole_optimization()
                .map_err(|e| format!("Failed to get SupportBlackHoleOptimization: {}", e))?;
            if support_blackhole {
                storage_adapter
                    .burn_trx(create_account_fee)
                    .map_err(|e| format!("Failed to burn TRX: {}", e))?;
            } else if let Some(blackhole_addr) = storage_adapter
                .get_blackhole_address()
                .map_err(|e| format!("Failed to get blackhole address: {}", e))?
            {
                let old_blackhole_account = storage_adapter
                    .get_account(&blackhole_addr)
                    .map_err(|e| format!("Failed to load blackhole account: {}", e))?
                    .unwrap_or_default();

                let new_blackhole_balance = old_blackhole_account
                    .balance
                    .checked_add(fee_u256)
                    .ok_or("Blackhole balance overflow")?;
                let new_blackhole_account = revm_primitives::AccountInfo {
                    balance: new_blackhole_balance,
                    nonce: old_blackhole_account.nonce,
                    code_hash: old_blackhole_account.code_hash,
                    code: old_blackhole_account.code.clone(),
                };

                storage_adapter
                    .set_account(blackhole_addr, new_blackhole_account.clone())
                    .map_err(|e| format!("Failed to persist blackhole account: {}", e))?;

                // Track for state_changes emission
                let old_bh = if old_blackhole_account.balance.is_zero()
                    && old_blackhole_account.nonce == 0
                {
                    None
                } else {
                    Some(old_blackhole_account)
                };
                blackhole_state_change = Some((blackhole_addr, old_bh, new_blackhole_account));
            }
        }

        info!(
            "TRC-10 Transfer: owner={}, to={}, asset_id_len={}, amount={}, create_fee={}",
            owner_tron,
            to_tron,
            asset_id.len(),
            amount,
            create_account_fee
        );

        // 2. Get execution configuration
        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();

        // 3. Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // 4. Track AEXT for bandwidth if in tracked mode
        let mut aext_map = std::collections::HashMap::new();
        if aext_mode == "tracked" {
            use tron_backend_execution::{AccountAext, ResourceTracker};

            // Get current AEXT for owner (or initialize with defaults)
            let current_aext = storage_adapter
                .get_account_aext(&owner)
                .map_err(|e| format!("Failed to get account AEXT: {}", e))?
                .unwrap_or_else(|| AccountAext::with_defaults());

            // Get FREE_NET_LIMIT from dynamic properties
            let free_net_limit = storage_adapter
                .get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;

            // Java uses headSlot = block_timestamp_ms / 3000 for resource windows.
            let now_slot = (context.block_timestamp / 3000) as i64;

            // Track bandwidth usage (returns path, before_aext, after_aext)
            let (_path, before_aext, after_aext) = ResourceTracker::track_bandwidth(
                &owner,
                bandwidth_used as i64,
                now_slot,
                &current_aext,
                free_net_limit,
            )
            .map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist after AEXT to storage
            storage_adapter
                .set_account_aext(&owner, &after_aext)
                .map_err(|e| format!("Failed to persist account AEXT: {}", e))?;
            storage_adapter
                .apply_bandwidth_aext_to_account_proto(&owner, &after_aext)
                .map_err(|e| {
                    format!("Failed to persist bandwidth usage to account proto: {}", e)
                })?;

            // Add to aext_map
            aext_map.insert(owner, (before_aext.clone(), after_aext.clone()));

            debug!(
                "AEXT tracked for TRC-10 transfer: owner={}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}",
                owner_tron, before_aext.net_usage, after_aext.net_usage,
                before_aext.free_net_usage, after_aext.free_net_usage
            );
        }

        // 5. Build state changes with real pre/post snapshots.
        let mut state_changes = Vec::new();

        // Owner account: emit real delta (balance changes if create_account_fee > 0)
        let new_owner_account = storage_adapter
            .get_account(&owner)
            .map_err(|e| format!("Failed to reload owner account: {}", e))?
            .ok_or("Owner account does not exist after mutation".to_string())?;

        state_changes.push(TronStateChange::AccountChange {
            address: owner,
            old_account: Some(old_owner_account),
            new_account: Some(new_owner_account),
        });

        // Recipient account: emit creation (old=None) or delta
        let new_recipient_account = storage_adapter
            .get_account(&to_address)
            .map_err(|e| format!("Failed to reload recipient account: {}", e))?;
        let new_recip = new_recipient_account.ok_or_else(|| {
            "Recipient account not found after mutation; storage inconsistency".to_string()
        })?;
        state_changes.push(TronStateChange::AccountChange {
            address: to_address,
            old_account: old_recipient_account,
            new_account: Some(new_recip),
        });

        // Blackhole account: emit delta when credited (not burning)
        if let Some((bh_addr, old_bh, new_bh)) = blackhole_state_change {
            state_changes.push(TronStateChange::AccountChange {
                address: bh_addr,
                old_account: old_bh,
                new_account: Some(new_bh),
            });
        }

        // 7. Sort state changes deterministically by address for CSV parity
        state_changes.sort_by_key(|change| match change {
            TronStateChange::AccountChange { address, .. } => *address,
            _ => Address::ZERO,
        });

        // 8. Determine token_id (numeric string); in legacy mode this comes from AssetIssueStore.
        let token_id = Some(token_id_str.clone());

        // 9. Build TRC-10 Asset Transferred change for Phase 2
        let trc10_change = tron_backend_execution::Trc10Change::AssetTransferred(
            tron_backend_execution::Trc10AssetTransferred {
                owner_address: owner,
                to_address,
                asset_name: asset_id.clone(),
                token_id,
                amount,
            },
        );

        info!(
            "TRC-10 Transfer completed: owner={}, to={}, asset_id_len={}, amount={}, create_fee={} SUN, state_changes={}, bandwidth={}",
            owner_tron, to_tron, asset_id.len(), amount, create_account_fee, state_changes.len(), bandwidth_used
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(), // No return data for TRC-10 transfers
            energy_used: 0,                             // Non-VM transactions use 0 energy
            bandwidth_used,
            state_changes,
            logs: Vec::new(), // No logs for TRC-10 transfers
            error: None,
            aext_map,
            freeze_changes: vec![], // Not applicable for TRC-10 transfers
            global_resource_changes: vec![], // Not applicable for TRC-10 transfers
            trc10_changes: vec![trc10_change], // Phase 2: emit TRC-10 semantic change
            vote_changes: vec![],   // Not applicable for TRC-10 transfers
            withdraw_changes: vec![], // Not applicable for TRC-10 transfers
            tron_transaction_result: None,
            contract_address: None,
        })
    }

    fn execute_asset_issue_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use revm_primitives::Address;
        use tron_backend_execution::{TronExecutionResult, TronStateChange};

        // Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.AssetIssueContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [AssetIssueContract],real type[class com.google.protobuf.Any]",
        )?;

        // Decode full AssetIssueContractData early so we can validate owner_address and
        // frozen_supply (java-tron AssetIssueActuator.validate).
        use prost::Message;
        let mut asset_proto =
            tron_backend_execution::protocol::AssetIssueContractData::decode(contract_bytes)
                .map_err(|e| format!("Failed to decode AssetIssueContractData: {}", e))?;

        // Validation parity: DecodeUtil.addressValid(ownerAddress)
        // Java requires exact match with network-specific prefix (0x41 mainnet, 0xa0 testnet)
        let expected_prefix = storage_adapter
            .tron_address_prefix()
            .map_err(|e| format!("Failed to get address prefix: {}", e))?;
        let owner = match validate_tron_address_prefix(
            asset_proto.owner_address.as_slice(),
            expected_prefix,
        ) {
            Ok(bytes) if bytes.len() == 20 => Address::from_slice(bytes),
            _ => return Err("Invalid ownerAddress".to_string()),
        };

        let owner_tron = tron_backend_common::to_tron_address(&owner);

        debug!("Executing ASSET_ISSUE_CONTRACT for owner {}", owner_tron);

        // 1. Parse AssetIssueContract proto from contract_bytes (same source as prost decode)
        // This ensures consistency: both parse_asset_issue_contract and AssetIssueContractData::decode
        // use the same bytes, avoiding divergence if callers populate only one field.
        let asset_info = Self::parse_asset_issue_contract(contract_bytes)?;

        info!(
            "AssetIssue: owner={}, name={}, total_supply={}, precision={}",
            owner_tron, asset_info.name, asset_info.total_supply, asset_info.precision
        );

        // 2. Get execution configuration
        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();

        if !execution_config.remote.trc10_enabled {
            return Err(
                "ASSET_ISSUE_CONTRACT execution is disabled - falling back to Java".to_string(),
            );
        }

        // 3. Contract validation (match java-tron's AssetIssueActuator.validate ordering)
        // Use raw bytes from asset_proto for validation to match Java's byte-based validation.
        // This avoids discrepancies from lossy UTF-8 conversion.
        let allow_same_token_name = storage_adapter
            .get_allow_same_token_name()
            .map_err(|e| format!("Failed to get ALLOW_SAME_TOKEN_NAME: {}", e))?;

        // Validate using raw bytes from proto (not lossy-decoded strings)
        if !Self::valid_asset_name(&asset_proto.name) {
            return Err("Invalid assetName".to_string());
        }

        // Java's "trx" check uses: new String(name).toLowerCase().equals("trx")
        // We use lossy decode here since Java does explicit String conversion for this check
        if allow_same_token_name != 0 {
            let name_str = String::from_utf8_lossy(&asset_proto.name).to_lowercase();
            if name_str == "trx" {
                return Err("assetName can't be trx".to_string());
            }
        }

        if asset_info.precision != 0
            && allow_same_token_name != 0
            && (asset_info.precision < 0 || asset_info.precision > 6)
        {
            return Err("precision cannot exceed 6".to_string());
        }

        // Validate abbr using raw bytes
        if !asset_proto.abbr.is_empty() && !Self::valid_asset_name(&asset_proto.abbr) {
            return Err("Invalid abbreviation for token".to_string());
        }

        // Validate url using raw bytes
        if !Self::valid_url(&asset_proto.url) {
            return Err("Invalid url".to_string());
        }

        // Validate description using raw bytes
        if !Self::valid_asset_description(&asset_proto.description) {
            return Err("Invalid description".to_string());
        }

        if asset_info.start_time == 0 {
            return Err("Start time should be not empty".to_string());
        }

        if asset_info.end_time == 0 {
            return Err("End time should be not empty".to_string());
        }

        if asset_info.end_time <= asset_info.start_time {
            return Err("End time should be greater than start time".to_string());
        }

        let head_block_time = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest_block_header_timestamp: {}", e))?;

        if asset_info.start_time <= head_block_time {
            return Err("Start time should be greater than HeadBlockTime".to_string());
        }

        // Use raw name bytes for legacy "Token exists" lookup (V1 store keyed by name)
        if allow_same_token_name == 0
            && storage_adapter
                .get_asset_issue(&asset_proto.name, allow_same_token_name)
                .map_err(|e| format!("Failed to query AssetIssueStore: {}", e))?
                .is_some()
        {
            return Err("Token exists".to_string());
        }

        if asset_info.total_supply <= 0 {
            return Err("TotalSupply must greater than 0!".to_string());
        }

        if asset_info.trx_num <= 0 {
            return Err("TrxNum must greater than 0!".to_string());
        }

        if asset_info.num <= 0 {
            return Err("Num must greater than 0!".to_string());
        }

        if asset_info.public_free_asset_net_usage != 0 {
            return Err("PublicFreeAssetNetUsage must be 0!".to_string());
        }

        let max_frozen_supply_number = storage_adapter
            .get_max_frozen_supply_number()
            .map_err(|e| format!("Failed to get MAX_FROZEN_SUPPLY_NUMBER: {}", e))?;

        if (asset_proto.frozen_supply.len() as i64) > max_frozen_supply_number {
            return Err("Frozen supply list length is too long".to_string());
        }

        let one_day_net_limit = storage_adapter
            .get_one_day_net_limit()
            .map_err(|e| format!("Failed to get ONE_DAY_NET_LIMIT: {}", e))?;

        if asset_info.free_asset_net_limit < 0
            || asset_info.free_asset_net_limit >= one_day_net_limit
        {
            return Err("Invalid FreeAssetNetLimit".to_string());
        }

        if asset_info.public_free_asset_net_limit < 0
            || asset_info.public_free_asset_net_limit >= one_day_net_limit
        {
            return Err("Invalid PublicFreeAssetNetLimit".to_string());
        }

        let min_frozen_supply_time = storage_adapter
            .get_min_frozen_supply_time()
            .map_err(|e| format!("Failed to get MIN_FROZEN_SUPPLY_TIME: {}", e))?;
        let max_frozen_supply_time = storage_adapter
            .get_max_frozen_supply_time()
            .map_err(|e| format!("Failed to get MAX_FROZEN_SUPPLY_TIME: {}", e))?;

        let mut remain_supply = asset_info.total_supply;
        for fs in &asset_proto.frozen_supply {
            if fs.frozen_amount <= 0 {
                return Err("Frozen supply must be greater than 0!".to_string());
            }
            if fs.frozen_amount > remain_supply {
                return Err("Frozen supply cannot exceed total supply".to_string());
            }
            if !(fs.frozen_days >= min_frozen_supply_time
                && fs.frozen_days <= max_frozen_supply_time)
            {
                return Err(format!(
                    "frozenDuration must be less than {} days and more than {} days",
                    max_frozen_supply_time, min_frozen_supply_time
                ));
            }
            remain_supply -= fs.frozen_amount;
        }

        let mut owner_account_proto = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to load owner account proto: {}", e))?
            .ok_or_else(|| "Account not exists".to_string())?;

        if !owner_account_proto.asset_issued_name.is_empty() {
            return Err("An account can only issue one asset".to_string());
        }

        // 4. Get asset issue fee from dynamic properties
        let asset_issue_fee = storage_adapter
            .get_asset_issue_fee()
            .map_err(|e| format!("Failed to get AssetIssueFee: {}", e))?;

        debug!("AssetIssueFee: {} SUN", asset_issue_fee);

        let fee_i64 =
            i64::try_from(asset_issue_fee).map_err(|_| "AssetIssueFee overflow".to_string())?;
        if owner_account_proto.balance < fee_i64 {
            return Err("No enough balance for fee!".to_string());
        }

        // 5. Load owner account
        let owner_account = storage_adapter
            .get_account(&owner)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or_else(|| "Account not exists".to_string())?;

        // 6. Allocate token id and persist asset metadata (TRC-10 issuance)
        let token_id_num = storage_adapter
            .get_token_id_num()
            .map_err(|e| format!("Failed to get TOKEN_ID_NUM: {}", e))?;
        let new_token_id_num = token_id_num
            .checked_add(1)
            .ok_or_else(|| "TOKEN_ID_NUM overflow".to_string())?;
        storage_adapter
            .save_token_id_num(new_token_id_num)
            .map_err(|e| format!("Failed to save TOKEN_ID_NUM: {}", e))?;
        let token_id_str = new_token_id_num.to_string();

        asset_proto.id = token_id_str.clone();

        // Persist AssetIssueStore (V1) and AssetIssueV2Store (V2) entries.
        if allow_same_token_name == 0 {
            // V1 store by name (no precision override)
            storage_adapter
                .put_asset_issue(&asset_proto.name, &asset_proto, false)
                .map_err(|e| format!("Failed to persist AssetIssue (V1): {}", e))?;

            // V2 store by token id; java-tron stores precision=0 in legacy mode
            let mut asset_v2 = asset_proto.clone();
            asset_v2.precision = 0;
            storage_adapter
                .put_asset_issue(token_id_str.as_bytes(), &asset_v2, true)
                .map_err(|e| format!("Failed to persist AssetIssue (V2): {}", e))?;
        } else {
            storage_adapter
                .put_asset_issue(token_id_str.as_bytes(), &asset_proto, true)
                .map_err(|e| format!("Failed to persist AssetIssue (V2): {}", e))?;
        }

        // 7. Deduct fee from owner
        let owner_balance_u256 = owner_account.balance;
        let fee_u256 = revm_primitives::U256::from(asset_issue_fee);
        let new_owner_balance = owner_balance_u256 - fee_u256;
        let new_owner_account = revm_primitives::AccountInfo {
            balance: new_owner_balance,
            nonce: owner_account.nonce,
            code_hash: owner_account.code_hash,
            code: owner_account.code.clone(),
        };

        // 8. Emit state changes (deterministic ordering by address)
        let mut state_changes = Vec::new();

        // Always emit owner account change
        state_changes.push(TronStateChange::AccountChange {
            address: owner,
            old_account: Some(owner_account.clone()),
            new_account: Some(new_owner_account.clone()),
        });

        // Persist owner account update (balance + TRC-10 issuer fields)
        owner_account_proto.balance = owner_account_proto
            .balance
            .checked_sub(fee_i64)
            .ok_or_else(|| "No enough balance for fee!".to_string())?;

        // Convert frozen_supply schedule (AssetIssueContractData::FrozenSupply) to
        // Account.frozen_supply entries (frozenBalance + expireTime).
        const FROZEN_PERIOD_MS: i64 = 86_400_000;
        let mut remain_supply = asset_proto.total_supply;
        for fs in &asset_proto.frozen_supply {
            let expire_time = asset_proto.start_time + fs.frozen_days * FROZEN_PERIOD_MS;
            owner_account_proto.frozen_supply.push(
                tron_backend_execution::protocol::account::Frozen {
                    frozen_balance: fs.frozen_amount,
                    expire_time,
                },
            );
            remain_supply -= fs.frozen_amount;
        }

        if allow_same_token_name == 0 {
            // Legacy map keyed by asset name string
            owner_account_proto
                .asset
                .insert(asset_info.name.clone(), remain_supply);
        }
        owner_account_proto.asset_issued_name = asset_proto.name.clone();
        owner_account_proto.asset_issued_id = token_id_str.as_bytes().to_vec();
        owner_account_proto
            .asset_v2
            .insert(token_id_str.clone(), remain_supply);

        storage_adapter
            .put_account_proto(&owner, &owner_account_proto)
            .map_err(|e| format!("Failed to persist owner account proto: {}", e))?;

        // 9. Handle fee burning/crediting
        let support_blackhole = storage_adapter
            .support_black_hole_optimization()
            .map_err(|e| format!("Failed to get blackhole optimization flag: {}", e))?;

        if support_blackhole {
            storage_adapter
                .burn_trx(asset_issue_fee)
                .map_err(|e| format!("Failed to burn trx: {}", e))?;
        } else {
            // Credit blackhole account
            if let Some(blackhole_addr) = storage_adapter
                .get_blackhole_address()
                .map_err(|e| format!("Failed to get blackhole address: {}", e))?
            {
                let blackhole_account = storage_adapter
                    .get_account(&blackhole_addr)
                    .map_err(|e| format!("Failed to load blackhole account: {}", e))?
                    .unwrap_or_default();

                let new_blackhole_account = revm_primitives::AccountInfo {
                    balance: blackhole_account.balance + fee_u256,
                    nonce: blackhole_account.nonce,
                    code_hash: blackhole_account.code_hash,
                    code: blackhole_account.code.clone(),
                };

                // Emit account change for blackhole
                state_changes.push(TronStateChange::AccountChange {
                    address: blackhole_addr,
                    old_account: Some(blackhole_account),
                    new_account: Some(new_blackhole_account.clone()),
                });

                // Persist blackhole account update
                storage_adapter
                    .set_account(blackhole_addr, new_blackhole_account.clone())
                    .map_err(|e| format!("Failed to persist blackhole account: {}", e))?;

                let bh_tron = tron_backend_common::to_tron_address(&blackhole_addr);
                info!(
                    "Credited {} SUN asset issue fee to blackhole address {}",
                    asset_issue_fee, bh_tron
                );
            }
        }

        // 9. Sort state changes deterministically by address for CSV parity
        state_changes.sort_by_key(|change| match change {
            TronStateChange::AccountChange { address, .. } => *address,
            _ => Address::ZERO,
        });

        // 10. Calculate bandwidth
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // 11. AEXT tracking (if enabled)
        let mut aext_map = std::collections::HashMap::new();

        if aext_mode == "tracked" {
            use tron_backend_execution::{AccountAext, ResourceTracker};

            // Get current AEXT for owner (or initialize with defaults)
            let current_aext = storage_adapter
                .get_account_aext(&owner)
                .map_err(|e| format!("Failed to get account AEXT: {}", e))?
                .unwrap_or_else(|| AccountAext::with_defaults());

            // Get FREE_NET_LIMIT from dynamic properties
            let free_net_limit = storage_adapter
                .get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;

            // Java uses headSlot = block_timestamp_ms / 3000 for resource windows.
            let now_slot = (context.block_timestamp / 3000) as i64;

            // Track bandwidth usage (returns path, before_aext, after_aext)
            let (_path, before_aext, after_aext) = ResourceTracker::track_bandwidth(
                &owner,
                bandwidth_used as i64,
                now_slot,
                &current_aext,
                free_net_limit,
            )
            .map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist after AEXT to storage
            storage_adapter
                .set_account_aext(&owner, &after_aext)
                .map_err(|e| format!("Failed to persist account AEXT: {}", e))?;
            storage_adapter
                .apply_bandwidth_aext_to_account_proto(&owner, &after_aext)
                .map_err(|e| {
                    format!("Failed to persist bandwidth usage to account proto: {}", e)
                })?;

            // Add to aext_map
            aext_map.insert(owner, (before_aext.clone(), after_aext.clone()));

            debug!(
                "AEXT tracked for asset_issue: owner={}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}",
                owner_tron, before_aext.net_usage, after_aext.net_usage,
                before_aext.free_net_usage, after_aext.free_net_usage
            );
        }

        info!(
            "AssetIssue completed: owner={}, name={}, fee={} SUN, state_changes={}, bandwidth={}",
            owner_tron,
            asset_info.name,
            asset_issue_fee,
            state_changes.len(),
            bandwidth_used
        );

        // 12. Build TRC-10 Asset Issued change for Phase 2
        let trc10_change = tron_backend_execution::Trc10Change::AssetIssued(
            tron_backend_execution::Trc10AssetIssued {
                owner_address: owner,
                name: asset_info.name.as_bytes().to_vec(),
                abbr: asset_info.abbr.as_bytes().to_vec(),
                total_supply: asset_info.total_supply,
                trx_num: asset_info.trx_num,
                // java-tron records issuance precision from the V2 capsule:
                // when allowSameTokenName==0, V2 precision is forced to 0.
                precision: if allow_same_token_name == 0 {
                    0
                } else {
                    asset_info.precision
                },
                num: asset_info.num,
                start_time: asset_info.start_time,
                end_time: asset_info.end_time,
                description: asset_info.description.as_bytes().to_vec(),
                url: asset_info.url.as_bytes().to_vec(),
                free_asset_net_limit: asset_info.free_asset_net_limit,
                public_free_asset_net_limit: asset_info.public_free_asset_net_limit,
                public_free_asset_net_usage: asset_info.public_free_asset_net_usage,
                public_latest_free_net_time: asset_info.public_latest_free_net_time,
                token_id: Some(token_id_str.clone()), // Self-contained: no Java dependency on TOKEN_ID_NUM
            },
        );

        // Receipt passthrough: include fee + assetIssueID (matches java-tron Transaction.Result.assetIssueID)
        let receipt_bytes = TransactionResultBuilder::new()
            .with_fee(fee_i64)
            .with_asset_issue_id(&token_id_str)
            .build();

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(), // No return data for asset issue
            energy_used: 0,                             // System contracts use 0 energy
            bandwidth_used,
            state_changes,
            logs: Vec::new(), // No logs for asset issue
            error: None,
            aext_map,
            freeze_changes: vec![],          // Not applicable for asset issue
            global_resource_changes: vec![], // Not applicable for asset issue
            trc10_changes: vec![trc10_change], // Phase 2: emit TRC-10 semantic change
            vote_changes: vec![],            // Not applicable for asset issue
            withdraw_changes: vec![],        // Not applicable for asset issue
            tron_transaction_result: Some(receipt_bytes),
            contract_address: None,
        })
    }

    /// Parse AssetIssueContract protobuf from transaction data
    /// Phase 1: Parse minimal fields (name, total_supply, precision, etc.)
    /// Returns basic asset information without full validation
    fn parse_asset_issue_contract(data: &[u8]) -> Result<AssetIssueInfo, String> {
        let mut name = String::new();
        let mut abbr = String::new();
        let mut total_supply: i64 = 0;
        let mut precision: i32 = 0;
        let mut trx_num: i32 = 0;
        let mut num: i32 = 0;
        let mut start_time: i64 = 0;
        let mut end_time: i64 = 0;
        let mut description = String::new();
        let mut url = String::new();
        // Phase 2 fields
        let mut free_asset_net_limit: i64 = 0;
        let mut public_free_asset_net_limit: i64 = 0;
        let mut public_free_asset_net_usage: i64 = 0;
        let mut public_latest_free_net_time: i64 = 0;

        let mut pos = 0;

        while pos < data.len() {
            // Read field header
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address (length-delimited) - skip, use transaction.from
                    let (_payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += total_len;
                }
                (2, 2) => {
                    // name (bytes)
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    name = String::from_utf8_lossy(payload).to_string();
                    pos += total_len;
                }
                (3, 2) => {
                    // abbr (bytes)
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    abbr = String::from_utf8_lossy(payload).to_string();
                    pos += total_len;
                }
                (4, 0) => {
                    // total_supply (int64, varint)
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    total_supply = value as i64;
                }
                (6, 0) => {
                    // trx_num (int32, varint)
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    trx_num = value as i32;
                }
                (7, 0) => {
                    // precision (int32, varint)
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    precision = value as i32;
                }
                (8, 0) => {
                    // num (int32, varint)
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    num = value as i32;
                }
                (9, 0) => {
                    // start_time (int64, varint)
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    start_time = value as i64;
                }
                (10, 0) => {
                    // end_time (int64, varint)
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    end_time = value as i64;
                }
                (20, 2) => {
                    // description (bytes)
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    description = String::from_utf8_lossy(payload).to_string();
                    pos += total_len;
                }
                (21, 2) => {
                    // url (bytes)
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    url = String::from_utf8_lossy(payload).to_string();
                    pos += total_len;
                }
                (22, 0) => {
                    // free_asset_net_limit (int64, varint)
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    free_asset_net_limit = value as i64;
                }
                (23, 0) => {
                    // public_free_asset_net_limit (int64, varint)
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    public_free_asset_net_limit = value as i64;
                }
                (24, 0) => {
                    // public_free_asset_net_usage (int64, varint)
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    public_free_asset_net_usage = value as i64;
                }
                (25, 0) => {
                    // public_latest_free_net_time (int64, varint)
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    public_latest_free_net_time = value as i64;
                }
                _ => {
                    // Skip unknown fields
                    let bytes_skipped = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += bytes_skipped;
                }
            }
        }

        Ok(AssetIssueInfo {
            name,
            abbr,
            total_supply,
            precision,
            trx_num,
            num,
            start_time,
            end_time,
            description,
            url,
            free_asset_net_limit,
            public_free_asset_net_limit,
            public_free_asset_net_usage,
            public_latest_free_net_time,
        })
    }

    // ==========================================================================
    // Phase 2.C: Contract Metadata Contracts (33/45/48)
    // ==========================================================================
    //
    // These contracts modify smart contract metadata:
    // - UpdateSettingContract (33): Updates consume_user_resource_percent
    // - UpdateEnergyLimitContract (45): Updates origin_energy_limit
    // - ClearABIContract (48): Clears the contract's ABI

    /// Execute an UPDATE_SETTING_CONTRACT (type 33)
    /// Updates the consume_user_resource_percent field of a smart contract
    ///
    /// Validation:
    /// - Owner must exist
    /// - Contract must exist
    /// - Owner must be the contract's origin_address
    /// - New percent must be in [0, 100]
    ///
    /// Execute:
    /// - Read SmartContract from ContractStore
    /// - Update consume_user_resource_percent field
    /// - Write back to ContractStore
    fn execute_update_setting_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use revm_primitives::Address;
        use tron_backend_execution::TronExecutionResult;

        // 1. Require contract_parameter with matching type_url (strict enforcement)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.UpdateSettingContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [UpdateSettingContract], real type[class com.google.protobuf.Any]",
        )?;
        let (owner_in_contract, contract_address, new_percent) =
            self.parse_update_setting_contract(contract_bytes)?;

        debug!(
            "Parsed UpdateSettingContract: contract_address={}, new_percent={}",
            hex::encode(&contract_address),
            new_percent
        );

        // 2. Validate owner address
        let owner_tron = Self::resolve_owner_address(
            owner_in_contract.as_slice(),
            transaction.metadata.from_raw.as_deref(),
        );

        let expected_prefix = storage_adapter.address_prefix();
        if owner_tron.len() != 21 || owner_tron[0] != expected_prefix {
            return Err("Invalid address".to_string());
        }
        let owner = Address::from_slice(&owner_tron[1..]);
        let owner_key = owner_tron.to_vec();
        let readable_owner_address = hex::encode(owner_tron);

        // 3. Validate owner exists
        let _owner_account = storage_adapter
            .get_account(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("Account[{}] does not exist", readable_owner_address))?;

        // 4. Validate new_percent is in [0, 100]
        if new_percent > 100 || new_percent < 0 {
            return Err("percent not in [0, 100]".to_string());
        }

        // 5. Get the smart contract
        let mut smart_contract = storage_adapter
            .get_smart_contract(&contract_address)
            .map_err(|e| format!("Failed to get contract: {}", e))?
            .ok_or_else(|| "Contract does not exist".to_string())?;

        // 6. Validate owner is the contract's origin_address
        if smart_contract.origin_address != owner_key {
            return Err(format!(
                "Account[{}] is not the owner of the contract",
                readable_owner_address
            ));
        }

        // 7. Update the consume_user_resource_percent field
        let old_percent = smart_contract.consume_user_resource_percent;
        smart_contract.consume_user_resource_percent = new_percent;

        debug!(
            "Updating consume_user_resource_percent: {} -> {}",
            old_percent, new_percent
        );

        // 8. Write back to ContractStore
        storage_adapter
            .put_smart_contract(&smart_contract)
            .map_err(|e| format!("Failed to update contract: {}", e))?;

        // 9. Build result - no state changes for account balances, fee = 0
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes: vec![], // No account balance changes
            logs: Vec::new(),
            error: None,
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

    /// Parse UpdateSettingContract from protobuf bytes
    /// UpdateSettingContract:
    ///   bytes owner_address = 1;
    ///   bytes contract_address = 2;
    ///   int64 consume_user_resource_percent = 3;
    fn parse_update_setting_contract(
        &self,
        data: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>, i64), String> {
        use contracts::proto::{
            read_length_delimited_typed, read_tag_typed, read_varint_typed,
            skip_protobuf_field_checked, ProtobufError,
        };

        let map_err = |e: ProtobufError| e.to_java_message();

        let mut owner_address: Vec<u8> = vec![];
        let mut contract_address: Vec<u8> = vec![];
        let mut consume_user_resource_percent: i64 = 0;
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, bytes_read) =
                read_tag_typed(&data[pos..]).map_err(map_err)?;
            pos += bytes_read;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address (bytes)
                    let (payload, total) =
                        read_length_delimited_typed(&data[pos..]).map_err(map_err)?;
                    owner_address = payload.to_vec();
                    pos += total;
                }
                (2, 2) => {
                    // contract_address (bytes)
                    let (payload, total) =
                        read_length_delimited_typed(&data[pos..]).map_err(map_err)?;
                    contract_address = payload.to_vec();
                    pos += total;
                }
                (3, 0) => {
                    // consume_user_resource_percent (varint)
                    let (value, bytes_read) =
                        read_varint_typed(&data[pos..]).map_err(|e| ProtobufError::from(e).to_java_message())?;
                    consume_user_resource_percent = value as i64;
                    pos += bytes_read;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(map_err)?;
                    pos += skip_len;
                }
            }
        }

        Ok((
            owner_address,
            contract_address,
            consume_user_resource_percent,
        ))
    }

    /// Execute an UPDATE_ENERGY_LIMIT_CONTRACT (type 45)
    /// Updates the origin_energy_limit field of a smart contract
    ///
    /// Validation:
    /// - Energy limit feature must be enabled (block_num >= BLOCK_NUM_FOR_ENERGY_LIMIT)
    /// - Owner must exist
    /// - Contract must exist
    /// - Owner must be the contract's origin_address
    /// - New origin_energy_limit must be > 0
    ///
    /// Execute:
    /// - Read SmartContract from ContractStore
    /// - Update origin_energy_limit field
    /// - Write back to ContractStore
    fn execute_update_energy_limit_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::TronExecutionResult;

        debug!("Executing UPDATE_ENERGY_LIMIT_CONTRACT");

        // 1. Check if energy limit feature is enabled (fork gate)
        // Java: ReceiptCapsule.checkForEnergyLimit(ds)
        // Must pass BEFORE any other validation.
        let energy_limit_enabled = storage_adapter
            .check_for_energy_limit()
            .map_err(|e| format!("Failed to check energy limit: {}", e))?;

        if !energy_limit_enabled {
            return Err(
                "contract type error, unexpected type [UpdateEnergyLimitContract]".to_string(),
            );
        }

        // 2. Require contract_parameter with matching type_url (strict enforcement)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.UpdateEnergyLimitContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [UpdateEnergyLimitContract],real type[class com.google.protobuf.Any]",
        )?;
        let (owner_in_contract, contract_address, new_origin_energy_limit) =
            self.parse_update_energy_limit_contract(contract_bytes)?;

        debug!(
            "Parsed UpdateEnergyLimitContract: contract_address={}, new_origin_energy_limit={}",
            hex::encode(&contract_address),
            new_origin_energy_limit
        );

        // 3. Validate owner address (Java: DecodeUtil.addressValid)
        let owner_tron = Self::resolve_owner_address(
            owner_in_contract.as_slice(),
            transaction.metadata.from_raw.as_deref(),
        );

        let expected_prefix = storage_adapter.address_prefix();
        if owner_tron.len() != 21 || owner_tron[0] != expected_prefix {
            return Err("Invalid address".to_string());
        }
        let owner = revm_primitives::Address::from_slice(&owner_tron[1..]);
        let owner_key = owner_tron.to_vec();
        let readable_owner_address = hex::encode(owner_tron);

        // 5. Validate owner account exists
        // Java: "Account[<hex 21-byte owner>] does not exist"
        let _owner_account = storage_adapter
            .get_account(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("Account[{}] does not exist", readable_owner_address))?;

        // 6. Validate new_origin_energy_limit > 0
        // Java: "origin energy limit must be > 0"
        if new_origin_energy_limit <= 0 {
            return Err("origin energy limit must be > 0".to_string());
        }

        // 7. Get the smart contract
        // Java: "Contract does not exist" (empty contract_address falls through here)
        let mut smart_contract = storage_adapter
            .get_smart_contract(&contract_address)
            .map_err(|e| format!("Failed to get contract: {}", e))?
            .ok_or_else(|| "Contract does not exist".to_string())?;

        // 8. Validate owner is the contract's origin_address
        // Java: "Account[<hex 21-byte owner>] is not the owner of the contract"
        if smart_contract.origin_address != owner_key {
            return Err(format!(
                "Account[{}] is not the owner of the contract",
                readable_owner_address
            ));
        }

        // 9. Update the origin_energy_limit field
        let old_limit = smart_contract.origin_energy_limit;
        smart_contract.origin_energy_limit = new_origin_energy_limit;

        debug!(
            "Updating origin_energy_limit: {} -> {}",
            old_limit, new_origin_energy_limit
        );

        // 10. Write back to ContractStore
        storage_adapter
            .put_smart_contract(&smart_contract)
            .map_err(|e| format!("Failed to update contract: {}", e))?;

        // 11. Build result - no state changes for account balances, fee = 0
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes: vec![], // No account balance changes
            logs: Vec::new(),
            error: None,
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

    /// Parse UpdateEnergyLimitContract from protobuf bytes
    /// UpdateEnergyLimitContract:
    ///   bytes owner_address = 1;
    ///   bytes contract_address = 2;
    ///   int64 origin_energy_limit = 3;
    ///
    /// Returns (owner_address, contract_address, origin_energy_limit).
    fn parse_update_energy_limit_contract(
        &self,
        data: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>, i64), String> {
        use contracts::proto::{
            read_length_delimited_typed, read_tag_typed, read_varint_typed,
            skip_protobuf_field_checked, ProtobufError,
        };

        let map_err = |e: ProtobufError| e.to_java_message();

        let mut owner_address: Vec<u8> = vec![];
        let mut contract_address: Vec<u8> = vec![];
        let mut origin_energy_limit: i64 = 0;
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, bytes_read) =
                read_tag_typed(&data[pos..]).map_err(map_err)?;
            pos += bytes_read;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address (bytes)
                    let (payload, total) =
                        read_length_delimited_typed(&data[pos..]).map_err(map_err)?;
                    owner_address = payload.to_vec();
                    pos += total;
                }
                (2, 2) => {
                    // contract_address (bytes)
                    let (payload, total) =
                        read_length_delimited_typed(&data[pos..]).map_err(map_err)?;
                    contract_address = payload.to_vec();
                    pos += total;
                }
                (3, 0) => {
                    // origin_energy_limit (varint)
                    let (value, bytes_read) =
                        read_varint_typed(&data[pos..]).map_err(|e| ProtobufError::from(e).to_java_message())?;
                    origin_energy_limit = value as i64;
                    pos += bytes_read;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(map_err)?;
                    pos += skip_len;
                }
            }
        }

        Ok((owner_address, contract_address, origin_energy_limit))
    }

    /// Execute a CLEAR_ABI_CONTRACT (type 48)
    /// Clears the ABI of a smart contract by writing an empty ABI
    ///
    /// Validation:
    /// - Constantinople fork must be enabled (getAllowTvmConstantinople() != 0)
    /// - Owner must exist
    /// - Contract must exist
    /// - Owner must be the contract's origin_address
    ///
    /// Execute:
    /// - Write default (empty) ABI to AbiStore
    fn execute_clear_abi_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::TronExecutionResult;

        // 1. Check if Constantinople is enabled
        let allow_constantinople = storage_adapter
            .get_allow_tvm_constantinople()
            .map_err(|e| format!("Failed to get Constantinople status: {}", e))?;

        if allow_constantinople == 0 {
            return Err("contract type error,unexpected type [ClearABIContract]".to_string());
        }

        // 2. Require contract_parameter with matching type_url (strict enforcement)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.ClearABIContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [ClearABIContract],real type[class com.google.protobuf.Any]",
        )?;
        let (owner_in_contract, contract_address) =
            self.parse_clear_abi_contract(contract_bytes)?;

        let owner_tron = Self::resolve_owner_address(
            owner_in_contract.as_slice(),
            transaction.metadata.from_raw.as_deref(),
        );

        debug!(
            "Executing CLEAR_ABI_CONTRACT for owner={}, contract={}",
            hex::encode(owner_tron),
            hex::encode(&contract_address)
        );

        // 3. Validate owner address
        let expected_prefix = storage_adapter.address_prefix();
        if owner_tron.len() != 21 || owner_tron[0] != expected_prefix {
            return Err("Invalid address".to_string());
        }
        let owner = revm_primitives::Address::from_slice(&owner_tron[1..]);

        // 4. Validate owner exists
        let _owner_account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("Account[{}] not exists", hex::encode(owner_tron)))?;

        // 5. Get the smart contract (to validate ownership)
        let smart_contract = storage_adapter
            .get_smart_contract(&contract_address)
            .map_err(|e| format!("Failed to get contract: {}", e))?
            .ok_or_else(|| "Contract not exists".to_string())?;

        // 6. Validate owner is the contract's origin_address
        if smart_contract.origin_address != owner_tron {
            return Err(format!(
                "Account[{}] is not the owner of the contract",
                hex::encode(owner_tron)
            ));
        }

        // 7. Clear ABI by writing default empty ABI to AbiStore
        storage_adapter
            .clear_abi(&contract_address)
            .map_err(|e| format!("Failed to clear ABI: {}", e))?;

        debug!(
            "ABI cleared for contract {}",
            hex::encode(&contract_address)
        );

        // 8. Build result - no state changes for account balances, fee = 0
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes: vec![], // No account balance changes
            logs: Vec::new(),
            error: None,
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

    // =========================================================================
    // Phase 2.C2: UpdateBrokerage Contract (49)
    // =========================================================================
    // Allows witnesses to set their brokerage (commission rate) for delegation rewards.
    // Java reference: UpdateBrokerageActuator.java

    /// Execute UPDATE_BROKERAGE_CONTRACT (type 49)
    /// Updates the brokerage (commission rate) for a witness in DelegationStore.
    fn execute_update_brokerage_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use revm_primitives::Address;
        use tron_backend_execution::TronExecutionResult;

        // 1. Check if delegation changes are allowed
        // Java: dynamicStore.allowChangeDelegation()
        let allow_delegation = storage_adapter
            .allow_change_delegation()
            .map_err(|e| format!("Failed to check delegation status: {}", e))?;

        if !allow_delegation {
            return Err(
                "contract type error, unexpected type [UpdateBrokerageContract]".to_string(),
            );
        }

        // 2. Require contract_parameter with matching type_url (strict enforcement)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.UpdateBrokerageContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [UpdateBrokerageContract], real type[class com.google.protobuf.Any]",
        )?;
        let (owner_in_contract, brokerage) =
            self.parse_update_brokerage_contract(contract_bytes)?;

        // 3. Validate owner address (DecodeUtil.addressValid)
        let owner_tron = Self::resolve_owner_address(
            owner_in_contract.as_slice(),
            transaction.metadata.from_raw.as_deref(),
        );
        let expected_prefix = storage_adapter.address_prefix();
        if owner_tron.len() != 21 || owner_tron[0] != expected_prefix {
            return Err("Invalid ownerAddress".to_string());
        }
        let owner = Address::from_slice(&owner_tron[1..]);

        debug!(
            "Executing UPDATE_BROKERAGE_CONTRACT for owner {}",
            hex::encode(owner_tron)
        );

        debug!("Parsed UpdateBrokerageContract: brokerage={}%", brokerage);

        // 4. Validate brokerage range: 0-100
        // Java: if (brokerage < 0 || brokerage > ActuatorConstant.ONE_HUNDRED)
        if brokerage < 0 || brokerage > 100 {
            return Err("Invalid brokerage".to_string());
        }

        // 5. Validate owner is a witness
        // Java: WitnessCapsule witnessCapsule = witnessStore.get(ownerAddress);
        //       if (witnessCapsule == null) throw "Not existed witness"
        let is_witness = storage_adapter
            .is_witness(&owner)
            .map_err(|e| format!("Failed to check witness: {}", e))?;

        if !is_witness {
            return Err(format!("Not existed witness:{}", hex::encode(owner_tron)));
        }

        // 6. Validate owner exists in AccountStore
        let _owner_account = storage_adapter
            .get_account(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| "Account does not exist".to_string())?;

        // 7. Set brokerage in DelegationStore
        // Java: delegationStore.setBrokerage(ownerAddress, brokerage)
        // This is equivalent to setBrokerage(-1, ownerAddress, brokerage)
        storage_adapter
            .set_delegation_brokerage(-1, &owner, brokerage)
            .map_err(|e| format!("Failed to set brokerage: {}", e))?;

        debug!(
            "Brokerage set to {}% for witness {}",
            brokerage,
            hex::encode(owner_tron)
        );

        // 8. Build result - no fee for this contract, no account balance changes
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes: vec![], // No account balance changes
            logs: Vec::new(),
            error: None,
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

    /// Parse UpdateBrokerageContract from protobuf bytes
    /// UpdateBrokerageContract:
    ///   bytes owner_address = 1;
    ///   int32 brokerage = 2;
    fn parse_update_brokerage_contract(&self, data: &[u8]) -> Result<(Vec<u8>, i32), String> {
        use contracts::proto::{
            read_length_delimited_typed, read_tag_typed, read_varint_typed,
            skip_protobuf_field_checked, ProtobufError,
        };

        let map_err = |e: ProtobufError| e.to_java_message();

        let mut owner_address: Vec<u8> = vec![];
        let mut brokerage: i32 = 0;
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, bytes_read) =
                read_tag_typed(&data[pos..]).map_err(map_err)?;
            pos += bytes_read;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address (bytes)
                    let (payload, total) =
                        read_length_delimited_typed(&data[pos..]).map_err(map_err)?;
                    owner_address = payload.to_vec();
                    pos += total;
                }
                (2, 0) => {
                    // brokerage (int32, wire type 0 = varint)
                    let (value, bytes_read) =
                        read_varint_typed(&data[pos..]).map_err(|e| ProtobufError::from(e).to_java_message())?;
                    brokerage = value as i32;
                    pos += bytes_read;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(map_err)?;
                    pos += skip_len;
                }
            }
        }

        Ok((owner_address, brokerage))
    }

    /// Parse ClearABIContract from protobuf bytes with exhaustive error handling
    /// ClearABIContract:
    ///   bytes owner_address = 1;
    ///   bytes contract_address = 2;
    ///
    /// Error handling matches protobuf-java 3.21.12 for exact parity:
    /// - Truncated input → PROTOBUF_TRUNCATED_MESSAGE
    /// - Malformed varint → PROTOBUF_MALFORMED_VARINT
    /// - Invalid tag (0) → PROTOBUF_INVALID_TAG_ZERO
    /// - Invalid wire type (6/7) → PROTOBUF_INVALID_WIRE_TYPE
    fn parse_clear_abi_contract(&self, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
        use contracts::proto::{
            read_varint_typed, skip_protobuf_field_checked, ProtobufError, VarintError,
            PROTOBUF_INVALID_TAG_ZERO, PROTOBUF_INVALID_WIRE_TYPE, PROTOBUF_MALFORMED_VARINT,
            PROTOBUF_TRUNCATED_MESSAGE,
        };

        let mut owner_address: Vec<u8> = vec![];
        let mut contract_address: Vec<u8> = vec![];
        let mut pos = 0;

        while pos < data.len() {
            // Read field tag (field_number << 3 | wire_type)
            let (field_header, bytes_read) =
                read_varint_typed(&data[pos..]).map_err(|e| match e {
                    VarintError::Truncated => PROTOBUF_TRUNCATED_MESSAGE.to_string(),
                    VarintError::TooLong => PROTOBUF_MALFORMED_VARINT.to_string(),
                })?;
            pos += bytes_read;

            // Check for invalid tag (0)
            // protobuf-java: InvalidProtocolBufferException.invalidTag()
            if field_header == 0 {
                return Err(PROTOBUF_INVALID_TAG_ZERO.to_string());
            }

            let field_number = field_header >> 3;
            let wire_type = (field_header & 0x7) as u8;

            // Check for invalid wire types (6/7) before processing
            if wire_type == 6 || wire_type == 7 {
                return Err(PROTOBUF_INVALID_WIRE_TYPE.to_string());
            }

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address (bytes, length-delimited)
                    let (length, bytes_read) =
                        read_varint_typed(&data[pos..]).map_err(|e| match e {
                            VarintError::Truncated => PROTOBUF_TRUNCATED_MESSAGE.to_string(),
                            VarintError::TooLong => PROTOBUF_MALFORMED_VARINT.to_string(),
                        })?;
                    pos += bytes_read;
                    let end = pos + length as usize;
                    if end > data.len() {
                        return Err(PROTOBUF_TRUNCATED_MESSAGE.to_string());
                    }
                    owner_address = data[pos..end].to_vec();
                    pos = end;
                }
                (2, 2) => {
                    // contract_address (bytes, length-delimited)
                    let (length, bytes_read) =
                        read_varint_typed(&data[pos..]).map_err(|e| match e {
                            VarintError::Truncated => PROTOBUF_TRUNCATED_MESSAGE.to_string(),
                            VarintError::TooLong => PROTOBUF_MALFORMED_VARINT.to_string(),
                        })?;
                    pos += bytes_read;
                    let end = pos + length as usize;
                    if end > data.len() {
                        return Err(PROTOBUF_TRUNCATED_MESSAGE.to_string());
                    }
                    contract_address = data[pos..end].to_vec();
                    pos = end;
                }
                _ => {
                    // Unknown field - skip with bounds checking
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message())?;
                    pos += skip_len;
                }
            }
        }

        Ok((owner_address, contract_address))
    }

    // ========================================================================
    // Phase 2.D: Resource/Freeze/Delegation Contracts (56/57/58/59)
    // ========================================================================

    /// Execute WITHDRAW_EXPIRE_UNFREEZE_CONTRACT (type 56)
    /// Withdraws TRX from expired unfrozenV2 entries
    ///
    /// Java oracle: WithdrawExpireUnfreezeActuator.java
    /// Receipt: withdraw_expire_amount
    fn execute_withdraw_expire_unfreeze_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        // 1. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.WithdrawExpireUnfreezeContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [WithdrawExpireUnfreezeContract], real type[class com.google.protobuf.Any]",
        )?;

        // 2. Gate check: supportUnfreezeDelay() must be true
        let support_unfreeze_delay = storage_adapter
            .support_unfreeze_delay()
            .map_err(|e| format!("Failed to check supportUnfreezeDelay: {}", e))?;
        if !support_unfreeze_delay {
            return Err("Not support WithdrawExpireUnfreeze transaction, need to be opened by the committee".to_string());
        }

        // 3. WithdrawExpireUnfreezeContract: bytes owner_address = 1 (field 1, length-delimited)
        let mut owner_tron: Vec<u8> = Vec::new();
        let mut pos = 0;
        while pos < contract_bytes.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&contract_bytes[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&contract_bytes[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_tron = payload.to_vec();
                    pos += total_len;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&contract_bytes[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        // 4. Validate owner address
        let expected_prefix = storage_adapter.address_prefix();
        if owner_tron.len() != 21 || owner_tron[0] != expected_prefix {
            return Err("Invalid address".to_string());
        }
        let owner = revm_primitives::Address::from_slice(&owner_tron[1..]);

        debug!("WithdrawExpireUnfreeze: owner={}", hex::encode(&owner_tron));

        // 5. Validate owner account exists
        let account_proto = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or_else(|| format!("Account[{}] not exists", hex::encode(&owner_tron)))?;

        // 6. Get latest block timestamp
        let now = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest timestamp: {}", e))?;

        // 7. Calculate total withdrawable amount from expired unfrozenV2 entries
        let unfrozen_v2_list = &account_proto.unfrozen_v2;
        let mut total_withdraw: i64 = 0;
        let mut remaining_unfrozen: Vec<tron_backend_execution::protocol::account::UnFreezeV2> =
            Vec::new();

        for entry in unfrozen_v2_list.iter() {
            if entry.unfreeze_expire_time <= now as i64 {
                // Expired - add to withdraw amount
                total_withdraw = total_withdraw.wrapping_add(entry.unfreeze_amount);
            } else {
                // Not expired - keep in list
                remaining_unfrozen.push(entry.clone());
            }
        }

        // 8. Validate there's something to withdraw
        if total_withdraw <= 0 {
            return Err("no unFreeze balance to withdraw ".to_string());
        }

        // 9. Check for overflow (java-tron: LongMath.checkedAdd)
        let new_balance = account_proto
            .balance
            .checked_add(total_withdraw)
            .ok_or_else(|| {
                format!(
                    "overflow: checkedAdd({}, {})",
                    account_proto.balance, total_withdraw
                )
            })?;

        // 10. Update account: balance += total_withdraw, clear and replace unfrozenV2 list
        let mut updated_account = account_proto.clone();
        updated_account.balance = new_balance;
        updated_account.unfrozen_v2.clear();
        for entry in remaining_unfrozen {
            updated_account.unfrozen_v2.push(entry);
        }

        // 11. Persist updated account
        storage_adapter
            .put_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to persist account: {}", e))?;

        // 12. Build state change for CSV parity
        let old_account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(account_proto.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };
        let new_account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(new_balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        let state_changes = vec![TronStateChange::AccountChange {
            address: owner,
            old_account: Some(old_account_info),
            new_account: Some(new_account_info),
        }];

        // 13. Build receipt with withdraw_expire_amount
        let receipt_bytes = TransactionResultBuilder::new()
            .with_withdraw_expire_amount(total_withdraw)
            .build();

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!(
            "WithdrawExpireUnfreeze: withdrew {} SUN, remaining entries: {}",
            total_withdraw,
            updated_account.unfrozen_v2.len()
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: Some(receipt_bytes),
            contract_address: None,
        })
    }

    /// Execute CANCEL_ALL_UNFREEZE_V2_CONTRACT (type 59)
    /// Cancels all pending unfreezeV2 entries, re-freezing unexpired and withdrawing expired
    ///
    /// Java oracle: CancelAllUnfreezeV2Actuator.java
    /// Receipt: withdraw_expire_amount + cancel_unfreezeV2_amount map
    fn execute_cancel_all_unfreeze_v2_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use contracts::proto::TransactionResultBuilder;

        // 1. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.CancelAllUnfreezeV2Contract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [CancelAllUnfreezeV2Contract], real type[class com.google.protobuf.Any]",
        )?;

        // Parse owner_address from contract bytes (CancelAllUnfreezeV2Contract: bytes owner_address = 1)
        let mut owner_tron: Vec<u8> = Vec::new();
        let mut pos = 0;
        while pos < contract_bytes.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&contract_bytes[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address (bytes, field 1, wire type 2 = length-delimited)
                    let (payload, total_len) = read_length_delimited_typed(&contract_bytes[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_tron = payload.to_vec();
                    pos += total_len;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&contract_bytes[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        debug!("CancelAllUnfreezeV2: owner={}", hex::encode(&owner_tron));

        // 2. Gate check: supportAllowCancelAllUnfreezeV2() must be true
        let allow_cancel = storage_adapter
            .support_allow_cancel_all_unfreeze_v2()
            .map_err(|e| format!("Failed to check supportAllowCancelAllUnfreezeV2: {}", e))?;
        if !allow_cancel {
            return Err(
                "Not support CancelAllUnfreezeV2 transaction, need to be opened by the committee"
                    .to_string(),
            );
        }

        // 3. Validate owner address (must be 21 bytes with correct prefix)
        let expected_prefix = storage_adapter.address_prefix();
        if owner_tron.len() != 21 || owner_tron[0] != expected_prefix {
            return Err("Invalid address".to_string());
        }
        let owner = revm_primitives::Address::from_slice(&owner_tron[1..]);

        // 4. Validate owner account exists
        let account_proto = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or_else(|| format!("Account[{}] not exists", hex::encode(&owner_tron)))?;

        // 5. Get latest block timestamp
        let now = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest timestamp: {}", e))?;

        // 6. Validate there are unfrozenV2 entries to process
        let unfrozen_v2_list = &account_proto.unfrozen_v2;
        if unfrozen_v2_list.is_empty() {
            return Err("No unfreezeV2 list to cancel".to_string());
        }

        // 6. Process each unfrozenV2 entry:
        //    - Expired (expire_time <= now): add to withdraw_expire_amount
        //    - Unexpired: re-freeze and add to cancel map
        let mut withdraw_expire_amount: i64 = 0;
        let mut cancel_bandwidth: i64 = 0;
        let mut cancel_energy: i64 = 0;
        let mut cancel_tron_power: i64 = 0;

        // Track delta for total weights
        let mut net_weight_delta: i64 = 0;
        let mut energy_weight_delta: i64 = 0;
        let mut tp_weight_delta: i64 = 0;

        let mut updated_account = account_proto.clone();

        for entry in unfrozen_v2_list.iter() {
            if entry.unfreeze_expire_time <= now as i64 {
                // Expired - add to withdraw amount
                withdraw_expire_amount = withdraw_expire_amount
                    .checked_add(entry.unfreeze_amount)
                    .ok_or("Overflow calculating withdraw amount")?;
            } else {
                // Unexpired - re-freeze
                let resource_type = entry.r#type; // 0=BANDWIDTH, 1=ENERGY, 2=TRON_POWER
                let amount = entry.unfreeze_amount;

                match resource_type {
                    0 => {
                        // BANDWIDTH
                        cancel_bandwidth += amount;
                        let weight_before =
                            Self::get_frozen_v2_balance_with_delegated_bandwidth(&updated_account)
                                / TRX_PRECISION as i64;
                        // Re-freeze: add to frozenV2 bandwidth
                        Self::add_frozen_v2_bandwidth(&mut updated_account, amount);
                        let weight_after =
                            Self::get_frozen_v2_balance_with_delegated_bandwidth(&updated_account)
                                / TRX_PRECISION as i64;
                        net_weight_delta += weight_after - weight_before;
                    }
                    1 => {
                        // ENERGY
                        cancel_energy += amount;
                        let weight_before =
                            Self::get_frozen_v2_balance_with_delegated_energy(&updated_account)
                                / TRX_PRECISION as i64;
                        Self::add_frozen_v2_energy(&mut updated_account, amount);
                        let weight_after =
                            Self::get_frozen_v2_balance_with_delegated_energy(&updated_account)
                                / TRX_PRECISION as i64;
                        energy_weight_delta += weight_after - weight_before;
                    }
                    2 => {
                        // TRON_POWER
                        cancel_tron_power += amount;
                        let weight_before =
                            Self::get_tron_power_frozen_v2_balance(&updated_account)
                                / TRX_PRECISION as i64;
                        Self::add_frozen_v2_tron_power(&mut updated_account, amount);
                        let weight_after = Self::get_tron_power_frozen_v2_balance(&updated_account)
                            / TRX_PRECISION as i64;
                        tp_weight_delta += weight_after - weight_before;
                    }
                    _ => {
                        warn!("Unknown resource type {} in unfrozenV2", resource_type);
                    }
                }
            }
        }

        // 7. Clear unfrozenV2 list
        updated_account.unfrozen_v2.clear();

        // 8. Add expired amount to balance
        if withdraw_expire_amount > 0 {
            updated_account.balance = updated_account
                .balance
                .checked_add(withdraw_expire_amount)
                .ok_or("Balance overflow")?;
        }

        // 9. Update total resource weights in DynamicPropertiesStore
        if net_weight_delta != 0 {
            storage_adapter
                .add_total_net_weight(net_weight_delta)
                .map_err(|e| format!("Failed to update total net weight: {}", e))?;
        }
        if energy_weight_delta != 0 {
            storage_adapter
                .add_total_energy_weight(energy_weight_delta)
                .map_err(|e| format!("Failed to update total energy weight: {}", e))?;
        }
        if tp_weight_delta != 0 {
            storage_adapter
                .add_total_tron_power_weight(tp_weight_delta)
                .map_err(|e| format!("Failed to update total tron power weight: {}", e))?;
        }

        // 10. Persist updated account
        storage_adapter
            .put_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to persist account: {}", e))?;

        // 11. Build state change for CSV parity
        let old_account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(account_proto.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };
        let new_account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(updated_account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        let state_changes = vec![TronStateChange::AccountChange {
            address: owner,
            old_account: Some(old_account_info),
            new_account: Some(new_account_info),
        }];

        // 12. Build receipt with withdraw_expire_amount and cancel_unfreezeV2_amount map
        // Java parity: withdraw_expire_amount is only emitted when non-zero
        let mut builder = TransactionResultBuilder::new();
        if withdraw_expire_amount > 0 {
            builder = builder.with_withdraw_expire_amount(withdraw_expire_amount);
        }
        let receipt_bytes = builder
            .with_cancel_unfreeze_v2_amounts(cancel_bandwidth, cancel_energy, cancel_tron_power)
            .build();

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!(
            "CancelAllUnfreezeV2: withdrew={}, cancel_bw={}, cancel_energy={}, cancel_tp={}",
            withdraw_expire_amount, cancel_bandwidth, cancel_energy, cancel_tron_power
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: Some(receipt_bytes),
            contract_address: None,
        })
    }

    /// Execute DELEGATE_RESOURCE_CONTRACT (type 57)
    /// Delegates frozen resources (bandwidth/energy) to another account
    ///
    /// Java oracle: DelegateResourceActuator.java
    fn execute_delegate_resource_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.DelegateResourceContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [DelegateResourceContract],real type[class com.google.protobuf.Any]",
        )?;

        // Parse contract data
        let delegate_info = self.parse_delegate_resource_contract(contract_bytes)?;

        // 1. Gate check: supportDR() must be true
        let support_dr = storage_adapter
            .support_dr()
            .map_err(|e| format!("Failed to check supportDR: {}", e))?;
        if !support_dr {
            return Err("No support for resource delegate".to_string());
        }

        // 2. Gate check: supportUnfreezeDelay() must be true
        let support_unfreeze_delay = storage_adapter
            .support_unfreeze_delay()
            .map_err(|e| format!("Failed to check supportUnfreezeDelay: {}", e))?;
        if !support_unfreeze_delay {
            return Err(
                "Not support Delegate resource transaction, need to be opened by the committee"
                    .to_string(),
            );
        }

        // 3. Validate owner address (DecodeUtil.addressValid)
        // Java parity: use owner_address from contract protobuf (field 1), not transaction.metadata.from_raw
        let expected_prefix = storage_adapter.address_prefix();
        let owner_raw = delegate_info.owner_address.as_slice();
        if owner_raw.len() != 21 || owner_raw[0] != expected_prefix {
            return Err("Invalid address".to_string());
        }
        let owner = revm_primitives::Address::from_slice(&owner_raw[1..]);
        let readable_owner_address = hex::encode(owner_raw);

        // 4. Validate owner account exists
        let owner_account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("Account[{}] not exists", readable_owner_address))?;

        // 5. Validate delegate balance >= 1 TRX
        if delegate_info.balance < TRX_PRECISION as i64 {
            return Err("delegateBalance must be greater than or equal to 1 TRX".to_string());
        }

        // 6. Validate sufficient AVAILABLE frozen balance for the resource type
        // Java parity: DelegateResourceActuator.validate() - uses "available FreezeV2" which
        // accounts for current resource usage (net_usage / energy_usage after decay)
        //
        // Get timestamp and compute head_slot for usage decay calculation
        let now_timestamp = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest timestamp: {}", e))?;
        let head_slot = (now_timestamp as i64) / 3000; // slot = timestamp_ms / BLOCK_PRODUCED_INTERVAL

        match delegate_info.resource {
            0 => {
                // BANDWIDTH
                // Get global totals for bandwidth
                let total_net_weight = storage_adapter
                    .get_total_net_weight()
                    .map_err(|e| format!("Failed to get total net weight: {}", e))?;
                let total_net_limit = storage_adapter
                    .get_total_net_limit()
                    .map_err(|e| format!("Failed to get total net limit: {}", e))?;

                // Compute available FreezeV2 balance after accounting for usage
                let available_bandwidth = Self::compute_available_freeze_v2_bandwidth(
                    &owner_account,
                    total_net_weight,
                    total_net_limit,
                    head_slot,
                );

                debug!("DelegateResource BANDWIDTH validation: frozen_v2={}, available={}, delegate_balance={}, net_usage={}, latest_consume_time={}, head_slot={}",
                       Self::get_frozen_v2_balance_for_bandwidth(&owner_account),
                       available_bandwidth,
                       delegate_info.balance,
                       owner_account.net_usage,
                       owner_account.latest_consume_time,
                       head_slot);

                if available_bandwidth < delegate_info.balance {
                    return Err("delegateBalance must be less than or equal to available FreezeBandwidthV2 balance".to_string());
                }
            }
            1 => {
                // ENERGY
                // Get global totals for energy
                let total_energy_weight = storage_adapter
                    .get_total_energy_weight()
                    .map_err(|e| format!("Failed to get total energy weight: {}", e))?;
                let total_energy_limit = storage_adapter
                    .get_total_energy_limit()
                    .map_err(|e| format!("Failed to get total energy limit: {}", e))?;

                // Compute available FreezeV2 balance after accounting for usage
                let available_energy = Self::compute_available_freeze_v2_energy(
                    &owner_account,
                    total_energy_weight,
                    total_energy_limit,
                    head_slot,
                );

                let (energy_usage, latest_consume_time) = match &owner_account.account_resource {
                    Some(res) => (res.energy_usage, res.latest_consume_time_for_energy),
                    None => (0, 0),
                };

                debug!("DelegateResource ENERGY validation: frozen_v2={}, available={}, delegate_balance={}, energy_usage={}, latest_consume_time={}, head_slot={}",
                       Self::get_frozen_v2_balance_for_energy(&owner_account),
                       available_energy,
                       delegate_info.balance,
                       energy_usage,
                       latest_consume_time,
                       head_slot);

                if available_energy < delegate_info.balance {
                    return Err("delegateBalance must be less than or equal to available FreezeEnergyV2 balance".to_string());
                }
            }
            _ => {
                return Err("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]".to_string());
            }
        }

        // 7. Validate receiver address (DecodeUtil.addressValid)
        let receiver_raw = delegate_info.receiver_address.as_slice();
        if receiver_raw.len() != 21 || receiver_raw[0] != expected_prefix {
            return Err("Invalid receiverAddress".to_string());
        }
        let receiver_address = revm_primitives::Address::from_slice(&receiver_raw[1..]);
        let readable_receiver_address = hex::encode(receiver_raw);

        // 8. Validate receiver is different from owner
        if owner == receiver_address {
            return Err("receiverAddress must not be the same as ownerAddress".to_string());
        }

        // 9. Validate receiver exists
        let receiver_account = storage_adapter
            .get_account_proto(&receiver_address)
            .map_err(|e| format!("Failed to get receiver account: {}", e))?
            .ok_or_else(|| format!("Account[{}] not exists", readable_receiver_address))?;

        // 10. Calculate expiration (reuse now_timestamp from step 6)
        let now = now_timestamp;

        let support_max_delegate_lock_period =
            storage_adapter
                .support_max_delegate_lock_period()
                .map_err(|e| format!("Failed to check supportMaxDelegateLockPeriod: {}", e))?;
        let default_lock_period: i64 = 86400; // DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL
        let lock_period = if delegate_info.lock {
            if support_max_delegate_lock_period {
                if delegate_info.lock_period == 0 {
                    default_lock_period
                } else {
                    delegate_info.lock_period
                }
            } else {
                default_lock_period
            }
        } else {
            0
        };

        if delegate_info.lock && support_max_delegate_lock_period {
            let max_lock_period = storage_adapter
                .get_max_delegate_lock_period()
                .map_err(|e| format!("Failed to get maxDelegateLockPeriod: {}", e))?;
            if lock_period < 0 || lock_period > max_lock_period {
                return Err(format!(
                    "The lock period of delegate resource cannot be less than 0 and cannot exceed {}!",
                    max_lock_period
                ));
            }

            if let Some(dr) = storage_adapter
                .get_delegated_resource(&owner, &receiver_address, true)
                .map_err(|e| format!("Failed to get DelegatedResource: {}", e))?
            {
                let expire_time_for_resource = match delegate_info.resource {
                    0 => dr.expire_time_for_bandwidth,
                    1 => dr.expire_time_for_energy,
                    _ => 0,
                };
                let remain_time = expire_time_for_resource - now as i64;
                let lock_period_ms = lock_period
                    .checked_mul(3000)
                    .ok_or_else(|| "Overflow calculating lock period".to_string())?;
                if lock_period_ms < remain_time {
                    let resource_name = match delegate_info.resource {
                        0 => "BANDWIDTH",
                        1 => "ENERGY",
                        _ => "UNKNOWN",
                    };
                    return Err(format!(
                        "The lock period for {} this time cannot be less than the remaining time[{}ms] of the last lock period for {}!",
                        resource_name, remain_time, resource_name
                    ));
                }
            }
        }

        if receiver_account.r#type == 2 {
            // Contract type
            return Err("Do not allow delegate resources to contract addresses".to_string());
        }

        debug!("DelegateResource: owner={}, receiver={}, balance={}, resource={}, lock={}, lock_period={}",
               readable_owner_address, readable_receiver_address,
               delegate_info.balance, delegate_info.resource, delegate_info.lock, lock_period);

        let expire_time = if delegate_info.lock {
            now as i64 + lock_period * 3000 // BLOCK_PRODUCED_INTERVAL = 3000ms
        } else {
            0
        };

        // Update owner account
        let mut updated_owner = owner_account.clone();
        match delegate_info.resource {
            0 => {
                // BANDWIDTH
                Self::add_delegated_frozen_v2_balance_for_bandwidth(
                    &mut updated_owner,
                    delegate_info.balance,
                );
                Self::add_frozen_v2_bandwidth(&mut updated_owner, -delegate_info.balance);
            }
            1 => {
                // ENERGY
                Self::add_delegated_frozen_v2_balance_for_energy(
                    &mut updated_owner,
                    delegate_info.balance,
                );
                Self::add_frozen_v2_energy(&mut updated_owner, -delegate_info.balance);
            }
            _ => {}
        }

        // 11. Update receiver account
        let mut updated_receiver = receiver_account.clone();
        match delegate_info.resource {
            0 => {
                // BANDWIDTH
                Self::add_acquired_delegated_frozen_v2_balance_for_bandwidth(
                    &mut updated_receiver,
                    delegate_info.balance,
                );
            }
            1 => {
                // ENERGY
                Self::add_acquired_delegated_frozen_v2_balance_for_energy(
                    &mut updated_receiver,
                    delegate_info.balance,
                );
            }
            _ => {}
        }

        // 12. Update/Create DelegatedResource record
        storage_adapter
            .delegate_resource(
                &owner,
                &receiver_address,
                delegate_info.resource == 0, // isBandwidth
                delegate_info.balance,
                delegate_info.lock,
                expire_time,
            )
            .map_err(|e| format!("Failed to update DelegatedResource: {}", e))?;

        // 13. Update DelegatedResourceAccountIndex
        storage_adapter
            .delegate_resource_account_index(&owner, &receiver_address, now as i64)
            .map_err(|e| format!("Failed to update DelegatedResourceAccountIndex: {}", e))?;

        // 14. Persist accounts
        storage_adapter
            .put_account_proto(&owner, &updated_owner)
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;
        storage_adapter
            .put_account_proto(&receiver_address, &updated_receiver)
            .map_err(|e| format!("Failed to persist receiver account: {}", e))?;

        // 15. Build state changes - track balance changes (even though TRX balance doesn't change)
        let state_changes = vec![
            TronStateChange::AccountChange {
                address: owner,
                old_account: Some(revm_primitives::AccountInfo {
                    balance: revm_primitives::U256::from(owner_account.balance as u64),
                    nonce: 0,
                    code_hash: revm_primitives::B256::ZERO,
                    code: None,
                }),
                new_account: Some(revm_primitives::AccountInfo {
                    balance: revm_primitives::U256::from(updated_owner.balance as u64),
                    nonce: 0,
                    code_hash: revm_primitives::B256::ZERO,
                    code: None,
                }),
            },
            TronStateChange::AccountChange {
                address: receiver_address,
                old_account: Some(revm_primitives::AccountInfo {
                    balance: revm_primitives::U256::from(receiver_account.balance as u64),
                    nonce: 0,
                    code_hash: revm_primitives::B256::ZERO,
                    code: None,
                }),
                new_account: Some(revm_primitives::AccountInfo {
                    balance: revm_primitives::U256::from(updated_receiver.balance as u64),
                    nonce: 0,
                    code_hash: revm_primitives::B256::ZERO,
                    code: None,
                }),
            },
        ];

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!(
            "DelegateResource: delegated {} SUN of resource {} from {} to {}",
            delegate_info.balance,
            delegate_info.resource,
            readable_owner_address,
            readable_receiver_address
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
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

    /// Execute UNDELEGATE_RESOURCE_CONTRACT (type 58)
    /// Reclaims delegated resources from a receiver
    ///
    /// Java oracle: UnDelegateResourceActuator.java
    fn execute_undelegate_resource_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.UnDelegateResourceContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [UnDelegateResourceContract],real type[class com.google.protobuf.Any]",
        )?;

        // 1. Gate checks
        let support_dr = storage_adapter
            .support_dr()
            .map_err(|e| format!("Failed to check supportDR: {}", e))?;
        if !support_dr {
            return Err("No support for resource delegate".to_string());
        }

        let support_unfreeze_delay = storage_adapter
            .support_unfreeze_delay()
            .map_err(|e| format!("Failed to check supportUnfreezeDelay: {}", e))?;
        if !support_unfreeze_delay {
            return Err(
                "Not support unDelegate resource transaction, need to be opened by the committee"
                    .to_string(),
            );
        }

        // 2. Validate owner address (DecodeUtil.addressValid)
        let expected_prefix = storage_adapter.address_prefix();
        let owner_raw = transaction.metadata.from_raw.as_deref().unwrap_or(&[]);
        if owner_raw.len() != 21 || owner_raw[0] != expected_prefix {
            return Err("Invalid address".to_string());
        }
        let owner = revm_primitives::Address::from_slice(&owner_raw[1..]);
        let readable_owner_address = hex::encode(owner_raw);

        // 3. Validate owner exists
        let owner_account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("Account[{}] does not exist", readable_owner_address))?;

        // Parse contract data
        let undelegate_info = self.parse_undelegate_resource_contract(contract_bytes)?;

        // 4. Validate receiver address (DecodeUtil.addressValid)
        let receiver_raw = undelegate_info.receiver_address.as_slice();
        if receiver_raw.len() != 21 || receiver_raw[0] != expected_prefix {
            return Err("Invalid receiverAddress".to_string());
        }
        let receiver_address = revm_primitives::Address::from_slice(&receiver_raw[1..]);
        let readable_receiver_address = hex::encode(receiver_raw);

        // 5. Validate receiver different from owner
        if owner == receiver_address {
            return Err("receiverAddress must not be the same as ownerAddress".to_string());
        }

        // 6. Get timestamp
        let now = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest timestamp: {}", e))?;

        // 7. Load DelegatedResource records and validate existence
        let unlock_resource = storage_adapter
            .get_delegated_resource(&owner, &receiver_address, false)
            .map_err(|e| format!("Failed to get DelegatedResource: {}", e))?;
        let lock_resource = storage_adapter
            .get_delegated_resource(&owner, &receiver_address, true)
            .map_err(|e| format!("Failed to get DelegatedResource: {}", e))?;
        if unlock_resource.is_none() && lock_resource.is_none() {
            return Err("delegated Resource does not exist".to_string());
        }

        // 8. Validate balance > 0
        if undelegate_info.balance <= 0 {
            return Err("unDelegateBalance must be more than 0 TRX".to_string());
        }

        // 9. Validate resource and available balance
        match undelegate_info.resource {
            0 => {
                // BANDWIDTH
                let mut delegate_balance = unlock_resource
                    .as_ref()
                    .map(|r| r.frozen_balance_for_bandwidth)
                    .unwrap_or(0);
                if let Some(lock) = lock_resource.as_ref() {
                    if lock.expire_time_for_bandwidth < now {
                        delegate_balance += lock.frozen_balance_for_bandwidth;
                    }
                }
                if delegate_balance < undelegate_info.balance {
                    return Err(format!(
                        "insufficient delegatedFrozenBalance(BANDWIDTH), request={}, unlock_balance={}",
                        undelegate_info.balance, delegate_balance
                    ));
                }
            }
            1 => {
                // ENERGY
                let mut delegate_balance = unlock_resource
                    .as_ref()
                    .map(|r| r.frozen_balance_for_energy)
                    .unwrap_or(0);
                if let Some(lock) = lock_resource.as_ref() {
                    if lock.expire_time_for_energy < now {
                        delegate_balance += lock.frozen_balance_for_energy;
                    }
                }
                if delegate_balance < undelegate_info.balance {
                    return Err(format!(
                        "insufficient delegateFrozenBalance(Energy), request={}, unlock_balance={}",
                        undelegate_info.balance, delegate_balance
                    ));
                }
            }
            _ => {
                return Err("ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]".to_string());
            }
        }

        debug!(
            "UnDelegateResource: owner={}, receiver={}, balance={}, resource={}",
            readable_owner_address,
            readable_receiver_address,
            undelegate_info.balance,
            undelegate_info.resource
        );

        // 10. Load dynamic properties needed for usage transfer computation
        let head_slot = (now as i64) / 3000; // Java: chainBaseManager.getHeadSlot()
        let support_cancel_all_unfreeze_v2 = storage_adapter
            .support_allow_cancel_all_unfreeze_v2()
            .map_err(|e| format!("Failed to check supportAllowCancelAllUnfreezeV2: {}", e))?;

        // 11. Get receiver account (might not exist if contract was destroyed)
        let receiver_account_opt = storage_adapter
            .get_account_proto(&receiver_address)
            .map_err(|e| format!("Failed to get receiver account: {}", e))?;

        // 12. Modify receiver account: recover usage, compute transferUsage, adjust balances
        // Java oracle: UnDelegateResourceActuator.execute() receiver section
        let mut transfer_usage: i64 = 0;
        let mut updated_receiver_opt = None;
        if let Some(receiver_account) = receiver_account_opt.as_ref() {
            let mut updated_receiver = receiver_account.clone();
            match undelegate_info.resource {
                0 => {
                    // BANDWIDTH
                    // Step A: BandwidthProcessor.updateUsageForDelegated(receiverCapsule)
                    // = increase(ac, BANDWIDTH, oldNetUsage, 0, latestConsumeTime, now)
                    let (recovered_net_usage, new_window_raw, new_window_optimized) =
                        Self::resource_increase_with_window(
                            updated_receiver.net_usage,
                            0,
                            updated_receiver.latest_consume_time,
                            head_slot,
                            updated_receiver.net_window_size,
                            updated_receiver.net_window_optimized,
                            support_cancel_all_unfreeze_v2,
                        );
                    // Apply window updates from increase()
                    updated_receiver.net_usage = recovered_net_usage;
                    updated_receiver.net_window_size = new_window_raw;
                    updated_receiver.net_window_optimized = new_window_optimized;

                    // Step B: Check acquired delegated balance and compute transferUsage
                    let current_acquired =
                        Self::get_acquired_delegated_frozen_v2_balance_for_bandwidth(
                            &updated_receiver,
                        );
                    if current_acquired < undelegate_info.balance {
                        // Edge case: TVM contract suicide/re-create
                        Self::set_acquired_delegated_frozen_v2_balance_for_bandwidth(
                            &mut updated_receiver,
                            0,
                        );
                        // transferUsage stays 0
                    } else {
                        // Compute transferUsage: proportional usage transfer
                        let total_net_limit = storage_adapter
                            .get_total_net_limit()
                            .map_err(|e| format!("Failed to get total net limit: {}", e))?;
                        let total_net_weight = storage_adapter
                            .get_total_net_weight()
                            .map_err(|e| format!("Failed to get total net weight: {}", e))?;

                        // Java: (long) ((double) unDelegateBalance / TRX_PRECISION
                        //              * ((double) totalNetLimit / totalNetWeight))
                        let un_delegate_max_usage = if total_net_weight == 0 {
                            0
                        } else {
                            (undelegate_info.balance as f64 / TRX_PRECISION as f64
                                * (total_net_limit as f64 / total_net_weight as f64))
                                as i64
                        };

                        // Java: (long) (receiverCapsule.getNetUsage()
                        //              * ((double) unDelegateBalance / receiverCapsule.getAllFrozenBalanceForBandwidth()))
                        let all_frozen_bw =
                            Self::get_all_frozen_balance_for_bandwidth(&updated_receiver);
                        transfer_usage = if all_frozen_bw == 0 {
                            0
                        } else {
                            (recovered_net_usage as f64
                                * (undelegate_info.balance as f64 / all_frozen_bw as f64))
                                as i64
                        };
                        transfer_usage = std::cmp::min(un_delegate_max_usage, transfer_usage);

                        Self::add_acquired_delegated_frozen_v2_balance_for_bandwidth(
                            &mut updated_receiver,
                            -undelegate_info.balance,
                        );
                    }

                    // Step C: Reduce receiver usage and set latest consume time
                    let new_net_usage = updated_receiver.net_usage - transfer_usage;
                    updated_receiver.net_usage = new_net_usage;
                    updated_receiver.latest_consume_time = head_slot;
                }
                1 => {
                    // ENERGY
                    // Ensure account_resource exists
                    if updated_receiver.account_resource.is_none() {
                        updated_receiver.account_resource = Some(
                            tron_backend_execution::protocol::account::AccountResource::default(),
                        );
                    }

                    let (
                        old_energy_usage,
                        old_energy_time,
                        old_energy_window,
                        old_energy_optimized,
                    ) = {
                        let ar = updated_receiver.account_resource.as_ref().unwrap();
                        (
                            ar.energy_usage,
                            ar.latest_consume_time_for_energy,
                            ar.energy_window_size,
                            ar.energy_window_optimized,
                        )
                    };

                    // Step A: EnergyProcessor.updateUsage(receiverCapsule)
                    // = increase(ac, ENERGY, oldEnergyUsage, 0, latestConsumeTimeForEnergy, now)
                    let (recovered_energy_usage, new_window_raw, new_window_optimized) =
                        Self::resource_increase_with_window(
                            old_energy_usage,
                            0,
                            old_energy_time,
                            head_slot,
                            old_energy_window,
                            old_energy_optimized,
                            support_cancel_all_unfreeze_v2,
                        );
                    // Apply window updates from increase()
                    if let Some(ar) = updated_receiver.account_resource.as_mut() {
                        ar.energy_usage = recovered_energy_usage;
                        ar.energy_window_size = new_window_raw;
                        ar.energy_window_optimized = new_window_optimized;
                    }

                    // Step B: Check acquired delegated balance and compute transferUsage
                    let current_acquired =
                        Self::get_acquired_delegated_frozen_v2_balance_for_energy(
                            &updated_receiver,
                        );
                    if current_acquired < undelegate_info.balance {
                        // Edge case: TVM contract suicide/re-create
                        Self::set_acquired_delegated_frozen_v2_balance_for_energy(
                            &mut updated_receiver,
                            0,
                        );
                        // transferUsage stays 0
                    } else {
                        // Compute transferUsage: proportional usage transfer
                        let total_energy_limit = storage_adapter
                            .get_total_energy_limit()
                            .map_err(|e| format!("Failed to get total energy limit: {}", e))?;
                        let total_energy_weight = storage_adapter
                            .get_total_energy_weight()
                            .map_err(|e| format!("Failed to get total energy weight: {}", e))?;

                        // Java: (long) ((double) unDelegateBalance / TRX_PRECISION
                        //              * ((double) totalEnergyCurrentLimit / totalEnergyWeight))
                        let un_delegate_max_usage = if total_energy_weight == 0 {
                            0
                        } else {
                            (undelegate_info.balance as f64 / TRX_PRECISION as f64
                                * (total_energy_limit as f64 / total_energy_weight as f64))
                                as i64
                        };

                        // Java: (long) (receiverCapsule.getEnergyUsage()
                        //              * ((double) unDelegateBalance / receiverCapsule.getAllFrozenBalanceForEnergy()))
                        let all_frozen_energy =
                            Self::get_all_frozen_balance_for_energy(&updated_receiver);
                        transfer_usage = if all_frozen_energy == 0 {
                            0
                        } else {
                            (recovered_energy_usage as f64
                                * (undelegate_info.balance as f64 / all_frozen_energy as f64))
                                as i64
                        };
                        transfer_usage = std::cmp::min(un_delegate_max_usage, transfer_usage);

                        Self::add_acquired_delegated_frozen_v2_balance_for_energy(
                            &mut updated_receiver,
                            -undelegate_info.balance,
                        );
                    }

                    // Step C: Reduce receiver usage and set latest consume time
                    if let Some(ar) = updated_receiver.account_resource.as_mut() {
                        let new_energy_usage = ar.energy_usage - transfer_usage;
                        ar.energy_usage = new_energy_usage;
                        ar.latest_consume_time_for_energy = head_slot;
                    }
                }
                _ => {}
            }
            updated_receiver_opt = Some(updated_receiver);
        }

        // 13. Persist receiver before reading it for unDelegateIncrease window data
        if let Some(updated_receiver) = updated_receiver_opt.as_ref() {
            storage_adapter
                .put_account_proto(&receiver_address, updated_receiver)
                .map_err(|e| format!("Failed to persist receiver account: {}", e))?;
        }

        // 14. Update DelegatedResourceStore (transfer lock delegate to unlock)
        storage_adapter
            .undelegate_resource(
                &owner,
                &receiver_address,
                undelegate_info.resource == 0,
                undelegate_info.balance,
                now as i64,
            )
            .map_err(|e| format!("Failed to update DelegatedResource: {}", e))?;

        // 15. If both lock/unlock records are gone, remove DelegatedResourceAccountIndex entries.
        let unlock_after = storage_adapter
            .get_delegated_resource(&owner, &receiver_address, false)
            .map_err(|e| format!("Failed to load DelegatedResource: {}", e))?;
        let lock_after = storage_adapter
            .get_delegated_resource(&owner, &receiver_address, true)
            .map_err(|e| format!("Failed to load DelegatedResource: {}", e))?;
        if unlock_after.is_none() && lock_after.is_none() {
            storage_adapter
                .undelegate_resource_account_index(&owner, &receiver_address)
                .map_err(|e| format!("Failed to update DelegatedResourceAccountIndex: {}", e))?;
        }

        // 16. Update owner account: frozen balance + unDelegateIncrease (usage/window transfer)
        let mut updated_owner = owner_account.clone();
        match undelegate_info.resource {
            0 => {
                // BANDWIDTH
                Self::add_delegated_frozen_v2_balance_for_bandwidth(
                    &mut updated_owner,
                    -undelegate_info.balance,
                );
                Self::add_frozen_v2_bandwidth(&mut updated_owner, undelegate_info.balance);

                // Java: if (Objects.nonNull(receiverCapsule) && transferUsage > 0)
                //         processor.unDelegateIncrease(ownerCapsule, receiverCapsule, transferUsage, BANDWIDTH, now)
                if updated_receiver_opt.is_some() && transfer_usage > 0 {
                    let receiver_ref = updated_receiver_opt.as_ref().unwrap();
                    let (new_usage, new_window, new_optimized, new_time) =
                        Self::un_delegate_increase(
                            updated_owner.net_usage,
                            updated_owner.latest_consume_time,
                            updated_owner.net_window_size,
                            updated_owner.net_window_optimized,
                            receiver_ref.net_window_size,
                            receiver_ref.net_window_optimized,
                            transfer_usage,
                            head_slot,
                            support_cancel_all_unfreeze_v2,
                        );
                    updated_owner.net_usage = new_usage;
                    updated_owner.net_window_size = new_window;
                    updated_owner.net_window_optimized = new_optimized;
                    updated_owner.latest_consume_time = new_time;
                }
            }
            1 => {
                // ENERGY
                Self::add_delegated_frozen_v2_balance_for_energy(
                    &mut updated_owner,
                    -undelegate_info.balance,
                );
                Self::add_frozen_v2_energy(&mut updated_owner, undelegate_info.balance);

                // Java: if (Objects.nonNull(receiverCapsule) && transferUsage > 0)
                //         processor.unDelegateIncrease(ownerCapsule, receiverCapsule, transferUsage, ENERGY, now)
                if updated_receiver_opt.is_some() && transfer_usage > 0 {
                    let receiver_ref = updated_receiver_opt.as_ref().unwrap();
                    let (recv_window, recv_optimized) = receiver_ref
                        .account_resource
                        .as_ref()
                        .map(|ar| (ar.energy_window_size, ar.energy_window_optimized))
                        .unwrap_or((0, false));
                    let (
                        owner_energy_usage,
                        owner_energy_time,
                        owner_energy_window,
                        owner_energy_optimized,
                    ) = updated_owner
                        .account_resource
                        .as_ref()
                        .map(|ar| {
                            (
                                ar.energy_usage,
                                ar.latest_consume_time_for_energy,
                                ar.energy_window_size,
                                ar.energy_window_optimized,
                            )
                        })
                        .unwrap_or((0, 0, 0, false));

                    let (new_usage, new_window, new_optimized, new_time) =
                        Self::un_delegate_increase(
                            owner_energy_usage,
                            owner_energy_time,
                            owner_energy_window,
                            owner_energy_optimized,
                            recv_window,
                            recv_optimized,
                            transfer_usage,
                            head_slot,
                            support_cancel_all_unfreeze_v2,
                        );

                    if updated_owner.account_resource.is_none() {
                        updated_owner.account_resource = Some(
                            tron_backend_execution::protocol::account::AccountResource::default(),
                        );
                    }
                    if let Some(ar) = updated_owner.account_resource.as_mut() {
                        ar.energy_usage = new_usage;
                        ar.energy_window_size = new_window;
                        ar.energy_window_optimized = new_optimized;
                        ar.latest_consume_time_for_energy = new_time;
                    }
                }
            }
            _ => {}
        }

        // 17. Persist owner account
        storage_adapter
            .put_account_proto(&owner, &updated_owner)
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;

        // 18. Build state changes
        let mut state_changes = vec![TronStateChange::AccountChange {
            address: owner,
            old_account: Some(revm_primitives::AccountInfo {
                balance: revm_primitives::U256::from(owner_account.balance as u64),
                nonce: 0,
                code_hash: revm_primitives::B256::ZERO,
                code: None,
            }),
            new_account: Some(revm_primitives::AccountInfo {
                balance: revm_primitives::U256::from(updated_owner.balance as u64),
                nonce: 0,
                code_hash: revm_primitives::B256::ZERO,
                code: None,
            }),
        }];

        if let (Some(receiver_account), Some(updated_receiver)) =
            (receiver_account_opt.as_ref(), updated_receiver_opt.as_ref())
        {
            state_changes.push(TronStateChange::AccountChange {
                address: receiver_address,
                old_account: Some(revm_primitives::AccountInfo {
                    balance: revm_primitives::U256::from(receiver_account.balance as u64),
                    nonce: 0,
                    code_hash: revm_primitives::B256::ZERO,
                    code: None,
                }),
                new_account: Some(revm_primitives::AccountInfo {
                    balance: revm_primitives::U256::from(updated_receiver.balance as u64),
                    nonce: 0,
                    code_hash: revm_primitives::B256::ZERO,
                    code: None,
                }),
            });
        }

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!("UnDelegateResource: undelegated {} SUN of resource {} from {} back to {}, transferUsage={}",
               undelegate_info.balance, undelegate_info.resource,
               readable_receiver_address, readable_owner_address, transfer_usage);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
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

    /// Parse DelegateResourceContract from protobuf bytes
    /// DelegateResourceContract:
    ///   bytes owner_address = 1;
    ///   ResourceCode resource = 2;
    ///   int64 balance = 3;
    ///   bytes receiver_address = 4;
    ///   bool lock = 5;
    ///   int64 lock_period = 6;
    fn parse_delegate_resource_contract(
        &self,
        data: &[u8],
    ) -> Result<DelegateResourceInfo, String> {
        let mut owner_address: Vec<u8> = vec![];
        let mut receiver_address: Vec<u8> = vec![];
        let mut balance: i64 = 0;
        let mut resource: i32 = 0;
        let mut lock: bool = false;
        let mut lock_period: i64 = 0;
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address = payload.to_vec();
                    pos += total_len;
                }
                (2, 0) => {
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    resource = value as i32;
                }
                (3, 0) => {
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    balance = value as i64;
                }
                (4, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    receiver_address = payload.to_vec();
                    pos += total_len;
                }
                (5, 0) => {
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    lock = value != 0;
                }
                (6, 0) => {
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    lock_period = value as i64;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        // Java parity: invalid/missing owner address maps to "Invalid address" (DelegateResourceActuator.validate()).
        if owner_address.is_empty() {
            return Err("Invalid address".to_string());
        }

        if receiver_address.is_empty() {
            return Err("Invalid receiverAddress".to_string());
        }

        Ok(DelegateResourceInfo {
            owner_address,
            receiver_address,
            balance,
            resource,
            lock,
            lock_period,
        })
    }

    /// Parse UnDelegateResourceContract from protobuf bytes
    /// UnDelegateResourceContract:
    ///   bytes owner_address = 1;
    ///   ResourceCode resource = 2;
    ///   int64 balance = 3;
    ///   bytes receiver_address = 4;
    fn parse_undelegate_resource_contract(
        &self,
        data: &[u8],
    ) -> Result<UnDelegateResourceInfo, String> {
        let mut receiver_address: Vec<u8> = vec![];
        let mut balance: i64 = 0;
        let mut resource: i32 = 0;
        let mut pos = 0;

        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    let (_payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += total_len;
                }
                (2, 0) => {
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    resource = value as i32;
                }
                (3, 0) => {
                    let (value, bytes_read) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += bytes_read;
                    balance = value as i64;
                }
                (4, 2) => {
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    receiver_address = payload.to_vec();
                    pos += total_len;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        if receiver_address.is_empty() {
            return Err("Invalid receiverAddress".to_string());
        }

        Ok(UnDelegateResourceInfo {
            receiver_address,
            balance,
            resource,
        })
    }

    // ========================================================================
    // Helper methods for Account frozen/delegated balance manipulation
    // ========================================================================

    /// Get frozenV2 balance for bandwidth
    fn get_frozen_v2_balance_for_bandwidth(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        account
            .frozen_v2
            .iter()
            .filter(|f| f.r#type == 0) // BANDWIDTH
            .map(|f| f.amount)
            .sum()
    }

    /// Get frozenV2 balance for energy
    fn get_frozen_v2_balance_for_energy(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        account
            .frozen_v2
            .iter()
            .filter(|f| f.r#type == 1) // ENERGY
            .map(|f| f.amount)
            .sum()
    }

    /// Get frozenV2 balance with delegated for bandwidth
    fn get_frozen_v2_balance_with_delegated_bandwidth(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        Self::get_frozen_v2_balance_for_bandwidth(account)
            + account.delegated_frozen_v2_balance_for_bandwidth
    }

    /// Get frozenV2 balance with delegated for energy
    fn get_frozen_v2_balance_with_delegated_energy(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        Self::get_frozen_v2_balance_for_energy(account)
            + account
                .account_resource
                .as_ref()
                .map(|r| r.delegated_frozen_v2_balance_for_energy)
                .unwrap_or(0)
    }

    /// Get tron power frozenV2 balance
    fn get_tron_power_frozen_v2_balance(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        account
            .frozen_v2
            .iter()
            .filter(|f| f.r#type == 2) // TRON_POWER
            .map(|f| f.amount)
            .sum()
    }

    /// Get acquired delegated frozenV2 balance for bandwidth
    fn get_acquired_delegated_frozen_v2_balance_for_bandwidth(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        account.acquired_delegated_frozen_v2_balance_for_bandwidth
    }

    /// Get acquired delegated frozenV2 balance for energy
    fn get_acquired_delegated_frozen_v2_balance_for_energy(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        account
            .account_resource
            .as_ref()
            .map(|r| r.acquired_delegated_frozen_v2_balance_for_energy)
            .unwrap_or(0)
    }

    /// Add to frozenV2 bandwidth amount
    fn add_frozen_v2_bandwidth(
        account: &mut tron_backend_execution::protocol::Account,
        amount: i64,
    ) {
        let mut found = false;
        for f in account.frozen_v2.iter_mut() {
            if f.r#type == 0 {
                // BANDWIDTH
                f.amount += amount;
                found = true;
                break;
            }
        }
        if !found && amount > 0 {
            account
                .frozen_v2
                .push(tron_backend_execution::protocol::account::FreezeV2 { r#type: 0, amount });
        }
    }

    /// Add to frozenV2 energy amount
    fn add_frozen_v2_energy(account: &mut tron_backend_execution::protocol::Account, amount: i64) {
        let mut found = false;
        for f in account.frozen_v2.iter_mut() {
            if f.r#type == 1 {
                // ENERGY
                f.amount += amount;
                found = true;
                break;
            }
        }
        if !found && amount > 0 {
            account
                .frozen_v2
                .push(tron_backend_execution::protocol::account::FreezeV2 { r#type: 1, amount });
        }
    }

    /// Add to frozenV2 tron power amount
    fn add_frozen_v2_tron_power(
        account: &mut tron_backend_execution::protocol::Account,
        amount: i64,
    ) {
        let mut found = false;
        for f in account.frozen_v2.iter_mut() {
            if f.r#type == 2 {
                // TRON_POWER
                f.amount += amount;
                found = true;
                break;
            }
        }
        if !found && amount > 0 {
            account
                .frozen_v2
                .push(tron_backend_execution::protocol::account::FreezeV2 { r#type: 2, amount });
        }
    }

    /// Add to delegated frozenV2 balance for bandwidth
    fn add_delegated_frozen_v2_balance_for_bandwidth(
        account: &mut tron_backend_execution::protocol::Account,
        amount: i64,
    ) {
        account.delegated_frozen_v2_balance_for_bandwidth += amount;
    }

    /// Add to delegated frozenV2 balance for energy
    fn add_delegated_frozen_v2_balance_for_energy(
        account: &mut tron_backend_execution::protocol::Account,
        amount: i64,
    ) {
        if account.account_resource.is_none() {
            account.account_resource =
                Some(tron_backend_execution::protocol::account::AccountResource::default());
        }
        if let Some(ref mut res) = account.account_resource {
            res.delegated_frozen_v2_balance_for_energy += amount;
        }
    }

    /// Add to acquired delegated frozenV2 balance for bandwidth
    fn add_acquired_delegated_frozen_v2_balance_for_bandwidth(
        account: &mut tron_backend_execution::protocol::Account,
        amount: i64,
    ) {
        account.acquired_delegated_frozen_v2_balance_for_bandwidth += amount;
    }

    /// Add to acquired delegated frozenV2 balance for energy
    fn add_acquired_delegated_frozen_v2_balance_for_energy(
        account: &mut tron_backend_execution::protocol::Account,
        amount: i64,
    ) {
        if account.account_resource.is_none() {
            account.account_resource =
                Some(tron_backend_execution::protocol::account::AccountResource::default());
        }
        if let Some(ref mut res) = account.account_resource {
            res.acquired_delegated_frozen_v2_balance_for_energy += amount;
        }
    }

    /// Set acquired delegated frozenV2 balance for bandwidth
    fn set_acquired_delegated_frozen_v2_balance_for_bandwidth(
        account: &mut tron_backend_execution::protocol::Account,
        amount: i64,
    ) {
        account.acquired_delegated_frozen_v2_balance_for_bandwidth = amount;
    }

    /// Set acquired delegated frozenV2 balance for energy
    fn set_acquired_delegated_frozen_v2_balance_for_energy(
        account: &mut tron_backend_execution::protocol::Account,
        amount: i64,
    ) {
        if account.account_resource.is_none() {
            account.account_resource =
                Some(tron_backend_execution::protocol::account::AccountResource::default());
        }
        if let Some(ref mut res) = account.account_resource {
            res.acquired_delegated_frozen_v2_balance_for_energy = amount;
        }
    }

    // ========================================================================
    // Helper methods for "available FreezeV2" calculation for delegation
    // Java parity: DelegateResourceActuator.validate()
    // ========================================================================

    /// Constants for resource calculation
    const PRECISION: i64 = 1_000_000;
    const WINDOW_SIZE_MS: i64 = 24 * 3600 * 1000; // 24 hours in milliseconds
    const BLOCK_PRODUCED_INTERVAL: i64 = 3000; // 3 seconds in milliseconds
    const WINDOW_SIZE_SLOTS: i64 = Self::WINDOW_SIZE_MS / Self::BLOCK_PRODUCED_INTERVAL; // 28800 slots
    const WINDOW_SIZE_PRECISION: i64 = 1000; // Java: Parameter.ChainConstant.WINDOW_SIZE_PRECISION

    /// Normalize Account window size fields to logical "slots" (Java parity).
    ///
    /// Java reference: `AccountCapsule.getWindowSize(ResourceCode)`.
    /// - raw == 0 => default 28800 slots
    /// - optimized:
    ///   - raw < 1000 => default 28800 slots
    ///   - else raw / 1000
    /// - not optimized => raw
    fn normalize_window_size_slots(raw: i64, optimized: bool) -> i64 {
        let default_slots = Self::WINDOW_SIZE_SLOTS;
        if raw == 0 {
            return default_slots;
        }

        let normalized = if optimized {
            if raw < Self::WINDOW_SIZE_PRECISION {
                default_slots
            } else {
                raw / Self::WINDOW_SIZE_PRECISION
            }
        } else {
            raw
        };

        if normalized <= 0 {
            default_slots
        } else {
            normalized
        }
    }

    /// Get V1 frozen balance for bandwidth (sum of account.frozen[].frozen_balance)
    /// Java: AccountCapsule.getFrozenBalance()
    fn get_frozen_v1_balance_for_bandwidth(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        account.frozen.iter().map(|f| f.frozen_balance).sum()
    }

    /// Get V1 frozen balance for energy (account_resource.frozen_balance_for_energy.frozen_balance)
    /// Java: AccountCapsule.getEnergyFrozenBalance()
    fn get_frozen_v1_balance_for_energy(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        account
            .account_resource
            .as_ref()
            .and_then(|r| r.frozen_balance_for_energy.as_ref())
            .map(|f| f.frozen_balance)
            .unwrap_or(0)
    }

    /// Get acquired delegated V1 balance for bandwidth
    /// Java: AccountCapsule.getAcquiredDelegatedFrozenBalanceForBandwidth()
    fn get_acquired_delegated_frozen_v1_balance_for_bandwidth(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        account.acquired_delegated_frozen_balance_for_bandwidth
    }

    /// Get acquired delegated V1 balance for energy
    /// Java: AccountCapsule.getAcquiredDelegatedFrozenBalanceForEnergy()
    fn get_acquired_delegated_frozen_v1_balance_for_energy(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        account
            .account_resource
            .as_ref()
            .map(|r| r.acquired_delegated_frozen_balance_for_energy)
            .unwrap_or(0)
    }

    /// Calculate decayed usage using the Java resource decay algorithm.
    /// Java: ResourceProcessor.increase(lastUsage, usage=0, lastTime, now, windowSize)
    ///
    /// This implements the decay portion (usage=0 case) which is what updateUsageForDelegated uses.
    /// The formula:
    ///   averageLastUsage = divideCeil(lastUsage * precision, windowSize)
    ///   if lastTime != now:
    ///       if lastTime + windowSize > now:
    ///           decay = (windowSize - (now - lastTime)) / windowSize
    ///           averageLastUsage = round(averageLastUsage * decay)
    ///       else:
    ///           averageLastUsage = 0
    ///   return averageLastUsage * windowSize / precision
    fn calculate_decayed_usage(last_usage: i64, last_time: i64, now: i64, window_size: i64) -> i64 {
        if last_usage <= 0 {
            return 0;
        }

        // divideCeil(lastUsage * precision, windowSize)
        let precision = Self::PRECISION;
        let numerator = last_usage.saturating_mul(precision);
        let mut average_last_usage =
            numerator / window_size + if numerator % window_size > 0 { 1 } else { 0 };

        if last_time != now {
            if last_time + window_size > now {
                let delta = now - last_time;
                let decay = (window_size - delta) as f64 / window_size as f64;
                // Java: round(averageLastUsage * decay)
                // Java's Math.round rounds to nearest integer (half-up)
                average_last_usage = (average_last_usage as f64 * decay).round() as i64;
            } else {
                average_last_usage = 0;
            }
        }

        // getUsage: averageLastUsage * windowSize / precision
        average_last_usage * window_size / precision
    }

    /// Calculate V2 net usage for bandwidth delegation validation.
    /// Java: FreezeV2Util.getV2NetUsage(ownerCapsule, netUsage, disableJavaLangMath)
    ///
    /// v2NetUsage = max(0, netUsage - frozenBalanceV1 - acquiredDelegatedV1 - acquiredDelegatedV2)
    fn get_v2_net_usage(
        account: &tron_backend_execution::protocol::Account,
        net_usage: i64,
    ) -> i64 {
        let frozen_v1 = Self::get_frozen_v1_balance_for_bandwidth(account);
        let acquired_delegated_v1 =
            Self::get_acquired_delegated_frozen_v1_balance_for_bandwidth(account);
        let acquired_delegated_v2 =
            Self::get_acquired_delegated_frozen_v2_balance_for_bandwidth(account);

        let v2_net_usage = net_usage
            .saturating_sub(frozen_v1)
            .saturating_sub(acquired_delegated_v1)
            .saturating_sub(acquired_delegated_v2);

        std::cmp::max(0, v2_net_usage)
    }

    /// Calculate V2 energy usage for energy delegation validation.
    /// Java: FreezeV2Util.getV2EnergyUsage(ownerCapsule, energyUsage, disableJavaLangMath)
    ///
    /// v2EnergyUsage = max(0, energyUsage - energyFrozenBalanceV1 - acquiredDelegatedV1 - acquiredDelegatedV2)
    fn get_v2_energy_usage(
        account: &tron_backend_execution::protocol::Account,
        energy_usage: i64,
    ) -> i64 {
        let frozen_v1 = Self::get_frozen_v1_balance_for_energy(account);
        let acquired_delegated_v1 =
            Self::get_acquired_delegated_frozen_v1_balance_for_energy(account);
        let acquired_delegated_v2 =
            Self::get_acquired_delegated_frozen_v2_balance_for_energy(account);

        let v2_energy_usage = energy_usage
            .saturating_sub(frozen_v1)
            .saturating_sub(acquired_delegated_v1)
            .saturating_sub(acquired_delegated_v2);

        std::cmp::max(0, v2_energy_usage)
    }

    // ========================================================================
    // Helper methods for UNDELEGATE_RESOURCE_CONTRACT usage transfer parity
    // Java oracle: UnDelegateResourceActuator.java, ResourceProcessor.java
    // ========================================================================

    /// Java parity: AccountCapsule.getAllFrozenBalanceForBandwidth()
    /// = getFrozenBalance() + getAcquiredDelegatedFrozenBalanceForBandwidth()
    ///   + getFrozenV2BalanceForBandwidth() + getAcquiredDelegatedFrozenV2BalanceForBandwidth()
    fn get_all_frozen_balance_for_bandwidth(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        let frozen_v1 = Self::get_frozen_v1_balance_for_bandwidth(account);
        let acquired_v1 = account.acquired_delegated_frozen_balance_for_bandwidth;
        let frozen_v2 = Self::get_frozen_v2_balance_for_bandwidth(account);
        let acquired_v2 = Self::get_acquired_delegated_frozen_v2_balance_for_bandwidth(account);
        frozen_v1 + acquired_v1 + frozen_v2 + acquired_v2
    }

    /// Java parity: AccountCapsule.getAllFrozenBalanceForEnergy()
    /// = getEnergyFrozenBalance() + getAcquiredDelegatedFrozenBalanceForEnergy()
    ///   + getFrozenV2BalanceForEnergy() + getAcquiredDelegatedFrozenV2BalanceForEnergy()
    fn get_all_frozen_balance_for_energy(
        account: &tron_backend_execution::protocol::Account,
    ) -> i64 {
        let frozen_v1 = Self::get_frozen_v1_balance_for_energy(account);
        let acquired_v1 = Self::get_acquired_delegated_frozen_v1_balance_for_energy(account);
        let frozen_v2 = Self::get_frozen_v2_balance_for_energy(account);
        let acquired_v2 = Self::get_acquired_delegated_frozen_v2_balance_for_energy(account);
        frozen_v1 + acquired_v1 + frozen_v2 + acquired_v2
    }

    /// Java parity: AccountCapsule.getWindowSizeV2(resourceCode)
    /// Returns raw precision-scaled window size (slots * WINDOW_SIZE_PRECISION).
    /// - raw == 0 => 28800 * 1000 = 28_800_000
    /// - optimized => raw (already in precision units)
    /// - not optimized => raw * 1000
    fn normalize_window_size_v2_raw(raw: i64, optimized: bool) -> i64 {
        if raw == 0 {
            return Self::WINDOW_SIZE_SLOTS * Self::WINDOW_SIZE_PRECISION;
        }
        if optimized {
            raw
        } else {
            raw * Self::WINDOW_SIZE_PRECISION
        }
    }

    /// Java parity: ResourceProcessor.divideCeil(numerator, denominator)
    fn divide_ceil_resource(numerator: i64, denominator: i64) -> i64 {
        if denominator == 0 {
            return 0;
        }
        numerator / denominator + if numerator % denominator > 0 { 1 } else { 0 }
    }

    /// Java parity: ResourceProcessor.increase(AccountCapsule, ResourceCode, lastUsage, usage, lastTime, now)
    /// V1 path (supportAllowCancelAllUnfreezeV2 == false, supportUnfreezeDelay == true).
    /// Returns (new_usage, new_window_raw, new_window_optimized).
    fn resource_increase_v1(
        last_usage: i64,
        usage: i64,
        last_time: i64,
        now: i64,
        current_window_raw: i64,
        current_window_optimized: bool,
    ) -> (i64, i64, bool) {
        let old_window_size =
            Self::normalize_window_size_slots(current_window_raw, current_window_optimized);
        let default_window = Self::WINDOW_SIZE_SLOTS; // 28800
        let precision = Self::PRECISION;

        let mut avg_last =
            Self::divide_ceil_resource(last_usage.saturating_mul(precision), old_window_size);
        let avg_new = Self::divide_ceil_resource(usage.saturating_mul(precision), default_window);

        if last_time != now {
            if last_time + old_window_size > now {
                let delta = now - last_time;
                let decay = (old_window_size - delta) as f64 / old_window_size as f64;
                avg_last = (avg_last as f64 * decay).round() as i64;
            } else {
                avg_last = 0;
            }
        }

        // getUsage(averageLastUsage, oldWindowSize, averageUsage, this.windowSize)
        let new_usage = (avg_last * old_window_size + avg_new * default_window) / precision;

        // supportUnfreezeDelay is always true for UNDELEGATE context
        let remain_usage = avg_last * old_window_size / precision;
        if remain_usage == 0 {
            return (new_usage, default_window, current_window_optimized);
        }
        let remain_window_size = old_window_size - (now - last_time);
        // getNewWindowSize(remainUsage, remainWindowSize, usage, windowSize, newUsage)
        let new_window_size =
            (remain_usage * remain_window_size + usage * default_window) / new_usage;

        (new_usage, new_window_size, current_window_optimized)
    }

    /// Java parity: ResourceProcessor.increaseV2(AccountCapsule, ResourceCode, lastUsage, usage, lastTime, now)
    /// Returns (new_usage, new_window_raw, true).
    fn resource_increase_v2_fn(
        last_usage: i64,
        usage: i64,
        last_time: i64,
        now: i64,
        current_window_raw: i64,
        current_window_optimized: bool,
    ) -> (i64, i64, bool) {
        let old_window_size_v2 =
            Self::normalize_window_size_v2_raw(current_window_raw, current_window_optimized);
        let old_window_size =
            Self::normalize_window_size_slots(current_window_raw, current_window_optimized);
        let default_window = Self::WINDOW_SIZE_SLOTS; // 28800
        let precision = Self::PRECISION;
        let wsp = Self::WINDOW_SIZE_PRECISION; // 1000

        let mut avg_last =
            Self::divide_ceil_resource(last_usage.saturating_mul(precision), old_window_size);
        let avg_new = Self::divide_ceil_resource(usage.saturating_mul(precision), default_window);

        if last_time != now {
            if last_time + old_window_size > now {
                let delta = now - last_time;
                let decay = (old_window_size - delta) as f64 / old_window_size as f64;
                avg_last = (avg_last as f64 * decay).round() as i64;
            } else {
                avg_last = 0;
            }
        }

        let new_usage = (avg_last * old_window_size + avg_new * default_window) / precision;
        let remain_usage = avg_last * old_window_size / precision;

        if remain_usage == 0 {
            return (new_usage, default_window * wsp, true);
        }

        let remain_window_size = old_window_size_v2 - (now - last_time) * wsp;
        let mut new_window_size = Self::divide_ceil_resource(
            remain_usage * remain_window_size + usage * default_window * wsp,
            new_usage,
        );
        new_window_size = std::cmp::min(new_window_size, default_window * wsp);

        (new_usage, new_window_size, true)
    }

    /// Dispatch to increase v1 or v2 based on supportAllowCancelAllUnfreezeV2 flag.
    /// Returns (new_usage, new_window_raw, new_window_optimized).
    fn resource_increase_with_window(
        last_usage: i64,
        usage: i64,
        last_time: i64,
        now: i64,
        current_window_raw: i64,
        current_window_optimized: bool,
        support_cancel_all_unfreeze_v2: bool,
    ) -> (i64, i64, bool) {
        if support_cancel_all_unfreeze_v2 {
            Self::resource_increase_v2_fn(
                last_usage,
                usage,
                last_time,
                now,
                current_window_raw,
                current_window_optimized,
            )
        } else {
            Self::resource_increase_v1(
                last_usage,
                usage,
                last_time,
                now,
                current_window_raw,
                current_window_optimized,
            )
        }
    }

    /// Java parity: ResourceProcessor.unDelegateIncrease (v1 path, supportAllowCancelAllUnfreezeV2 == false).
    /// Returns (new_owner_usage, new_owner_window_raw, new_owner_window_optimized, new_owner_latest_time).
    fn un_delegate_increase_v1(
        owner_usage: i64,
        owner_latest_time: i64,
        owner_window_raw: i64,
        owner_window_optimized: bool,
        receiver_window_raw: i64,
        receiver_window_optimized: bool,
        transfer_usage: i64,
        now: i64,
    ) -> (i64, i64, bool, i64) {
        // Step 1: Update owner usage to "now" (recovery via increase v1)
        let (decayed_owner_usage, new_owner_window_raw, new_owner_optimized) =
            Self::resource_increase_v1(
                owner_usage,
                0,
                owner_latest_time,
                now,
                owner_window_raw,
                owner_window_optimized,
            );

        // Step 2: Read updated window sizes (after increase modified them)
        let remain_owner_window = std::cmp::max(
            0,
            Self::normalize_window_size_slots(new_owner_window_raw, new_owner_optimized),
        );
        let remain_receiver_window = std::cmp::max(
            0,
            Self::normalize_window_size_slots(receiver_window_raw, receiver_window_optimized),
        );

        let new_owner_usage = decayed_owner_usage + transfer_usage;

        if new_owner_usage == 0 {
            return (0, Self::WINDOW_SIZE_SLOTS, new_owner_optimized, now);
        }

        // getNewWindowSize(ownerUsage, remainOwnerWindowSize, transferUsage, remainReceiverWindowSize, newOwnerUsage)
        let new_window = (decayed_owner_usage * remain_owner_window
            + transfer_usage * remain_receiver_window)
            / new_owner_usage;

        (new_owner_usage, new_window, new_owner_optimized, now)
    }

    /// Java parity: ResourceProcessor.unDelegateIncreaseV2.
    /// Returns (new_owner_usage, new_owner_window_raw, true, new_owner_latest_time).
    fn un_delegate_increase_v2_fn(
        owner_usage: i64,
        owner_latest_time: i64,
        owner_window_raw: i64,
        owner_window_optimized: bool,
        receiver_window_raw: i64,
        receiver_window_optimized: bool,
        transfer_usage: i64,
        now: i64,
    ) -> (i64, i64, bool, i64) {
        let wsp = Self::WINDOW_SIZE_PRECISION;
        let default_window = Self::WINDOW_SIZE_SLOTS;

        // Step 1: Update owner usage to "now" (recovery via increaseV2)
        let (decayed_owner_usage, new_owner_window_raw, _) = Self::resource_increase_v2_fn(
            owner_usage,
            0,
            owner_latest_time,
            now,
            owner_window_raw,
            owner_window_optimized,
        );

        let new_owner_usage = decayed_owner_usage + transfer_usage;

        if new_owner_usage == 0 {
            return (0, default_window * wsp, true, now);
        }

        // Read updated V2 window sizes (increaseV2 always sets optimized=true)
        let remain_owner_window_v2 = std::cmp::max(
            0,
            Self::normalize_window_size_v2_raw(new_owner_window_raw, true),
        );
        let remain_receiver_window_v2 = std::cmp::max(
            0,
            Self::normalize_window_size_v2_raw(receiver_window_raw, receiver_window_optimized),
        );

        // divideCeil(ownerUsage * remainOwnerWindowSizeV2 + transferUsage * remainReceiverWindowSizeV2, newOwnerUsage)
        let mut new_window = Self::divide_ceil_resource(
            decayed_owner_usage * remain_owner_window_v2
                + transfer_usage * remain_receiver_window_v2,
            new_owner_usage,
        );
        new_window = std::cmp::min(new_window, default_window * wsp);

        (new_owner_usage, new_window, true, now)
    }

    /// Dispatch unDelegateIncrease to v1 or v2 based on supportAllowCancelAllUnfreezeV2 flag.
    /// Returns (new_owner_usage, new_owner_window_raw, new_owner_window_optimized, new_owner_latest_time).
    fn un_delegate_increase(
        owner_usage: i64,
        owner_latest_time: i64,
        owner_window_raw: i64,
        owner_window_optimized: bool,
        receiver_window_raw: i64,
        receiver_window_optimized: bool,
        transfer_usage: i64,
        now: i64,
        support_cancel_all_unfreeze_v2: bool,
    ) -> (i64, i64, bool, i64) {
        if support_cancel_all_unfreeze_v2 {
            Self::un_delegate_increase_v2_fn(
                owner_usage,
                owner_latest_time,
                owner_window_raw,
                owner_window_optimized,
                receiver_window_raw,
                receiver_window_optimized,
                transfer_usage,
                now,
            )
        } else {
            Self::un_delegate_increase_v1(
                owner_usage,
                owner_latest_time,
                owner_window_raw,
                owner_window_optimized,
                receiver_window_raw,
                receiver_window_optimized,
                transfer_usage,
                now,
            )
        }
    }

    /// Compute available FreezeV2 balance for bandwidth delegation.
    /// Java: DelegateResourceActuator.validate() BANDWIDTH case
    ///
    /// Steps:
    /// 1. Update net_usage by decaying old usage to current time (like BandwidthProcessor.updateUsageForDelegated)
    /// 2. Calculate weighted net_usage in SUN: netUsage = accountNetUsage * TRX_PRECISION * (totalNetWeight / totalNetLimit)
    /// 3. Calculate V2-only usage: v2NetUsage = max(0, netUsage - frozenV1 - acquiredDelegatedV1 - acquiredDelegatedV2)
    /// 4. Return: frozenV2Balance - v2NetUsage
    fn compute_available_freeze_v2_bandwidth(
        account: &tron_backend_execution::protocol::Account,
        total_net_weight: i64,
        total_net_limit: i64,
        head_slot: i64,
    ) -> i64 {
        // Get frozen V2 balance
        let frozen_v2_bandwidth = Self::get_frozen_v2_balance_for_bandwidth(account);

        // 1. Get current net_usage from account and decay it to current time
        let old_net_usage = account.net_usage;
        let latest_consume_time = account.latest_consume_time;

        // Java parity: AccountCapsule.getWindowSize(BANDWIDTH)
        let window_size = Self::normalize_window_size_slots(
            account.net_window_size,
            account.net_window_optimized,
        );

        // Decay the usage to current time (parity with BandwidthProcessor.updateUsageForDelegated)
        let decayed_net_usage = Self::calculate_decayed_usage(
            old_net_usage,
            latest_consume_time,
            head_slot,
            window_size,
        );

        // 2. Calculate weighted net usage in SUN units
        // Java: (long) (accountNetUsage * TRX_PRECISION * ((double) totalNetWeight / totalNetLimit))
        if total_net_limit == 0 {
            // Avoid division by zero - if no limit, all frozen balance is available
            return frozen_v2_bandwidth;
        }

        let net_usage_scaled = (decayed_net_usage as f64
            * TRX_PRECISION as f64
            * (total_net_weight as f64 / total_net_limit as f64))
            as i64;

        // 3. Calculate V2 usage (subtract V1 and acquired delegated amounts)
        let v2_net_usage = Self::get_v2_net_usage(account, net_usage_scaled);

        // 4. Return available balance
        frozen_v2_bandwidth.saturating_sub(v2_net_usage)
    }

    /// Compute available FreezeV2 balance for energy delegation.
    /// Java: DelegateResourceActuator.validate() ENERGY case
    ///
    /// Steps:
    /// 1. Update energy_usage by decaying old usage to current time (like EnergyProcessor.updateUsage)
    /// 2. Calculate weighted energy_usage in SUN: energyUsage = accountEnergyUsage * TRX_PRECISION * (totalEnergyWeight / totalEnergyCurrentLimit)
    /// 3. Calculate V2-only usage: v2EnergyUsage = max(0, energyUsage - frozenV1 - acquiredDelegatedV1 - acquiredDelegatedV2)
    /// 4. Return: frozenV2Balance - v2EnergyUsage
    fn compute_available_freeze_v2_energy(
        account: &tron_backend_execution::protocol::Account,
        total_energy_weight: i64,
        total_energy_current_limit: i64,
        head_slot: i64,
    ) -> i64 {
        // Get frozen V2 balance
        let frozen_v2_energy = Self::get_frozen_v2_balance_for_energy(account);

        // 1. Get current energy_usage from account and decay it to current time
        let (old_energy_usage, latest_consume_time, window_size) = match &account.account_resource {
            Some(res) => (
                res.energy_usage,
                res.latest_consume_time_for_energy,
                Self::normalize_window_size_slots(
                    res.energy_window_size,
                    res.energy_window_optimized,
                ),
            ),
            None => (0, 0, Self::WINDOW_SIZE_SLOTS),
        };

        // Decay the usage to current time (parity with EnergyProcessor.updateUsage)
        let decayed_energy_usage = Self::calculate_decayed_usage(
            old_energy_usage,
            latest_consume_time,
            head_slot,
            window_size,
        );

        // 2. Calculate weighted energy usage in SUN units
        // Java: (long) (ownerCapsule.getEnergyUsage() * TRX_PRECISION * ((double) totalEnergyWeight / totalEnergyCurrentLimit))
        if total_energy_current_limit == 0 {
            // Avoid division by zero - if no limit, all frozen balance is available
            return frozen_v2_energy;
        }

        let energy_usage_scaled = (decayed_energy_usage as f64
            * TRX_PRECISION as f64
            * (total_energy_weight as f64 / total_energy_current_limit as f64))
            as i64;

        // 3. Calculate V2 usage (subtract V1 and acquired delegated amounts)
        let v2_energy_usage = Self::get_v2_energy_usage(account, energy_usage_scaled);

        // 4. Return available balance
        frozen_v2_energy.saturating_sub(v2_energy_usage)
    }

    // ==========================================================================
    // Phase 2.E: TRC-10 Extension Contracts (9/14/15)
    // ==========================================================================

    /// Execute PARTICIPATE_ASSET_ISSUE_CONTRACT (type 9)
    /// Allows users to participate in a TRC-10 token sale by exchanging TRX for tokens
    ///
    /// Java oracle: ParticipateAssetIssueActuator.java
    fn execute_participate_asset_issue_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.ParticipateAssetIssueContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [ParticipateAssetIssueContract],real type[class com.google.protobuf.Any]",
        )?;

        // Parse contract data first to get all fields
        let participate_info = self.parse_participate_asset_issue_contract(contract_bytes)?;

        // Get expected address prefix for validation (Java: DecodeUtil.addressPreFixByte)
        let expected_prefix = storage_adapter.address_prefix();

        // 1. Validate owner_address (Java parity: DecodeUtil.addressValid(ownerAddress))
        // Java requires: length == 21 and prefix == configured prefix
        if participate_info.owner_address.is_empty() || participate_info.owner_address.len() != 21 {
            return Err("Invalid ownerAddress".to_string());
        }
        if participate_info.owner_address[0] != expected_prefix {
            return Err("Invalid ownerAddress".to_string());
        }
        let owner = revm_primitives::Address::from_slice(&participate_info.owner_address[1..]);

        // 2. Validate to_address (Java parity: DecodeUtil.addressValid(toAddress))
        // Java requires: length == 21 and prefix == configured prefix
        if participate_info.to_address.is_empty() || participate_info.to_address.len() != 21 {
            return Err("Invalid toAddress".to_string());
        }
        if participate_info.to_address[0] != expected_prefix {
            return Err("Invalid toAddress".to_string());
        }
        let to_address = revm_primitives::Address::from_slice(&participate_info.to_address[1..]);

        debug!(
            "ParticipateAssetIssue: owner={}, to={}, asset={}, amount={}",
            hex::encode(&participate_info.owner_address),
            hex::encode(&participate_info.to_address),
            String::from_utf8_lossy(&participate_info.asset_name),
            participate_info.amount
        );

        // 3. Validate amount > 0 (before self-participation check, matching Java order)
        if participate_info.amount <= 0 {
            return Err("Amount must greater than 0!".to_string());
        }

        // 4. Validate not self-participation (Java: Arrays.equals(ownerAddress, toAddress))
        if owner == to_address {
            return Err("Cannot participate asset Issue yourself !".to_string());
        }

        // 5. Validate owner account exists
        let owner_account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or("Account does not exist!")?;

        // 6. Validate owner has enough balance (amount + fee)
        // Java oracle validates balance before time window checks.
        let fee = 0i64; // ParticipateAssetIssue has no fee
        if owner_account.balance < participate_info.amount + fee {
            return Err("No enough balance !".to_string());
        }

        // 7. Get asset issue (using asset name as key)
        let allow_same_token_name = storage_adapter
            .get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;

        // Java parity: ByteArray.toStr([]) returns "null", not ""
        let asset_name_display = if participate_info.asset_name.is_empty() {
            "null".to_string()
        } else {
            String::from_utf8_lossy(&participate_info.asset_name).to_string()
        };
        let asset_issue = storage_adapter
            .get_asset_issue(&participate_info.asset_name, allow_same_token_name)
            .map_err(|e| format!("Failed to get asset issue: {}", e))?
            .ok_or_else(|| format!("No asset named {}", asset_name_display))?;

        // 8. Validate to_address is the asset owner
        let asset_owner_address = if asset_issue.owner_address.len() == 21 {
            &asset_issue.owner_address[1..]
        } else {
            &asset_issue.owner_address[..]
        };
        if to_address.as_slice() != asset_owner_address {
            return Err(format!(
                "The asset is not issued by {}",
                hex::encode(&participate_info.to_address)
            ));
        }

        // 9. Validate time window
        let now = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get timestamp: {}", e))?;
        if now >= asset_issue.end_time || now < asset_issue.start_time {
            return Err("No longer valid period!".to_string());
        }

        // 10. Calculate exchange amount
        let exchange_amount = Self::safe_multiply_divide(
            participate_info.amount,
            asset_issue.num as i64,
            asset_issue.trx_num as i64,
        )?;
        if exchange_amount <= 0 {
            return Err("Can not process the exchange!".to_string());
        }

        // 11. Validate to account exists (asset issuer)
        let to_account = storage_adapter
            .get_account_proto(&to_address)
            .map_err(|e| format!("Failed to get to account: {}", e))?
            .ok_or("To account does not exist!")?;

        // 12. Validate issuer has enough tokens
        let issuer_asset_balance = Self::get_asset_balance_v2(
            &to_account,
            &participate_info.asset_name,
            allow_same_token_name,
        );
        if issuer_asset_balance < exchange_amount {
            return Err("Asset balance is not enough !".to_string());
        }

        let token_id_str = if !asset_issue.id.is_empty() {
            asset_issue.id.clone()
        } else {
            String::from_utf8_lossy(&participate_info.asset_name).to_string()
        };
        // NOTE: This check is stricter than Java. Java does not explicitly validate empty token_id
        // in the actuator - it relies on the asset's existence. We keep this as a safety invariant.
        if token_id_str.is_empty() {
            return Err("token_id cannot be empty".to_string());
        }

        // 13. Execute the exchange
        let mut updated_owner = owner_account.clone();
        let mut updated_to = to_account.clone();

        // Subtract TRX from owner
        updated_owner.balance = updated_owner
            .balance
            .checked_sub(participate_info.amount)
            .ok_or("Balance underflow")?;

        // Add TRX to issuer
        updated_to.balance = updated_to
            .balance
            .checked_add(participate_info.amount)
            .ok_or("Balance overflow")?;

        // Add tokens to owner
        Self::add_asset_amount_v2(
            &mut updated_owner,
            &participate_info.asset_name,
            &token_id_str,
            exchange_amount,
            allow_same_token_name,
        )?;

        // Subtract tokens from issuer
        Self::reduce_asset_amount_v2(
            &mut updated_to,
            &participate_info.asset_name,
            &token_id_str,
            exchange_amount,
            allow_same_token_name,
        )?;

        // 14. Persist updated accounts
        storage_adapter
            .put_account_proto(&owner, &updated_owner)
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;
        storage_adapter
            .put_account_proto(&to_address, &updated_to)
            .map_err(|e| format!("Failed to persist to account: {}", e))?;

        // 15. Build state changes for CSV parity
        let mut state_changes = Vec::new();

        let old_owner_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(owner_account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };
        let new_owner_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(updated_owner.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };
        state_changes.push(TronStateChange::AccountChange {
            address: owner,
            old_account: Some(old_owner_info),
            new_account: Some(new_owner_info),
        });

        let old_to_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(to_account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };
        let new_to_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(updated_to.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };
        state_changes.push(TronStateChange::AccountChange {
            address: to_address,
            old_account: Some(old_to_info),
            new_account: Some(new_to_info),
        });

        // Sort for determinism
        state_changes.sort_by_key(|c| match c {
            TronStateChange::AccountChange { address, .. } => address.to_vec(),
            _ => vec![],
        });

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!(
            "ParticipateAssetIssue: exchanged {} TRX for {} tokens",
            participate_info.amount, exchange_amount
        );

        // Emit TRC-10 semantic change (issuer -> participant) for CSV parity and Java-side apply (Phase 2)
        let trc10_change = tron_backend_execution::Trc10Change::AssetTransferred(
            tron_backend_execution::Trc10AssetTransferred {
                owner_address: to_address, // issuer (sender of tokens)
                to_address: owner,         // participant (receiver of tokens)
                asset_name: participate_info.asset_name.clone(),
                token_id: Some(token_id_str.clone()),
                amount: exchange_amount,
            },
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![trc10_change],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: None,
            contract_address: None,
        })
    }

    /// Execute UNFREEZE_ASSET_CONTRACT (type 14)
    /// Unfreezes frozen TRC-10 supply and returns it to the asset issuer's balance
    ///
    /// Java oracle: UnfreezeAssetActuator.java
    ///
    /// Validation order (matches Java):
    ///   1. Any.is(UnfreezeAssetContract.class) type_url check
    ///   2. DecodeUtil.addressValid(ownerAddress)  — address parsed from contract bytes
    ///   3. Account exists
    ///   4. frozenSupplyCount > 0
    ///   5. assetIssuedName / assetIssuedID non-empty
    ///   6. allowedUnfreezeCount > 0 (time gate)
    ///
    /// Overflow parity: Java uses unchecked `+=` for the frozen-balance summation,
    /// then `addExact()` (throws ArithmeticException) inside `addAssetAmountV2`.
    /// Rust matches this: `wrapping_add` for the summation, `checked_add` in
    /// `add_asset_amount_v2` (→ "long overflow").
    fn execute_unfreeze_asset_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        // 1. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.UnfreezeAssetContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [UnfreezeAssetContract], real type[class com.google.protobuf.Any]",
        )?;

        // 2. Parse owner_address from contract bytes (Java: any.unpack(UnfreezeAssetContract.class))
        //    UnfreezeAssetContract proto: bytes owner_address = 1;
        let owner_tron = Self::parse_unfreeze_asset_owner_address(contract_bytes)?;

        // Use parsed owner_address for validation; fall back to from_raw only if proto is empty
        let owner_address_bytes = if !owner_tron.is_empty() {
            owner_tron.as_slice()
        } else {
            transaction.metadata.from_raw.as_deref().unwrap_or(&[])
        };

        // Validation parity: DecodeUtil.addressValid(ownerAddress)
        let expected_prefix = storage_adapter.address_prefix();
        if owner_address_bytes.len() != 21 || owner_address_bytes[0] != expected_prefix {
            return Err("Invalid address".to_string());
        }
        let owner = revm_primitives::Address::from_slice(&owner_address_bytes[1..]);
        let readable_owner_address = hex::encode(owner_address_bytes);

        debug!("UnfreezeAsset: owner={}", readable_owner_address);

        // 3. Validate owner account exists
        let account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or_else(|| format!("Account[{}] does not exist", readable_owner_address))?;

        // 4. Validate account has frozen supply
        if account.frozen_supply.is_empty() {
            return Err("no frozen supply balance".to_string());
        }

        // 5. Get asset issued info (name or id must be non-empty)
        let allow_same_token_name = storage_adapter
            .get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;

        let asset_key = if allow_same_token_name == 0 {
            if account.asset_issued_name.is_empty() {
                return Err("this account has not issued any asset".to_string());
            }
            account.asset_issued_name.clone()
        } else {
            if account.asset_issued_id.is_empty() {
                return Err("this account has not issued any asset".to_string());
            }
            account.asset_issued_id.clone()
        };

        // 6. Time gate: at least one frozen entry must have expired
        let now = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get timestamp: {}", e))?;

        let expired_count = account
            .frozen_supply
            .iter()
            .filter(|frozen| frozen.expire_time <= now)
            .count();
        if expired_count == 0 {
            return Err("It's not time to unfreeze asset supply".to_string());
        }

        // --- Execution phase (matches UnfreezeAssetActuator.execute) ---

        // Resolve token_id_str for addAssetAmountV2:
        //   - allowSameTokenName == 0: need AssetIssue lookup to map name → tokenId
        //     Java: AssetIssueCapsule.getId() used directly (no fallback)
        //   - allowSameTokenName == 1: key IS the tokenId, no lookup needed (Java parity)
        let token_id_str = if allow_same_token_name == 0 {
            let asset_issue = storage_adapter
                .get_asset_issue(&asset_key, allow_same_token_name)
                .map_err(|e| format!("Failed to get asset issue: {}", e))?
                .ok_or("No asset!".to_string())?;
            // Java: String tokenID = assetIssueCapsule.getId(); — used as-is
            asset_issue.id.clone()
        } else {
            // allowSameTokenName == 1: key is already the tokenId string
            String::from_utf8_lossy(&asset_key).to_string()
        };

        // Process frozen supply — separate expired from non-expired.
        // Java: unfreezeAsset += frozenBalance  (unchecked long +=, wrapping on overflow)
        // Overflow is caught later by addExact() in addAssetAmountV2.
        let mut unfreeze_asset: i64 = 0;
        let mut remaining_frozen = Vec::new();

        for frozen in account.frozen_supply.iter() {
            if frozen.expire_time <= now {
                // Expired — wrapping_add matches Java's unchecked `+=`
                unfreeze_asset = unfreeze_asset.wrapping_add(frozen.frozen_balance);
            } else {
                // Not expired — keep in frozen list
                remaining_frozen.push(frozen.clone());
            }
        }

        // Update account
        let mut updated_account = account.clone();
        updated_account.frozen_supply = remaining_frozen;

        // Import asset from AccountAssetStore if ALLOW_ASSET_OPTIMIZATION == 1
        // (mirrors Java's AccountCapsule.importAsset(key) called inside addAssetAmountV2)
        Self::import_asset_if_optimized(storage_adapter, &mut updated_account, &asset_key, &owner)?;

        // Add unfrozen assets back to balance
        // (Java: addAssetAmountV2 uses addExact → ArithmeticException on overflow)
        Self::add_asset_amount_v2(
            &mut updated_account,
            &asset_key,
            &token_id_str,
            unfreeze_asset,
            allow_same_token_name,
        )?;

        // Persist updated account
        storage_adapter
            .put_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to persist account: {}", e))?;

        // Build state change for CSV parity (balance unchanged, but for consistency)
        let old_account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };
        let new_account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(updated_account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        let state_changes = vec![TronStateChange::AccountChange {
            address: owner,
            old_account: Some(old_account_info),
            new_account: Some(new_account_info),
        }];

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!("UnfreezeAsset: unfroze {} tokens", unfreeze_asset);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
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

    /// Parse `owner_address` (field 1, length-delimited) from a serialized
    /// `UnfreezeAssetContract` protobuf message.
    ///
    /// Proto definition (asset_issue_contract.proto):
    ///   bytes owner_address = 1;
    fn parse_unfreeze_asset_owner_address(data: &[u8]) -> Result<Vec<u8>, String> {
        let mut owner_address: Vec<u8> = Vec::new();
        let mut pos = 0;
        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match (field_number, wire_type) {
                (1, 2) => {
                    // bytes owner_address = 1;
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address = payload.to_vec();
                    pos += total_len;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }
        Ok(owner_address)
    }

    /// Execute UPDATE_ASSET_CONTRACT (type 15)
    /// Updates TRC-10 asset metadata: url, description, free_asset_net_limit, public_free_asset_net_limit
    ///
    /// Java oracle: UpdateAssetActuator.java
    /// Validation order matches Java exactly:
    ///   1) Any.is(UpdateAssetContract.class)
    ///   2) Parse contract → owner_address, newLimit, newPublicLimit, url, description
    ///   3) DecodeUtil.addressValid(ownerAddress) — 21 bytes, prefix 0x41
    ///   4) Account exists
    ///   5) Asset issued name/ID + store existence
    ///   6) URL valid
    ///   7) Description valid
    ///   8) Limit bounds
    fn execute_update_asset_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        let owner = transaction.from;

        // 1. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.UpdateAssetContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error, expected type [UpdateAssetContract],real type[class com.google.protobuf.Any]",
        )?;

        // 2. Parse contract data (including owner_address for Java parity)
        let update_info = self.parse_update_asset_contract(contract_bytes)?;

        debug!(
            "UpdateAsset: owner={}, new_limit={}, new_public_limit={}",
            hex::encode(&update_info.owner_address),
            update_info.new_limit,
            update_info.new_public_limit
        );

        // 3. Validate owner address (Java parity: DecodeUtil.addressValid)
        // Java requires exactly 21 bytes with correct prefix (0x41 mainnet).
        // Use owner_address from the parsed contract bytes (same as Java's any.unpack()).
        let owner_address = &update_info.owner_address;
        let owner_address_valid =
            owner_address.len() == 21 && (owner_address[0] == 0x41 || owner_address[0] == 0xa0);
        if !owner_address_valid {
            return Err("Invalid ownerAddress".to_string());
        }

        // 4. Validate owner account exists
        let account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or("Account does not exist")?;

        // 5. Get asset info and validate ownership + store existence
        // Java checks store existence BEFORE url/description/limits.
        let allow_same_token_name = storage_adapter
            .get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;

        if allow_same_token_name == 0 {
            if account.asset_issued_name.is_empty() {
                return Err("Account has not issued any asset".to_string());
            }
            // Check legacy store existence (Java: assetIssueStore.get(account.getAssetIssuedName().toByteArray()) == null)
            let _legacy_asset = storage_adapter
                .get_asset_issue(&account.asset_issued_name, 0)
                .map_err(|e| format!("Failed to get asset issue: {}", e))?
                .ok_or("Asset is not existed in AssetIssueStore")?;
        } else {
            if account.asset_issued_id.is_empty() {
                return Err("Account has not issued any asset".to_string());
            }
            // Check V2 store existence (Java: assetIssueV2Store.get(account.getAssetIssuedID().toByteArray()) == null)
            let _v2_asset = storage_adapter
                .get_asset_issue(&account.asset_issued_id, 1)
                .map_err(|e| format!("Failed to get asset issue: {}", e))?
                .ok_or("Asset is not existed in AssetIssueV2Store")?;
        }

        // 6. Validate URL
        if !Self::valid_url(&update_info.url) {
            return Err("Invalid url".to_string());
        }

        // 7. Validate description
        if !Self::valid_asset_description(&update_info.description) {
            return Err("Invalid description".to_string());
        }

        // 8. Validate limits
        let one_day_net_limit = storage_adapter
            .get_one_day_net_limit()
            .map_err(|e| format!("Failed to get oneDayNetLimit: {}", e))?;

        if update_info.new_limit < 0 || update_info.new_limit >= one_day_net_limit {
            return Err("Invalid FreeAssetNetLimit".to_string());
        }

        if update_info.new_public_limit < 0 || update_info.new_public_limit >= one_day_net_limit {
            return Err("Invalid PublicFreeAssetNetLimit".to_string());
        }

        // === Execution (Java: UpdateAssetActuator.execute) ===
        // Java always loads V2 entry by account.assetIssuedID first, then updates it.
        // If allowSameTokenName == 0, also loads legacy entry by name and updates it separately.
        // Each store entry is updated in-place (only 4 fields), preserving all other fields.

        // Load V2 entry (Java always loads this in execute, regardless of allowSameTokenName)
        let mut asset_issue_v2 = storage_adapter
            .get_asset_issue(&account.asset_issued_id, 1)
            .map_err(|e| format!("Failed to get asset issue V2: {}", e))?
            .ok_or("Asset is not existed in AssetIssueV2Store")?;

        // Update only the four fields on V2 entry
        asset_issue_v2.free_asset_net_limit = update_info.new_limit;
        asset_issue_v2.public_free_asset_net_limit = update_info.new_public_limit;
        asset_issue_v2.url = update_info.url.clone();
        asset_issue_v2.description = update_info.description.clone();

        if allow_same_token_name == 0 {
            // Load legacy entry separately and update only 4 fields (preserving per-store fields)
            let mut asset_issue_legacy = storage_adapter
                .get_asset_issue(&account.asset_issued_name, 0)
                .map_err(|e| format!("Failed to get asset issue: {}", e))?
                .ok_or("Asset is not existed in AssetIssueStore")?;

            asset_issue_legacy.free_asset_net_limit = update_info.new_limit;
            asset_issue_legacy.public_free_asset_net_limit = update_info.new_public_limit;
            asset_issue_legacy.url = update_info.url.clone();
            asset_issue_legacy.description = update_info.description.clone();

            // Persist both stores (Java: assetIssueStore.put + assetIssueStoreV2.put)
            storage_adapter
                .put_asset_issue(&account.asset_issued_name, &asset_issue_legacy, false)
                .map_err(|e| format!("Failed to persist asset issue: {}", e))?;
            storage_adapter
                .put_asset_issue(&account.asset_issued_id, &asset_issue_v2, true)
                .map_err(|e| format!("Failed to persist asset issue V2: {}", e))?;
        } else {
            // Only update AssetIssueV2Store (Java: assetIssueV2Store.put)
            storage_adapter
                .put_asset_issue(&account.asset_issued_id, &asset_issue_v2, true)
                .map_err(|e| format!("Failed to persist asset issue V2: {}", e))?;
        }

        // Java's UpdateAssetActuator only mutates AssetIssue stores. It does not update the
        // issuer account, so embedded CSV reporting sees no account/state changes for this tx.
        let state_changes = Vec::new();

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!(
            "UpdateAsset: updated asset id={}",
            String::from_utf8_lossy(&account.asset_issued_id)
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
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

    /// Parse ParticipateAssetIssueContract protobuf bytes
    ///
    /// Parses all fields including owner_address for Java-parity validation.
    /// Java oracle: ParticipateAssetIssueActuator validates owner_address via DecodeUtil.addressValid
    fn parse_participate_asset_issue_contract(
        &self,
        data: &[u8],
    ) -> Result<ParticipateAssetIssueInfo, String> {
        let mut owner_address = Vec::new();
        let mut to_address = Vec::new();
        let mut asset_name = Vec::new();
        let mut amount: i64 = 0;

        let mut pos = 0;
        while pos < data.len() {
            // Read tag
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match field_number {
                // owner_address = 1 (parsed for Java-parity validation)
                1 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for owner_address".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address = payload.to_vec();
                    pos += total_len;
                }
                // to_address = 2
                2 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for to_address".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    to_address = payload.to_vec();
                    pos += total_len;
                }
                // asset_name = 3
                3 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for asset_name".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    asset_name = payload.to_vec();
                    pos += total_len;
                }
                // amount = 4
                4 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for amount".to_string());
                    }
                    let (value, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    amount = value as i64;
                    pos += new_pos;
                }
                _ => {
                    // Skip unknown fields
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        Ok(ParticipateAssetIssueInfo {
            owner_address,
            to_address,
            asset_name,
            amount,
        })
    }

    /// Parse UpdateAssetContract protobuf bytes.
    /// Parses all fields including owner_address for Java-parity validation.
    /// Java oracle: UpdateAssetActuator validates owner_address via DecodeUtil.addressValid
    fn parse_update_asset_contract(&self, data: &[u8]) -> Result<UpdateAssetInfo, String> {
        let mut owner_address = Vec::new();
        let mut description = Vec::new();
        let mut url = Vec::new();
        let mut new_limit: i64 = 0;
        let mut new_public_limit: i64 = 0;

        let mut pos = 0;
        while pos < data.len() {
            // Read tag
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match field_number {
                // owner_address = 1 (parsed for Java-parity validation)
                1 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for owner_address".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address = payload.to_vec();
                    pos += total_len;
                }
                // description = 2
                2 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for description".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    description = payload.to_vec();
                    pos += total_len;
                }
                // url = 3
                3 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for url".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    url = payload.to_vec();
                    pos += total_len;
                }
                // new_limit = 4
                4 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for new_limit".to_string());
                    }
                    let (value, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    new_limit = value as i64;
                    pos += new_pos;
                }
                // new_public_limit = 5
                5 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for new_public_limit".to_string());
                    }
                    let (value, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    new_public_limit = value as i64;
                    pos += new_pos;
                }
                _ => {
                    // Skip unknown fields
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        Ok(UpdateAssetInfo {
            owner_address,
            description,
            url,
            new_limit,
            new_public_limit,
        })
    }

    /// Safe multiply and floor divide (like Java's multiplyExact and floorDiv)
    fn safe_multiply_divide(value: i64, multiplier: i64, divisor: i64) -> Result<i64, String> {
        if divisor == 0 {
            return Err("Division by zero".to_string());
        }
        let product = value
            .checked_mul(multiplier)
            .ok_or("Overflow in multiplication")?;
        // Floor division (rounds toward negative infinity for negative results)
        let result = product / divisor;
        Ok(result)
    }

    /// Get asset balance from account (TRC-10; matches java-tron AccountCapsule.getAsset)
    ///
    /// - `allowSameTokenName == 0`: `asset_key` is the asset name, balance lives in `Account.asset`
    /// - `allowSameTokenName == 1`: `asset_key` is the token id, balance lives in `Account.asset_v2`
    fn get_asset_balance_v2(
        account: &tron_backend_execution::protocol::Account,
        asset_key: &[u8],
        allow_same_token_name: i64,
    ) -> i64 {
        let key_str = String::from_utf8_lossy(asset_key).to_string();

        if allow_same_token_name == 0 {
            return *account.asset.get(&key_str).unwrap_or(&0);
        }

        *account.asset_v2.get(&key_str).unwrap_or(&0)
    }

    /// Add asset amount to account (TRC-10; matches java-tron addAssetAmountV2)
    ///
    /// `asset_key` is the raw contract key:
    /// - `allowSameTokenName == 0`: asset name bytes (updates `asset[name]` and `asset_v2[token_id]`)
    /// - `allowSameTokenName == 1`: token id bytes (updates `asset_v2[token_id]` only)
    fn add_asset_amount_v2(
        account: &mut tron_backend_execution::protocol::Account,
        asset_key: &[u8],
        token_id: &str,
        amount: i64,
        allow_same_token_name: i64,
    ) -> Result<(), String> {
        if allow_same_token_name == 0 {
            let name_key = String::from_utf8_lossy(asset_key).to_string();
            let current = *account.asset.get(&name_key).unwrap_or(&0);
            let new_amount = current
                .checked_add(amount)
                .ok_or_else(|| "long overflow".to_string())?;

            account.asset.insert(name_key, new_amount);
            account.asset_v2.insert(token_id.to_string(), new_amount);
            return Ok(());
        }

        let current = *account.asset_v2.get(token_id).unwrap_or(&0);
        let new_amount = current
            .checked_add(amount)
            .ok_or_else(|| "long overflow".to_string())?;
        account.asset_v2.insert(token_id.to_string(), new_amount);
        Ok(())
    }

    /// Reduce asset amount from account (TRC-10; matches java-tron reduceAssetAmountV2)
    ///
    /// `token_id` must be the numeric token id string. In legacy mode (`allowSameTokenName == 0`),
    /// the contract key is the asset name, but `asset_v2` must be updated under `token_id`.
    fn reduce_asset_amount_v2(
        account: &mut tron_backend_execution::protocol::Account,
        asset_key: &[u8],
        token_id: &str,
        amount: i64,
        allow_same_token_name: i64,
    ) -> Result<(), String> {
        if allow_same_token_name == 0 {
            let name_key = String::from_utf8_lossy(asset_key).to_string();
            let current = *account.asset.get(&name_key).unwrap_or(&0);
            if current < amount {
                return Err("Insufficient asset balance".to_string());
            }

            let new_amount = current
                .checked_sub(amount)
                .ok_or_else(|| "long overflow".to_string())?;

            account.asset.insert(name_key, new_amount);
            account.asset_v2.insert(token_id.to_string(), new_amount);
            return Ok(());
        }

        let current = *account.asset_v2.get(token_id).unwrap_or(&0);
        if current < amount {
            return Err("Insufficient asset balance".to_string());
        }

        let new_amount = current
            .checked_sub(amount)
            .ok_or_else(|| "long overflow".to_string())?;
        account.asset_v2.insert(token_id.to_string(), new_amount);
        Ok(())
    }

    /// Import asset balance from AccountAssetStore into account.asset_v2 map
    /// when ALLOW_ASSET_OPTIMIZATION == 1.
    ///
    /// This mirrors Java's AccountCapsule.importAsset() which loads balances
    /// from the external AccountAssetStore into the in-memory asset map.
    fn import_asset_if_optimized(
        storage_adapter: &tron_backend_execution::EngineBackedEvmStateStore,
        account: &mut tron_backend_execution::protocol::Account,
        token_id: &[u8],
        address: &revm_primitives::Address,
    ) -> Result<(), String> {
        // Check if asset optimization is enabled
        let allow_asset_optimization = storage_adapter
            .get_allow_asset_optimization()
            .map_err(|e| format!("Failed to get ALLOW_ASSET_OPTIMIZATION: {}", e))?;
        let allow_same_token_name = storage_adapter
            .get_allow_same_token_name()
            .map_err(|e| format!("Failed to get ALLOW_SAME_TOKEN_NAME: {}", e))?;

        // Only import when asset optimization is enabled and using V2 format
        if allow_asset_optimization != 1 || allow_same_token_name != 1 {
            return Ok(());
        }

        let token_key = String::from_utf8_lossy(token_id).to_string();

        // Skip if already in the asset_v2 map
        if account.asset_v2.contains_key(&token_key) {
            return Ok(());
        }

        // Import from AccountAssetStore
        let balance = storage_adapter
            .get_asset_balance_from_asset_store(address, token_id)
            .map_err(|e| format!("Failed to read from AccountAssetStore: {}", e))?;

        if balance > 0 {
            account.asset_v2.insert(token_key, balance);
        }

        Ok(())
    }

    fn valid_readable_bytes(bytes: &[u8], max_length: usize) -> bool {
        if bytes.is_empty() || bytes.len() > max_length {
            return false;
        }

        bytes.iter().all(|b| matches!(*b, 0x21..=0x7e))
    }

    /// Validate asset name (matches Java's TransactionUtil.validAssetName)
    fn valid_asset_name(asset_name: &[u8]) -> bool {
        Self::valid_readable_bytes(asset_name, 32)
    }

    /// Validate URL (simplified version of Java's TransactionUtil.validUrl)
    fn valid_url(url: &[u8]) -> bool {
        // URL must be non-empty and <= 256 bytes
        !url.is_empty() && url.len() <= 256
    }

    /// Validate asset description (simplified version of Java's TransactionUtil.validAssetDescription)
    fn valid_asset_description(description: &[u8]) -> bool {
        // Description must be <= 200 bytes
        description.len() <= 200
    }

    // ============================================================================
    // Phase 2.F: Exchange Contract Handlers (41/42/43/44)
    // ============================================================================

    /// Execute EXCHANGE_CREATE_CONTRACT (type 41)
    /// Creates a new Bancor-style exchange with initial token balances
    ///
    /// Java reference: ExchangeCreateActuator.java
    fn execute_exchange_create_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use contracts::exchange::{is_number, is_trx};
        use contracts::proto::TransactionResultBuilder;
        use revm::primitives::Address;

        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.ExchangeCreateContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [ExchangeCreateContract],real type[class com.google.protobuf.Any]",
        )?;

        debug!(
            "Executing EXCHANGE_CREATE_CONTRACT: owner={:?}",
            transaction.from
        );

        // 1. Parse contract data
        let create_info = self.parse_exchange_create_contract(contract_bytes)?;
        debug!(
            "Parsed ExchangeCreate: first_token={}, second_token={}, balances={}/{}",
            String::from_utf8_lossy(&create_info.first_token_id),
            String::from_utf8_lossy(&create_info.second_token_id),
            create_info.first_token_balance,
            create_info.second_token_balance
        );

        // 2. Validate + get owner account (matches Java's ExchangeCreateActuator.validate)
        let owner_address = create_info.owner_address.clone();
        let readable_owner_address = hex::encode(&owner_address);

        let address_prefix = storage_adapter.address_prefix();
        let owner = if owner_address.len() == 21 && owner_address[0] == address_prefix {
            Address::from_slice(&owner_address[1..])
        } else {
            return Err("Invalid address".to_string());
        };

        let owner_tron = owner_address;
        let account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or(format!("account[{}] not exists", readable_owner_address))?;

        // 3. Get dynamic properties
        let allow_same_token_name = storage_adapter
            .get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;
        let exchange_create_fee = storage_adapter
            .get_exchange_create_fee()
            .map_err(|e| format!("Failed to get exchange create fee: {}", e))?;
        let balance_limit = storage_adapter
            .get_exchange_balance_limit()
            .map_err(|e| format!("Failed to get exchange balance limit: {}", e))?;

        // 4. Validate
        // - Balance for fee
        if account.balance < exchange_create_fee {
            return Err("No enough balance for exchange create fee!".to_string());
        }

        // - Token ID format validation (allowSameTokenName=1)
        if allow_same_token_name == 1 {
            if !is_trx(&create_info.first_token_id) && !is_number(&create_info.first_token_id) {
                return Err("first token id is not a valid number".to_string());
            }
            if !is_trx(&create_info.second_token_id) && !is_number(&create_info.second_token_id) {
                return Err("second token id is not a valid number".to_string());
            }
        }

        // - Cannot exchange same tokens
        if create_info.first_token_id == create_info.second_token_id {
            return Err("cannot exchange same tokens".to_string());
        }

        // - Token balances must be positive
        if create_info.first_token_balance <= 0 || create_info.second_token_balance <= 0 {
            return Err("token balance must greater than zero".to_string());
        }

        // - Token balances must be within limit
        if create_info.first_token_balance > balance_limit
            || create_info.second_token_balance > balance_limit
        {
            return Err(format!("token balance must less than {}", balance_limit));
        }

        // - Sufficient balance for first token
        if is_trx(&create_info.first_token_id) {
            if account.balance < create_info.first_token_balance + exchange_create_fee {
                return Err("balance is not enough".to_string());
            }
        } else {
            // Use asset_balance_enough_v2 which handles:
            // - ALLOW_SAME_TOKEN_NAME == 0: reads Account.asset (token name key)
            // - ALLOW_SAME_TOKEN_NAME == 1: reads Account.asset_v2 (token ID key)
            // - ALLOW_ASSET_OPTIMIZATION == 1: also checks AccountAssetStore
            let has_enough = storage_adapter
                .asset_balance_enough_v2(
                    &owner,
                    &create_info.first_token_id,
                    create_info.first_token_balance,
                )
                .map_err(|e| format!("Failed to check first token balance: {}", e))?;
            if !has_enough {
                return Err("first token balance is not enough".to_string());
            }
        }

        // - Sufficient balance for second token
        if is_trx(&create_info.second_token_id) {
            if account.balance < create_info.second_token_balance + exchange_create_fee {
                return Err("balance is not enough".to_string());
            }
        } else {
            // Use asset_balance_enough_v2 which handles:
            // - ALLOW_SAME_TOKEN_NAME == 0: reads Account.asset (token name key)
            // - ALLOW_SAME_TOKEN_NAME == 1: reads Account.asset_v2 (token ID key)
            // - ALLOW_ASSET_OPTIMIZATION == 1: also checks AccountAssetStore
            let has_enough = storage_adapter
                .asset_balance_enough_v2(
                    &owner,
                    &create_info.second_token_id,
                    create_info.second_token_balance,
                )
                .map_err(|e| format!("Failed to check second token balance: {}", e))?;
            if !has_enough {
                return Err("second token balance is not enough".to_string());
            }
        }

        // 5. Execute
        let mut updated_account = account.clone();

        let first_token_id_str = if is_trx(&create_info.first_token_id) {
            String::new()
        } else if allow_same_token_name == 0 {
            storage_adapter
                .get_asset_issue(&create_info.first_token_id, 0)
                .map_err(|e| format!("Failed to get first token asset issue: {}", e))?
                .ok_or("No asset!".to_string())?
                .id
        } else {
            String::from_utf8_lossy(&create_info.first_token_id).to_string()
        };

        let second_token_id_str = if is_trx(&create_info.second_token_id) {
            String::new()
        } else if allow_same_token_name == 0 {
            storage_adapter
                .get_asset_issue(&create_info.second_token_id, 0)
                .map_err(|e| format!("Failed to get second token asset issue: {}", e))?
                .ok_or("No asset!".to_string())?
                .id
        } else {
            String::from_utf8_lossy(&create_info.second_token_id).to_string()
        };

        // Deduct fee
        updated_account.balance -= exchange_create_fee;

        // Deduct first token
        if is_trx(&create_info.first_token_id) {
            updated_account.balance -= create_info.first_token_balance;
        } else {
            // Import from AccountAssetStore if ALLOW_ASSET_OPTIMIZATION == 1
            Self::import_asset_if_optimized(
                storage_adapter,
                &mut updated_account,
                &create_info.first_token_id,
                &owner,
            )?;
            Self::reduce_asset_amount_v2(
                &mut updated_account,
                &create_info.first_token_id,
                &first_token_id_str,
                create_info.first_token_balance,
                allow_same_token_name,
            )?;
        }

        // Deduct second token
        if is_trx(&create_info.second_token_id) {
            updated_account.balance -= create_info.second_token_balance;
        } else {
            // Import from AccountAssetStore if ALLOW_ASSET_OPTIMIZATION == 1
            Self::import_asset_if_optimized(
                storage_adapter,
                &mut updated_account,
                &create_info.second_token_id,
                &owner,
            )?;
            Self::reduce_asset_amount_v2(
                &mut updated_account,
                &create_info.second_token_id,
                &second_token_id_str,
                create_info.second_token_balance,
                allow_same_token_name,
            )?;
        }

        // Create exchange
        let exchange_id = storage_adapter
            .get_latest_exchange_num()
            .map_err(|e| format!("Failed to get latest exchange num: {}", e))?
            + 1;
        let now = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest block header timestamp: {}", e))?;

        // For allowSameTokenName=0, resolve token names to IDs
        let mut first_token_id_v2 = create_info.first_token_id.clone();
        let mut second_token_id_v2 = create_info.second_token_id.clone();

        if allow_same_token_name == 0 {
            // Save to old store with names
            let exchange_v1 = tron_backend_execution::protocol::Exchange {
                exchange_id,
                creator_address: owner_tron.clone(),
                create_time: now,
                first_token_id: create_info.first_token_id.clone(),
                first_token_balance: create_info.first_token_balance,
                second_token_id: create_info.second_token_id.clone(),
                second_token_balance: create_info.second_token_balance,
            };
            storage_adapter
                .put_exchange_to_store(&exchange_v1, false)
                .map_err(|e| format!("Failed to store exchange V1: {}", e))?;

            // Resolve to real IDs for V2 store
            if !is_trx(&create_info.first_token_id) {
                if let Ok(Some(asset)) =
                    storage_adapter.get_asset_issue(&create_info.first_token_id, 0)
                {
                    first_token_id_v2 = asset.id.as_bytes().to_vec();
                }
            }
            if !is_trx(&create_info.second_token_id) {
                if let Ok(Some(asset)) =
                    storage_adapter.get_asset_issue(&create_info.second_token_id, 0)
                {
                    second_token_id_v2 = asset.id.as_bytes().to_vec();
                }
            }
        }

        // Save to V2 store
        let exchange_v2 = tron_backend_execution::protocol::Exchange {
            exchange_id,
            creator_address: owner_tron.clone(),
            create_time: now,
            first_token_id: first_token_id_v2,
            first_token_balance: create_info.first_token_balance,
            second_token_id: second_token_id_v2,
            second_token_balance: create_info.second_token_balance,
        };
        storage_adapter
            .put_exchange(&exchange_v2)
            .map_err(|e| format!("Failed to store exchange V2: {}", e))?;

        // Update latest exchange num
        storage_adapter
            .set_latest_exchange_num(exchange_id)
            .map_err(|e| format!("Failed to update latest exchange num: {}", e))?;

        // Update account
        storage_adapter
            .set_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to update account: {}", e))?;

        // Handle fee (burn or blackhole)
        // Java: DynamicPropertiesStore.supportBlackHoleOptimization()
        //   - if true: burnTrx(fee) -> increment BURN_TRX_AMOUNT
        //   - if false: credit blackhole account balance
        let support_black_hole = storage_adapter
            .support_black_hole_optimization()
            .map_err(|e| format!("Failed to get blackhole optimization flag: {}", e))?;
        if support_black_hole {
            // Burn the fee by incrementing BURN_TRX_AMOUNT
            storage_adapter
                .burn_trx(exchange_create_fee as u64)
                .map_err(|e| format!("Failed to burn fee: {}", e))?;
        } else {
            // Credit blackhole account
            let blackhole_addr = storage_adapter.get_blackhole_address_evm();
            storage_adapter
                .add_balance(&blackhole_addr, exchange_create_fee as u64)
                .map_err(|e| format!("Failed to credit blackhole: {}", e))?;
        }

        // 6. Build result
        let account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(updated_account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        let state_changes = vec![TronStateChange::AccountChange {
            address: owner,
            old_account: Some(revm_primitives::AccountInfo {
                balance: revm_primitives::U256::from(account.balance as u64),
                nonce: 0,
                code_hash: revm_primitives::B256::ZERO,
                code: None,
            }),
            new_account: Some(account_info),
        }];

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Build receipt with fee and exchange_id
        // Java: ret.setStatus(fee, SUCESS) + ret.setExchangeId(id)
        // Proto fields: fee=1, exchange_id=21
        let receipt = TransactionResultBuilder::new()
            .with_fee(exchange_create_fee)
            .with_exchange_id(exchange_id)
            .build();

        debug!(
            "ExchangeCreate: created exchange {} with tokens {}/{}",
            exchange_id,
            String::from_utf8_lossy(&create_info.first_token_id),
            String::from_utf8_lossy(&create_info.second_token_id)
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: Some(receipt),
            contract_address: None,
        })
    }

    /// Execute EXCHANGE_INJECT_CONTRACT (type 42)
    /// Injects liquidity into an existing exchange (creator only)
    ///
    /// Java reference: ExchangeInjectActuator.java
    fn execute_exchange_inject_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use contracts::exchange::{
            calculate_inject_another_amount, calculate_inject_another_amount_multiply_exact,
            is_number, is_trx,
        };
        use contracts::proto::TransactionResultBuilder;

        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.ExchangeInjectContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [ExchangeInjectContract],real type[class com.google.protobuf.Any]",
        )?;

        debug!(
            "Executing EXCHANGE_INJECT_CONTRACT: owner={:?}",
            transaction.from
        );

        // 1. Parse contract data
        let inject_info = self.parse_exchange_inject_contract(contract_bytes)?;
        debug!(
            "Parsed ExchangeInject: exchange_id={}, token={}, quant={}",
            inject_info.exchange_id,
            String::from_utf8_lossy(&inject_info.token_id),
            inject_info.quant
        );

        // 2. Validate owner address (Java: DecodeUtil.addressValid)
        let address_prefix = storage_adapter.address_prefix();
        if inject_info.owner_address.len() != 21 || inject_info.owner_address[0] != address_prefix {
            return Err("Invalid address".to_string());
        }

        // 3. Get owner account
        // Java: ExchangeInjectActuator.validate() - "account[<hex>] not exists"
        let owner = transaction.from;
        let owner_tron = storage_adapter.to_tron_address_21(&owner).to_vec();
        let account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("account[{}] not exists", hex::encode(&owner_tron)))?;

        // 4. Get exchange (routed by allowSameTokenName)
        // Java: Commons.getExchangeStoreFinal() - reads from v1 when allowSameTokenName=0, v2 otherwise
        let allow_same_token_name = storage_adapter
            .get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;
        let exchange = storage_adapter
            .get_exchange_routed(inject_info.exchange_id, allow_same_token_name)
            .map_err(|e| format!("Failed to get exchange: {}", e))?
            .ok_or(format!("Exchange[{}] not exists", inject_info.exchange_id))?;

        // 5. Validate
        // - Must be creator
        if owner_tron != exchange.creator_address {
            return Err(format!(
                "account[{}] is not creator",
                hex::encode(&owner_tron)
            ));
        }

        // - Token ID format validation
        if allow_same_token_name == 1
            && !is_trx(&inject_info.token_id)
            && !is_number(&inject_info.token_id)
        {
            return Err("token id is not a valid number".to_string());
        }

        // - Token must be in exchange
        let is_first_token = inject_info.token_id == exchange.first_token_id;
        let is_second_token = inject_info.token_id == exchange.second_token_id;
        if !is_first_token && !is_second_token {
            return Err("token id is not in exchange".to_string());
        }

        // - Exchange must not be closed
        if exchange.first_token_balance == 0 || exchange.second_token_balance == 0 {
            return Err(
                "Token balance in exchange is equal with 0,the exchange has been closed"
                    .to_string(),
            );
        }

        // - Quant must be positive
        if inject_info.quant <= 0 {
            return Err("injected token quant must greater than zero".to_string());
        }

        // Calculate another token amount
        let (
            another_token_id,
            another_token_quant_validate,
            new_first_balance_validate,
            new_second_balance_validate,
        ) = if is_first_token {
            let another_quant = calculate_inject_another_amount(
                exchange.first_token_balance,
                exchange.second_token_balance,
                inject_info.quant,
            );
            (
                exchange.second_token_id.clone(),
                another_quant,
                exchange.first_token_balance + inject_info.quant,
                exchange.second_token_balance + another_quant,
            )
        } else {
            let another_quant = calculate_inject_another_amount(
                exchange.second_token_balance,
                exchange.first_token_balance,
                inject_info.quant,
            );
            (
                exchange.first_token_id.clone(),
                another_quant,
                exchange.first_token_balance + another_quant,
                exchange.second_token_balance + inject_info.quant,
            )
        };

        // - Another quant must be positive
        if another_token_quant_validate <= 0 {
            return Err("the calculated token quant  must be greater than 0".to_string());
        }

        // - Balance limits
        let balance_limit = storage_adapter
            .get_exchange_balance_limit()
            .map_err(|e| format!("Failed to get balance limit: {}", e))?;
        if new_first_balance_validate > balance_limit || new_second_balance_validate > balance_limit
        {
            return Err(format!("token balance must less than {}", balance_limit));
        }

        // - Sufficient balance for token
        // Java: AccountCapsule.assetBalanceEnoughV2() routes by allowSameTokenName
        if is_trx(&inject_info.token_id) {
            if account.balance < inject_info.quant {
                return Err("balance is not enough".to_string());
            }
        } else {
            let balance = storage_adapter
                .get_asset_balance_routed(&owner, &inject_info.token_id, allow_same_token_name)
                .map_err(|e| format!("Failed to get token balance: {}", e))?;
            if balance < inject_info.quant {
                return Err("token balance is not enough".to_string());
            }
        }

        // - Sufficient balance for another token
        if is_trx(&another_token_id) {
            if account.balance < another_token_quant_validate {
                return Err("balance is not enough".to_string());
            }
        } else {
            let balance = storage_adapter
                .get_asset_balance_routed(&owner, &another_token_id, allow_same_token_name)
                .map_err(|e| format!("Failed to get another token balance: {}", e))?;
            if balance < another_token_quant_validate {
                return Err("another token balance is not enough".to_string());
            }
        }

        // 5. Execute
        let another_token_quant = if is_first_token {
            calculate_inject_another_amount_multiply_exact(
                exchange.first_token_balance,
                exchange.second_token_balance,
                inject_info.quant,
            )?
        } else {
            calculate_inject_another_amount_multiply_exact(
                exchange.second_token_balance,
                exchange.first_token_balance,
                inject_info.quant,
            )?
        };
        let (new_first_balance, new_second_balance) = if is_first_token {
            (
                exchange.first_token_balance + inject_info.quant,
                exchange.second_token_balance + another_token_quant,
            )
        } else {
            (
                exchange.first_token_balance + another_token_quant,
                exchange.second_token_balance + inject_info.quant,
            )
        };

        let mut updated_account = account.clone();

        let token_id_str = if is_trx(&inject_info.token_id) {
            String::new()
        } else if allow_same_token_name == 0 {
            match storage_adapter.get_asset_issue(&inject_info.token_id, 0) {
                Ok(Some(asset)) if !asset.id.is_empty() => asset.id,
                _ => String::from_utf8_lossy(&inject_info.token_id).to_string(),
            }
        } else {
            String::from_utf8_lossy(&inject_info.token_id).to_string()
        };

        let another_token_id_str = if is_trx(&another_token_id) {
            String::new()
        } else if allow_same_token_name == 0 {
            match storage_adapter.get_asset_issue(&another_token_id, 0) {
                Ok(Some(asset)) if !asset.id.is_empty() => asset.id,
                _ => String::from_utf8_lossy(&another_token_id).to_string(),
            }
        } else {
            String::from_utf8_lossy(&another_token_id).to_string()
        };

        // Deduct token
        if is_trx(&inject_info.token_id) {
            updated_account.balance -= inject_info.quant;
        } else {
            // Import from AccountAssetStore if ALLOW_ASSET_OPTIMIZATION == 1
            Self::import_asset_if_optimized(
                storage_adapter,
                &mut updated_account,
                &inject_info.token_id,
                &owner,
            )?;
            Self::reduce_asset_amount_v2(
                &mut updated_account,
                &inject_info.token_id,
                &token_id_str,
                inject_info.quant,
                allow_same_token_name,
            )?;
        }

        // Deduct another token
        if is_trx(&another_token_id) {
            updated_account.balance -= another_token_quant;
        } else {
            // Import from AccountAssetStore if ALLOW_ASSET_OPTIMIZATION == 1
            Self::import_asset_if_optimized(
                storage_adapter,
                &mut updated_account,
                &another_token_id,
                &owner,
            )?;
            Self::reduce_asset_amount_v2(
                &mut updated_account,
                &another_token_id,
                &another_token_id_str,
                another_token_quant,
                allow_same_token_name,
            )?;
        }

        // Update exchange (dual-write when allowSameTokenName=0)
        // Java: Commons.putExchangeCapsule() writes to both v1 and v2 in legacy mode
        let mut updated_exchange = exchange.clone();
        updated_exchange.first_token_balance = new_first_balance;
        updated_exchange.second_token_balance = new_second_balance;
        storage_adapter
            .put_exchange_dual_write(&updated_exchange, allow_same_token_name)
            .map_err(|e| format!("Failed to update exchange: {}", e))?;

        // Update account
        storage_adapter
            .set_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to update account: {}", e))?;

        // 6. Build result - ExchangeInject
        let account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(updated_account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        let state_changes = vec![TronStateChange::AccountChange {
            address: owner,
            old_account: Some(revm_primitives::AccountInfo {
                balance: revm_primitives::U256::from(account.balance as u64),
                nonce: 0,
                code_hash: revm_primitives::B256::ZERO,
                code: None,
            }),
            new_account: Some(account_info),
        }];

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Build receipt with inject_another_amount
        let receipt = TransactionResultBuilder::new()
            .with_exchange_inject_another_amount(another_token_quant)
            .build();

        debug!(
            "ExchangeInject: injected {} of token, calculated {} of another token",
            inject_info.quant, another_token_quant
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: Some(receipt),
            contract_address: None,
        })
    }

    /// Execute EXCHANGE_WITHDRAW_CONTRACT (type 43)
    /// Withdraws liquidity from an exchange (creator only)
    ///
    /// Java reference: ExchangeWithdrawActuator.java
    fn execute_exchange_withdraw_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use contracts::exchange::{
            calculate_withdraw_another_amount, is_number, is_trx, is_withdraw_precise_enough,
        };
        use contracts::proto::TransactionResultBuilder;

        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.ExchangeWithdrawContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [ExchangeWithdrawContract],real type[class com.google.protobuf.Any]",
        )?;

        debug!(
            "Executing EXCHANGE_WITHDRAW_CONTRACT: owner={:?}",
            transaction.from
        );

        // 1. Parse contract data
        let withdraw_info = self.parse_exchange_withdraw_contract(contract_bytes)?;
        debug!(
            "Parsed ExchangeWithdraw: exchange_id={}, token={}, quant={}",
            withdraw_info.exchange_id,
            String::from_utf8_lossy(&withdraw_info.token_id),
            withdraw_info.quant
        );

        // 2. Validate owner address (Java: DecodeUtil.addressValid)
        let address_prefix = storage_adapter.address_prefix();
        if withdraw_info.owner_address.len() != 21
            || withdraw_info.owner_address[0] != address_prefix
        {
            return Err("Invalid address".to_string());
        }

        // 3. Get owner account
        // Java: ExchangeWithdrawActuator.validate() - "account[<hex>] not exists"
        let owner = transaction.from;
        let owner_tron = storage_adapter.to_tron_address_21(&owner).to_vec();
        let account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("account[{}] not exists", hex::encode(&owner_tron)))?;

        // 4. Get exchange (routed by allowSameTokenName)
        // Java: Commons.getExchangeStoreFinal() - reads from v1 when allowSameTokenName=0, v2 otherwise
        let allow_same_token_name = storage_adapter
            .get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;
        let exchange = storage_adapter
            .get_exchange_routed(withdraw_info.exchange_id, allow_same_token_name)
            .map_err(|e| format!("Failed to get exchange: {}", e))?
            .ok_or(format!(
                "Exchange[{}] not exists",
                withdraw_info.exchange_id
            ))?;

        // 5. Validate
        // - Must be creator
        if owner_tron != exchange.creator_address {
            return Err(format!(
                "account[{}] is not creator",
                hex::encode(&owner_tron)
            ));
        }

        // - Token ID format validation
        if allow_same_token_name == 1
            && !is_trx(&withdraw_info.token_id)
            && !is_number(&withdraw_info.token_id)
        {
            return Err("token id is not a valid number".to_string());
        }

        // - Token must be in exchange
        let is_first_token = withdraw_info.token_id == exchange.first_token_id;
        let is_second_token = withdraw_info.token_id == exchange.second_token_id;
        if !is_first_token && !is_second_token {
            return Err("token is not in exchange".to_string());
        }

        // - Quant must be positive
        if withdraw_info.quant <= 0 {
            return Err("withdraw token quant must greater than zero".to_string());
        }

        // - Exchange must not be closed
        if exchange.first_token_balance == 0 || exchange.second_token_balance == 0 {
            return Err(
                "Token balance in exchange is equal with 0,the exchange has been closed"
                    .to_string(),
            );
        }

        // Calculate another token amount and validate
        let (
            another_token_id,
            another_token_quant,
            new_first_balance,
            new_second_balance,
            token_balance,
            other_balance,
        ) = if is_first_token {
            let another_quant = calculate_withdraw_another_amount(
                exchange.first_token_balance,
                exchange.second_token_balance,
                withdraw_info.quant,
            );
            (
                exchange.second_token_id.clone(),
                another_quant,
                exchange.first_token_balance - withdraw_info.quant,
                exchange.second_token_balance - another_quant,
                exchange.first_token_balance,
                exchange.second_token_balance,
            )
        } else {
            let another_quant = calculate_withdraw_another_amount(
                exchange.second_token_balance,
                exchange.first_token_balance,
                withdraw_info.quant,
            );
            (
                exchange.first_token_id.clone(),
                another_quant,
                exchange.first_token_balance - another_quant,
                exchange.second_token_balance - withdraw_info.quant,
                exchange.second_token_balance,
                exchange.first_token_balance,
            )
        };

        // - Exchange balance sufficient
        if new_first_balance < 0 || new_second_balance < 0 {
            return Err("exchange balance is not enough".to_string());
        }

        // - Another quant must be positive
        if another_token_quant <= 0 {
            return Err("withdraw another token quant must greater than zero".to_string());
        }

        // - Precision check
        if !is_withdraw_precise_enough(token_balance, other_balance, withdraw_info.quant) {
            return Err("Not precise enough".to_string());
        }

        // 5. Execute
        let mut updated_account = account.clone();

        let token_id_str = if is_trx(&withdraw_info.token_id) {
            String::new()
        } else if allow_same_token_name == 0 {
            match storage_adapter.get_asset_issue(&withdraw_info.token_id, 0) {
                Ok(Some(asset)) if !asset.id.is_empty() => asset.id,
                _ => String::from_utf8_lossy(&withdraw_info.token_id).to_string(),
            }
        } else {
            String::from_utf8_lossy(&withdraw_info.token_id).to_string()
        };

        let another_token_id_str = if is_trx(&another_token_id) {
            String::new()
        } else if allow_same_token_name == 0 {
            match storage_adapter.get_asset_issue(&another_token_id, 0) {
                Ok(Some(asset)) if !asset.id.is_empty() => asset.id,
                _ => String::from_utf8_lossy(&another_token_id).to_string(),
            }
        } else {
            String::from_utf8_lossy(&another_token_id).to_string()
        };

        // Add token to account
        if is_trx(&withdraw_info.token_id) {
            updated_account.balance += withdraw_info.quant;
        } else {
            // Import from AccountAssetStore if ALLOW_ASSET_OPTIMIZATION == 1
            Self::import_asset_if_optimized(
                storage_adapter,
                &mut updated_account,
                &withdraw_info.token_id,
                &owner,
            )?;
            Self::add_asset_amount_v2(
                &mut updated_account,
                &withdraw_info.token_id,
                &token_id_str,
                withdraw_info.quant,
                allow_same_token_name,
            )?;
        }

        // Add another token to account
        if is_trx(&another_token_id) {
            updated_account.balance += another_token_quant;
        } else {
            // Import from AccountAssetStore if ALLOW_ASSET_OPTIMIZATION == 1
            Self::import_asset_if_optimized(
                storage_adapter,
                &mut updated_account,
                &another_token_id,
                &owner,
            )?;
            Self::add_asset_amount_v2(
                &mut updated_account,
                &another_token_id,
                &another_token_id_str,
                another_token_quant,
                allow_same_token_name,
            )?;
        }

        // Update exchange (dual-write when allowSameTokenName=0)
        // Java: Commons.putExchangeCapsule() writes to both v1 and v2 in legacy mode
        let mut updated_exchange = exchange.clone();
        updated_exchange.first_token_balance = new_first_balance;
        updated_exchange.second_token_balance = new_second_balance;
        storage_adapter
            .put_exchange_dual_write(&updated_exchange, allow_same_token_name)
            .map_err(|e| format!("Failed to update exchange: {}", e))?;

        // Update account
        storage_adapter
            .set_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to update account: {}", e))?;

        // 6. Build result - ExchangeWithdraw
        let account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(updated_account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        let state_changes = vec![TronStateChange::AccountChange {
            address: owner,
            old_account: Some(revm_primitives::AccountInfo {
                balance: revm_primitives::U256::from(account.balance as u64),
                nonce: 0,
                code_hash: revm_primitives::B256::ZERO,
                code: None,
            }),
            new_account: Some(account_info),
        }];

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Build receipt with withdraw_another_amount
        let receipt = TransactionResultBuilder::new()
            .with_exchange_withdraw_another_amount(another_token_quant)
            .build();

        debug!(
            "ExchangeWithdraw: withdrew {} of token, plus {} of another token",
            withdraw_info.quant, another_token_quant
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: Some(receipt),
            contract_address: None,
        })
    }

    /// Execute EXCHANGE_TRANSACTION_CONTRACT (type 44)
    /// Executes a token swap using the Bancor AMM formula
    ///
    /// Java reference: ExchangeTransactionActuator.java
    fn execute_exchange_transaction_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use contracts::exchange::{is_number, is_trx, ExchangeProcessor};
        use contracts::proto::TransactionResultBuilder;

        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.ExchangeTransactionContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [ExchangeTransactionContract],real type[class com.google.protobuf.Any]",
        )?;

        debug!(
            "Executing EXCHANGE_TRANSACTION_CONTRACT: owner={:?}",
            transaction.from
        );

        // 1. Parse contract data
        let tx_info = self.parse_exchange_transaction_contract(contract_bytes)?;
        debug!(
            "Parsed ExchangeTransaction: exchange_id={}, token={}, quant={}, expected={}",
            tx_info.exchange_id,
            String::from_utf8_lossy(&tx_info.token_id),
            tx_info.quant,
            tx_info.expected
        );

        // 2. Validate owner address (Java: DecodeUtil.addressValid)
        // Must be 21 bytes with correct network prefix
        let address_prefix = storage_adapter.address_prefix();
        if tx_info.owner_address.len() != 21 || tx_info.owner_address[0] != address_prefix {
            return Err("Invalid address".to_string());
        }

        // 3. Get owner account
        // Java: ExchangeTransactionActuator.validate() - "account[<hex>] not exists"
        let owner = transaction.from;
        let owner_tron = storage_adapter.to_tron_address_21(&owner).to_vec();
        let account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("account[{}] not exists", hex::encode(&owner_tron)))?;

        // 4. Get exchange and properties (routed by allowSameTokenName)
        // Java: Commons.getExchangeStoreFinal() - reads from v1 when allowSameTokenName=0, v2 otherwise
        let allow_same_token_name = storage_adapter
            .get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;
        let use_strict_math = storage_adapter
            .allow_strict_math()
            .map_err(|e| format!("Failed to get allowStrictMath: {}", e))?;
        let mut exchange = storage_adapter
            .get_exchange_routed(tx_info.exchange_id, allow_same_token_name)
            .map_err(|e| format!("Failed to get exchange: {}", e))?
            .ok_or(format!("Exchange[{}] not exists", tx_info.exchange_id))?;

        // 5. Validate
        // - Token ID format validation
        if allow_same_token_name == 1 && !is_trx(&tx_info.token_id) && !is_number(&tx_info.token_id)
        {
            return Err("token id is not a valid number".to_string());
        }

        // - Token must be in exchange
        let is_first_token = tx_info.token_id == exchange.first_token_id;
        let is_second_token = tx_info.token_id == exchange.second_token_id;
        if !is_first_token && !is_second_token {
            return Err("token is not in exchange".to_string());
        }

        // - Quant must be positive
        if tx_info.quant <= 0 {
            return Err("token quant must greater than zero".to_string());
        }

        // - Expected must be positive
        if tx_info.expected <= 0 {
            return Err("token expected must greater than zero".to_string());
        }

        // - Exchange must not be closed
        if exchange.first_token_balance == 0 || exchange.second_token_balance == 0 {
            return Err(
                "Token balance in exchange is equal with 0,the exchange has been closed"
                    .to_string(),
            );
        }

        // - Balance limit check
        let balance_limit = storage_adapter
            .get_exchange_balance_limit()
            .map_err(|e| format!("Failed to get balance limit: {}", e))?;
        let token_balance = if is_first_token {
            exchange.first_token_balance
        } else {
            exchange.second_token_balance
        };
        if token_balance + tx_info.quant > balance_limit {
            return Err(format!("token balance must less than {}", balance_limit));
        }

        // - Sufficient balance for token
        // Java: AccountCapsule.assetBalanceEnoughV2() routes by allowSameTokenName
        if is_trx(&tx_info.token_id) {
            if account.balance < tx_info.quant {
                return Err("balance is not enough".to_string());
            }
        } else {
            let balance = storage_adapter
                .get_asset_balance_routed(&owner, &tx_info.token_id, allow_same_token_name)
                .map_err(|e| format!("Failed to get token balance: {}", e))?;
            if balance < tx_info.quant {
                return Err("token balance is not enough".to_string());
            }
        }

        // Calculate received amount using AMM
        let (another_token_id, another_token_quant) = {
            let mut processor = ExchangeProcessor::new(use_strict_math);

            let (sell_balance, buy_balance, another_id) = if is_first_token {
                (
                    exchange.first_token_balance,
                    exchange.second_token_balance,
                    exchange.second_token_id.clone(),
                )
            } else {
                (
                    exchange.second_token_balance,
                    exchange.first_token_balance,
                    exchange.first_token_id.clone(),
                )
            };

            let buy_quant = processor.exchange(sell_balance, buy_balance, tx_info.quant);
            (another_id, buy_quant)
        };

        // - Check expected amount
        if another_token_quant < tx_info.expected {
            return Err("token required must greater than expected".to_string());
        }

        // 6. Execute
        let mut updated_account = account.clone();

        let token_id_str = if is_trx(&tx_info.token_id) {
            String::new()
        } else if allow_same_token_name == 0 {
            match storage_adapter.get_asset_issue(&tx_info.token_id, 0) {
                Ok(Some(asset)) if !asset.id.is_empty() => asset.id,
                _ => String::from_utf8_lossy(&tx_info.token_id).to_string(),
            }
        } else {
            String::from_utf8_lossy(&tx_info.token_id).to_string()
        };

        let another_token_id_str = if is_trx(&another_token_id) {
            String::new()
        } else if allow_same_token_name == 0 {
            match storage_adapter.get_asset_issue(&another_token_id, 0) {
                Ok(Some(asset)) if !asset.id.is_empty() => asset.id,
                _ => String::from_utf8_lossy(&another_token_id).to_string(),
            }
        } else {
            String::from_utf8_lossy(&another_token_id).to_string()
        };

        // Deduct sold token
        if is_trx(&tx_info.token_id) {
            updated_account.balance -= tx_info.quant;
        } else {
            // Import from AccountAssetStore if ALLOW_ASSET_OPTIMIZATION == 1
            Self::import_asset_if_optimized(
                storage_adapter,
                &mut updated_account,
                &tx_info.token_id,
                &owner,
            )?;
            Self::reduce_asset_amount_v2(
                &mut updated_account,
                &tx_info.token_id,
                &token_id_str,
                tx_info.quant,
                allow_same_token_name,
            )?;
        }

        // Add bought token
        if is_trx(&another_token_id) {
            updated_account.balance += another_token_quant;
        } else {
            // Import from AccountAssetStore if ALLOW_ASSET_OPTIMIZATION == 1
            Self::import_asset_if_optimized(
                storage_adapter,
                &mut updated_account,
                &another_token_id,
                &owner,
            )?;
            Self::add_asset_amount_v2(
                &mut updated_account,
                &another_token_id,
                &another_token_id_str,
                another_token_quant,
                allow_same_token_name,
            )?;
        }

        // Update exchange balances (dual-write when allowSameTokenName=0)
        // Java: Commons.putExchangeCapsule() writes to both v1 and v2 in legacy mode
        if is_first_token {
            exchange.first_token_balance += tx_info.quant;
            exchange.second_token_balance -= another_token_quant;
        } else {
            exchange.first_token_balance -= another_token_quant;
            exchange.second_token_balance += tx_info.quant;
        }
        storage_adapter
            .put_exchange_dual_write(&exchange, allow_same_token_name)
            .map_err(|e| format!("Failed to update exchange: {}", e))?;

        // Update account
        storage_adapter
            .set_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to update account: {}", e))?;

        // 7. Build result - ExchangeTransaction
        let account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(updated_account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        let state_changes = vec![TronStateChange::AccountChange {
            address: owner,
            old_account: Some(revm_primitives::AccountInfo {
                balance: revm_primitives::U256::from(account.balance as u64),
                nonce: 0,
                code_hash: revm_primitives::B256::ZERO,
                code: None,
            }),
            new_account: Some(account_info),
        }];

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Build receipt with received_amount
        let receipt = TransactionResultBuilder::new()
            .with_exchange_received_amount(another_token_quant)
            .build();

        debug!(
            "ExchangeTransaction: sold {} of token, received {} of another token",
            tx_info.quant, another_token_quant
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: Some(receipt),
            contract_address: None,
        })
    }

    /// Parse ExchangeCreateContract protobuf bytes
    fn parse_exchange_create_contract(&self, data: &[u8]) -> Result<ExchangeCreateInfo, String> {
        let mut owner_address = Vec::new();
        let mut first_token_id = Vec::new();
        let mut first_token_balance: i64 = 0;
        let mut second_token_id = Vec::new();
        let mut second_token_balance: i64 = 0;

        let mut pos = 0;
        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match field_number {
                // owner_address = 1
                1 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for owner_address".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address = payload.to_vec();
                    pos += total_len;
                }
                // first_token_id = 2
                2 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for first_token_id".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    first_token_id = payload.to_vec();
                    pos += total_len;
                }
                // first_token_balance = 3
                3 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for first_token_balance".to_string());
                    }
                    let (val, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += new_pos;
                    first_token_balance = val as i64;
                }
                // second_token_id = 4
                4 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for second_token_id".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    second_token_id = payload.to_vec();
                    pos += total_len;
                }
                // second_token_balance = 5
                5 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for second_token_balance".to_string());
                    }
                    let (val, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += new_pos;
                    second_token_balance = val as i64;
                }
                _ => {
                    // Skip unknown fields
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        Ok(ExchangeCreateInfo {
            owner_address,
            first_token_id,
            first_token_balance,
            second_token_id,
            second_token_balance,
        })
    }

    /// Parse ExchangeInjectContract protobuf bytes
    fn parse_exchange_inject_contract(&self, data: &[u8]) -> Result<ExchangeInjectInfo, String> {
        let mut owner_address = Vec::new();
        let mut exchange_id: i64 = 0;
        let mut token_id = Vec::new();
        let mut quant: i64 = 0;

        let mut pos = 0;
        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match field_number {
                // owner_address = 1
                1 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for owner_address".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address = payload.to_vec();
                    pos += total_len;
                }
                // exchange_id = 2
                2 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for exchange_id".to_string());
                    }
                    let (val, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += new_pos;
                    exchange_id = val as i64;
                }
                // token_id = 3
                3 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for token_id".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    token_id = payload.to_vec();
                    pos += total_len;
                }
                // quant = 4
                4 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for quant".to_string());
                    }
                    let (val, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += new_pos;
                    quant = val as i64;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        Ok(ExchangeInjectInfo {
            owner_address,
            exchange_id,
            token_id,
            quant,
        })
    }

    /// Parse ExchangeWithdrawContract protobuf bytes
    fn parse_exchange_withdraw_contract(
        &self,
        data: &[u8],
    ) -> Result<ExchangeWithdrawInfo, String> {
        // Same structure as inject
        let inject_info = self.parse_exchange_inject_contract(data)?;
        Ok(ExchangeWithdrawInfo {
            owner_address: inject_info.owner_address,
            exchange_id: inject_info.exchange_id,
            token_id: inject_info.token_id,
            quant: inject_info.quant,
        })
    }

    /// Parse ExchangeTransactionContract protobuf bytes
    fn parse_exchange_transaction_contract(
        &self,
        data: &[u8],
    ) -> Result<ExchangeTransactionInfo, String> {
        let mut owner_address = Vec::new();
        let mut exchange_id: i64 = 0;
        let mut token_id = Vec::new();
        let mut quant: i64 = 0;
        let mut expected: i64 = 0;

        let mut pos = 0;
        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match field_number {
                // owner_address = 1
                1 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for owner_address".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address = payload.to_vec();
                    pos += total_len;
                }
                // exchange_id = 2
                2 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for exchange_id".to_string());
                    }
                    let (val, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += new_pos;
                    exchange_id = val as i64;
                }
                // token_id = 3
                3 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for token_id".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    token_id = payload.to_vec();
                    pos += total_len;
                }
                // quant = 4
                4 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for quant".to_string());
                    }
                    let (val, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += new_pos;
                    quant = val as i64;
                }
                // expected = 5
                5 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for expected".to_string());
                    }
                    let (val, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += new_pos;
                    expected = val as i64;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        Ok(ExchangeTransactionInfo {
            owner_address,
            exchange_id,
            token_id,
            quant,
            expected,
        })
    }

    // ==========================================================================
    // Phase 2.G: Market (DEX) Contracts (52/53)
    // ==========================================================================

    /// Execute MARKET_CANCEL_ORDER_CONTRACT (type 53)
    ///
    /// Cancels an existing active order and returns remaining tokens to the owner.
    ///
    /// Implementation matches Java: MarketCancelOrderActuator.java
    ///
    /// Validation:
    /// - Market transaction must be enabled (ALLOW_MARKET_TRANSACTION)
    /// - Order must exist
    /// - Order must be active
    /// - Owner must match
    /// - Sufficient balance for fee
    ///
    /// Execution:
    /// 1. Charge fee (to blackhole or burn)
    /// 2. Return remaining sell tokens to owner
    /// 3. Update order state to CANCELED
    /// 4. Remove order from order book (linked list + price index)
    fn execute_market_cancel_order_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        debug!("Executing MARKET_CANCEL_ORDER_CONTRACT");

        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.MarketCancelOrderContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [MarketCancelOrderContract],real type[class com.google.protobuf.Any]",
        )?;

        // Parse the contract
        let MarketCancelOrderInfo {
            owner_address,
            order_id,
        } = self.parse_market_cancel_order_contract(contract_bytes)?;
        debug!("MarketCancelOrder: order_id={:?}", hex::encode(&order_id));

        // 1. Validate: market transactions must be enabled
        let allow_market = storage_adapter
            .allow_market_transaction()
            .map_err(|e| format!("Failed to check ALLOW_MARKET_TRANSACTION: {}", e))?;
        if !allow_market {
            return Err(
                "Not support Market Transaction, need to be opened by the committee".to_string(),
            );
        }

        // 2. Validate: owner address
        let address_prefix = storage_adapter.address_prefix();
        if owner_address.len() != 21 || owner_address[0] != address_prefix {
            return Err("Invalid address".to_string());
        }
        let owner = revm_primitives::Address::from_slice(&owner_address[1..]);

        // 3. Whether the account exist
        let mut account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or("Account does not exist!")?;

        // 4. Whether the order exist
        let order = storage_adapter
            .get_market_order(&order_id)
            .map_err(|e| format!("Failed to get order: {}", e))?
            .ok_or("orderId not exists")?;

        // 5. Validate: order must be active
        if order.state != 0 {
            // 0 = ACTIVE
            return Err("Order is not active!".to_string());
        }

        // 6. Validate: order owner must match
        let order_owner = Self::market_owner_address(&order.owner_address)?;
        if order_owner != owner {
            return Err("Order does not belong to the account!".to_string());
        }

        let fee = storage_adapter
            .get_market_cancel_fee()
            .map_err(|e| format!("Failed to get MARKET_CANCEL_FEE: {}", e))?;

        if account.balance < fee {
            return Err("No enough balance !".to_string());
        }

        // 6. Deduct fee
        let old_balance = account.balance;
        account.balance = account
            .balance
            .checked_sub(fee)
            .ok_or("Balance underflow")?;

        // Handle fee: burn or credit to blackhole
        let support_blackhole = storage_adapter
            .support_black_hole_optimization()
            .map_err(|e| format!("Failed to check ALLOW_BLACKHOLE_OPTIMIZATION: {}", e))?;
        let state_changes = if fee > 0 {
            if support_blackhole {
                storage_adapter
                    .burn_trx(fee as u64)
                    .map_err(|e| format!("Failed to burn TRX: {}", e))?;
            } else {
                let blackhole = storage_adapter.get_blackhole_address_evm();
                storage_adapter
                    .add_balance(&blackhole, fee as u64)
                    .map_err(|e| format!("Failed to credit blackhole: {}", e))?;
            }
            vec![]
        } else {
            vec![]
        };

        // 7. Return remaining sell tokens to owner
        // This mirrors Java's MarketUtils.returnSellTokenRemain() which calls
        // AccountCapsule.addAssetAmountV2()
        let sell_token_remain = order.sell_token_quantity_remain;
        if sell_token_remain > 0 {
            let sell_token_id = &order.sell_token_id;
            if sell_token_id == b"_" || sell_token_id.is_empty() {
                // TRX
                account.balance = account
                    .balance
                    .checked_add(sell_token_remain)
                    .ok_or("Balance overflow")?;
            } else {
                // TRC-10 token
                // Java's addAssetAmountV2 behavior depends on ALLOW_SAME_TOKEN_NAME:
                // - When 0: key is token name, updates both asset[name] and assetV2[id]
                // - When 1: key is token id, updates only assetV2[id]
                let allow_same_token_name = storage_adapter
                    .get_allow_same_token_name()
                    .map_err(|e| format!("Failed to get ALLOW_SAME_TOKEN_NAME: {}", e))?;

                if allow_same_token_name == 0 {
                    // Legacy mode: key is token name
                    // Look up asset by name to get numeric ID
                    let asset_opt = storage_adapter
                        .get_asset_issue(sell_token_id, 0)
                        .map_err(|e| format!("Failed to get asset issue: {}", e))?;

                    if let Some(asset) = asset_opt {
                        let token_name = String::from_utf8_lossy(sell_token_id).to_string();
                        let token_id = asset.id.clone(); // numeric ID string

                        // Update asset[name] (legacy map)
                        let current_name = account.asset.get(&token_name).copied().unwrap_or(0);
                        let updated_name = current_name
                            .checked_add(sell_token_remain)
                            .ok_or("Token balance overflow (asset)")?;
                        account.asset.insert(token_name, updated_name);

                        // Update assetV2[id] (modern map)
                        let current_id = account.asset_v2.get(&token_id).copied().unwrap_or(0);
                        let updated_id = current_id
                            .checked_add(sell_token_remain)
                            .ok_or("Token balance overflow (assetV2)")?;
                        account.asset_v2.insert(token_id, updated_id);
                    } else {
                        // Asset not found - fallback to treating key as ID (defensive)
                        let token_key = String::from_utf8_lossy(sell_token_id).to_string();
                        let current = account.asset_v2.get(&token_key).copied().unwrap_or(0);
                        let updated = current
                            .checked_add(sell_token_remain)
                            .ok_or("Token balance overflow")?;
                        account.asset_v2.insert(token_key, updated);
                    }
                } else {
                    // Modern mode: key is token id, only update assetV2[id]
                    let token_key = String::from_utf8_lossy(sell_token_id).to_string();
                    let current = account.asset_v2.get(&token_key).copied().unwrap_or(0);
                    let updated = current
                        .checked_add(sell_token_remain)
                        .ok_or("Token balance overflow")?;
                    account.asset_v2.insert(token_key, updated);
                }
            }
        }

        // 8. Update order state to CANCELED (2)
        let mut updated_order = order.clone();
        updated_order.state = 2; // CANCELED
        updated_order.sell_token_quantity_remain = 0;

        // Get strict parity mode flag
        let execution_config = self.get_execution_config()?;
        let strict_parity = execution_config.remote.market_strict_index_parity;

        // 9. Update MarketAccountOrder (remove order from account's list)
        // Java: MarketUtils.updateOrderState() calls marketAccountStore.get(ownerAddress) which
        // throws ItemNotFoundException if missing. Rust behavior depends on strict_parity flag.
        let account_order_opt = storage_adapter
            .get_market_account_order(&owner)
            .map_err(|e| format!("Failed to get account order: {}", e))?;

        if let Some(mut account_order) = account_order_opt {
            // Remove order_id from the list - match Java's single-removal semantics
            // Java's List.remove(orderId) removes only the first occurrence
            if let Some(pos) = account_order.orders.iter().position(|id| id == &order_id) {
                account_order.orders.remove(pos);
            }
            account_order.count -= 1;
            storage_adapter
                .put_market_account_order(&owner, &account_order)
                .map_err(|e| format!("Failed to update account order: {}", e))?;
        } else if strict_parity {
            // Java throws ItemNotFoundException when MarketAccountStore.get(owner) is missing
            return Err(format!("MarketAccountOrder not found for owner"));
        }
        // else: defensive recovery mode - skip removal if not found

        // 10. Remove from order book linked list
        let pair_price_key = Self::create_pair_price_key(
            &order.sell_token_id,
            &order.buy_token_id,
            order.sell_token_quantity,
            order.buy_token_quantity,
        )?;

        let order_list_opt = storage_adapter
            .get_market_order_id_list(&pair_price_key)
            .map_err(|e| format!("Failed to get order list: {}", e))?;

        if let Some(mut order_list) = order_list_opt {
            // Handle linked list removal
            self.remove_order_from_linked_list(
                storage_adapter,
                &mut order_list,
                &updated_order,
                &pair_price_key,
                strict_parity,
            )?;

            // Update or delete the order list
            if order_list.head.is_empty() {
                // List is empty, delete the price key
                storage_adapter
                    .delete_market_order_id_list(&pair_price_key)
                    .map_err(|e| format!("Failed to delete order list: {}", e))?;

                // Decrease price count for the pair
                let pair_key = Self::create_pair_key(&order.sell_token_id, &order.buy_token_id)?;
                let price_count = storage_adapter
                    .get_market_pair_price_count(&pair_key)
                    .map_err(|e| format!("Failed to get price count: {}", e))?;

                if price_count <= 1 {
                    // Delete the pair
                    storage_adapter
                        .delete_market_pair(&pair_key)
                        .map_err(|e| format!("Failed to delete pair: {}", e))?;
                } else {
                    storage_adapter
                        .set_market_pair_price_count(&pair_key, price_count - 1)
                        .map_err(|e| format!("Failed to update price count: {}", e))?;
                }
            } else {
                storage_adapter
                    .put_market_order_id_list(&pair_price_key, &order_list)
                    .map_err(|e| format!("Failed to update order list: {}", e))?;
            }
        } else if strict_parity {
            // Java throws ItemNotFoundException when pairPriceToOrderStore.get(pairPriceKey) is missing
            return Err(format!("MarketOrderIdList not found for pairPriceKey"));
        }
        // else: defensive recovery mode - skip orderbook removal if list not found

        // Keep `updated_order` consistent with what was persisted during linked-list removal.
        updated_order.prev = vec![];
        updated_order.next = vec![];

        // 11. Save order and account
        storage_adapter
            .put_market_order(&order_id, &updated_order)
            .map_err(|e| format!("Failed to update order: {}", e))?;
        storage_adapter
            .set_account_proto(&owner, &account)
            .map_err(|e| format!("Failed to update account: {}", e))?;

        // 12. Build result
        let account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        let mut final_state_changes = vec![TronStateChange::AccountChange {
            address: owner,
            old_account: Some(revm_primitives::AccountInfo {
                balance: revm_primitives::U256::from(old_balance as u64),
                nonce: 0,
                code_hash: revm_primitives::B256::ZERO,
                code: None,
            }),
            new_account: Some(account_info),
        }];
        final_state_changes.extend(state_changes);

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!(
            "MarketCancelOrder: order canceled, returned {} sell tokens",
            sell_token_remain
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes: final_state_changes,
            logs: Vec::new(),
            error: None,
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

    /// Execute MARKET_SELL_ASSET_CONTRACT (type 52)
    ///
    /// Creates a sell order and matches against existing orders.
    ///
    /// Implementation matches Java: MarketSellAssetActuator.java
    ///
    /// This contract includes order matching, MAX_MATCH_NUM limits, and price-queue cleanup.
    fn execute_market_sell_asset_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        debug!("Executing MARKET_SELL_ASSET_CONTRACT");

        // 0. Validate contract parameter presence and type (strict Java parity)
        let contract_bytes = Self::require_contract_parameter(
            transaction,
            "protocol.MarketSellAssetContract",
            Self::CONTRACT_NOT_EXIST,
            "contract type error,expected type [MarketSellAssetContract],real type[class com.google.protobuf.Any]",
        )?;

        // Parse the contract
        let tx_info = self.parse_market_sell_asset_contract(contract_bytes)?;
        debug!(
            "MarketSellAsset: owner={:?}, sell_token={:?}, sell_qty={}, buy_token={:?}, buy_qty={}",
            hex::encode(&tx_info.owner_address),
            String::from_utf8_lossy(&tx_info.sell_token_id),
            tx_info.sell_token_quantity,
            String::from_utf8_lossy(&tx_info.buy_token_id),
            tx_info.buy_token_quantity
        );

        // 1. Validate: market transactions must be enabled
        let allow_market = storage_adapter
            .allow_market_transaction()
            .map_err(|e| format!("Failed to check ALLOW_MARKET_TRANSACTION: {}", e))?;
        if !allow_market {
            return Err(
                "Not support Market Transaction, need to be opened by the committee".to_string(),
            );
        }

        // 2. Validate owner address from contract (java-tron: DecodeUtil.addressValid)
        // Java validates contract.owner_address, not transaction metadata
        let prefix = storage_adapter.address_prefix();
        if tx_info.owner_address.len() != 21 || tx_info.owner_address[0] != prefix {
            return Err("Invalid address".to_string());
        }
        // Derive 20-byte EVM address from 21-byte TRON address
        let owner = revm_primitives::Address::from_slice(&tx_info.owner_address[1..]);

        // 3. Validate account exists (java-tron: AccountStore.get)
        let mut account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or("Account does not exist!")?;

        // 4. Validate token ID format (java-tron: TransactionUtil.isNumber)
        use contracts::exchange::{is_number, is_trx};

        if !is_trx(&tx_info.sell_token_id) && !is_number(&tx_info.sell_token_id) {
            return Err("sellTokenId is not a valid number".to_string());
        }
        if !is_trx(&tx_info.buy_token_id) && !is_number(&tx_info.buy_token_id) {
            return Err("buyTokenId is not a valid number".to_string());
        }

        // 5. Validate token IDs
        if tx_info.sell_token_id == tx_info.buy_token_id {
            return Err("cannot exchange same tokens".to_string());
        }

        // 6. Validate quantities
        if tx_info.sell_token_quantity <= 0 || tx_info.buy_token_quantity <= 0 {
            return Err("token quantity must greater than zero".to_string());
        }

        let quantity_limit = storage_adapter
            .get_market_quantity_limit()
            .map_err(|e| format!("Failed to get MARKET_QUANTITY_LIMIT: {}", e))?;
        if tx_info.sell_token_quantity > quantity_limit
            || tx_info.buy_token_quantity > quantity_limit
        {
            return Err(format!("token quantity must less than {}", quantity_limit));
        }

        // 7. Validate order count limit
        let max_active_orders: i64 = 100;
        if let Some(account_order) = storage_adapter
            .get_market_account_order(&owner)
            .map_err(|e| format!("Failed to get account order: {}", e))?
        {
            if account_order.count >= max_active_orders {
                return Err(format!(
                    "Maximum number of orders exceeded，{}",
                    max_active_orders
                ));
            }
        }

        // 8. Validate balance and token existence (java-tron: MarketSellAssetActuator.validate)
        let fee = storage_adapter
            .get_market_sell_fee()
            .map_err(|e| format!("Failed to get MARKET_SELL_FEE: {}", e))?;

        let is_sell_trx = is_trx(&tx_info.sell_token_id);
        let is_buy_trx = is_trx(&tx_info.buy_token_id);

        let allow_same_token_name = storage_adapter
            .get_allow_same_token_name()
            .map_err(|e| format!("Failed to get ALLOW_SAME_TOKEN_NAME: {}", e))?;

        if is_sell_trx {
            // Selling TRX: need sell_qty + fee
            let required = tx_info
                .sell_token_quantity
                .checked_add(fee)
                .ok_or("Amount overflow")?;
            if account.balance < required {
                return Err("No enough balance !".to_string());
            }
        } else {
            // Selling TRC-10: need fee in TRX + token balance
            if account.balance < fee {
                return Err("No enough balance !".to_string());
            }

            // Must exist in the AssetIssue store
            let sell_asset = storage_adapter
                .get_asset_issue(&tx_info.sell_token_id, allow_same_token_name)
                .map_err(|e| format!("Failed to get sell token: {}", e))?;
            if sell_asset.is_none() {
                return Err("No sellTokenId !".to_string());
            }

            let token_key = String::from_utf8_lossy(&tx_info.sell_token_id).to_string();
            let token_balance = account.asset_v2.get(&token_key).copied().unwrap_or(0);
            if token_balance < tx_info.sell_token_quantity {
                return Err("SellToken balance is not enough !".to_string());
            }
        }

        if !is_buy_trx {
            let buy_asset = storage_adapter
                .get_asset_issue(&tx_info.buy_token_id, allow_same_token_name)
                .map_err(|e| format!("Failed to get buy token: {}", e))?;
            if buy_asset.is_none() {
                return Err("No buyTokenId !".to_string());
            }
        }

        // 9. Deduct fee
        let old_balance = account.balance;
        account.balance = account
            .balance
            .checked_sub(fee)
            .ok_or("Balance underflow")?;

        // Handle fee: burn or credit to blackhole (matches Java parity)
        // Java behavior: MarketSellAssetActuator.execute reads ALLOW_BLACKHOLE_OPTIMIZATION
        // from DynamicPropertiesStore, not from static config.
        // When ALLOW_BLACKHOLE_OPTIMIZATION == 1: burnTrx(fee) updates BURN_TRX_AMOUNT
        // When ALLOW_BLACKHOLE_OPTIMIZATION == 0: credit blackhole account balance
        let support_blackhole = storage_adapter
            .support_black_hole_optimization()
            .map_err(|e| format!("Failed to check ALLOW_BLACKHOLE_OPTIMIZATION: {}", e))?;
        if fee > 0 {
            if support_blackhole {
                storage_adapter
                    .burn_trx(fee as u64)
                    .map_err(|e| format!("Failed to burn TRX: {}", e))?;
            } else {
                let blackhole = storage_adapter.get_blackhole_address_evm();
                storage_adapter
                    .add_balance(&blackhole, fee as u64)
                    .map_err(|e| format!("Failed to credit blackhole: {}", e))?;
            }
        }

        // 10. Transfer sell tokens from account to order (escrow)
        if is_sell_trx {
            account.balance = account
                .balance
                .checked_sub(tx_info.sell_token_quantity)
                .ok_or("Balance underflow")?;
        } else {
            let token_key = String::from_utf8_lossy(&tx_info.sell_token_id).to_string();
            let current = account.asset_v2.get(&token_key).copied().unwrap_or(0);
            account
                .asset_v2
                .insert(token_key, current - tx_info.sell_token_quantity);
        }

        // 11. Create order (persisted before matching, matching java-tron behavior)
        let owner_tron_addr = storage_adapter.to_tron_address_21(&owner).to_vec();
        let mut account_order = storage_adapter
            .get_market_account_order(&owner)
            .map_err(|e| format!("Failed to get account order: {}", e))?
            .unwrap_or_else(|| tron_backend_execution::protocol::MarketAccountOrder {
                owner_address: owner_tron_addr.clone(),
                orders: vec![],
                count: 0,
                total_count: 0,
            });

        let order_id = Self::calculate_order_id(
            &owner_tron_addr,
            &tx_info.sell_token_id,
            &tx_info.buy_token_id,
            account_order.total_count,
        )?;

        let timestamp = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get timestamp: {}", e))?;

        let mut order = tron_backend_execution::protocol::MarketOrder {
            order_id: order_id.clone(),
            owner_address: owner_tron_addr.clone(),
            create_time: timestamp,
            sell_token_id: tx_info.sell_token_id.clone(),
            sell_token_quantity: tx_info.sell_token_quantity,
            buy_token_id: tx_info.buy_token_id.clone(),
            buy_token_quantity: tx_info.buy_token_quantity,
            sell_token_quantity_remain: tx_info.sell_token_quantity,
            sell_token_quantity_return: 0,
            state: 0, // ACTIVE
            prev: vec![],
            next: vec![],
        };

        // 12. Update account order
        account_order.orders.push(order_id.clone());
        account_order.count += 1;
        account_order.total_count += 1;

        // 13. Save order + account-order (java-tron does this before matching).
        storage_adapter
            .put_market_order(&order_id, &order)
            .map_err(|e| format!("Failed to save order: {}", e))?;
        storage_adapter
            .put_market_account_order(&owner, &account_order)
            .map_err(|e| format!("Failed to save account order: {}", e))?;

        // 14. Match order (updates maker-side state as it goes) and collect fill details.
        let order_details =
            self.match_market_sell_order(storage_adapter, &mut order, &mut account)?;

        // 15. Save remain order into order book (only if still active with non-zero remain).
        if order.sell_token_quantity_remain != 0 {
            self.save_remain_market_order(storage_adapter, &mut order)?;
        }

        // 16. Persist final taker order + account.
        storage_adapter
            .put_market_order(&order_id, &order)
            .map_err(|e| format!("Failed to update order: {}", e))?;
        storage_adapter
            .set_account_proto(&owner, &account)
            .map_err(|e| format!("Failed to update account: {}", e))?;

        // 14. Build result
        let account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        let state_changes = vec![TronStateChange::AccountChange {
            address: owner,
            old_account: Some(revm_primitives::AccountInfo {
                balance: revm_primitives::U256::from(old_balance as u64),
                nonce: 0,
                code_hash: revm_primitives::B256::ZERO,
                code: None,
            }),
            new_account: Some(account_info),
        }];

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Build receipt with order_id and order details (Java parity)
        let receipt = TransactionResultBuilder::new()
            .with_order_id(&order_id)
            .with_order_details(order_details)
            .build();

        debug!(
            "MarketSellAsset: order created with id={}",
            hex::encode(&order_id)
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: Vec::new(),
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: Some(receipt),
            contract_address: None,
        })
    }

    /// Match taker order against maker orders and return fill details for receipt
    fn match_market_sell_order(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        taker_order: &mut tron_backend_execution::protocol::MarketOrder,
        taker_account: &mut tron_backend_execution::protocol::Account,
    ) -> Result<Vec<MarketOrderDetail>, String> {
        const MAX_MATCH_NUM: i32 = 20;

        // Collect order details for receipt
        let mut order_details: Vec<MarketOrderDetail> = Vec::new();

        // Get strict parity mode flag for missing-index behavior
        let execution_config = self.get_execution_config()?;
        let strict_parity = execution_config.remote.market_strict_index_parity;

        let maker_sell_token_id = taker_order.buy_token_id.clone();
        let maker_buy_token_id = taker_order.sell_token_id.clone();
        let maker_pair_key = Self::create_pair_key(&maker_sell_token_id, &maker_buy_token_id)?;

        let maker_price_number = storage_adapter
            .get_market_pair_price_count(&maker_pair_key)
            .map_err(|e| format!("Failed to get maker price count: {}", e))?;
        if maker_price_number == 0 {
            return Ok(order_details);
        }

        let mut remain_count = maker_price_number;
        let mut price_keys_list = self.market_get_price_keys_list(
            storage_adapter,
            &maker_sell_token_id,
            &maker_buy_token_id,
            (MAX_MATCH_NUM + 1) as i64,
            maker_price_number,
        )?;

        let mut match_order_count: i32 = 0;

        while taker_order.sell_token_quantity_remain != 0 {
            if !self.market_has_match(&price_keys_list, taker_order)? {
                return Ok(order_details);
            }

            let pair_price_key = match price_keys_list.first() {
                Some(key) => key.clone(),
                None => return Ok(order_details),
            };

            // Java: MarketPairPriceToOrderStore.get(pairPriceKey) throws if missing
            // when a price key exists but the order list is missing (corrupt index)
            let mut order_list = match storage_adapter
                .get_market_order_id_list(&pair_price_key)
                .map_err(|e| format!("Failed to get order list: {}", e))?
            {
                Some(list) => list,
                None => {
                    if strict_parity {
                        return Err(format!(
                            "MarketOrderIdList not found for price key (corrupt index): {}",
                            hex::encode(&pair_price_key)
                        ));
                    }
                    // Defensive recovery: skip this price level
                    return Ok(order_details);
                }
            };

            while taker_order.sell_token_quantity_remain != 0 && !order_list.head.is_empty() {
                let maker_order_id = order_list.head.clone();
                let mut maker_order = storage_adapter
                    .get_market_order(&maker_order_id)
                    .map_err(|e| format!("Failed to get maker order: {}", e))?
                    .ok_or("Maker order does not exist")?;

                // Match and collect order detail for receipt
                if let Some(detail) = self.market_match_single_order(
                    storage_adapter,
                    taker_order,
                    &mut maker_order,
                    taker_account,
                )? {
                    order_details.push(detail);
                }

                // Remove maker order from order book when fully consumed.
                if maker_order.sell_token_quantity_remain == 0 {
                    // For MARKET_SELL_ASSET, we use strict_parity=true since the order matching
                    // logic assumes the order book is consistent - a fully consumed order should
                    // have valid prev/next pointers.
                    self.remove_order_from_linked_list(
                        storage_adapter,
                        &mut order_list,
                        &maker_order,
                        &pair_price_key,
                        true, // strict parity for order matching
                    )?;

                    // Persist list updates even if it becomes empty (matches java-tron behavior).
                    storage_adapter
                        .put_market_order_id_list(&pair_price_key, &order_list)
                        .map_err(|e| format!("Failed to update order list: {}", e))?;
                }

                match_order_count += 1;
                if match_order_count > MAX_MATCH_NUM {
                    return Err(format!(
                        "Too many matches. MAX_MATCH_NUM = {}",
                        MAX_MATCH_NUM
                    ));
                }
            }

            // The orders at this price level have been all consumed.
            if order_list.head.is_empty() {
                storage_adapter
                    .delete_market_order_id_list(&pair_price_key)
                    .map_err(|e| format!("Failed to delete price key: {}", e))?;
                price_keys_list.remove(0);

                remain_count = remain_count
                    .checked_sub(1)
                    .ok_or("Market pair price count underflow")?;
                if remain_count == 0 {
                    storage_adapter
                        .delete_market_pair(&maker_pair_key)
                        .map_err(|e| format!("Failed to delete maker pair: {}", e))?;
                    break;
                }
                storage_adapter
                    .set_market_pair_price_count(&maker_pair_key, remain_count)
                    .map_err(|e| format!("Failed to update maker price count: {}", e))?;
            }
        }

        Ok(order_details)
    }

    /// Match a single order and return fill details for receipt
    /// Returns Some(detail) when there's a fill, None when quantity too small
    fn market_match_single_order(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        taker_order: &mut tron_backend_execution::protocol::MarketOrder,
        maker_order: &mut tron_backend_execution::protocol::MarketOrder,
        taker_account: &mut tron_backend_execution::protocol::Account,
    ) -> Result<Option<MarketOrderDetail>, String> {
        let taker_sell_remain = taker_order.sell_token_quantity_remain;
        let maker_sell_quantity = maker_order.sell_token_quantity;
        let maker_buy_quantity = maker_order.buy_token_quantity;
        let maker_sell_remain = maker_order.sell_token_quantity_remain;

        // According to the price of maker, calculate the quantity of taker can buy:
        // maker_sell_qty / maker_buy_qty = taker_buy_qty / taker_sell_remain
        let taker_buy_remain = self.market_multiply_and_divide(
            taker_sell_remain,
            maker_sell_quantity,
            maker_buy_quantity,
        )?;

        if taker_buy_remain == 0 {
            // Quantity too small, return sellToken to user.
            taker_order.sell_token_quantity_return = taker_order.sell_token_quantity_remain;
            self.market_return_sell_token_remain(taker_order, taker_account)?;
            self.market_update_order_state(storage_adapter, taker_order, 1)?;
            return Ok(None); // No fill occurred
        }

        let (taker_buy_receive, maker_buy_receive) = if taker_buy_remain == maker_sell_remain {
            // taker == maker
            let maker_buy_receive = self.market_multiply_and_divide(
                maker_sell_remain,
                maker_buy_quantity,
                maker_sell_quantity,
            )?;
            let taker_buy_receive = maker_sell_remain;

            let taker_sell_left = taker_order
                .sell_token_quantity_remain
                .checked_sub(maker_buy_receive)
                .ok_or("Balance underflow")?;
            taker_order.sell_token_quantity_remain = taker_sell_left;
            maker_order.sell_token_quantity_remain = 0;

            if taker_sell_left == 0 {
                self.market_update_order_state(storage_adapter, taker_order, 1)?;
            }
            self.market_update_order_state(storage_adapter, maker_order, 1)?;

            (taker_buy_receive, maker_buy_receive)
        } else if taker_buy_remain < maker_sell_remain {
            // taker < maker
            let taker_buy_receive = taker_buy_remain;
            let maker_buy_receive = taker_order.sell_token_quantity_remain;

            taker_order.sell_token_quantity_remain = 0;
            self.market_update_order_state(storage_adapter, taker_order, 1)?;

            maker_order.sell_token_quantity_remain = maker_order
                .sell_token_quantity_remain
                .checked_sub(taker_buy_remain)
                .ok_or("Balance underflow")?;

            (taker_buy_receive, maker_buy_receive)
        } else {
            // taker > maker
            let taker_buy_receive = maker_sell_remain;
            let maker_buy_receive = self.market_multiply_and_divide(
                maker_sell_remain,
                maker_buy_quantity,
                maker_sell_quantity,
            )?;

            self.market_update_order_state(storage_adapter, maker_order, 1)?;
            if maker_buy_receive == 0 {
                // Quantity too small, return remaining sellToken to maker (should not happen).
                maker_order.sell_token_quantity_return = maker_order.sell_token_quantity_remain;
                self.market_return_sell_token_remain_to_owner(storage_adapter, maker_order)?;
                return Ok(None); // No fill occurred
            }

            maker_order.sell_token_quantity_remain = 0;
            taker_order.sell_token_quantity_remain = taker_order
                .sell_token_quantity_remain
                .checked_sub(maker_buy_receive)
                .ok_or("Balance underflow")?;

            (taker_buy_receive, maker_buy_receive)
        };

        // Save maker order
        storage_adapter
            .put_market_order(&maker_order.order_id, maker_order)
            .map_err(|e| format!("Failed to save maker order: {}", e))?;

        // Add token into accounts
        self.market_add_trx_or_token_in_place(
            taker_account,
            &taker_order.buy_token_id,
            taker_buy_receive,
        )?;
        self.market_add_trx_or_token_to_owner(
            storage_adapter,
            &maker_order.owner_address,
            &maker_order.buy_token_id,
            maker_buy_receive,
        )?;

        // Build order detail for receipt
        // Java: fillSellQuantity = makerBuyTokenQuantityReceive (what maker received = taker sold)
        // Java: fillBuyQuantity = takerBuyTokenQuantityReceive (what taker received = maker sold)
        let order_detail = MarketOrderDetail::new(
            maker_order.order_id.clone(),
            taker_order.order_id.clone(),
            maker_buy_receive, // fillSellQuantity
            taker_buy_receive, // fillBuyQuantity
        );

        Ok(Some(order_detail))
    }

    fn save_remain_market_order(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        order: &mut tron_backend_execution::protocol::MarketOrder,
    ) -> Result<(), String> {
        let pair_price_key = Self::create_pair_price_key(
            &order.sell_token_id,
            &order.buy_token_id,
            order.sell_token_quantity,
            order.buy_token_quantity,
        )?;

        let existing_order_list = storage_adapter
            .get_market_order_id_list(&pair_price_key)
            .map_err(|e| format!("Failed to get order list: {}", e))?;
        let is_new_price_key = existing_order_list.is_none();
        let mut order_list =
            existing_order_list.unwrap_or(tron_backend_execution::protocol::MarketOrderIdList {
                head: vec![],
                tail: vec![],
            });

        // If this price key is new, increase the pair's price count (and create the head key if needed).
        if is_new_price_key {
            self.market_add_new_price_key(
                storage_adapter,
                &order.sell_token_id,
                &order.buy_token_id,
            )?;
        }

        // Add to linked list (at tail).
        if order_list.head.is_empty() {
            order_list.head = order.order_id.clone();
            order_list.tail = order.order_id.clone();
        } else {
            let tail_id = order_list.tail.clone();
            if let Some(mut tail_order) = storage_adapter
                .get_market_order(&tail_id)
                .map_err(|e| format!("Failed to get tail order: {}", e))?
            {
                tail_order.next = order.order_id.clone();
                storage_adapter
                    .put_market_order(&tail_id, &tail_order)
                    .map_err(|e| format!("Failed to update tail order: {}", e))?;
            }

            order.prev = tail_id;
            storage_adapter
                .put_market_order(&order.order_id, order)
                .map_err(|e| format!("Failed to update order pointers: {}", e))?;

            order_list.tail = order.order_id.clone();
        }

        storage_adapter
            .put_market_order_id_list(&pair_price_key, &order_list)
            .map_err(|e| format!("Failed to save order list: {}", e))?;

        Ok(())
    }

    fn market_add_new_price_key(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        sell_token_id: &[u8],
        buy_token_id: &[u8],
    ) -> Result<(), String> {
        let pair_key = Self::create_pair_key(sell_token_id, buy_token_id)?;
        let has_pair = storage_adapter
            .has_market_pair(&pair_key)
            .map_err(|e| format!("Failed to check pair: {}", e))?;

        if has_pair {
            let current = storage_adapter
                .get_market_pair_price_count(&pair_key)
                .map_err(|e| format!("Failed to get price count: {}", e))?;
            storage_adapter
                .set_market_pair_price_count(&pair_key, current + 1)
                .map_err(|e| format!("Failed to update price count: {}", e))?;
            return Ok(());
        }

        storage_adapter
            .set_market_pair_price_count(&pair_key, 1)
            .map_err(|e| format!("Failed to set price count: {}", e))?;

        let head_key = Self::create_pair_price_key(sell_token_id, buy_token_id, 0, 0)?;
        let empty_list = tron_backend_execution::protocol::MarketOrderIdList {
            head: vec![],
            tail: vec![],
        };
        storage_adapter
            .put_market_order_id_list(&head_key, &empty_list)
            .map_err(|e| format!("Failed to create head key: {}", e))?;

        Ok(())
    }

    fn market_get_price_keys_list(
        &self,
        storage_adapter: &tron_backend_execution::EngineBackedEvmStateStore,
        sell_token_id: &[u8],
        buy_token_id: &[u8],
        count: i64,
        total_count: i64,
    ) -> Result<Vec<Vec<u8>>, String> {
        if count <= 0 || total_count <= 0 {
            return Ok(Vec::new());
        }

        let head_key = Self::create_pair_price_key(sell_token_id, buy_token_id, 0, 0)?;
        let has_head = storage_adapter
            .has_market_price_key(&head_key)
            .map_err(|e| format!("Failed to check head key: {}", e))?;
        if !has_head {
            return Ok(Vec::new());
        }

        let pair_key = Self::create_pair_key(sell_token_id, buy_token_id)?;
        let mut keys = storage_adapter
            .list_market_pair_price_keys(&pair_key)
            .map_err(|e| format!("Failed to list price keys: {}", e))?;
        keys.sort_by(|a, b| Self::market_compare_price_key(a, b));

        let limit = std::cmp::min(count, total_count) as usize;
        let mut result = Vec::with_capacity(limit);
        for key in keys {
            if key == head_key {
                continue;
            }
            if result.len() >= limit {
                break;
            }
            result.push(key);
        }

        Ok(result)
    }

    fn market_has_match(
        &self,
        price_keys_list: &[Vec<u8>],
        taker_order: &tron_backend_execution::protocol::MarketOrder,
    ) -> Result<bool, String> {
        if price_keys_list.is_empty() {
            return Ok(false);
        }

        // Get the best (lowest) maker price.
        let (maker_sell_qty, maker_buy_qty) =
            Self::market_decode_key_to_market_price(&price_keys_list[0])?;

        Ok(Self::market_price_match(
            taker_order.sell_token_quantity,
            taker_order.buy_token_quantity,
            maker_sell_qty,
            maker_buy_qty,
        ))
    }

    fn market_price_match(
        taker_sell_qty: i64,
        taker_buy_qty: i64,
        maker_sell_qty: i64,
        maker_buy_qty: i64,
    ) -> bool {
        Self::market_compare_price(taker_buy_qty, taker_sell_qty, maker_sell_qty, maker_buy_qty)
            != std::cmp::Ordering::Less
    }

    fn market_compare_price(
        price1_sell: i64,
        price1_buy: i64,
        price2_sell: i64,
        price2_buy: i64,
    ) -> std::cmp::Ordering {
        let left = BigInt::from(price1_buy) * BigInt::from(price2_sell);
        let right = BigInt::from(price2_buy) * BigInt::from(price1_sell);
        left.cmp(&right)
    }

    fn market_compare_price_key(key1: &[u8], key2: &[u8]) -> std::cmp::Ordering {
        const PAIR_LEN: usize = 38;

        let pair1 = key1.get(..PAIR_LEN).unwrap_or(key1);
        let pair2 = key2.get(..PAIR_LEN).unwrap_or(key2);
        let pair_cmp = pair1.cmp(pair2);
        if pair_cmp != std::cmp::Ordering::Equal {
            return pair_cmp;
        }

        let (sell1, buy1) = match Self::market_decode_key_to_market_price(key1) {
            Ok(v) => v,
            Err(_) => return key1.cmp(key2),
        };
        let (sell2, buy2) = match Self::market_decode_key_to_market_price(key2) {
            Ok(v) => v,
            Err(_) => return key1.cmp(key2),
        };

        let is_head1 = sell1 == 0 || buy1 == 0;
        let is_head2 = sell2 == 0 || buy2 == 0;
        if is_head1 && is_head2 {
            return std::cmp::Ordering::Equal;
        }
        if is_head1 {
            return std::cmp::Ordering::Less;
        }
        if is_head2 {
            return std::cmp::Ordering::Greater;
        }

        Self::market_compare_price(sell1, buy1, sell2, buy2)
    }

    fn market_decode_key_to_market_price(key: &[u8]) -> Result<(i64, i64), String> {
        if key.len() < 54 {
            return Err(format!("Invalid pair price key length: {}", key.len()));
        }

        let mut sell_bytes = [0u8; 8];
        sell_bytes.copy_from_slice(&key[38..46]);
        let mut buy_bytes = [0u8; 8];
        buy_bytes.copy_from_slice(&key[46..54]);

        Ok((
            i64::from_be_bytes(sell_bytes),
            i64::from_be_bytes(buy_bytes),
        ))
    }

    fn market_multiply_and_divide(&self, a: i64, b: i64, c: i64) -> Result<i64, String> {
        if c == 0 {
            return Err("Division by zero".to_string());
        }

        let result = BigInt::from(a) * BigInt::from(b) / BigInt::from(c);
        Self::market_bigint_to_i64(&result)
    }

    fn market_bigint_to_i64(value: &BigInt) -> Result<i64, String> {
        let (sign, bytes) = value.to_bytes_be();
        if bytes.is_empty() || sign == Sign::NoSign {
            return Ok(0);
        }

        if bytes.len() > 8 {
            return Err("Integer overflow".to_string());
        }

        let mut magnitude: u64 = 0;
        for b in bytes {
            magnitude = (magnitude << 8) | (b as u64);
        }

        match sign {
            Sign::Plus => {
                if magnitude > i64::MAX as u64 {
                    return Err("Integer overflow".to_string());
                }
                Ok(magnitude as i64)
            }
            Sign::Minus => {
                if magnitude > (i64::MAX as u64) + 1 {
                    return Err("Integer overflow".to_string());
                }
                Ok(-(magnitude as i64))
            }
            Sign::NoSign => Ok(0),
        }
    }

    fn market_update_order_state(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        order: &mut tron_backend_execution::protocol::MarketOrder,
        state: i32,
    ) -> Result<(), String> {
        order.state = state;

        // Remove from account order list when inactive/canceled.
        if state == 1 || state == 2 {
            // Get strict parity mode flag for missing-index behavior
            let execution_config = self.get_execution_config()?;
            let strict_parity = execution_config.remote.market_strict_index_parity;

            let owner = Self::market_owner_address(&order.owner_address)?;
            let account_order_opt = storage_adapter
                .get_market_account_order(&owner)
                .map_err(|e| format!("Failed to get account order: {}", e))?;

            if let Some(mut account_order) = account_order_opt {
                account_order.orders.retain(|id| id != &order.order_id);
                account_order.count = account_order
                    .count
                    .checked_sub(1)
                    .ok_or("MarketAccountOrder count underflow")?;
                storage_adapter
                    .put_market_account_order(&owner, &account_order)
                    .map_err(|e| format!("Failed to update account order: {}", e))?;
            } else if strict_parity {
                // Java: MarketAccountStore.get(owner) throws ItemNotFoundException if missing
                return Err(format!(
                    "MarketAccountOrder not found for owner: {}",
                    hex::encode(storage_adapter.to_tron_address_21(&owner))
                ));
            }
            // else: defensive recovery mode - skip removal if not found
        }

        Ok(())
    }

    fn market_owner_address(owner_address: &[u8]) -> Result<revm_primitives::Address, String> {
        if owner_address.len() == 21 && (owner_address[0] == 0x41 || owner_address[0] == 0xa0) {
            return Ok(revm_primitives::Address::from_slice(&owner_address[1..]));
        }
        if owner_address.len() == 20 {
            return Ok(revm_primitives::Address::from_slice(owner_address));
        }
        Err(format!(
            "Invalid owner address length: {}",
            owner_address.len()
        ))
    }

    fn market_add_trx_or_token_in_place(
        &self,
        account: &mut tron_backend_execution::protocol::Account,
        token_id: &[u8],
        amount: i64,
    ) -> Result<(), String> {
        if amount == 0 {
            return Ok(());
        }

        if token_id == b"_" || token_id.is_empty() {
            account.balance = account
                .balance
                .checked_add(amount)
                .ok_or("Balance overflow")?;
            return Ok(());
        }

        let token_key = String::from_utf8_lossy(token_id).to_string();
        let current = account.asset_v2.get(&token_key).copied().unwrap_or(0);
        let updated = current
            .checked_add(amount)
            .ok_or("Token balance overflow")?;
        account.asset_v2.insert(token_key, updated);

        Ok(())
    }

    fn market_add_trx_or_token_to_owner(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        owner_address: &[u8],
        token_id: &[u8],
        amount: i64,
    ) -> Result<(), String> {
        if amount == 0 {
            return Ok(());
        }

        let owner = Self::market_owner_address(owner_address)?;
        let mut account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or("Account does not exist!")?;

        self.market_add_trx_or_token_in_place(&mut account, token_id, amount)?;

        storage_adapter
            .set_account_proto(&owner, &account)
            .map_err(|e| format!("Failed to update account: {}", e))?;
        Ok(())
    }

    fn market_return_sell_token_remain(
        &self,
        order: &mut tron_backend_execution::protocol::MarketOrder,
        account: &mut tron_backend_execution::protocol::Account,
    ) -> Result<(), String> {
        let remain = order.sell_token_quantity_remain;
        if remain == 0 {
            return Ok(());
        }

        if order.sell_token_id == b"_" || order.sell_token_id.is_empty() {
            account.balance = account
                .balance
                .checked_add(remain)
                .ok_or("Balance overflow")?;
        } else {
            let token_key = String::from_utf8_lossy(&order.sell_token_id).to_string();
            let current = account.asset_v2.get(&token_key).copied().unwrap_or(0);
            let updated = current
                .checked_add(remain)
                .ok_or("Token balance overflow")?;
            account.asset_v2.insert(token_key, updated);
        }

        order.sell_token_quantity_remain = 0;
        Ok(())
    }

    fn market_return_sell_token_remain_to_owner(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        order: &mut tron_backend_execution::protocol::MarketOrder,
    ) -> Result<(), String> {
        let owner = Self::market_owner_address(&order.owner_address)?;
        let mut account = storage_adapter
            .get_account_proto(&owner)
            .map_err(|e| format!("Failed to get maker account: {}", e))?
            .ok_or("Account does not exist!")?;

        self.market_return_sell_token_remain(order, &mut account)?;

        storage_adapter
            .set_account_proto(&owner, &account)
            .map_err(|e| format!("Failed to update maker account: {}", e))?;
        Ok(())
    }

    /// Parse MarketCancelOrderContract protobuf bytes
    fn parse_market_cancel_order_contract(
        &self,
        data: &[u8],
    ) -> Result<MarketCancelOrderInfo, String> {
        let mut owner_address = Vec::new();
        let mut order_id = Vec::new();

        let mut pos = 0;
        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match field_number {
                // owner_address = 1
                1 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for owner_address".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address = payload.to_vec();
                    pos += total_len;
                }
                // order_id = 2
                2 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for order_id".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    order_id = payload.to_vec();
                    pos += total_len;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        Ok(MarketCancelOrderInfo {
            owner_address,
            order_id,
        })
    }

    /// Parse MarketSellAssetContract protobuf bytes
    /// Now parses owner_address (field 1) for Java parity validation
    fn parse_market_sell_asset_contract(&self, data: &[u8]) -> Result<MarketSellAssetInfo, String> {
        let mut owner_address = Vec::new();
        let mut sell_token_id = Vec::new();
        let mut sell_token_quantity: i64 = 0;
        let mut buy_token_id = Vec::new();
        let mut buy_token_quantity: i64 = 0;

        let mut pos = 0;
        while pos < data.len() {
            let (field_number, wire_type, tag_len) = read_tag_typed(&data[pos..])
                .map_err(|e| e.to_java_message().to_string())?;
            pos += tag_len;

            match field_number {
                // owner_address = 1 (parse for Java parity)
                1 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for owner_address".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    owner_address = payload.to_vec();
                    pos += total_len;
                }
                // sell_token_id = 2
                2 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for sell_token_id".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    sell_token_id = payload.to_vec();
                    pos += total_len;
                }
                // sell_token_quantity = 3
                3 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for sell_token_quantity".to_string());
                    }
                    let (val, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += new_pos;
                    sell_token_quantity = val as i64;
                }
                // buy_token_id = 4
                4 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for buy_token_id".to_string());
                    }
                    let (payload, total_len) = read_length_delimited_typed(&data[pos..])
                        .map_err(|e| e.to_java_message().to_string())?;
                    buy_token_id = payload.to_vec();
                    pos += total_len;
                }
                // buy_token_quantity = 5
                5 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for buy_token_quantity".to_string());
                    }
                    let (val, new_pos) = read_varint_typed(&data[pos..])
                        .map_err(|e| ProtobufError::from(e).to_java_message().to_string())?;
                    pos += new_pos;
                    buy_token_quantity = val as i64;
                }
                _ => {
                    let skip_len = skip_protobuf_field_checked(&data[pos..], wire_type)
                        .map_err(|e| e.to_java_message().to_string())?;
                    pos += skip_len;
                }
            }
        }

        Ok(MarketSellAssetInfo {
            owner_address,
            sell_token_id,
            sell_token_quantity,
            buy_token_id,
            buy_token_quantity,
        })
    }

    /// Remove order from the linked list in MarketOrderIdList
    ///
    /// Java behavior (MarketOrderIdListCapsule.removeOrder):
    /// - Calls getPrevCapsule/getNextCapsule which call marketOrderStore.get()
    /// - If prev/next ID is non-empty but order not found, throws ItemNotFoundException
    ///
    /// When strict_parity=true, we match Java's behavior and fail on missing neighbors.
    /// When strict_parity=false, we use defensive recovery (skip updating missing neighbors).
    fn remove_order_from_linked_list(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        order_list: &mut tron_backend_execution::protocol::MarketOrderIdList,
        order: &tron_backend_execution::protocol::MarketOrder,
        _price_key: &[u8],
        strict_parity: bool,
    ) -> Result<(), String> {
        let order_id = &order.order_id;

        // Get prev and next
        let prev_id = &order.prev;
        let next_id = &order.next;

        // Update prev's next pointer
        if !prev_id.is_empty() {
            let prev_order_opt = storage_adapter
                .get_market_order(prev_id)
                .map_err(|e| format!("Failed to get prev order: {}", e))?;

            if let Some(mut prev_order) = prev_order_opt {
                prev_order.next = next_id.clone();
                storage_adapter
                    .put_market_order(prev_id, &prev_order)
                    .map_err(|e| format!("Failed to update prev order: {}", e))?;
            } else if strict_parity {
                // Java's getPrevCapsule() calls orderStore.get() which throws ItemNotFoundException
                return Err(format!("Prev order not found: {:?}", hex::encode(prev_id)));
            }
            // else: defensive recovery - skip updating missing prev neighbor
        } else {
            // Order is head, update list head
            order_list.head = next_id.clone();
        }

        // Update next's prev pointer
        if !next_id.is_empty() {
            let next_order_opt = storage_adapter
                .get_market_order(next_id)
                .map_err(|e| format!("Failed to get next order: {}", e))?;

            if let Some(mut next_order) = next_order_opt {
                next_order.prev = prev_id.clone();
                storage_adapter
                    .put_market_order(next_id, &next_order)
                    .map_err(|e| format!("Failed to update next order: {}", e))?;
            } else if strict_parity {
                // Java's getNextCapsule() calls orderStore.get() which throws ItemNotFoundException
                return Err(format!("Next order not found: {:?}", hex::encode(next_id)));
            }
            // else: defensive recovery - skip updating missing next neighbor
        } else {
            // Order is tail, update list tail
            order_list.tail = prev_id.clone();
        }

        // Clear order's prev and next pointers
        let mut updated_order = order.clone();
        updated_order.prev = vec![];
        updated_order.next = vec![];
        storage_adapter
            .put_market_order(order_id, &updated_order)
            .map_err(|e| format!("Failed to clear order pointers: {}", e))?;

        Ok(())
    }

    /// Create pair key: sellTokenId(19) + buyTokenId(19) = 38 bytes
    /// Matches MarketUtils.createPairKey
    ///
    /// Java throws ArrayIndexOutOfBoundsException if token ID length > 19 bytes.
    /// For strict parity, we return an error in this case.
    fn create_pair_key(sell_token_id: &[u8], buy_token_id: &[u8]) -> Result<Vec<u8>, String> {
        const TOKEN_ID_LENGTH: usize = 19;

        // Java's System.arraycopy throws ArrayIndexOutOfBoundsException if length > 19
        if sell_token_id.len() > TOKEN_ID_LENGTH {
            return Err(format!(
                "sellTokenId length {} exceeds maximum {}",
                sell_token_id.len(),
                TOKEN_ID_LENGTH
            ));
        }
        if buy_token_id.len() > TOKEN_ID_LENGTH {
            return Err(format!(
                "buyTokenId length {} exceeds maximum {}",
                buy_token_id.len(),
                TOKEN_ID_LENGTH
            ));
        }

        let mut result = vec![0u8; TOKEN_ID_LENGTH * 2];

        result[..sell_token_id.len()].copy_from_slice(sell_token_id);
        result[TOKEN_ID_LENGTH..TOKEN_ID_LENGTH + buy_token_id.len()].copy_from_slice(buy_token_id);

        Ok(result)
    }

    /// Create pair price key: sellTokenId(19) + buyTokenId(19) + sellQty(8) + buyQty(8) = 54 bytes
    /// Matches MarketUtils.createPairPriceKey (with GCD normalization)
    ///
    /// Java throws ArrayIndexOutOfBoundsException if token ID length > 19 bytes.
    /// For strict parity, we return an error in this case.
    fn create_pair_price_key(
        sell_token_id: &[u8],
        buy_token_id: &[u8],
        sell_token_quantity: i64,
        buy_token_quantity: i64,
    ) -> Result<Vec<u8>, String> {
        const TOKEN_ID_LENGTH: usize = 19;

        // Java's System.arraycopy throws ArrayIndexOutOfBoundsException if length > 19
        if sell_token_id.len() > TOKEN_ID_LENGTH {
            return Err(format!(
                "sellTokenId length {} exceeds maximum {}",
                sell_token_id.len(),
                TOKEN_ID_LENGTH
            ));
        }
        if buy_token_id.len() > TOKEN_ID_LENGTH {
            return Err(format!(
                "buyTokenId length {} exceeds maximum {}",
                buy_token_id.len(),
                TOKEN_ID_LENGTH
            ));
        }

        // Calculate GCD for price normalization
        let gcd = Self::find_gcd(sell_token_quantity, buy_token_quantity);
        let (norm_sell, norm_buy) = if gcd == 0 {
            (sell_token_quantity, buy_token_quantity)
        } else {
            (sell_token_quantity / gcd, buy_token_quantity / gcd)
        };

        let mut result = vec![0u8; TOKEN_ID_LENGTH * 2 + 16];

        // Copy token IDs
        result[..sell_token_id.len()].copy_from_slice(sell_token_id);
        result[TOKEN_ID_LENGTH..TOKEN_ID_LENGTH + buy_token_id.len()].copy_from_slice(buy_token_id);

        // Append quantities as big-endian
        result[TOKEN_ID_LENGTH * 2..TOKEN_ID_LENGTH * 2 + 8]
            .copy_from_slice(&norm_sell.to_be_bytes());
        result[TOKEN_ID_LENGTH * 2 + 8..].copy_from_slice(&norm_buy.to_be_bytes());

        Ok(result)
    }

    /// Find GCD of two numbers
    fn find_gcd(a: i64, b: i64) -> i64 {
        if a == 0 || b == 0 {
            return 0;
        }
        Self::calc_gcd(a.abs(), b.abs())
    }

    fn calc_gcd(a: i64, b: i64) -> i64 {
        if b == 0 {
            a
        } else {
            Self::calc_gcd(b, a % b)
        }
    }

    /// Calculate order ID: SHA3(ownerAddress + sellTokenId(padded) + buyTokenId(padded) + count)
    /// Matches MarketUtils.calculateOrderId
    /// Calculate order ID using SHA3 hash
    /// Java's MarketUtils.calculateOrderId throws ArrayIndexOutOfBoundsException if token ID > 19 bytes
    fn calculate_order_id(
        owner_address: &[u8],
        sell_token_id: &[u8],
        buy_token_id: &[u8],
        count: i64,
    ) -> Result<Vec<u8>, String> {
        use sha3::{Digest, Keccak256};

        const TOKEN_ID_LENGTH: usize = 19;

        // Java's System.arraycopy throws ArrayIndexOutOfBoundsException if length > 19
        if sell_token_id.len() > TOKEN_ID_LENGTH {
            return Err(format!(
                "sellTokenId length {} exceeds maximum {}",
                sell_token_id.len(),
                TOKEN_ID_LENGTH
            ));
        }
        if buy_token_id.len() > TOKEN_ID_LENGTH {
            return Err(format!(
                "buyTokenId length {} exceeds maximum {}",
                buy_token_id.len(),
                TOKEN_ID_LENGTH
            ));
        }

        let count_bytes = count.to_be_bytes();

        let mut data = Vec::with_capacity(owner_address.len() + TOKEN_ID_LENGTH * 2 + 8);
        data.extend_from_slice(owner_address);

        // Pad sell token ID
        let mut sell_padded = vec![0u8; TOKEN_ID_LENGTH];
        sell_padded[..sell_token_id.len()].copy_from_slice(sell_token_id);
        data.extend_from_slice(&sell_padded);

        // Pad buy token ID
        let mut buy_padded = vec![0u8; TOKEN_ID_LENGTH];
        buy_padded[..buy_token_id.len()].copy_from_slice(buy_token_id);
        data.extend_from_slice(&buy_padded);

        data.extend_from_slice(&count_bytes);

        let mut hasher = Keccak256::new();
        hasher.update(&data);
        Ok(hasher.finalize().to_vec())
    }
}

/// Parsed ExchangeCreateContract information
#[derive(Debug, Clone)]
struct ExchangeCreateInfo {
    owner_address: Vec<u8>,
    first_token_id: Vec<u8>,
    first_token_balance: i64,
    second_token_id: Vec<u8>,
    second_token_balance: i64,
}

/// Parsed ExchangeInjectContract information
#[derive(Debug, Clone)]
struct ExchangeInjectInfo {
    owner_address: Vec<u8>,
    exchange_id: i64,
    token_id: Vec<u8>,
    quant: i64,
}

/// Parsed ExchangeWithdrawContract information
#[derive(Debug, Clone)]
struct ExchangeWithdrawInfo {
    owner_address: Vec<u8>,
    exchange_id: i64,
    token_id: Vec<u8>,
    quant: i64,
}

/// Parsed ExchangeTransactionContract information
#[derive(Debug, Clone)]
struct ExchangeTransactionInfo {
    owner_address: Vec<u8>,
    exchange_id: i64,
    token_id: Vec<u8>,
    quant: i64,
    expected: i64,
}

/// Parsed ParticipateAssetIssueContract information
#[derive(Debug, Clone)]
struct ParticipateAssetIssueInfo {
    owner_address: Vec<u8>,
    to_address: Vec<u8>,
    asset_name: Vec<u8>,
    amount: i64,
}

/// Parsed UpdateAssetContract information
#[derive(Debug, Clone)]
struct UpdateAssetInfo {
    owner_address: Vec<u8>,
    description: Vec<u8>,
    url: Vec<u8>,
    new_limit: i64,
    new_public_limit: i64,
}

/// Parsed AssetIssueContract information (Phase 1 + Phase 2)
#[derive(Debug, Clone)]
struct AssetIssueInfo {
    name: String,
    abbr: String,
    total_supply: i64,
    precision: i32,
    trx_num: i32,
    num: i32,
    start_time: i64,
    end_time: i64,
    description: String,
    url: String,
    // Phase 2 fields
    free_asset_net_limit: i64,
    public_free_asset_net_limit: i64,
    public_free_asset_net_usage: i64,
    public_latest_free_net_time: i64,
}

/// Parsed DelegateResourceContract information
#[derive(Debug, Clone)]
struct DelegateResourceInfo {
    owner_address: Vec<u8>, // Java parity: use contract's owner_address, not transaction.from_raw
    receiver_address: Vec<u8>,
    balance: i64,
    resource: i32, // 0 = BANDWIDTH, 1 = ENERGY
    lock: bool,
    lock_period: i64,
}

/// Parsed UnDelegateResourceContract information
#[derive(Debug, Clone)]
struct UnDelegateResourceInfo {
    receiver_address: Vec<u8>,
    balance: i64,
    resource: i32, // 0 = BANDWIDTH, 1 = ENERGY
}

/// Parsed MarketCancelOrderContract information
#[derive(Debug, Clone)]
struct MarketCancelOrderInfo {
    owner_address: Vec<u8>,
    order_id: Vec<u8>,
}

/// Parsed MarketSellAssetContract information
#[derive(Debug, Clone)]
struct MarketSellAssetInfo {
    owner_address: Vec<u8>,
    sell_token_id: Vec<u8>,
    sell_token_quantity: i64,
    buy_token_id: Vec<u8>,
    buy_token_quantity: i64,
}

#[cfg(test)]
mod tests;
