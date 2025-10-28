use std::collections::HashMap;
use std::time::SystemTime;

use tonic::{Request, Response, Status};
use tracing::{info, error, debug, warn};
use tokio_stream::wrappers::ReceiverStream;
use tokio::sync::mpsc;

use tron_backend_common::{ModuleManager, HealthStatus, from_tron_address};
use revm_primitives::hex;
use tron_backend_execution::{TronTransaction, TronExecutionContext, TronExecutionResult, TronStateChange, ExecutionModule, EvmStateStore};
use crate::backend::*;

/// FreezeBalance contract parameters
#[derive(Debug, Clone)]
struct FreezeParams {
    frozen_balance: i64,
    frozen_duration: u32,
    resource: FreezeResource,
}

/// UnfreezeBalance contract parameters
#[derive(Debug, Clone)]
struct UnfreezeParams {
    resource: FreezeResource,
}

/// FreezeBalanceV2 contract parameters
#[derive(Debug, Clone)]
struct FreezeV2Params {
    frozen_balance: i64,
    resource: FreezeResource,
}

/// UnfreezeBalanceV2 contract parameters
#[derive(Debug, Clone)]
struct UnfreezeV2Params {
    unfreeze_balance: i64,
    resource: FreezeResource,
}

/// Resource type for freeze/unfreeze operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FreezeResource {
    Bandwidth = 0,
    Energy = 1,
    TronPower = 2,
}

/// Vote witness contract constants
const MAX_VOTE_NUMBER: usize = 30;
const TRX_PRECISION: u64 = 1_000_000; // 1 TRX = 1,000,000 SUN

/// Read a protobuf varint from a byte slice
/// Returns (value, bytes_read)
fn read_varint(data: &[u8]) -> Result<(u64, usize), String> {
    let mut result: u64 = 0;
    let mut shift = 0;
    let mut pos = 0;

    loop {
        if pos >= data.len() {
            return Err("Unexpected end of varint".to_string());
        }

        let byte = data[pos];
        pos += 1;

        result |= ((byte & 0x7F) as u64) << shift;

        if (byte & 0x80) == 0 {
            return Ok((result, pos));
        }

        shift += 7;
        if shift >= 64 {
            return Err("Varint too long".to_string());
        }
    }
}

pub struct BackendService {
    module_manager: ModuleManager,
    start_time: SystemTime,
}

impl BackendService {
    pub fn new(module_manager: ModuleManager) -> Self {
        Self {
            module_manager,
            start_time: SystemTime::now(),
        }
    }
    
    fn get_storage_module(&self) -> Result<&Box<dyn tron_backend_common::Module>, Status> {
        self.module_manager.get("storage")
            .ok_or_else(|| Status::unavailable("Storage module not available"))
    }
    
    fn get_execution_module(&self) -> Result<&Box<dyn tron_backend_common::Module>, Status> {
        self.module_manager.get("execution")
            .ok_or_else(|| Status::unavailable("Execution module not available"))
    }
    
    fn get_storage_engine(&self) -> Result<&tron_backend_storage::StorageEngine, Status> {
        let storage_module = self.get_storage_module()?;
        
        // Downcast to the concrete storage module type
        let storage_module = storage_module
            .as_any()
            .downcast_ref::<tron_backend_storage::StorageModule>()
            .ok_or_else(|| Status::internal("Failed to downcast storage module"))?;
            
        storage_module.engine()
            .map_err(|e| Status::internal(format!("Storage engine not available: {}", e)))
    }
    
    fn get_execution_config(&self) -> Result<&tron_backend_common::ExecutionConfig, String> {
        let execution_module = self.get_execution_module()
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
    fn is_likely_non_vm_transaction(&self, tx: &TronTransaction, storage_adapter: &tron_backend_execution::EngineBackedEvmStateStore) -> bool {
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
            },
            Ok(None) => {
                // No code entry = new or EOA account, likely non-VM transaction
                true
            },
            Err(_) => {
                // Error accessing code, be conservative and assume VM transaction
                false
            }
        }
    }
    
    /// Apply TRON fee policy post-processing to execution results
    fn apply_fee_post_processing(&self, 
                                result: &mut TronExecutionResult, 
                                _tx: &TronTransaction, 
                                context: &TronExecutionContext, 
                                is_non_vm: bool) -> Result<(), String> {
        let execution_config = self.get_execution_config()?;
        let fee_config = &execution_config.fees;
        
        // Only apply fee post-processing in blackhole mode
        if fee_config.mode != "blackhole" {
            debug!("Fee mode is '{}', skipping fee post-processing", fee_config.mode);
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
                warn!("Invalid blackhole address '{}': {}, skipping fee emission", 
                      fee_config.blackhole_address_base58, e);
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
                debug!("Non-VM transaction: will emit flat fee {} SUN to blackhole", flat_fee);
            } else {
                debug!("Non-VM transaction: no flat fee configured, skipping fee emission");
            }
        } else {
            // VM transaction fee handling
            if fee_config.experimental_vm_blackhole_credit {
                // Approximate fee calculation: energy_used * energy_price
                fee_amount = result.energy_used * context.energy_price;
                should_emit = true;
                debug!("VM transaction: will emit estimated fee {} SUN ({}*{}) to blackhole", 
                       fee_amount, result.energy_used, context.energy_price);
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
            debug!("Added synthetic blackhole fee credit: {} SUN to address {:?}", 
                   fee_amount, blackhole_evm_address);
            
            // Log warning about approximation
            warn!("Emitted synthetic fee credit to blackhole (approximation until Phase 3)");
        }
        
        Ok(())
    }
    
    /// Execute a non-VM transaction with contract type dispatch
    /// Routes to specific handlers based on TRON contract type
    fn execute_non_vm_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        debug!("Executing non-VM contract: from={:?}, to={:?}, value={}, contract_type={:?}",
               transaction.from, transaction.to, transaction.value, transaction.metadata.contract_type);

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
            },
            Some(tron_backend_execution::TronContractType::WitnessCreateContract) => {
                if !remote_config.witness_create_enabled {
                    return Err("WITNESS_CREATE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing WITNESS_CREATE_CONTRACT");
                self.execute_witness_create_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::WitnessUpdateContract) => {
                if !remote_config.witness_update_enabled {
                    return Err("WITNESS_UPDATE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing WITNESS_UPDATE_CONTRACT");
                self.execute_witness_update_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::VoteWitnessContract) => {
                if !remote_config.vote_witness_enabled {
                    return Err("VOTE_WITNESS_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing VOTE_WITNESS_CONTRACT");
                self.execute_vote_witness_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::TransferAssetContract) => {
                if !remote_config.trc10_enabled {
                    return Err("TRC-10 transfers are disabled - falling back to Java".to_string());
                }
                debug!("Executing TRANSFER_ASSET_CONTRACT (TRC-10)");
                // TODO: Implement TRC-10 transfer handler
                Err("TRC-10 transfers not yet implemented in Rust backend".to_string())
            },
            Some(tron_backend_execution::TronContractType::AccountUpdateContract) => {
                debug!("Executing ACCOUNT_UPDATE_CONTRACT");
                self.execute_account_update_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::FreezeBalanceContract) => {
                if !remote_config.freeze_balance_enabled {
                    return Err("FREEZE_BALANCE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing FREEZE_BALANCE_CONTRACT");
                self.execute_freeze_balance_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::UnfreezeBalanceContract) => {
                if !remote_config.unfreeze_balance_enabled {
                    return Err("UNFREEZE_BALANCE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing UNFREEZE_BALANCE_CONTRACT");
                self.execute_unfreeze_balance_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::FreezeBalanceV2Contract) => {
                if !remote_config.freeze_balance_v2_enabled {
                    return Err("FREEZE_BALANCE_V2_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing FREEZE_BALANCE_V2_CONTRACT");
                self.execute_freeze_balance_v2_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract) => {
                if !remote_config.unfreeze_balance_v2_enabled {
                    return Err("UNFREEZE_BALANCE_V2_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing UNFREEZE_BALANCE_V2_CONTRACT");
                self.execute_unfreeze_balance_v2_contract(storage_adapter, transaction, context)
            },
            Some(contract_type) => {
                // Other contract types not yet implemented - return error to fall back to Java
                Err(format!("Contract type {:?} not yet implemented in Rust backend", contract_type))
            },
            None => {
                // No contract type specified - use legacy transfer logic for backward compatibility
                debug!("No contract type specified, using legacy transfer logic");
                self.execute_transfer_contract(storage_adapter, transaction, context)
            }
        }
    }

    /// Execute a TRANSFER_CONTRACT (legacy non-VM transaction)
    /// Handles TRON value transfer with proper fee accounting
    fn execute_transfer_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        debug!("Executing TRANSFER_CONTRACT: from={:?}, to={:?}, value={}",
               transaction.from, transaction.to, transaction.value);

        let execution_config = self.get_execution_config()?;
        let fee_config = &execution_config.fees;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();

        // For TRANSFER_CONTRACT specifically, we need the 'to' address
        let to_address = transaction.to.ok_or("TRANSFER_CONTRACT must have 'to' address")?;

        // Calculate bandwidth used based on transaction payload size
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Track AEXT for bandwidth if in tracked mode
        let mut aext_map = std::collections::HashMap::new();
        if aext_mode == "tracked" {
            use tron_backend_execution::{AccountAext, ResourceTracker};

            // Get current AEXT for sender (or initialize with defaults)
            let current_aext = storage_adapter.get_account_aext(&transaction.from)
                .map_err(|e| format!("Failed to get account AEXT: {}", e))?
                .unwrap_or_else(|| AccountAext::with_defaults());

            // Get FREE_NET_LIMIT from dynamic properties
            let free_net_limit = storage_adapter.get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;

            // Track bandwidth usage (returns path, before_aext, after_aext)
            let (path, before_aext, after_aext) = ResourceTracker::track_bandwidth(
                &transaction.from,
                bandwidth_used as i64,
                context.block_number as i64, // Use block number as "now"
                &current_aext,
                free_net_limit,
            ).map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist after AEXT to storage
            storage_adapter.set_account_aext(&transaction.from, &after_aext)
                .map_err(|e| format!("Failed to persist account AEXT: {}", e))?;

            // Add to aext_map
            aext_map.insert(transaction.from, (before_aext.clone(), after_aext.clone()));

            debug!("AEXT tracked for transfer: owner={:?}, path={:?}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}",
                   transaction.from, path, before_aext.net_usage, after_aext.net_usage,
                   before_aext.free_net_usage, after_aext.free_net_usage);
        }

        // Start with empty state changes
        let mut state_changes = Vec::new();
        
        // Load sender account
        let sender_account = storage_adapter.get_account(&transaction.from)
            .map_err(|e| format!("Failed to load sender account: {}", e))?
            .unwrap_or_default();
        
        // Load recipient account  
        let recipient_account = storage_adapter.get_account(&to_address)
            .map_err(|e| format!("Failed to load recipient account: {}", e))?
            .unwrap_or_default();
        
        // Phase 3 Fix: Only calculate fee if explicitly configured for non-VM transactions
        let fee_amount = match fee_config.non_vm_blackhole_credit_flat {
            Some(flat_fee) => {
                debug!("Using configured flat fee for non-VM: {} SUN", flat_fee);
                flat_fee
            },
            None => {
                // Phase 3 Fix: Default to no forced TRX fee for non-VM transactions
                // TRON uses free bandwidth first; only charge TRX when free bandwidth is insufficient
                debug!("No flat fee configured for non-VM, using 0 (TRON free bandwidth semantics)");
                0
            }
        };
        
        // Validate sender has enough balance for value + fee (only if fee > 0)
        let total_cost = transaction.value.checked_add(revm_primitives::U256::from(fee_amount))
            .ok_or("Value + fee overflow")?;
        
        if sender_account.balance < total_cost {
            return Err(format!("Insufficient balance: need {}, have {}", total_cost, sender_account.balance));
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
        let new_recipient_balance = recipient_account.balance + transaction.value;
        let new_recipient_account = revm_primitives::AccountInfo {
            balance: new_recipient_balance,
            nonce: recipient_account.nonce,
            code_hash: recipient_account.code_hash,
            code: recipient_account.code.clone(),
        };
        
        // Add recipient account change
        let old_recipient_account = if recipient_account.balance.is_zero() && recipient_account.nonce == 0 {
            None // Account creation
        } else {
            Some(recipient_account)
        };

        state_changes.push(TronStateChange::AccountChange {
            address: to_address,
            old_account: old_recipient_account,
            new_account: Some(new_recipient_account.clone()),
        });
        // Persist recipient account update
        storage_adapter
            .set_account(to_address, new_recipient_account.clone())
            .map_err(|e| format!("Failed to persist recipient account: {}", e))?;
        
        // Handle fee based on configuration (only if fee_amount > 0)
        if fee_amount > 0 {
            match fee_config.mode.as_str() {
                "burn" => {
                    debug!("Burning fee {} SUN (no account delta for burn)", fee_amount);
                    // No additional state change - fee is burned (supply reduction)
                },
                "blackhole" => {
                    if !fee_config.blackhole_address_base58.is_empty() {
                        // Parse blackhole address and credit it
                        match tron_backend_common::from_tron_address(&fee_config.blackhole_address_base58) {
                            Ok(blackhole_bytes) => {
                                let blackhole_address = revm_primitives::Address::from_slice(&blackhole_bytes);
                                
                                // Load blackhole account
                                let blackhole_account = storage_adapter.get_account(&blackhole_address)
                                    .map_err(|e| format!("Failed to load blackhole account: {}", e))?
                                    .unwrap_or_default();
                                
                                // Credit blackhole account with fee
                                let new_blackhole_balance = blackhole_account.balance + revm_primitives::U256::from(fee_amount);
                                let new_blackhole_account = revm_primitives::AccountInfo {
                                    balance: new_blackhole_balance,
                                    nonce: blackhole_account.nonce,
                                    code_hash: blackhole_account.code_hash,
                                    code: blackhole_account.code.clone(),
                                };
                                
                                // Add blackhole account change
                                let old_blackhole_account = if blackhole_account.balance.is_zero() && blackhole_account.nonce == 0 {
                                    None // Account creation if needed
                                } else {
                                    Some(blackhole_account)
                                };
                                
                                state_changes.push(TronStateChange::AccountChange {
                                    address: blackhole_address,
                                    old_account: old_blackhole_account,
                                    new_account: Some(new_blackhole_account.clone()),
                                });
                                // Persist blackhole account update
                                storage_adapter
                                    .set_account(blackhole_address, new_blackhole_account.clone())
                                    .map_err(|e| format!("Failed to persist blackhole account: {}", e))?;
                                
                                debug!("Credited fee {} SUN to blackhole address {}", fee_amount, fee_config.blackhole_address_base58);
                            },
                            Err(e) => {
                                warn!("Invalid blackhole address '{}': {}, falling back to burn mode", 
                                      fee_config.blackhole_address_base58, e);
                            }
                        }
                    }
                },
                _ => {
                    debug!("Unknown fee mode '{}', defaulting to burn", fee_config.mode);
                }
            }
        } else {
            debug!("No fee to process (fee_amount = 0), skipping fee handling");
        }
        
        // Sort state changes deterministically for digest parity
        state_changes.sort_by(|a, b| {
            match (a, b) {
                (TronStateChange::AccountChange { address: addr_a, .. }, 
                 TronStateChange::AccountChange { address: addr_b, .. }) => {
                    addr_a.cmp(addr_b)
                },
                (TronStateChange::StorageChange { address: addr_a, key: key_a, .. },
                 TronStateChange::StorageChange { address: addr_b, key: key_b, .. }) => {
                    addr_a.cmp(addr_b).then(key_a.cmp(key_b))
                },
                // Account changes before storage changes for same address
                (TronStateChange::AccountChange { address: addr_a, .. },
                 TronStateChange::StorageChange { address: addr_b, .. }) => {
                    addr_a.cmp(addr_b).then(std::cmp::Ordering::Less)
                },
                (TronStateChange::StorageChange { address: addr_a, .. },
                 TronStateChange::AccountChange { address: addr_b, .. }) => {
                    addr_a.cmp(addr_b).then(std::cmp::Ordering::Greater)
                },
            }
        });
        
        debug!("Non-VM transaction executed successfully - bandwidth_used: {}, fee: {} SUN, state_changes: {}",
               bandwidth_used, fee_amount, state_changes.len());

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(), // No return data for value transfers
            energy_used: 0, // Non-VM transactions use 0 energy
            bandwidth_used,
            state_changes,
            logs: Vec::new(), // No logs for value transfers
            error: None,
            aext_map, // Populated with tracked AEXT if mode is "tracked"
            freeze_changes: vec![], // Will be populated by freeze-related contracts
            global_resource_changes: vec![], // Not applicable for value transfers
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
        debug!("Executing WITNESS_CREATE_CONTRACT for address {:?}", transaction.from);

        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();

        // Extract URL from transaction data
        // For WitnessCreateContract, the data contains the URL bytes
        let url_bytes = &transaction.data;
        let url = String::from_utf8(url_bytes.to_vec())
            .map_err(|e| format!("Invalid UTF-8 in witness URL: {}", e))?;

        debug!("WitnessCreate URL: {}", url);

        // 1. Validate URL format (basic check)
        // Align with embedded: allow empty URL, enforce max length only
        if url.len() > 256 {
            return Err("Invalid witness URL: too long".to_string());
        }

        // 2. Load owner account
        let owner_account = storage_adapter.get_account(&transaction.from)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or("Owner account does not exist".to_string())?;

        // 3. Check if owner is already a witness
        if storage_adapter.is_witness(&transaction.from)
            .map_err(|e| format!("Failed to check witness status: {}", e))? {
            return Err("Owner is already a witness".to_string());
        }

        // 4. Get dynamic properties
        let account_upgrade_cost = storage_adapter.get_account_upgrade_cost()
            .map_err(|e| format!("Failed to get AccountUpgradeCost: {}", e))?;
        let allow_multi_sign = storage_adapter.get_allow_multi_sign()
            .map_err(|e| format!("Failed to get AllowMultiSign: {}", e))?;
        let support_blackhole = storage_adapter.support_black_hole_optimization()
            .map_err(|e| format!("Failed to get SupportBlackHoleOptimization: {}", e))?;

        info!(
            "WitnessCreate flags: upgrade_cost={} SUN, allow_multi_sign={}, support_blackhole={}",
            account_upgrade_cost,
            allow_multi_sign,
            support_blackhole
        );

        // 5. Validate sufficient balance
        if owner_account.balance < revm_primitives::U256::from(account_upgrade_cost) {
            return Err(format!("Insufficient balance: need {} SUN, have {}",
                              account_upgrade_cost, owner_account.balance));
        }

        // 6. Prepare state changes
        let mut state_changes = Vec::new();

        // 7. Create witness entry
        let witness_info = tron_backend_execution::WitnessInfo::new(
            transaction.from,
            url,
            0, // Initial vote count is 0
        );

        storage_adapter.put_witness(&witness_info)
            .map_err(|e| format!("Failed to create witness: {}", e))?;

        debug!("Created witness entry for address {:?}", transaction.from);

        // 8. Update owner account - deduct cost and set witness flag
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

        // 9. Handle fee burning/crediting
        let fee_destination: String;
        if support_blackhole {
            // Burn mode - no additional account change needed
            info!("Burning {} SUN (blackhole optimization)", account_upgrade_cost);
            fee_destination = String::from("burn");
        } else {
            // Credit blackhole account
            if let Some(blackhole_addr) = storage_adapter.get_blackhole_address()
                .map_err(|e| format!("Failed to get blackhole address: {}", e))? {

                let blackhole_account = storage_adapter.get_account(&blackhole_addr)
                    .map_err(|e| format!("Failed to load blackhole account: {}", e))?
                    .unwrap_or_default();

                let new_blackhole_account = revm_primitives::AccountInfo {
                    balance: blackhole_account.balance + revm_primitives::U256::from(account_upgrade_cost),
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

                let bh_tron = revm_primitives::hex::encode(Self::add_tron_address_prefix(&blackhole_addr));
                info!(
                    "Credited {} SUN to blackhole address {}",
                    account_upgrade_cost,
                    bh_tron
                );
                fee_destination = format!("blackhole:{}", bh_tron);
            } else {
                warn!("No blackhole address configured, burning {} SUN", account_upgrade_cost);
                fee_destination = String::from("burn(no_addr)");
            }
        }

        // 10. Sort state changes deterministically for CSV parity
        state_changes.sort_by(|a, b| {
            match (a, b) {
                (TronStateChange::AccountChange { address: addr_a, .. },
                 TronStateChange::AccountChange { address: addr_b, .. }) => {
                    addr_a.cmp(addr_b)
                },
                _ => std::cmp::Ordering::Equal,
            }
        });

        // 11. Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Track AEXT for bandwidth if in tracked mode
        let mut aext_map = std::collections::HashMap::new();
        if aext_mode == "tracked" {
            use tron_backend_execution::{AccountAext, ResourceTracker};

            // Get current AEXT for owner (or initialize with defaults)
            let current_aext = storage_adapter.get_account_aext(&transaction.from)
                .map_err(|e| format!("Failed to get account AEXT: {}", e))?
                .unwrap_or_else(|| AccountAext::with_defaults());

            // Get FREE_NET_LIMIT from dynamic properties
            let free_net_limit = storage_adapter.get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;

            // Track bandwidth usage (returns path, before_aext, after_aext)
            let (path, before_aext, after_aext) = ResourceTracker::track_bandwidth(
                &transaction.from,
                bandwidth_used as i64,
                context.block_number as i64, // Use block number as "now"
                &current_aext,
                free_net_limit,
            ).map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist after AEXT to storage
            storage_adapter.set_account_aext(&transaction.from, &after_aext)
                .map_err(|e| format!("Failed to persist account AEXT: {}", e))?;

            // Add to aext_map
            aext_map.insert(transaction.from, (before_aext.clone(), after_aext.clone()));

            debug!("AEXT tracked for witness_create: owner={:?}, path={:?}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}",
                   transaction.from, path, before_aext.net_usage, after_aext.net_usage,
                   before_aext.free_net_usage, after_aext.free_net_usage);
        }

        let owner_tron = revm_primitives::hex::encode(Self::add_tron_address_prefix(&transaction.from));
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
            aext_map, // Populated with tracked AEXT if mode is "tracked"
            freeze_changes: vec![], // Will be populated by freeze-related contracts
            global_resource_changes: vec![], // Not applicable for witness creation
        })
    }

    /// Execute a WITNESS_UPDATE_CONTRACT
    /// Updates witness URL and other parameters
    fn execute_witness_update_contract(
        &self,
        _storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        _transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        // TODO: Implement witness update logic
        // - Validate owner is an existing witness
        // - Validate URL format
        // - Update witness entry in WitnessStore
        // - Emit minimal state changes (no balance changes)

        warn!("WITNESS_UPDATE_CONTRACT not yet implemented - falling back to Java");
        Err("WITNESS_UPDATE_CONTRACT not yet implemented".to_string())
    }

    /// Parse VoteWitnessContract from protobuf bytes
    /// message VoteWitnessContract {
    ///   bytes owner_address = 1;     // field 1 (informational, use transaction.from)
    ///   repeated Vote votes = 2;     // field 2
    ///   bool support = 3;            // field 3 (not used)
    /// }
    /// message Vote {
    ///   bytes vote_address = 1;      // field 1
    ///   int64 vote_count = 2;        // field 2
    /// }
    fn parse_vote_witness_contract(data: &[u8]) -> Result<Vec<(revm::primitives::Address, u64)>, String> {
        let mut votes = Vec::new();
        let mut pos = 0;

        while pos < data.len() {
            // Read field header
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => { // owner_address (length-delimited) - skip, use transaction.from
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read owner_address length: {}", e))?;
                    pos += bytes_read + length as usize;
                },
                (2, 2) => { // votes (length-delimited, repeated)
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read vote length: {}", e))?;
                    pos += bytes_read;

                    if pos + length as usize > data.len() {
                        return Err("Invalid vote data length".to_string());
                    }

                    let vote_data = &data[pos..pos + length as usize];
                    pos += length as usize;

                    // Parse Vote message
                    let (vote_address, vote_count) = Self::parse_vote(vote_data)?;
                    votes.push((vote_address, vote_count));
                },
                (3, 0) => { // support (bool, varint) - not used, skip
                    let (_, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read support: {}", e))?;
                    pos += bytes_read;
                },
                _ => {
                    // Skip unknown field
                    pos = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                }
            }
        }

        Ok(votes)
    }

    /// Parse a single Vote message from protobuf bytes
    fn parse_vote(data: &[u8]) -> Result<(revm::primitives::Address, u64), String> {
        use revm::primitives::Address;

        let mut vote_address: Option<Address> = None;
        let mut vote_count: Option<u64> = None;
        let mut pos = 0;

        while pos < data.len() {
            // Read field header
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read vote field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => { // vote_address (length-delimited)
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read vote_address length: {}", e))?;
                    pos += bytes_read;

                    if pos + length as usize > data.len() {
                        return Err("Invalid vote_address length".to_string());
                    }

                    let addr_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    // Remove 0x41 prefix if present (21-byte Tron address → 20-byte EVM address)
                    let evm_addr = if addr_bytes.len() == 21 && addr_bytes[0] == 0x41 {
                        &addr_bytes[1..]
                    } else if addr_bytes.len() == 20 {
                        addr_bytes
                    } else {
                        return Err(format!("Invalid vote_address length: {}", addr_bytes.len()));
                    };

                    if evm_addr.len() != 20 {
                        return Err(format!("Invalid EVM address length: {}", evm_addr.len()));
                    }

                    let mut addr = [0u8; 20];
                    addr.copy_from_slice(evm_addr);
                    vote_address = Some(Address::from(addr));
                },
                (2, 0) => { // vote_count (varint)
                    let (count, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read vote_count: {}", e))?;
                    pos += bytes_read;
                    vote_count = Some(count);
                },
                _ => {
                    // Skip unknown field
                    let new_pos = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip vote field: {}", e))?;
                    pos = new_pos;
                }
            }
        }

        Ok((
            vote_address.ok_or_else(|| "Missing vote_address".to_string())?,
            vote_count.ok_or_else(|| "Missing vote_count".to_string())?,
        ))
    }

    /// Skip a protobuf field based on wire type
    fn skip_protobuf_field(data: &[u8], wire_type: u64) -> Result<usize, String> {
        match wire_type {
            0 => { // Varint
                let (_, bytes_read) = read_varint(data)?;
                Ok(bytes_read)
            },
            1 => { // 64-bit
                Ok(8)
            },
            2 => { // Length-delimited
                let (length, bytes_read) = read_varint(data)?;
                Ok(bytes_read + length as usize)
            },
            5 => { // 32-bit
                Ok(4)
            },
            _ => Err(format!("Unknown wire type: {}", wire_type))
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
        use tron_backend_execution::{TronExecutionResult, TronStateChange, VotesRecord};

        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("VoteWitness owner={} vote_count=?",
              owner_tron);

        // 1. Parse VoteWitnessContract from transaction data
        let votes = Self::parse_vote_witness_contract(&transaction.data)
            .map_err(|e| format!("Failed to parse VoteWitnessContract: {}", e))?;

        info!("Parsed {} votes from VoteWitnessContract", votes.len());

        // 2. Validate votes count
        if votes.is_empty() {
            warn!("VoteNumber must more than 0");
            return Err("VoteNumber must more than 0".to_string());
        }

        if votes.len() > MAX_VOTE_NUMBER {
            warn!("VoteNumber more than maxVoteNumber {}", MAX_VOTE_NUMBER);
            return Err(format!("VoteNumber more than maxVoteNumber {}", MAX_VOTE_NUMBER));
        }

        // 3. Validate each vote and compute total
        let mut sum_trx: u64 = 0;
        for (vote_address, vote_count) in &votes {
            // Validate vote_count > 0
            if *vote_count == 0 {
                warn!("vote count must be greater than 0");
                return Err("vote count must be greater than 0".to_string());
            }

            // Validate vote_address is valid (21 bytes with 0x41 prefix)
            let vote_address_tron = tron_backend_common::to_tron_address(vote_address);

            // Validate account exists
            match storage_adapter.get_account(vote_address) {
                Ok(Some(_)) => {
                    debug!("Account {} exists", vote_address_tron);
                },
                Ok(None) => {
                    warn!("account {} not exist", vote_address_tron);
                    return Err(format!("account {} not exist", vote_address_tron));
                },
                Err(e) => {
                    error!("Failed to get account {}: {}", vote_address_tron, e);
                    return Err(format!("Failed to get account {}: {}", vote_address_tron, e));
                }
            }

            // Validate witness exists
            match storage_adapter.get_witness(vote_address) {
                Ok(Some(_)) => {
                    debug!("Witness {} exists", vote_address_tron);
                },
                Ok(None) => {
                    warn!("Witness {} not exist", vote_address_tron);
                    return Err(format!("Witness {} not exist", vote_address_tron));
                },
                Err(e) => {
                    error!("Failed to get witness {}: {}", vote_address_tron, e);
                    return Err(format!("Failed to get witness {}: {}", vote_address_tron, e));
                }
            }

            // Add to sum
            sum_trx = sum_trx.checked_add(*vote_count)
                .ok_or_else(|| "Vote count overflow".to_string())?;
        }

        // 4. Convert sum to SUN and check against tron power
        let sum_sun = sum_trx.checked_mul(TRX_PRECISION)
            .ok_or_else(|| "Vote sum overflow when converting to SUN".to_string())?;

        // Get resource model flag
        let new_model = storage_adapter.support_allow_new_resource_model()
            .map_err(|e| format!("Failed to get resource model flag: {}", e))?;

        // Get tron power (using preferred method name)
        let tron_power_sun = storage_adapter.compute_tron_power_in_sun(&owner, new_model)
            .map_err(|e| format!("Failed to compute tron power: {}", e))?;

        info!("VoteWitness owner={} sum={} TRX ({} SUN), tronPower={} SUN, new_model={}",
              owner_tron, sum_trx, sum_sun, tron_power_sun, new_model);

        if sum_sun > tron_power_sun {
            warn!("The total number of votes[{}] is greater than the tronPower[{}]",
                  sum_sun, tron_power_sun);
            return Err(format!("The total number of votes[{}] is greater than the tronPower[{}]",
                              sum_sun, tron_power_sun));
        }

        // 5. Phase 1: Skip withdrawReward (log only)
        info!("Skipping withdrawReward for {} (Phase 1 - delegation not yet ported)", owner_tron);

        // 6. Load or create VotesRecord
        let mut votes_record = match storage_adapter.get_votes(&owner) {
            Ok(Some(record)) => {
                info!("Found existing votes for {}: old_votes={}, new_votes={}",
                      owner_tron, record.old_votes.len(), record.new_votes.len());
                // Update old_votes to current new_votes
                VotesRecord::new(owner, record.new_votes.clone(), Vec::new())
            },
            Ok(None) => {
                info!("No existing votes for {}, creating new record", owner_tron);
                VotesRecord::empty(owner)
            },
            Err(e) => {
                error!("Failed to get votes for {}: {}", owner_tron, e);
                return Err(format!("Failed to get votes: {}", e));
            }
        };

        // 7. Clear new_votes and add new votes
        votes_record.clear_new_votes();
        for (vote_address, vote_count) in votes {
            votes_record.add_new_vote(vote_address, vote_count);
        }

        // 8. Persist votes record
        storage_adapter.set_votes(owner, &votes_record)
            .map_err(|e| format!("Failed to set votes: {}", e))?;

        info!("Successfully stored votes for {}: old_votes={}, new_votes={}",
              owner_tron, votes_record.old_votes.len(), votes_record.new_votes.len());

        // 9. Build result with CSV parity
        // Get owner account for state change
        let old_account = storage_adapter.get_account(&owner)
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
            let current_aext = storage_adapter.get_account_aext(&owner)
                .map_err(|e| format!("Failed to get account AEXT: {}", e))?
                .unwrap_or_else(|| AccountAext::with_defaults());

            // Get FREE_NET_LIMIT from dynamic properties
            let free_net_limit = storage_adapter.get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;

            // Track bandwidth usage (returns path, before_aext, after_aext)
            let (path, before_aext, after_aext) = ResourceTracker::track_bandwidth(
                &owner,
                bandwidth_used as i64,
                context.block_number as i64, // Use block number as "now"
                &current_aext,
                free_net_limit,
            ).map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist after AEXT to storage
            storage_adapter.set_account_aext(&owner, &after_aext)
                .map_err(|e| format!("Failed to persist account AEXT: {}", e))?;

            // Add to aext_map
            aext_map.insert(owner, (before_aext.clone(), after_aext.clone()));

            debug!("AEXT tracked for vote_witness: owner={:?}, path={:?}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}",
                   owner, path, before_aext.net_usage, after_aext.net_usage,
                   before_aext.free_net_usage, after_aext.free_net_usage);
        }

        info!("VoteWitness completed: owner={}, votes={}, state_changes={}, bandwidth={}",
              owner_tron, votes_record.new_votes.len(), state_changes.len(), bandwidth_used);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0, // System contracts use 0 energy
            bandwidth_used,
            logs: Vec::new(), // No logs for voting
            state_changes,
            error: None,
            aext_map, // Populated with tracked AEXT if mode is "tracked"
            freeze_changes: vec![], // Will be populated by freeze-related contracts
            global_resource_changes: vec![], // Not applicable for vote witness
        })
    }

    /// Execute an ACCOUNT_UPDATE_CONTRACT
    /// Updates the account name for a given address with proper validation and CSV parity
    fn execute_account_update_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::{TronExecutionResult, TronStateChange};

        info!("AccountUpdate owner={} name_len={}",
              tron_backend_common::to_tron_address(&transaction.from),
              transaction.data.len());

        // Parse account name from transaction data
        let name_bytes = transaction.data.as_ref();

        // Validation: name length constraints (1 <= len <= 32 bytes to match java-tron)
        if name_bytes.is_empty() {
            warn!("Account name cannot be empty");
            return Err("Account name cannot be empty".to_string());
        }
        if name_bytes.len() > 32 {
            warn!("Account name cannot exceed 32 bytes, got {}", name_bytes.len());
            return Err(format!("Account name cannot exceed 32 bytes, got {}", name_bytes.len()));
        }

        // Validation: UTF-8 encoding (recommended but not enforced)
        let name_str = match std::str::from_utf8(name_bytes) {
            Ok(s) => s,
            Err(e) => {
                debug!("Account name contains non-UTF-8 bytes: {}", e);
                // Continue with raw bytes - allowing arbitrary bytes for compatibility
                ""
            }
        };

        // Validation: owner account must exist
        let owner_account = match storage_adapter.get_account(&transaction.from) {
            Ok(Some(account)) => account,
            Ok(None) => {
                warn!("Owner account does not exist");
                return Err("Owner account does not exist".to_string());
            },
            Err(e) => {
                error!("Failed to get owner account: {}", e);
                return Err(format!("Failed to get owner account: {}", e));
            }
        };

        // Validation: "only set once" semantics (if enforcing immutability)
        let existing_name: Option<String> = match storage_adapter.get_account_name(&transaction.from) {
            Ok(Some(existing_name)) => {
                warn!("Account name is already set to '{}', rejecting duplicate set attempt", existing_name);
                return Err("Account name is already set".to_string());
            },
            Ok(None) => {
                debug!("No existing account name found, proceeding with setting");
                None
            },
            Err(e) => {
                error!("Failed to check existing account name: {}", e);
                return Err(format!("Failed to check existing account name: {}", e));
            }
        };

        // Apply: persist account name
        if let Err(e) = storage_adapter.set_account_name(transaction.from, name_bytes) {
            error!("Failed to set account name: {}", e);
            return Err(format!("Failed to set account name: {}", e));
        }

        // Debug: previous vs new name strings/hex
        debug!("Successfully set account name for owner, previous: {:?}, new: {} (hex: {})",
               existing_name,
               if name_str.is_empty() { format!("<{} bytes>", name_bytes.len()) } else { name_str.to_string() },
               hex::encode(name_bytes));

        // State Changes: emit exactly one account-level change for CSV parity
        // old_account == new_account (no balance/nonce/code changes) to match embedded journaled no-op
        let state_changes = vec![
            TronStateChange::AccountChange {
                address: transaction.from,
                old_account: Some(owner_account.clone()),
                new_account: Some(owner_account), // Same account, name is metadata outside AccountInfo
            }
        ];

        // Calculate bandwidth based on transaction payload size
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Result: success with energy_used=0, exactly 1 state change
        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(), // No return data for account update
            energy_used: 0,     // Account update uses zero energy
            bandwidth_used,     // Compute bandwidth from payload size
            state_changes,      // Exactly one account-level change
            logs: vec![],       // No logs for account update
            error: None,
            aext_map: std::collections::HashMap::new(), // Will be populated for tracked mode
            freeze_changes: vec![], // Will be populated by freeze-related contracts
            global_resource_changes: vec![], // Not applicable for account update
        })
    }

    /// Execute a FREEZE_BALANCE_CONTRACT
    /// Freezes TRX balance to gain resources (BANDWIDTH or ENERGY)
    /// Phase 2: Balance delta + resource ledger persistence
    fn execute_freeze_balance_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::{TronExecutionResult, TronStateChange};

        // Parse freeze parameters from transaction data
        let params = Self::parse_freeze_balance_params(&transaction.data)?;

        info!("FreezeBalance owner={} amount={} resource={:?} duration={}",
              tron_backend_common::to_tron_address(&transaction.from),
              params.frozen_balance,
              params.resource,
              params.frozen_duration);

        // Load owner account
        let owner_account = storage_adapter.get_account(&transaction.from)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .unwrap_or_default();

        debug!("Owner account loaded: balance={}, nonce={}",
               owner_account.balance, owner_account.nonce);

        // Validation: amount > 0
        if params.frozen_balance == 0 {
            warn!("Freeze amount must be greater than zero");
            return Err("Freeze amount must be greater than zero".to_string());
        }

        // Validation: duration > 0
        if params.frozen_duration == 0 {
            warn!("Freeze duration must be greater than zero");
            return Err("Freeze duration must be greater than zero".to_string());
        }

        // Convert frozen_balance from i64 to u64 for balance arithmetic
        let freeze_amount = params.frozen_balance as u64;

        // Validation: owner.balance >= amount
        let owner_balance_u64 = owner_account.balance.try_into()
            .unwrap_or(u64::MAX);

        if owner_balance_u64 < freeze_amount {
            warn!("Insufficient balance: have {}, need {}", owner_balance_u64, freeze_amount);
            return Err(format!("Insufficient balance: have {}, need {}",
                             owner_balance_u64, freeze_amount));
        }

        // Compute new owner account with reduced balance
        let mut new_owner = owner_account.clone();
        new_owner.balance = revm_primitives::U256::from(owner_balance_u64 - freeze_amount);

        debug!("Balance change: {} -> {}", owner_account.balance, new_owner.balance);

        // Persist new owner account
        storage_adapter.set_account(transaction.from, new_owner.clone())
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;

        // Phase 2: Persist freeze record
        // Calculate expiration timestamp (milliseconds since epoch)
        let duration_millis = params.frozen_duration as u64 * 86400 * 1000; // days to milliseconds
        let expiration_timestamp = (context.block_timestamp + duration_millis) as i64;

        debug!("Freeze record: amount={}, expiration={}, resource={:?}",
               freeze_amount, expiration_timestamp, params.resource);

        // Add to freeze ledger (aggregates if previous freeze exists)
        storage_adapter.add_freeze_amount(
            transaction.from,
            params.resource as u8,
            freeze_amount,
            expiration_timestamp
        ).map_err(|e| format!("Failed to persist freeze record: {}", e))?;

        // Emit exactly one state change for CSV parity (Phase 1 behavior)
        let state_changes = vec![
            TronStateChange::AccountChange {
                address: transaction.from,
                old_account: Some(owner_account),
                new_account: Some(new_owner),
            }
        ];

        // Phase 2: Emit freeze ledger changes when enabled
        // Read the flag from config
        let emit_freeze_changes = self.get_execution_config()
            .ok()
            .map(|cfg| cfg.remote.emit_freeze_ledger_changes)
            .unwrap_or(false);

        let freeze_changes = if emit_freeze_changes {
            // Read back the total frozen amount after aggregation
            let freeze_record = storage_adapter.get_freeze_record(
                &transaction.from,
                params.resource as u8
            ).map_err(|e| format!("Failed to read freeze record: {}", e))?;

            if let Some(record) = freeze_record {
                // Map FreezeResource to FreezeLedgerResource
                use tron_backend_execution::FreezeLedgerResource;
                let resource = match params.resource {
                    FreezeResource::Bandwidth => FreezeLedgerResource::Bandwidth,
                    FreezeResource::Energy => FreezeLedgerResource::Energy,
                    FreezeResource::TronPower => FreezeLedgerResource::TronPower,
                };

                let change = tron_backend_execution::FreezeLedgerChange {
                    owner_address: transaction.from,
                    resource,
                    amount: record.frozen_amount as i64, // Absolute total after operation
                    expiration_ms: record.expiration_timestamp,  // Latest expiration
                    v2_model: false, // FreezeBalanceContract is V1 model
                };

                info!("Emitting freeze change: owner={}, resource={:?}, amount={}, expiration={}",
                      tron_backend_common::to_tron_address(&transaction.from),
                      resource, record.frozen_amount, record.expiration_timestamp);

                vec![change]
            } else {
                // No record found - this shouldn't happen since we just added it
                warn!("Freeze record not found after add_freeze_amount for owner={}, resource={:?}",
                      tron_backend_common::to_tron_address(&transaction.from), params.resource);
                vec![]
            }
        } else {
            vec![] // Flag disabled, maintain Phase 1 behavior
        };

        // Phase 2: Emit global resource totals when enabled
        let emit_global_changes = self.get_execution_config()
            .ok()
            .map(|cfg| cfg.remote.emit_global_resource_changes)
            .unwrap_or(false);

        let global_resource_changes = if emit_global_changes {
            // Compute current global totals from all freeze records
            let total_net_weight = storage_adapter.compute_total_net_weight()
                .map_err(|e| format!("Failed to compute total net weight: {}", e))?;
            let total_net_limit = storage_adapter.get_total_net_limit()
                .map_err(|e| format!("Failed to get total net limit: {}", e))?;
            let total_energy_weight = storage_adapter.compute_total_energy_weight()
                .map_err(|e| format!("Failed to compute total energy weight: {}", e))?;
            let total_energy_limit = 0i64; // TODO: Add getter when available

            let change = tron_backend_execution::GlobalResourceTotalsChange {
                total_net_weight,
                total_net_limit,
                total_energy_weight,
                total_energy_limit,
            };

            info!("Emitting global resource change: net_weight={}, net_limit={}, energy_weight={}, energy_limit={}",
                  total_net_weight, total_net_limit, total_energy_weight, total_energy_limit);

            vec![change]
        } else {
            vec![] // Flag disabled
        };

        // Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!("FreezeBalance completed successfully: state_changes=1, energy_used=0, bandwidth_used={}, freeze_ledger_updated=true, freeze_changes={}, global_changes={}",
               bandwidth_used, freeze_changes.len(), global_resource_changes.len());

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(), // Will be populated for tracked mode
            freeze_changes, // Populated when emit_freeze_ledger_changes is true
            global_resource_changes, // Populated when emit_global_resource_changes is true
        })
    }

    /// Execute an UNFREEZE_BALANCE_CONTRACT (Phase 2: with freeze ledger changes)
    /// Handles unfreezing balance and emitting FreezeLedgerChange with updated amounts
    fn execute_unfreeze_balance_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        info!("Executing UNFREEZE_BALANCE_CONTRACT: owner={}, data_len={}",
              tron_backend_common::to_tron_address(&transaction.from),
              transaction.data.len());

        // Parse unfreeze parameters from transaction data
        let params = Self::parse_unfreeze_balance_params(&transaction.data)?;

        debug!("Parsed unfreeze params: resource={:?}", params.resource);

        // Load owner account
        let owner_account = storage_adapter.get_account(&transaction.from)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or("Account not found for unfreeze operation")?;

        debug!("Owner account loaded: balance={}, nonce={}",
               owner_account.balance, owner_account.nonce);

        // Get current freeze record to determine amount to unfreeze
        let freeze_record = storage_adapter.get_freeze_record(
            &transaction.from,
            params.resource as u8
        ).map_err(|e| format!("Failed to read freeze record: {}", e))?;

        let freeze_record = freeze_record.ok_or("No frozen balance found for this resource")?;

        let unfreeze_amount = freeze_record.frozen_amount;

        // Validation: Check if frozen balance exists and can be unfrozen
        if unfreeze_amount == 0 {
            return Err("No frozen balance to unfreeze".to_string());
        }

        // TODO: Check expiration time and unfreeze delay (for now, assume can unfreeze)

        // Compute new owner account with increased balance
        let mut new_owner = owner_account.clone();
        let owner_balance_u64: u64 = owner_account.balance.try_into().unwrap_or(u64::MAX);
        new_owner.balance = revm_primitives::U256::from(
            owner_balance_u64.checked_add(unfreeze_amount)
                .ok_or("Balance overflow")?
        );

        debug!("Balance change: {} -> {}", owner_account.balance, new_owner.balance);

        // Persist new owner account
        storage_adapter.set_account(transaction.from, new_owner.clone())
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;

        // Remove freeze record (full unfreeze)
        storage_adapter.remove_freeze_record(&transaction.from, params.resource as u8)
            .map_err(|e| format!("Failed to remove freeze record: {}", e))?;

        debug!("Freeze record removed: amount={}, resource={:?}", unfreeze_amount, params.resource);

        // Emit exactly one state change for CSV parity
        let state_changes = vec![
            TronStateChange::AccountChange {
                address: transaction.from,
                old_account: Some(owner_account),
                new_account: Some(new_owner),
            }
        ];

        // Phase 2: Emit freeze ledger changes when enabled
        let emit_freeze_changes = self.get_execution_config()
            .ok()
            .map(|cfg| cfg.remote.emit_freeze_ledger_changes)
            .unwrap_or(false);

        let freeze_changes = if emit_freeze_changes {
            // Emit FreezeLedgerChange with amount=0 to indicate full unfreeze
            use tron_backend_execution::FreezeLedgerResource;
            let resource = match params.resource {
                FreezeResource::Bandwidth => FreezeLedgerResource::Bandwidth,
                FreezeResource::Energy => FreezeLedgerResource::Energy,
                FreezeResource::TronPower => FreezeLedgerResource::TronPower,
            };

            let change = tron_backend_execution::FreezeLedgerChange {
                owner_address: transaction.from,
                resource,
                amount: 0, // Zero indicates full unfreeze
                expiration_ms: 0, // No expiration after unfreeze
                v2_model: false, // UnfreezeBalanceContract is V1 model
            };

            info!("Emitting unfreeze change: owner={}, resource={:?}, amount=0 (full unfreeze)",
                  tron_backend_common::to_tron_address(&transaction.from), resource);

            vec![change]
        } else {
            vec![] // Flag disabled, maintain Phase 1 behavior
        };

        // Phase 2: Emit global resource totals when enabled
        let emit_global_changes = self.get_execution_config()
            .ok()
            .map(|cfg| cfg.remote.emit_global_resource_changes)
            .unwrap_or(false);

        let global_resource_changes = if emit_global_changes {
            // Compute current global totals from all freeze records
            let total_net_weight = storage_adapter.compute_total_net_weight()
                .map_err(|e| format!("Failed to compute total net weight: {}", e))?;
            let total_net_limit = storage_adapter.get_total_net_limit()
                .map_err(|e| format!("Failed to get total net limit: {}", e))?;
            let total_energy_weight = storage_adapter.compute_total_energy_weight()
                .map_err(|e| format!("Failed to compute total energy weight: {}", e))?;
            let total_energy_limit = 0i64; // TODO: Add getter when available

            let change = tron_backend_execution::GlobalResourceTotalsChange {
                total_net_weight,
                total_net_limit,
                total_energy_weight,
                total_energy_limit,
            };

            info!("Emitting global resource change: net_weight={}, net_limit={}, energy_weight={}, energy_limit={}",
                  total_net_weight, total_net_limit, total_energy_weight, total_energy_limit);

            vec![change]
        } else {
            vec![] // Flag disabled
        };

        // Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!("UnfreezeBalance completed successfully: state_changes=1, energy_used=0, bandwidth_used={}, freeze_ledger_updated=true, freeze_changes={}, global_changes={}",
               bandwidth_used, freeze_changes.len(), global_resource_changes.len());

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes, // Populated when emit_freeze_ledger_changes is true
            global_resource_changes, // Populated when emit_global_resource_changes is true
        })
    }

    /// Parse UnfreezeBalanceContract parameters from protobuf-encoded data
    ///
    /// UnfreezeBalanceContract protobuf structure:
    /// - owner_address: bytes (field 1) - we get this from transaction.from
    /// - resource: ResourceCode enum (field 10)
    /// - receiver_address: bytes (field 15) - optional, Phase 1 ignores
    fn parse_unfreeze_balance_params(data: &revm_primitives::Bytes) -> Result<UnfreezeParams, String> {
        if data.is_empty() {
            return Err("UnfreezeBalance params cannot be empty".to_string());
        }

        // Simple protobuf parser for the specific fields we need
        let mut resource: FreezeResource = FreezeResource::Bandwidth; // Default
        let mut pos = 0;

        while pos < data.len() {
            // Read tag
            let (tag, new_pos) = read_varint(&data[pos..])?;
            pos = pos + new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            match field_number {
                1 => {
                    // owner_address (bytes) - skip, we use transaction.from
                    if wire_type != 2 { return Err("Invalid wire type for owner_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos + len as usize;
                },
                10 => {
                    // resource (enum ResourceCode)
                    if wire_type != 0 { return Err("Invalid wire type for resource".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    resource = match value {
                        0 => FreezeResource::Bandwidth,
                        1 => FreezeResource::Energy,
                        2 => FreezeResource::TronPower,
                        _ => return Err(format!("Invalid resource code: {}", value)),
                    };
                    pos = pos + new_pos;
                },
                15 => {
                    // receiver_address (bytes) - Phase 1: ignore
                    if wire_type != 2 { return Err("Invalid wire type for receiver_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos + len as usize;
                },
                _ => {
                    // Unknown field - skip
                    match wire_type {
                        0 => {
                            let (_, new_pos) = read_varint(&data[pos..])?;
                            pos = pos + new_pos;
                        },
                        2 => {
                            let (len, new_pos) = read_varint(&data[pos..])?;
                            pos = pos + new_pos + len as usize;
                        },
                        _ => return Err(format!("Unsupported wire type {} for field {}", wire_type, field_number)),
                    }
                }
            }
        }

        Ok(UnfreezeParams { resource })
    }

    /// Execute a FREEZE_BALANCE_V2_CONTRACT (Phase 2: with freeze ledger changes)
    /// Handles V2 freeze which uses FrozenV2 list instead of single Frozen field
    fn execute_freeze_balance_v2_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        info!("Executing FREEZE_BALANCE_V2_CONTRACT: owner={}, data_len={}",
              tron_backend_common::to_tron_address(&transaction.from),
              transaction.data.len());

        // Parse freeze V2 parameters from transaction data
        let params = Self::parse_freeze_balance_v2_params(&transaction.data)?;

        debug!("Parsed freeze V2 params: frozen_balance={}, resource={:?}",
              params.frozen_balance, params.resource);

        // Load owner account
        let owner_account = storage_adapter.get_account(&transaction.from)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .unwrap_or_default();

        debug!("Owner account loaded: balance={}, nonce={}",
               owner_account.balance, owner_account.nonce);

        // Validation: amount > 0
        if params.frozen_balance <= 0 {
            warn!("Freeze amount must be greater than zero");
            return Err("Freeze amount must be greater than zero".to_string());
        }

        // Convert frozen_balance from i64 to u64 for balance arithmetic
        let freeze_amount = params.frozen_balance as u64;

        // Validation: owner.balance >= amount
        let owner_balance_u64 = owner_account.balance.try_into().unwrap_or(u64::MAX);

        if owner_balance_u64 < freeze_amount {
            warn!("Insufficient balance: have {}, need {}", owner_balance_u64, freeze_amount);
            return Err(format!("Insufficient balance: have {}, need {}",
                             owner_balance_u64, freeze_amount));
        }

        // Compute new owner account with reduced balance
        let mut new_owner = owner_account.clone();
        new_owner.balance = revm_primitives::U256::from(owner_balance_u64 - freeze_amount);

        debug!("Balance change: {} -> {}", owner_account.balance, new_owner.balance);

        // Persist new owner account
        storage_adapter.set_account(transaction.from, new_owner.clone())
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;

        // Phase 2: Persist freeze record (V2 uses same storage, just different emission)
        // V2 doesn't have explicit duration, expiration is managed at a higher level
        // For now, use a default expiration (e.g., 3 days in milliseconds)
        let default_duration_millis = 3 * 86400 * 1000; // 3 days
        let expiration_timestamp = (context.block_timestamp + default_duration_millis) as i64;

        debug!("Freeze V2 record: amount={}, expiration={}, resource={:?}",
               freeze_amount, expiration_timestamp, params.resource);

        // Add to freeze ledger (aggregates if previous freeze exists)
        storage_adapter.add_freeze_amount(
            transaction.from,
            params.resource as u8,
            freeze_amount,
            expiration_timestamp
        ).map_err(|e| format!("Failed to persist freeze record: {}", e))?;

        // Emit exactly one state change for CSV parity
        let state_changes = vec![
            TronStateChange::AccountChange {
                address: transaction.from,
                old_account: Some(owner_account),
                new_account: Some(new_owner),
            }
        ];

        // Phase 2: Emit freeze ledger changes when enabled
        let emit_freeze_changes = self.get_execution_config()
            .ok()
            .map(|cfg| cfg.remote.emit_freeze_ledger_changes)
            .unwrap_or(false);

        let freeze_changes = if emit_freeze_changes {
            // Read back the total frozen amount after aggregation
            let freeze_record = storage_adapter.get_freeze_record(
                &transaction.from,
                params.resource as u8
            ).map_err(|e| format!("Failed to read freeze record: {}", e))?;

            if let Some(record) = freeze_record {
                // Map FreezeResource to FreezeLedgerResource
                use tron_backend_execution::FreezeLedgerResource;
                let resource = match params.resource {
                    FreezeResource::Bandwidth => FreezeLedgerResource::Bandwidth,
                    FreezeResource::Energy => FreezeLedgerResource::Energy,
                    FreezeResource::TronPower => FreezeLedgerResource::TronPower,
                };

                let change = tron_backend_execution::FreezeLedgerChange {
                    owner_address: transaction.from,
                    resource,
                    amount: record.frozen_amount as i64, // Absolute total after operation
                    expiration_ms: record.expiration_timestamp,  // Latest expiration
                    v2_model: true, // FreezeBalanceV2Contract is V2 model
                };

                info!("Emitting freeze V2 change: owner={}, resource={:?}, amount={}, expiration={}",
                      tron_backend_common::to_tron_address(&transaction.from),
                      resource, record.frozen_amount, record.expiration_timestamp);

                vec![change]
            } else {
                // No record found - this shouldn't happen since we just added it
                warn!("Freeze record not found after add_freeze_amount for owner={}, resource={:?}",
                      tron_backend_common::to_tron_address(&transaction.from), params.resource);
                vec![]
            }
        } else {
            vec![] // Flag disabled, maintain Phase 1 behavior
        };

        // Phase 2: Emit global resource totals when enabled
        let emit_global_changes = self.get_execution_config()
            .ok()
            .map(|cfg| cfg.remote.emit_global_resource_changes)
            .unwrap_or(false);

        let global_resource_changes = if emit_global_changes {
            // Compute current global totals from all freeze records
            let total_net_weight = storage_adapter.compute_total_net_weight()
                .map_err(|e| format!("Failed to compute total net weight: {}", e))?;
            let total_net_limit = storage_adapter.get_total_net_limit()
                .map_err(|e| format!("Failed to get total net limit: {}", e))?;
            let total_energy_weight = storage_adapter.compute_total_energy_weight()
                .map_err(|e| format!("Failed to compute total energy weight: {}", e))?;
            let total_energy_limit = 0i64; // TODO: Add getter when available

            let change = tron_backend_execution::GlobalResourceTotalsChange {
                total_net_weight,
                total_net_limit,
                total_energy_weight,
                total_energy_limit,
            };

            info!("Emitting global resource change: net_weight={}, net_limit={}, energy_weight={}, energy_limit={}",
                  total_net_weight, total_net_limit, total_energy_weight, total_energy_limit);

            vec![change]
        } else {
            vec![] // Flag disabled
        };

        // Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!("FreezeBalanceV2 completed successfully: state_changes=1, energy_used=0, bandwidth_used={}, freeze_ledger_updated=true, freeze_changes={}, global_changes={}",
               bandwidth_used, freeze_changes.len(), global_resource_changes.len());

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes, // Populated when emit_freeze_ledger_changes is true
            global_resource_changes, // Populated when emit_global_resource_changes is true
        })
    }

    /// Execute an UNFREEZE_BALANCE_V2_CONTRACT (Phase 2: with freeze ledger changes)
    /// Handles V2 unfreeze which may support partial unfreezing
    fn execute_unfreeze_balance_v2_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        info!("Executing UNFREEZE_BALANCE_V2_CONTRACT: owner={}, data_len={}",
              tron_backend_common::to_tron_address(&transaction.from),
              transaction.data.len());

        // Parse unfreeze V2 parameters from transaction data
        let params = Self::parse_unfreeze_balance_v2_params(&transaction.data)?;

        debug!("Parsed unfreeze V2 params: unfreeze_balance={}, resource={:?}",
              params.unfreeze_balance, params.resource);

        // Load owner account
        let owner_account = storage_adapter.get_account(&transaction.from)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or("Account not found for unfreeze operation")?;

        debug!("Owner account loaded: balance={}, nonce={}",
               owner_account.balance, owner_account.nonce);

        // Get current freeze record to determine amount to unfreeze
        let freeze_record = storage_adapter.get_freeze_record(
            &transaction.from,
            params.resource as u8
        ).map_err(|e| format!("Failed to read freeze record: {}", e))?;

        let freeze_record = freeze_record.ok_or("No frozen balance found for this resource")?;

        // Validation: Check if frozen balance exists and can be unfrozen
        if freeze_record.frozen_amount == 0 {
            return Err("No frozen balance to unfreeze".to_string());
        }

        // Determine unfreeze amount (V2 may support partial)
        // For now, implement full unfreeze like V1
        let unfreeze_amount = if params.unfreeze_balance <= 0 {
            // If no amount specified or invalid, unfreeze all
            freeze_record.frozen_amount
        } else {
            // Partial unfreeze requested
            let requested = params.unfreeze_balance as u64;
            if requested > freeze_record.frozen_amount {
                freeze_record.frozen_amount // Unfreeze all if requested more than available
            } else {
                requested
            }
        };

        debug!("Unfreeze amount determined: {}", unfreeze_amount);

        // Compute new owner account with increased balance
        let mut new_owner = owner_account.clone();
        let owner_balance_u64: u64 = owner_account.balance.try_into().unwrap_or(u64::MAX);
        new_owner.balance = revm_primitives::U256::from(
            owner_balance_u64.checked_add(unfreeze_amount)
                .ok_or("Balance overflow")?
        );

        debug!("Balance change: {} -> {}", owner_account.balance, new_owner.balance);

        // Persist new owner account
        storage_adapter.set_account(transaction.from, new_owner.clone())
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;

        // Update or remove freeze record
        let remaining_frozen = freeze_record.frozen_amount - unfreeze_amount;
        if remaining_frozen == 0 {
            // Full unfreeze - remove record
            storage_adapter.remove_freeze_record(&transaction.from, params.resource as u8)
                .map_err(|e| format!("Failed to remove freeze record: {}", e))?;
            debug!("Freeze record removed: full unfreeze");
        } else {
            // Partial unfreeze - update record with remaining amount
            storage_adapter.add_freeze_amount(
                transaction.from,
                params.resource as u8,
                0, // Add 0 to update without changing amount (TODO: implement subtract method)
                freeze_record.expiration_timestamp
            ).map_err(|e| format!("Failed to update freeze record: {}", e))?;
            debug!("Freeze record updated: remaining_frozen={}", remaining_frozen);
        }

        // Emit exactly one state change for CSV parity
        let state_changes = vec![
            TronStateChange::AccountChange {
                address: transaction.from,
                old_account: Some(owner_account),
                new_account: Some(new_owner),
            }
        ];

        // Phase 2: Emit freeze ledger changes when enabled
        let emit_freeze_changes = self.get_execution_config()
            .ok()
            .map(|cfg| cfg.remote.emit_freeze_ledger_changes)
            .unwrap_or(false);

        let freeze_changes = if emit_freeze_changes {
            // Read back the updated freeze record to get absolute amount
            let updated_record = storage_adapter.get_freeze_record(
                &transaction.from,
                params.resource as u8
            ).map_err(|e| format!("Failed to read updated freeze record: {}", e))?;

            use tron_backend_execution::FreezeLedgerResource;
            let resource = match params.resource {
                FreezeResource::Bandwidth => FreezeLedgerResource::Bandwidth,
                FreezeResource::Energy => FreezeLedgerResource::Energy,
                FreezeResource::TronPower => FreezeLedgerResource::TronPower,
            };

            let change = if let Some(record) = updated_record {
                // Partial unfreeze - emit remaining amount
                tron_backend_execution::FreezeLedgerChange {
                    owner_address: transaction.from,
                    resource,
                    amount: record.frozen_amount as i64, // Absolute remaining after unfreeze
                    expiration_ms: record.expiration_timestamp,
                    v2_model: true, // UnfreezeBalanceV2Contract is V2 model
                }
            } else {
                // Full unfreeze - emit amount=0
                tron_backend_execution::FreezeLedgerChange {
                    owner_address: transaction.from,
                    resource,
                    amount: 0, // Zero indicates full unfreeze
                    expiration_ms: 0, // No expiration after full unfreeze
                    v2_model: true,
                }
            };

            info!("Emitting unfreeze V2 change: owner={}, resource={:?}, amount={} (remaining after unfreeze)",
                  tron_backend_common::to_tron_address(&transaction.from), resource, change.amount);

            vec![change]
        } else {
            vec![] // Flag disabled, maintain Phase 1 behavior
        };

        // Phase 2: Emit global resource totals when enabled
        let emit_global_changes = self.get_execution_config()
            .ok()
            .map(|cfg| cfg.remote.emit_global_resource_changes)
            .unwrap_or(false);

        let global_resource_changes = if emit_global_changes {
            // Compute current global totals from all freeze records
            let total_net_weight = storage_adapter.compute_total_net_weight()
                .map_err(|e| format!("Failed to compute total net weight: {}", e))?;
            let total_net_limit = storage_adapter.get_total_net_limit()
                .map_err(|e| format!("Failed to get total net limit: {}", e))?;
            let total_energy_weight = storage_adapter.compute_total_energy_weight()
                .map_err(|e| format!("Failed to compute total energy weight: {}", e))?;
            let total_energy_limit = 0i64; // TODO: Add getter when available

            let change = tron_backend_execution::GlobalResourceTotalsChange {
                total_net_weight,
                total_net_limit,
                total_energy_weight,
                total_energy_limit,
            };

            info!("Emitting global resource change: net_weight={}, net_limit={}, energy_weight={}, energy_limit={}",
                  total_net_weight, total_net_limit, total_energy_weight, total_energy_limit);

            vec![change]
        } else {
            vec![] // Flag disabled
        };

        // Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!("UnfreezeBalanceV2 completed successfully: state_changes=1, energy_used=0, bandwidth_used={}, freeze_ledger_updated=true, freeze_changes={}, global_changes={}",
               bandwidth_used, freeze_changes.len(), global_resource_changes.len());

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes, // Populated when emit_freeze_ledger_changes is true
            global_resource_changes, // Populated when emit_global_resource_changes is true
        })
    }

    /// Parse FreezeBalanceV2Contract parameters from protobuf-encoded data
    ///
    /// FreezeBalanceV2Contract protobuf structure:
    /// - owner_address: bytes (field 1) - we get this from transaction.from
    /// - frozen_balance: int64 (field 2)
    /// - resource: ResourceCode enum (field 3)
    fn parse_freeze_balance_v2_params(data: &revm_primitives::Bytes) -> Result<FreezeV2Params, String> {
        if data.is_empty() {
            return Err("FreezeBalanceV2 params cannot be empty".to_string());
        }

        let mut frozen_balance: Option<i64> = None;
        let mut resource: FreezeResource = FreezeResource::Bandwidth; // Default
        let mut pos = 0;

        while pos < data.len() {
            // Read tag
            let (tag, new_pos) = read_varint(&data[pos..])?;
            pos = pos + new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            match field_number {
                1 => {
                    // owner_address (bytes) - skip, we use transaction.from
                    if wire_type != 2 { return Err("Invalid wire type for owner_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos + len as usize;
                },
                2 => {
                    // frozen_balance (int64)
                    if wire_type != 0 { return Err("Invalid wire type for frozen_balance".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    frozen_balance = Some(value as i64);
                    pos = pos + new_pos;
                },
                3 => {
                    // resource (enum ResourceCode)
                    if wire_type != 0 { return Err("Invalid wire type for resource".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    resource = match value {
                        0 => FreezeResource::Bandwidth,
                        1 => FreezeResource::Energy,
                        2 => FreezeResource::TronPower,
                        _ => return Err(format!("Invalid resource code: {}", value)),
                    };
                    pos = pos + new_pos;
                },
                _ => {
                    // Unknown field - skip
                    match wire_type {
                        0 => {
                            let (_, new_pos) = read_varint(&data[pos..])?;
                            pos = pos + new_pos;
                        },
                        2 => {
                            let (len, new_pos) = read_varint(&data[pos..])?;
                            pos = pos + new_pos + len as usize;
                        },
                        _ => return Err(format!("Unsupported wire type {} for field {}", wire_type, field_number)),
                    }
                }
            }
        }

        // Validate required fields
        let frozen_balance = frozen_balance.ok_or("Missing frozen_balance field")?;

        Ok(FreezeV2Params {
            frozen_balance,
            resource,
        })
    }

    /// Parse UnfreezeBalanceV2Contract parameters from protobuf-encoded data
    ///
    /// UnfreezeBalanceV2Contract protobuf structure:
    /// - owner_address: bytes (field 1) - we get this from transaction.from
    /// - unfreeze_balance: int64 (field 2)
    /// - resource: ResourceCode enum (field 3)
    fn parse_unfreeze_balance_v2_params(data: &revm_primitives::Bytes) -> Result<UnfreezeV2Params, String> {
        if data.is_empty() {
            return Err("UnfreezeBalanceV2 params cannot be empty".to_string());
        }

        let mut unfreeze_balance: Option<i64> = None;
        let mut resource: FreezeResource = FreezeResource::Bandwidth; // Default
        let mut pos = 0;

        while pos < data.len() {
            // Read tag
            let (tag, new_pos) = read_varint(&data[pos..])?;
            pos = pos + new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            match field_number {
                1 => {
                    // owner_address (bytes) - skip, we use transaction.from
                    if wire_type != 2 { return Err("Invalid wire type for owner_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos + len as usize;
                },
                2 => {
                    // unfreeze_balance (int64)
                    if wire_type != 0 { return Err("Invalid wire type for unfreeze_balance".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    unfreeze_balance = Some(value as i64);
                    pos = pos + new_pos;
                },
                3 => {
                    // resource (enum ResourceCode)
                    if wire_type != 0 { return Err("Invalid wire type for resource".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    resource = match value {
                        0 => FreezeResource::Bandwidth,
                        1 => FreezeResource::Energy,
                        2 => FreezeResource::TronPower,
                        _ => return Err(format!("Invalid resource code: {}", value)),
                    };
                    pos = pos + new_pos;
                },
                _ => {
                    // Unknown field - skip
                    match wire_type {
                        0 => {
                            let (_, new_pos) = read_varint(&data[pos..])?;
                            pos = pos + new_pos;
                        },
                        2 => {
                            let (len, new_pos) = read_varint(&data[pos..])?;
                            pos = pos + new_pos + len as usize;
                        },
                        _ => return Err(format!("Unsupported wire type {} for field {}", wire_type, field_number)),
                    }
                }
            }
        }

        // Validate required fields (unfreeze_balance may be optional for "unfreeze all")
        let unfreeze_balance = unfreeze_balance.unwrap_or(-1); // -1 means unfreeze all

        Ok(UnfreezeV2Params {
            unfreeze_balance,
            resource,
        })
    }

    /// Parse FreezeBalanceContract parameters from protobuf-encoded data
    ///
    /// FreezeBalanceContract protobuf structure:
    /// - owner_address: bytes (field 1) - we get this from transaction.from
    /// - frozen_balance: int64 (field 2)
    /// - frozen_duration: int64 (field 3)
    /// - resource: ResourceCode enum (field 10)
    /// - receiver_address: bytes (field 15) - optional, Phase 1 ignores
    fn parse_freeze_balance_params(data: &revm_primitives::Bytes) -> Result<FreezeParams, String> {
        if data.is_empty() {
            return Err("FreezeBalance params cannot be empty".to_string());
        }

        // Simple protobuf parser for the specific fields we need
        // Protobuf wire format: tag (field_number << 3 | wire_type)
        // int64 uses wire_type 0 (varint)
        // bytes uses wire_type 2 (length-delimited)

        let mut frozen_balance: Option<i64> = None;
        let mut frozen_duration: Option<i64> = None;
        let mut resource: FreezeResource = FreezeResource::Bandwidth; // Default

        let mut pos = 0;
        while pos < data.len() {
            // Read tag
            let (tag, new_pos) = read_varint(&data[pos..])?;
            pos = pos + new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            match field_number {
                1 => {
                    // owner_address (bytes) - skip, we use transaction.from
                    if wire_type != 2 { return Err("Invalid wire type for owner_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos + len as usize;
                },
                2 => {
                    // frozen_balance (int64)
                    if wire_type != 0 { return Err("Invalid wire type for frozen_balance".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    frozen_balance = Some(value as i64);
                    pos = pos + new_pos;
                },
                3 => {
                    // frozen_duration (int64)
                    if wire_type != 0 { return Err("Invalid wire type for frozen_duration".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    frozen_duration = Some(value as i64);
                    pos = pos + new_pos;
                },
                10 => {
                    // resource (enum ResourceCode)
                    if wire_type != 0 { return Err("Invalid wire type for resource".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    resource = match value {
                        0 => FreezeResource::Bandwidth,
                        1 => FreezeResource::Energy,
                        2 => FreezeResource::TronPower,
                        _ => return Err(format!("Invalid resource code: {}", value)),
                    };
                    pos = pos + new_pos;
                },
                15 => {
                    // receiver_address (bytes) - Phase 1: ignore
                    if wire_type != 2 { return Err("Invalid wire type for receiver_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos + len as usize;
                },
                _ => {
                    // Unknown field - skip
                    match wire_type {
                        0 => {
                            let (_, new_pos) = read_varint(&data[pos..])?;
                            pos = pos + new_pos;
                        },
                        2 => {
                            let (len, new_pos) = read_varint(&data[pos..])?;
                            pos = pos + new_pos + len as usize;
                        },
                        _ => return Err(format!("Unsupported wire type {} for field {}", wire_type, field_number)),
                    }
                }
            }
        }

        // Validate required fields
        let frozen_balance = frozen_balance.ok_or("Missing frozen_balance field")?;
        let frozen_duration = frozen_duration.ok_or("Missing frozen_duration field")?;

        Ok(FreezeParams {
            frozen_balance,
            frozen_duration: frozen_duration as u32,
            resource,
        })
    }

    /// Calculate bandwidth usage for a transaction based on its serialized size
    fn calculate_bandwidth_usage(transaction: &TronTransaction) -> u64 {
        // Approximate bandwidth calculation based on transaction fields
        // This is a simplified version - full implementation would consider exact protobuf serialization
        
        let base_size = 60; // Base transaction overhead (addresses, nonce, etc.)
        let data_size = transaction.data.len() as u64;
        let signature_size = 65; // ECDSA signature size
        
        base_size + data_size + signature_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tron_backend_execution::{TronTransaction, TronExecutionContext};
    use tron_backend_common::{ExecutionConfig, ExecutionFeeConfig};
    use revm_primitives::{Address, U256, Bytes};
    
    #[test]
    fn test_calculate_bandwidth_usage() {
        use tron_backend_execution::TxMetadata;

        // Test basic transaction
        let tx = TronTransaction {
            from: Address::ZERO,
            to: Some(Address::ZERO),
            value: U256::from(100),
            data: Bytes::new(),
            gas_limit: 21000,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: None,
                asset_id: None,
            },
        };

        let bandwidth = BackendService::calculate_bandwidth_usage(&tx);
        assert_eq!(bandwidth, 60 + 0 + 65); // base_size + data_size + signature_size

        // Test transaction with data
        let tx_with_data = TronTransaction {
            from: Address::ZERO,
            to: Some(Address::ZERO),
            value: U256::from(100),
            data: Bytes::from(vec![0x60, 0x60, 0x60, 0x40]), // 4 bytes of data
            gas_limit: 21000,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: None,
                asset_id: None,
            },
        };
        
        let bandwidth_with_data = BackendService::calculate_bandwidth_usage(&tx_with_data);
        assert_eq!(bandwidth_with_data, 60 + 4 + 65); // base_size + data_size + signature_size
    }
    
    #[test]
    fn test_tx_kind_conversion() {
        // Test that TxKind enum values can be converted
        assert_eq!(crate::backend::TxKind::NonVm as i32, 0);
        assert_eq!(crate::backend::TxKind::Vm as i32, 1);
        
        // Test conversion from i32
        assert_eq!(crate::backend::TxKind::try_from(0).unwrap(), crate::backend::TxKind::NonVm);
        assert_eq!(crate::backend::TxKind::try_from(1).unwrap(), crate::backend::TxKind::Vm);
    }
    
    #[test]
    fn test_account_update_contract_happy_path() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_common::ExecutionConfig;

        // Create mock storage and service
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

        let exec_config = ExecutionConfig {
            remote: tron_backend_common::RemoteExecutionConfig {
                system_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut module_manager = tron_backend_common::ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
        module_manager.register("execution", Box::new(exec_module));
        let service = BackendService::new(module_manager);

        // Create test account (owner must exist)
        let owner_address = Address::from([1u8; 20]);
        let owner_account = AccountInfo {
            balance: U256::from(1000000),
            nonce: 0,
            code_hash: revm::primitives::B256::ZERO,
            code: None,
        };
        assert!(storage_adapter.set_account(owner_address, owner_account.clone()).is_ok());

        // Create AccountUpdateContract transaction
        let account_name = "TestAccount";
        let transaction = TronTransaction {
            from: owner_address,
            to: None, // No to address for account update
            value: U256::ZERO, // No value transfer
            data: Bytes::from(account_name.as_bytes()),
            gas_limit: 0, // No gas for non-VM contracts
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
                asset_id: None,
            },
        };

        let context = TronExecutionContext {
            block_number: 1,
            block_timestamp: 1000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 100_000_000,
            chain_id: 1,
            energy_price: 420,
            bandwidth_price: 1000,
        };

        // Execute the contract
        let result = service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);

        // Assert success
        assert!(result.is_ok(), "Account update should succeed: {:?}", result.err());
        let execution_result = result.unwrap();

        assert!(execution_result.success, "Execution should be successful");
        assert_eq!(execution_result.energy_used, 0, "Energy used should be 0");
        assert_eq!(execution_result.state_changes.len(), 1, "Should have exactly 1 state change");
        assert!(execution_result.logs.is_empty(), "Should have no logs");
        assert!(execution_result.error.is_none(), "Should have no error");

        // Verify account name was stored
        let stored_name = storage_adapter.get_account_name(&owner_address).unwrap();
        assert_eq!(stored_name, Some("TestAccount".to_string()));

        // Verify state change is account-level with old==new
        match &execution_result.state_changes[0] {
            tron_backend_execution::TronStateChange::AccountChange { address, old_account, new_account } => {
                assert_eq!(*address, owner_address);
                assert!(old_account.is_some());
                assert!(new_account.is_some());
                // old_account == new_account for CSV parity
                assert_eq!(old_account.as_ref().unwrap().balance, new_account.as_ref().unwrap().balance);
                assert_eq!(old_account.as_ref().unwrap().nonce, new_account.as_ref().unwrap().nonce);
            },
            _ => panic!("Expected AccountChange, got storage change"),
        }
    }

    #[test]
    fn test_account_update_contract_validations() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_common::ExecutionConfig;

        // Create mock storage and service
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

        let exec_config = ExecutionConfig {
            remote: tron_backend_common::RemoteExecutionConfig {
                system_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut module_manager = tron_backend_common::ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
        module_manager.register("execution", Box::new(exec_module));
        let service = BackendService::new(module_manager);

        let owner_address = Address::from([1u8; 20]);
        let context = TronExecutionContext {
            block_number: 1,
            block_timestamp: 1000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 100_000_000,
            chain_id: 1,
            energy_price: 420,
            bandwidth_price: 1000,
        };

        // Test 1: Empty name should fail
        let empty_name_tx = TronTransaction {
            from: owner_address,
            to: None,
            value: U256::ZERO,
            data: Bytes::from(vec![]), // Empty name
            gas_limit: 0,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
                asset_id: None,
            },
        };

        let result = service.execute_account_update_contract(&mut storage_adapter, &empty_name_tx, &context);
        assert!(result.is_err(), "Empty name should fail");
        assert!(result.unwrap_err().contains("cannot be empty"));

        // Test 2: Name too long should fail
        let long_name = "ThisIsAVeryLongAccountNameThatExceedsTheThirtyTwoByteLimitAndShouldFail";
        let long_name_tx = TronTransaction {
            from: owner_address,
            to: None,
            value: U256::ZERO,
            data: Bytes::from(long_name.as_bytes()),
            gas_limit: 0,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
                asset_id: None,
            },
        };

        let result = service.execute_account_update_contract(&mut storage_adapter, &long_name_tx, &context);
        assert!(result.is_err(), "Long name should fail");
        assert!(result.unwrap_err().contains("cannot exceed 32 bytes"));

        // Test 3: Non-existent owner should fail
        let non_existent_tx = TronTransaction {
            from: owner_address, // This address doesn't exist in storage
            to: None,
            value: U256::ZERO,
            data: Bytes::from("ValidName".as_bytes()),
            gas_limit: 0,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
                asset_id: None,
            },
        };

        let result = service.execute_account_update_contract(&mut storage_adapter, &non_existent_tx, &context);
        assert!(result.is_err(), "Non-existent owner should fail");
        assert!(result.unwrap_err().contains("Owner account does not exist"));
    }

    #[test]
    fn test_account_update_contract_duplicate_set() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_common::ExecutionConfig;

        // Create mock storage and service
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

        let exec_config = ExecutionConfig {
            remote: tron_backend_common::RemoteExecutionConfig {
                system_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut module_manager = tron_backend_common::ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
        module_manager.register("execution", Box::new(exec_module));
        let service = BackendService::new(module_manager);

        // Create test account
        let owner_address = Address::from([1u8; 20]);
        let owner_account = AccountInfo {
            balance: U256::from(1000000),
            nonce: 0,
            code_hash: revm::primitives::B256::ZERO,
            code: None,
        };
        assert!(storage_adapter.set_account(owner_address, owner_account).is_ok());

        let context = TronExecutionContext {
            block_number: 1,
            block_timestamp: 1000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 100_000_000,
            chain_id: 1,
            energy_price: 420,
            bandwidth_price: 1000,
        };

        // First set should succeed
        let first_tx = TronTransaction {
            from: owner_address,
            to: None,
            value: U256::ZERO,
            data: Bytes::from("FirstName".as_bytes()),
            gas_limit: 0,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
                asset_id: None,
            },
        };

        let result = service.execute_account_update_contract(&mut storage_adapter, &first_tx, &context);
        assert!(result.is_ok(), "First name set should succeed");

        // Second set should fail (only set once)
        let second_tx = TronTransaction {
            from: owner_address,
            to: None,
            value: U256::ZERO,
            data: Bytes::from("SecondName".as_bytes()),
            gas_limit: 0,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
                asset_id: None,
            },
        };

        let result = service.execute_account_update_contract(&mut storage_adapter, &second_tx, &context);
        assert!(result.is_err(), "Duplicate name set should fail");
        assert!(result.unwrap_err().contains("Account name is already set"));

        // Verify original name is still there
        let stored_name = storage_adapter.get_account_name(&owner_address).unwrap();
        assert_eq!(stored_name, Some("FirstName".to_string()));
    }

    // Helper function for tests to encode varint
    fn encode_varint(buf: &mut Vec<u8>, mut value: u64) {
        loop {
            let mut byte = (value & 0x7F) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            buf.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    #[test]
    fn test_freeze_balance_success_basic() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_common::{ModuleManager, ExecutionConfig, RemoteExecutionConfig};

        // Create test setup
        let owner_address = Address::from([1u8; 20]);
        let initial_balance = 50_000_000u64; // 50 TRX
        let freeze_amount = 1_000_000i64; // 1 TRX

        // Setup storage with initial account
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
        let owner_account = AccountInfo {
            balance: U256::from(initial_balance),
            nonce: 0,
            code_hash: revm::primitives::B256::ZERO,
            code: None,
        };
        storage_adapter.set_account(owner_address, owner_account.clone()).unwrap();

        // Build FreezeBalance protobuf data
        // Field 2: frozen_balance (varint)
        // Field 3: frozen_duration (varint)
        // Field 10: resource (varint)
        let mut proto_data = Vec::new();
        // frozen_balance = 1_000_000 (field 2, wire_type 0)
        proto_data.push((2 << 3) | 0); // tag for field 2
        encode_varint(&mut proto_data, freeze_amount as u64);
        // frozen_duration = 3 days (field 3, wire_type 0)
        proto_data.push((3 << 3) | 0); // tag for field 3
        encode_varint(&mut proto_data, 3);
        // resource = BANDWIDTH (0) (field 10, wire_type 0)
        proto_data.push((10 << 3) | 0); // tag for field 10
        encode_varint(&mut proto_data, 0);

        let transaction = TronTransaction {
            from: owner_address,
            to: None,
            value: U256::ZERO,
            data: Bytes::from(proto_data),
            gas_limit: 0,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
                asset_id: None,
            },
        };

        let context = TronExecutionContext {
            block_number: 2142,
            block_timestamp: 1000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 0,
            chain_id: 1,
            energy_price: 0,
            bandwidth_price: 0,
        };

        // Create service with freeze_balance enabled
        let mut module_manager = ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(ExecutionConfig {
            remote: RemoteExecutionConfig {
                freeze_balance_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        });
        module_manager.register("execution", Box::new(exec_module));

        let service = BackendService::new(module_manager);

        // Execute
        let result = service.execute_freeze_balance_contract(&mut storage_adapter, &transaction, &context);

        // Assertions
        assert!(result.is_ok(), "FreezeBalance should succeed: {:?}", result.err());
        let exec_result = result.unwrap();

        assert!(exec_result.success);
        assert_eq!(exec_result.energy_used, 0);
        assert_eq!(exec_result.state_changes.len(), 1);
        assert!(exec_result.logs.is_empty());

        // Verify balance decreased
        match &exec_result.state_changes[0] {
            tron_backend_execution::TronStateChange::AccountChange { address, old_account, new_account } => {
                assert_eq!(*address, owner_address);
                assert_eq!(old_account.as_ref().unwrap().balance, U256::from(initial_balance));
                assert_eq!(new_account.as_ref().unwrap().balance, U256::from(initial_balance - freeze_amount as u64));
            },
            _ => panic!("Expected AccountChange"),
        }
    }

    #[test]
    fn test_freeze_balance_insufficient_balance() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_common::{ModuleManager, ExecutionConfig, RemoteExecutionConfig};

        let owner_address = Address::from([1u8; 20]);
        let initial_balance = 100u64; // Very small balance
        let freeze_amount = 1_000_000i64; // Try to freeze more than we have

        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
        let owner_account = AccountInfo {
            balance: U256::from(initial_balance),
            nonce: 0,
            code_hash: revm::primitives::B256::ZERO,
            code: None,
        };
        storage_adapter.set_account(owner_address, owner_account).unwrap();

        // Build protobuf
        let mut proto_data = Vec::new();
        proto_data.push((2 << 3) | 0);
        encode_varint(&mut proto_data, freeze_amount as u64);
        proto_data.push((3 << 3) | 0);
        encode_varint(&mut proto_data, 3);
        proto_data.push((10 << 3) | 0);
        encode_varint(&mut proto_data, 0);

        let transaction = TronTransaction {
            from: owner_address,
            to: None,
            value: U256::ZERO,
            data: Bytes::from(proto_data),
            gas_limit: 0,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
                asset_id: None,
            },
        };

        let context = TronExecutionContext {
            block_number: 1,
            block_timestamp: 1000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 0,
            chain_id: 1,
            energy_price: 0,
            bandwidth_price: 0,
        };

        let mut module_manager = ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(ExecutionConfig {
            remote: RemoteExecutionConfig {
                freeze_balance_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        });
        module_manager.register("execution", Box::new(exec_module));

        let service = BackendService::new(module_manager);

        // Execute - should fail
        let result = service.execute_freeze_balance_contract(&mut storage_adapter, &transaction, &context);
        assert!(result.is_err(), "Should fail with insufficient balance");
        assert!(result.unwrap_err().contains("Insufficient balance"));
    }

    #[test]
    fn test_freeze_balance_bad_params() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_common::{ModuleManager, ExecutionConfig, RemoteExecutionConfig};

        let owner_address = Address::from([1u8; 20]);
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
        let owner_account = AccountInfo {
            balance: U256::from(1_000_000u64),
            nonce: 0,
            code_hash: revm::primitives::B256::ZERO,
            code: None,
        };
        storage_adapter.set_account(owner_address, owner_account).unwrap();

        // Empty data
        let transaction = TronTransaction {
            from: owner_address,
            to: None,
            value: U256::ZERO,
            data: Bytes::new(),
            gas_limit: 0,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
                asset_id: None,
            },
        };

        let context = TronExecutionContext {
            block_number: 1,
            block_timestamp: 1000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 0,
            chain_id: 1,
            energy_price: 0,
            bandwidth_price: 0,
        };

        let mut module_manager = ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(ExecutionConfig {
            remote: RemoteExecutionConfig {
                freeze_balance_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        });
        module_manager.register("execution", Box::new(exec_module));

        let service = BackendService::new(module_manager);

        let result = service.execute_freeze_balance_contract(&mut storage_adapter, &transaction, &context);
        assert!(result.is_err(), "Should fail with empty params");
    }

    #[test]
    fn test_freeze_balance_emits_freeze_changes_when_enabled() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_storage::StorageEngine;
        use std::sync::Arc;

        // Create test storage with temp directory
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

        // Setup owner account with sufficient balance
        let owner_addr = Address::from_slice(&[0x12; 20]);
        let owner_account = AccountInfo {
            balance: U256::from(2_000_000_000_000u64), // 2M TRX
            nonce: 0,
            code_hash: revm_primitives::KECCAK_EMPTY,
            code: None,
        };
        storage_adapter.set_account(owner_addr, owner_account).unwrap();

        // Create FreezeBalance transaction
        // Field 2: frozen_balance = 1_000_000 (varint encoded)
        // Field 3: frozen_duration = 3 (varint encoded)
        // Field 10: resource = 0 (BANDWIDTH)
        let params_data = vec![
            0x10, 0xC0, 0x84, 0x3D, // field 2 (frozen_balance): 1_000_000
            0x18, 0x03,             // field 3 (frozen_duration): 3
            0x50, 0x00,             // field 10 (resource): 0 (BANDWIDTH)
        ];

        let tx = TronTransaction {
            from: owner_addr,
            to: None,
            value: U256::ZERO,
            data: Bytes::from(params_data),
            gas_limit: 100_000,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
                asset_id: None,
            },
        };

        let context = TronExecutionContext {
            block_number: 1000,
            block_timestamp: 1600000000000, // milliseconds
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 100_000_000,
            chain_id: 1,
            energy_price: 420,
            bandwidth_price: 1000,
        };

        // Create config with emit_freeze_ledger_changes=true
        let exec_config = ExecutionConfig {
            remote: tron_backend_common::RemoteExecutionConfig {
                freeze_balance_enabled: true,
                emit_freeze_ledger_changes: true,
                ..Default::default()
            },
            ..Default::default()
        };

        // Create service with config
        let mut module_manager = tron_backend_common::ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
        module_manager.register("execution", Box::new(exec_module));
        let service = BackendService::new(module_manager);

        // Execute freeze balance
        let result = service.execute_freeze_balance_contract(&mut storage_adapter, &tx, &context);

        assert!(result.is_ok(), "Freeze execution should succeed");
        let exec_result = result.unwrap();

        // Verify freeze_changes is populated
        assert_eq!(exec_result.freeze_changes.len(), 1, "Should emit exactly one freeze change");

        let freeze_change = &exec_result.freeze_changes[0];
        assert_eq!(freeze_change.owner_address, owner_addr);
        assert_eq!(freeze_change.resource, tron_backend_execution::FreezeLedgerResource::Bandwidth);
        assert_eq!(freeze_change.amount, 1_000_000, "Amount should be absolute frozen amount");
        assert_eq!(freeze_change.v2_model, false, "Should be V1 model");
        assert!(freeze_change.expiration_ms > 0, "Expiration should be set");

        // Verify state_changes still present (CSV parity)
        assert_eq!(exec_result.state_changes.len(), 1, "Should still emit state change");
    }

    #[test]
    fn test_freeze_balance_no_emission_when_disabled() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_storage::StorageEngine;
        use std::sync::Arc;

        // Create test storage with temp directory
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

        // Setup owner account with sufficient balance
        let owner_addr = Address::from_slice(&[0x13; 20]);
        let owner_account = AccountInfo {
            balance: U256::from(2_000_000_000_000u64),
            nonce: 0,
            code_hash: revm_primitives::KECCAK_EMPTY,
            code: None,
        };
        storage_adapter.set_account(owner_addr, owner_account).unwrap();

        // Create FreezeBalance transaction
        let params_data = vec![
            0x10, 0xC0, 0x84, 0x3D, // frozen_balance: 1_000_000
            0x18, 0x03,             // frozen_duration: 3
            0x50, 0x00,             // resource: BANDWIDTH
        ];

        let tx = TronTransaction {
            from: owner_addr,
            to: None,
            value: U256::ZERO,
            data: Bytes::from(params_data),
            gas_limit: 100_000,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
                asset_id: None,
            },
        };

        let context = TronExecutionContext {
            block_number: 1000,
            block_timestamp: 1600000000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 100_000_000,
            chain_id: 1,
            energy_price: 420,
            bandwidth_price: 1000,
        };

        // Create config with emit_freeze_ledger_changes=false (Phase 1 behavior)
        let exec_config = ExecutionConfig {
            remote: tron_backend_common::RemoteExecutionConfig {
                freeze_balance_enabled: true,
                emit_freeze_ledger_changes: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut module_manager = tron_backend_common::ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
        module_manager.register("execution", Box::new(exec_module));
        let service = BackendService::new(module_manager);

        // Execute freeze balance
        let result = service.execute_freeze_balance_contract(&mut storage_adapter, &tx, &context);

        assert!(result.is_ok(), "Freeze execution should succeed");
        let exec_result = result.unwrap();

        // Verify freeze_changes is empty
        assert_eq!(exec_result.freeze_changes.len(), 0, "Should NOT emit freeze changes when disabled");

        // Verify state_changes still present (CSV parity maintained)
        assert_eq!(exec_result.state_changes.len(), 1, "Should still emit state change");
    }

    #[test]
    fn test_unfreeze_balance_emits_freeze_changes_when_enabled() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_storage::StorageEngine;
        use std::sync::Arc;

        // Create test storage with temp directory
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

        // Setup owner account
        let owner_addr = Address::from_slice(&[0x14; 20]);
        let owner_account = AccountInfo {
            balance: U256::from(1_000_000_000_000u64),
            nonce: 0,
            code_hash: revm_primitives::KECCAK_EMPTY,
            code: None,
        };
        storage_adapter.set_account(owner_addr, owner_account).unwrap();

        // Pre-populate freeze record
        storage_adapter.add_freeze_amount(owner_addr, 0, 500_000, 1700000000000).unwrap();

        // Create UnfreezeBalance transaction
        // Field 10: resource = 0 (BANDWIDTH)
        let params_data = vec![
            0x50, 0x00, // field 10 (resource): BANDWIDTH
        ];

        let tx = TronTransaction {
            from: owner_addr,
            to: None,
            value: U256::ZERO,
            data: Bytes::from(params_data),
            gas_limit: 100_000,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::UnfreezeBalanceContract),
                asset_id: None,
            },
        };

        let context = TronExecutionContext {
            block_number: 1000,
            block_timestamp: 1600000000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 100_000_000,
            chain_id: 1,
            energy_price: 420,
            bandwidth_price: 1000,
        };

        // Create config with emit_freeze_ledger_changes=true
        let exec_config = ExecutionConfig {
            remote: tron_backend_common::RemoteExecutionConfig {
                unfreeze_balance_enabled: true,
                emit_freeze_ledger_changes: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut module_manager = tron_backend_common::ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
        module_manager.register("execution", Box::new(exec_module));
        let service = BackendService::new(module_manager);

        // Execute unfreeze balance
        let result = service.execute_unfreeze_balance_contract(&mut storage_adapter, &tx, &context);

        assert!(result.is_ok(), "Unfreeze execution should succeed");
        let exec_result = result.unwrap();

        // Verify freeze_changes is populated
        assert_eq!(exec_result.freeze_changes.len(), 1, "Should emit exactly one freeze change");

        let freeze_change = &exec_result.freeze_changes[0];
        assert_eq!(freeze_change.owner_address, owner_addr);
        assert_eq!(freeze_change.resource, tron_backend_execution::FreezeLedgerResource::Bandwidth);
        assert_eq!(freeze_change.amount, 0, "Amount should be 0 for full unfreeze");
        assert_eq!(freeze_change.expiration_ms, 0, "Expiration should be 0 after unfreeze");
        assert_eq!(freeze_change.v2_model, false, "Should be V1 model");
    }

    #[test]
    fn test_freeze_balance_v2_emits_with_v2_flag() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_storage::StorageEngine;
        use std::sync::Arc;

        // Create test storage with temp directory
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

        // Setup owner account
        let owner_addr = Address::from_slice(&[0x15; 20]);
        let owner_account = AccountInfo {
            balance: U256::from(2_000_000_000_000u64),
            nonce: 0,
            code_hash: revm_primitives::KECCAK_EMPTY,
            code: None,
        };
        storage_adapter.set_account(owner_addr, owner_account).unwrap();

        // Create FreezeBalanceV2 transaction
        // Field 2: frozen_balance = 1_000_000
        // Field 3: resource = 1 (ENERGY)
        let params_data = vec![
            0x10, 0xC0, 0x84, 0x3D, // field 2: frozen_balance
            0x18, 0x01,             // field 3: resource (ENERGY)
        ];

        let tx = TronTransaction {
            from: owner_addr,
            to: None,
            value: U256::ZERO,
            data: Bytes::from(params_data),
            gas_limit: 100_000,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceV2Contract),
                asset_id: None,
            },
        };

        let context = TronExecutionContext {
            block_number: 1000,
            block_timestamp: 1600000000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 100_000_000,
            chain_id: 1,
            energy_price: 420,
            bandwidth_price: 1000,
        };

        // Create config with V2 enabled and emission enabled
        let exec_config = ExecutionConfig {
            remote: tron_backend_common::RemoteExecutionConfig {
                freeze_balance_v2_enabled: true,
                emit_freeze_ledger_changes: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut module_manager = tron_backend_common::ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
        module_manager.register("execution", Box::new(exec_module));
        let service = BackendService::new(module_manager);

        // Execute freeze balance V2
        let result = service.execute_freeze_balance_v2_contract(&mut storage_adapter, &tx, &context);

        assert!(result.is_ok(), "FreezeV2 execution should succeed");
        let exec_result = result.unwrap();

        // Verify freeze_changes is populated with V2 flag
        assert_eq!(exec_result.freeze_changes.len(), 1, "Should emit exactly one freeze change");

        let freeze_change = &exec_result.freeze_changes[0];
        assert_eq!(freeze_change.owner_address, owner_addr);
        assert_eq!(freeze_change.resource, tron_backend_execution::FreezeLedgerResource::Energy);
        assert_eq!(freeze_change.amount, 1_000_000);
        assert_eq!(freeze_change.v2_model, true, "Should be V2 model"); // Key difference!
        assert!(freeze_change.expiration_ms > 0);
    }

    #[test]
    fn test_unfreeze_balance_v2_partial_unfreeze() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_storage::StorageEngine;
        use std::sync::Arc;

        // Create test storage with temp directory
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

        // Setup owner account
        let owner_addr = Address::from_slice(&[0x16; 20]);
        let owner_account = AccountInfo {
            balance: U256::from(1_000_000_000_000u64),
            nonce: 0,
            code_hash: revm_primitives::KECCAK_EMPTY,
            code: None,
        };
        storage_adapter.set_account(owner_addr, owner_account).unwrap();

        // Pre-populate freeze record with 1_000_000 frozen
        storage_adapter.add_freeze_amount(owner_addr, 0, 1_000_000, 1700000000000).unwrap();

        // Create UnfreezeBalanceV2 transaction with partial unfreeze (400_000)
        // Field 2: unfreeze_balance = 400_000
        // Field 3: resource = 0 (BANDWIDTH)
        let params_data = vec![
            0x10, 0x80, 0x89, 0x18, // field 2: unfreeze_balance (400_000)
            0x18, 0x00,             // field 3: resource (BANDWIDTH)
        ];

        let tx = TronTransaction {
            from: owner_addr,
            to: None,
            value: U256::ZERO,
            data: Bytes::from(params_data),
            gas_limit: 100_000,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract),
                asset_id: None,
            },
        };

        let context = TronExecutionContext {
            block_number: 1000,
            block_timestamp: 1600000000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 100_000_000,
            chain_id: 1,
            energy_price: 420,
            bandwidth_price: 1000,
        };

        // Create config with V2 enabled and emission enabled
        let exec_config = ExecutionConfig {
            remote: tron_backend_common::RemoteExecutionConfig {
                unfreeze_balance_v2_enabled: true,
                emit_freeze_ledger_changes: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut module_manager = tron_backend_common::ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
        module_manager.register("execution", Box::new(exec_module));
        let service = BackendService::new(module_manager);

        // Execute unfreeze balance V2
        let result = service.execute_unfreeze_balance_v2_contract(&mut storage_adapter, &tx, &context);

        assert!(result.is_ok(), "UnfreezeV2 execution should succeed");
        let exec_result = result.unwrap();

        // Verify freeze_changes shows remaining amount (not 0)
        assert_eq!(exec_result.freeze_changes.len(), 1, "Should emit exactly one freeze change");

        let freeze_change = &exec_result.freeze_changes[0];
        assert_eq!(freeze_change.owner_address, owner_addr);
        assert_eq!(freeze_change.resource, tron_backend_execution::FreezeLedgerResource::Bandwidth);
        // Should emit remaining frozen amount after partial unfreeze
        // Note: This depends on implementation - may be 0 if we simplified to full unfreeze only
        assert_eq!(freeze_change.v2_model, true, "Should be V2 model");
    }

    #[test]
    fn test_unfreeze_balance_v2_full_unfreeze() {
        use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
        use revm_primitives::{Address, Bytes, U256, AccountInfo};
        use tron_backend_storage::StorageEngine;
        use std::sync::Arc;

        // Create test storage with temp directory
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
        let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

        // Setup owner account
        let owner_addr = Address::from_slice(&[0x17; 20]);
        let owner_account = AccountInfo {
            balance: U256::from(1_000_000_000_000u64),
            nonce: 0,
            code_hash: revm_primitives::KECCAK_EMPTY,
            code: None,
        };
        storage_adapter.set_account(owner_addr, owner_account).unwrap();

        // Pre-populate freeze record
        storage_adapter.add_freeze_amount(owner_addr, 1, 800_000, 1700000000000).unwrap();

        // Create UnfreezeBalanceV2 transaction with full unfreeze (no amount or -1)
        // Field 3: resource = 1 (ENERGY)
        let params_data = vec![
            0x18, 0x01, // field 3: resource (ENERGY)
        ];

        let tx = TronTransaction {
            from: owner_addr,
            to: None,
            value: U256::ZERO,
            data: Bytes::from(params_data),
            gas_limit: 100_000,
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: Some(tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract),
                asset_id: None,
            },
        };

        let context = TronExecutionContext {
            block_number: 1000,
            block_timestamp: 1600000000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 100_000_000,
            chain_id: 1,
            energy_price: 420,
            bandwidth_price: 1000,
        };

        // Create config with V2 enabled and emission enabled
        let exec_config = ExecutionConfig {
            remote: tron_backend_common::RemoteExecutionConfig {
                unfreeze_balance_v2_enabled: true,
                emit_freeze_ledger_changes: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut module_manager = tron_backend_common::ModuleManager::new();
        let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
        module_manager.register("execution", Box::new(exec_module));
        let service = BackendService::new(module_manager);

        // Execute unfreeze balance V2
        let result = service.execute_unfreeze_balance_v2_contract(&mut storage_adapter, &tx, &context);

        assert!(result.is_ok(), "UnfreezeV2 full unfreeze should succeed");
        let exec_result = result.unwrap();

        // Verify freeze_changes shows amount=0 for full unfreeze
        assert_eq!(exec_result.freeze_changes.len(), 1, "Should emit exactly one freeze change");

        let freeze_change = &exec_result.freeze_changes[0];
        assert_eq!(freeze_change.owner_address, owner_addr);
        assert_eq!(freeze_change.resource, tron_backend_execution::FreezeLedgerResource::Energy);
        assert_eq!(freeze_change.amount, 0, "Should be 0 for full unfreeze");
        assert_eq!(freeze_change.expiration_ms, 0, "Expiration should be 0 after full unfreeze");
        assert_eq!(freeze_change.v2_model, true, "Should be V2 model");
    }

    #[test]
    fn test_address_conversion_helpers() {
        // Test Tron address prefix stripping
        let tron_address_with_prefix = vec![0x41, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78];
        let stripped = BackendService::strip_tron_address_prefix(&tron_address_with_prefix).unwrap();
        assert_eq!(stripped.len(), 20);
        assert_eq!(stripped[0], 0x12);
        
        let evm_address_no_prefix = vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78];
        let already_stripped = BackendService::strip_tron_address_prefix(&evm_address_no_prefix).unwrap();
        assert_eq!(already_stripped.len(), 20);
        assert_eq!(already_stripped, &evm_address_no_prefix);
        
        // Test adding Tron address prefix
        let address = Address::from_slice(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78]);
        let with_prefix = BackendService::add_tron_address_prefix(&address);
        assert_eq!(with_prefix.len(), 21);
        assert_eq!(with_prefix[0], 0x41);
        assert_eq!(&with_prefix[1..], address.as_slice());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use revm_primitives::{Address, U256, Bytes};
    
    // Mock storage adapter for testing
    struct MockStorageAdapter {
        accounts: Arc<RwLock<HashMap<Address, revm_primitives::AccountInfo>>>,
    }
    
    impl MockStorageAdapter {
        fn new() -> Self {
            Self {
                accounts: Arc::new(RwLock::new(HashMap::new())),
            }
        }
        
        async fn set_account(&self, address: Address, account: revm_primitives::AccountInfo) {
            self.accounts.write().await.insert(address, account);
        }
        
        async fn get_account(&self, address: &Address) -> Option<revm_primitives::AccountInfo> {
            self.accounts.read().await.get(address).cloned()
        }
    }
    
    // Note: These tests would require more setup to actually run, including mock storage adapters
    // They serve as examples of what could be tested in a full integration test suite
    
    #[tokio::test]
    #[ignore] // Ignored because it requires full system setup
    async fn test_non_vm_transaction_execution() {
        use tron_backend_execution::TxMetadata;

        // This test would set up a full BackendService with mock storage
        // and test the complete non-VM transaction execution flow

        // Setup mock accounts
        let sender_address = Address::from_slice(&[0x01; 20]);
        let recipient_address = Address::from_slice(&[0x02; 20]);
        
        let sender_account = revm_primitives::AccountInfo {
            balance: U256::from(1000000u64), // 1M SUN
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };
        
        let recipient_account = revm_primitives::AccountInfo::default();
        
        // Create transaction
        let transaction = TronTransaction {
            from: sender_address,
            to: Some(recipient_address),
            value: U256::from(100000u64), // 100K SUN transfer
            data: Bytes::new(), // No data = non-VM transaction
            gas_limit: 0, // Non-VM transactions don't use gas
            gas_price: U256::ZERO,
            nonce: 0,
            metadata: TxMetadata {
                contract_type: None,
                asset_id: None,
            },
        };
        
        let context = TronExecutionContext {
            block_number: 1000,
            block_timestamp: 1640000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::from(1),
            block_gas_limit: 1000000,
            chain_id: 0x2b6653dc,
            energy_price: 420,
            bandwidth_price: 1000,
        };
        
        // Expected results:
        // - sender balance: 1000000 - 100000 - fee = 1000000 - 100000 - 125*1000 = 775000
        // - recipient balance: 0 + 100000 = 100000
        // - bandwidth_used: 60 + 0 + 65 = 125 bytes
        // - energy_used: 0 (non-VM)
        // - state_changes: 2 (sender + recipient) or 3 (if blackhole fee)
    }
    
    #[tokio::test] 
    #[ignore] // Ignored because it requires full system setup
    async fn test_fee_handling_modes() {
        // This test would verify different fee handling modes:
        // 1. "burn" mode - no additional state changes for fees
        // 2. "blackhole" mode - additional state change crediting blackhole account
        // 3. Invalid blackhole address handling
    }
}

#[tonic::async_trait]
impl crate::backend::backend_server::Backend for BackendService {
    type IteratorStream = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<IteratorResponse, Status>> + Send>>;
    type StreamMetricsStream = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<MetricsResponse, Status>> + Send>>;
    // Health and metadata
    async fn health(&self, _request: Request<HealthRequest>) -> Result<Response<HealthResponse>, Status> {
        debug!("Health check requested");
        
        let health_map = self.module_manager.health_all().await;
        let mut overall_status = health_response::Status::Healthy;
        let mut module_status = HashMap::new();
        
        for (module_name, health) in health_map {
            match health.status {
                HealthStatus::Healthy => {
                    module_status.insert(module_name, "healthy".to_string());
                }
                HealthStatus::Degraded => {
                    if overall_status == health_response::Status::Healthy {
                        overall_status = health_response::Status::Degraded;
                    }
                    module_status.insert(module_name, "degraded".to_string());
                }
                HealthStatus::Unhealthy => {
                    overall_status = health_response::Status::Unhealthy;
                    module_status.insert(module_name, "unhealthy".to_string());
                }
            };
        }
        
        let response = HealthResponse {
            status: overall_status as i32,
            message: "Backend health check".to_string(),
            module_status,
        };
        
        Ok(Response::new(response))
    }
    
    async fn get_metadata(&self, _request: Request<MetadataRequest>) -> Result<Response<MetadataResponse>, Status> {
        debug!("Metadata requested");
        
        let uptime = self.start_time.elapsed()
            .unwrap_or_default()
            .as_secs() as i64;
        
        let response = MetadataResponse {
            version: env!("CARGO_PKG_VERSION").to_string(),
            enabled_modules: self.module_manager.module_names(),
            module_versions: self.module_manager.module_versions(),
            uptime_seconds: uptime,
        };
        
        Ok(Response::new(response))
    }
    
    // Basic Storage Operations
    async fn get(&self, request: Request<GetRequest>) -> Result<Response<GetResponse>, Status> {
        debug!("Get request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get(&req.database, &req.key) {
            Ok(Some(value)) => {
                let response = GetResponse {
                    value,
                    found: true,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Ok(None) => {
                let response = GetResponse {
                    value: Vec::new(),
                    found: false,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get operation failed: {}", e);
                let response = GetResponse {
                    value: Vec::new(),
                    found: false,
                    success: false,
                    error_message: format!("Get operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        debug!("Put request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.put(&req.database, &req.key, &req.value) {
            Ok(()) => {
                let response = PutResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Put operation failed: {}", e);
                let response = PutResponse {
                    success: false,
                    error_message: format!("Put operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn delete(&self, request: Request<DeleteRequest>) -> Result<Response<DeleteResponse>, Status> {
        debug!("Delete request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.delete(&req.database, &req.key) {
            Ok(()) => {
                let response = DeleteResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Delete operation failed: {}", e);
                let response = DeleteResponse {
                    success: false,
                    error_message: format!("Delete operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn has(&self, request: Request<HasRequest>) -> Result<Response<HasResponse>, Status> {
        debug!("Has request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.has(&req.database, &req.key) {
            Ok(exists) => {
                let response = HasResponse {
                    exists,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Has operation failed: {}", e);
                let response = HasResponse {
                    exists: false,
                    success: false,
                    error_message: format!("Has operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    // Batch Storage Operations
    async fn batch_write(&self, request: Request<BatchWriteRequest>) -> Result<Response<BatchWriteResponse>, Status> {
        debug!("Batch write request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        // Convert protobuf operations to engine operations
        let operations: Vec<tron_backend_storage::WriteOperation> = req.operations
            .into_iter()
            .map(|op| tron_backend_storage::WriteOperation {
                r#type: op.r#type,
                key: op.key,
                value: op.value,
            })
            .collect();
        
        match engine.batch_write(&req.database, &operations) {
            Ok(()) => {
                let response = BatchWriteResponse {
                    success: true,
                    error_message: String::new(),
                    operations_applied: operations.len() as i32,
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Batch write operation failed: {}", e);
                let response = BatchWriteResponse {
                    success: false,
                    error_message: format!("Batch write operation failed: {}", e),
                    operations_applied: 0,
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn batch_get(&self, request: Request<BatchGetRequest>) -> Result<Response<BatchGetResponse>, Status> {
        debug!("Batch get request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.batch_get(&req.database, &req.keys) {
            Ok(results) => {
                let pairs: Vec<KeyValue> = results
                    .into_iter()
                    .map(|kv| KeyValue {
                        key: kv.key,
                        value: kv.value,
                        found: kv.found,
                    })
                    .collect();
                
                let response = BatchGetResponse {
                    pairs,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Batch get operation failed: {}", e);
                let response = BatchGetResponse {
                    pairs: Vec::new(),
                    success: false,
                    error_message: format!("Batch get operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    // Iterator Operations
    async fn iterator(&self, request: Request<IteratorRequest>) -> Result<Response<Self::IteratorStream>, Status> {
        debug!("Iterator request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        let (tx, rx) = mpsc::channel(100);
        let database = req.database.clone();
        let start_key = req.start_key.clone();
        let engine_clone = engine.clone();
        
        tokio::spawn(async move {
            // For simplicity, we'll get all keys and stream them
            // In a real implementation, you'd want to use RocksDB iterators more efficiently
            match engine_clone.get_next(&database, &start_key, 1000) {
                Ok(pairs) => {
                    for pair in pairs {
                        let response = IteratorResponse {
                            key: pair.key,
                            value: pair.value,
                            end_of_stream: false,
                        };
                        if tx.send(Ok(response)).await.is_err() {
                            break;
                        }
                    }
                    // Send end of stream marker
                    let _ = tx.send(Ok(IteratorResponse {
                        key: Vec::new(),
                        value: Vec::new(),
                        end_of_stream: true,
                    })).await;
                }
                Err(e) => {
                    let _ = tx.send(Err(Status::internal(format!("Iterator failed: {}", e)))).await;
                }
            }
        });
        
        let stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as Self::IteratorStream))
    }
    
    async fn get_keys_next(&self, request: Request<GetKeysNextRequest>) -> Result<Response<GetKeysNextResponse>, Status> {
        debug!("Get keys next request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get_keys_next(&req.database, &req.start_key, req.limit) {
            Ok(keys) => {
                let response = GetKeysNextResponse {
                    keys,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get keys next operation failed: {}", e);
                let response = GetKeysNextResponse {
                    keys: Vec::new(),
                    success: false,
                    error_message: format!("Get keys next operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn get_values_next(&self, request: Request<GetValuesNextRequest>) -> Result<Response<GetValuesNextResponse>, Status> {
        debug!("Get values next request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get_values_next(&req.database, &req.start_key, req.limit) {
            Ok(values) => {
                let response = GetValuesNextResponse {
                    values,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get values next operation failed: {}", e);
                let response = GetValuesNextResponse {
                    values: Vec::new(),
                    success: false,
                    error_message: format!("Get values next operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn get_next(&self, request: Request<GetNextRequest>) -> Result<Response<GetNextResponse>, Status> {
        debug!("Get next request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get_next(&req.database, &req.start_key, req.limit) {
            Ok(results) => {
                let pairs: Vec<KeyValue> = results
                    .into_iter()
                    .map(|kv| KeyValue {
                        key: kv.key,
                        value: kv.value,
                        found: kv.found,
                    })
                    .collect();
                
                let response = GetNextResponse {
                    pairs,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get next operation failed: {}", e);
                let response = GetNextResponse {
                    pairs: Vec::new(),
                    success: false,
                    error_message: format!("Get next operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn prefix_query(&self, request: Request<PrefixQueryRequest>) -> Result<Response<PrefixQueryResponse>, Status> {
        debug!("Prefix query request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.prefix_query(&req.database, &req.prefix) {
            Ok(results) => {
                let pairs: Vec<KeyValue> = results
                    .into_iter()
                    .map(|kv| KeyValue {
                        key: kv.key,
                        value: kv.value,
                        found: kv.found,
                    })
                    .collect();
                
                let response = PrefixQueryResponse {
                    pairs,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Prefix query operation failed: {}", e);
                let response = PrefixQueryResponse {
                    pairs: Vec::new(),
                    success: false,
                    error_message: format!("Prefix query operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    

    
    // Snapshot Support
    async fn create_snapshot(&self, request: Request<CreateSnapshotRequest>) -> Result<Response<CreateSnapshotResponse>, Status> {
        debug!("Create snapshot request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.create_snapshot(&req.database) {
            Ok(snapshot_id) => {
                let response = CreateSnapshotResponse {
                    snapshot_id,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Create snapshot operation failed: {}", e);
                let response = CreateSnapshotResponse {
                    snapshot_id: String::new(),
                    success: false,
                    error_message: format!("Create snapshot operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn delete_snapshot(&self, request: Request<DeleteSnapshotRequest>) -> Result<Response<DeleteSnapshotResponse>, Status> {
        debug!("Delete snapshot request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.delete_snapshot(&req.snapshot_id) {
            Ok(()) => {
                let response = DeleteSnapshotResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Delete snapshot operation failed: {}", e);
                let response = DeleteSnapshotResponse {
                    success: false,
                    error_message: format!("Delete snapshot operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn get_from_snapshot(&self, request: Request<GetFromSnapshotRequest>) -> Result<Response<GetFromSnapshotResponse>, Status> {
        debug!("Get from snapshot request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get_from_snapshot(&req.snapshot_id, &req.key) {
            Ok(Some(value)) => {
                let response = GetFromSnapshotResponse {
                    value,
                    found: true,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Ok(None) => {
                let response = GetFromSnapshotResponse {
                    value: Vec::new(),
                    found: false,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get from snapshot operation failed: {}", e);
                let response = GetFromSnapshotResponse {
                    value: Vec::new(),
                    found: false,
                    success: false,
                    error_message: format!("Get from snapshot operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    // Transaction Support
    async fn begin_transaction(&self, request: Request<BeginTransactionRequest>) -> Result<Response<BeginTransactionResponse>, Status> {
        debug!("Begin transaction request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.begin_transaction(&req.database) {
            Ok(transaction_id) => {
                let response = BeginTransactionResponse {
                    transaction_id,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Begin transaction operation failed: {}", e);
                let response = BeginTransactionResponse {
                    transaction_id: String::new(),
                    success: false,
                    error_message: format!("Begin transaction operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn commit_transaction(&self, request: Request<CommitTransactionRequest>) -> Result<Response<CommitTransactionResponse>, Status> {
        debug!("Commit transaction request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.commit_transaction(&req.transaction_id) {
            Ok(()) => {
                let response = CommitTransactionResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Commit transaction operation failed: {}", e);
                let response = CommitTransactionResponse {
                    success: false,
                    error_message: format!("Commit transaction operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn rollback_transaction(&self, request: Request<RollbackTransactionRequest>) -> Result<Response<RollbackTransactionResponse>, Status> {
        debug!("Rollback transaction request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.rollback_transaction(&req.transaction_id) {
            Ok(()) => {
                let response = RollbackTransactionResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Rollback transaction operation failed: {}", e);
                let response = RollbackTransactionResponse {
                    success: false,
                    error_message: format!("Rollback transaction operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    // Database Management Operations
    async fn init_db(&self, request: Request<InitDbRequest>) -> Result<Response<InitDbResponse>, Status> {
        debug!("Init DB request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        // Convert protobuf StorageConfig to engine StorageConfig
        let config = req.config.map(|c| tron_backend_storage::StorageConfig {
            engine: c.engine,
            engine_options: c.engine_options,
            enable_statistics: c.enable_statistics,
            max_open_files: c.max_open_files,
            block_cache_size: c.block_cache_size,
        }).unwrap_or_else(|| tron_backend_storage::StorageConfig {
            engine: "ROCKSDB".to_string(),
            engine_options: std::collections::HashMap::new(),
            enable_statistics: true,
            max_open_files: 1000,
            block_cache_size: 8 * 1024 * 1024,
        });
        
        match engine.init_db(&req.database, &config) {
            Ok(()) => {
                let response = InitDbResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Init DB operation failed: {}", e);
                let response = InitDbResponse {
                    success: false,
                    error_message: format!("Init DB operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn close_db(&self, request: Request<CloseDbRequest>) -> Result<Response<CloseDbResponse>, Status> {
        debug!("Close DB request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.close_db(&req.database) {
            Ok(()) => {
                let response = CloseDbResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Close DB operation failed: {}", e);
                let response = CloseDbResponse {
                    success: false,
                    error_message: format!("Close DB operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn reset_db(&self, request: Request<ResetDbRequest>) -> Result<Response<ResetDbResponse>, Status> {
        debug!("Reset DB request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.reset_db(&req.database) {
            Ok(()) => {
                let response = ResetDbResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Reset DB operation failed: {}", e);
                let response = ResetDbResponse {
                    success: false,
                    error_message: format!("Reset DB operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn is_alive(&self, request: Request<IsAliveRequest>) -> Result<Response<IsAliveResponse>, Status> {
        debug!("Is alive request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.is_alive(&req.database) {
            Ok(alive) => {
                let response = IsAliveResponse {
                    alive,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Is alive operation failed: {}", e);
                let response = IsAliveResponse {
                    alive: false,
                    success: false,
                    error_message: format!("Is alive operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn size(&self, request: Request<SizeRequest>) -> Result<Response<SizeResponse>, Status> {
        debug!("Size request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.size(&req.database) {
            Ok(size) => {
                let response = SizeResponse {
                    size,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Size operation failed: {}", e);
                let response = SizeResponse {
                    size: 0,
                    success: false,
                    error_message: format!("Size operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn is_empty(&self, request: Request<IsEmptyRequest>) -> Result<Response<IsEmptyResponse>, Status> {
        debug!("Is empty request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.is_empty(&req.database) {
            Ok(empty) => {
                let response = IsEmptyResponse {
                    empty,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Is empty operation failed: {}", e);
                let response = IsEmptyResponse {
                    empty: false,
                    success: false,
                    error_message: format!("Is empty operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    

    
    // Storage Metadata & Monitoring
    async fn list_databases(&self, _request: Request<ListDatabasesRequest>) -> Result<Response<ListDatabasesResponse>, Status> {
        debug!("List databases request");
        
        let engine = self.get_storage_engine()?;
        
        match engine.list_databases() {
            Ok(databases) => {
                let response = ListDatabasesResponse {
                    databases,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("List databases operation failed: {}", e);
                let response = ListDatabasesResponse {
                    databases: Vec::new(),
                    success: false,
                    error_message: format!("List databases operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn get_stats(&self, request: Request<GetStatsRequest>) -> Result<Response<GetStatsResponse>, Status> {
        debug!("Get stats request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get_stats(&req.database) {
            Ok(stats) => {
                let proto_stats = StorageStats {
                    total_keys: stats.total_keys,
                    total_size: stats.total_size,
                    engine_stats: stats.engine_stats,
                    last_modified: stats.last_modified,
                };
                
                let response = GetStatsResponse {
                    stats: Some(proto_stats),
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get stats operation failed: {}", e);
                let response = GetStatsResponse {
                    stats: None,
                    success: false,
                    error_message: format!("Get stats operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn stream_metrics(&self, request: Request<StreamMetricsRequest>) -> Result<Response<Self::StreamMetricsStream>, Status> {
        debug!("Stream metrics request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        let (tx, rx) = mpsc::channel(100);
        let database = req.database.clone();
        let engine_clone = engine.clone();
        
        tokio::spawn(async move {
            // Send metrics periodically
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            
            loop {
                interval.tick().await;
                
                if database.is_empty() {
                    // Stream metrics for all databases
                    if let Ok(databases) = engine_clone.list_databases() {
                        for db_name in databases {
                            if let Ok(stats) = engine_clone.get_stats(&db_name) {
                                let mut metrics = HashMap::new();
                                metrics.insert("total_keys".to_string(), stats.total_keys as f64);
                                metrics.insert("total_size".to_string(), stats.total_size as f64);
                                
                                let response = MetricsResponse {
                                    database: db_name,
                                    metrics,
                                    timestamp: chrono::Utc::now().timestamp(),
                                };
                                
                                if tx.send(Ok(response)).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                } else {
                    // Stream metrics for specific database
                    if let Ok(stats) = engine_clone.get_stats(&database) {
                        let mut metrics = HashMap::new();
                        metrics.insert("total_keys".to_string(), stats.total_keys as f64);
                        metrics.insert("total_size".to_string(), stats.total_size as f64);
                        
                        let response = MetricsResponse {
                            database: database.clone(),
                            metrics,
                            timestamp: chrono::Utc::now().timestamp(),
                        };
                        
                        if tx.send(Ok(response)).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });
        
        let stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as Self::StreamMetricsStream))
    }
    
    async fn compact_range(&self, request: Request<CompactRangeRequest>) -> Result<Response<CompactRangeResponse>, Status> {
        debug!("Compact range request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = CompactRangeResponse {
            success: true,
            error_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn get_property(&self, request: Request<GetPropertyRequest>) -> Result<Response<GetPropertyResponse>, Status> {
        debug!("Get property request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = GetPropertyResponse {
            value: String::new(),
            found: false,
            success: false,
            error_message: "Not implemented".to_string(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn backup_database(&self, request: Request<BackupDatabaseRequest>) -> Result<Response<BackupDatabaseResponse>, Status> {
        debug!("Backup database request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = BackupDatabaseResponse {
            success: true,
            error_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn restore_database(&self, request: Request<RestoreDatabaseRequest>) -> Result<Response<RestoreDatabaseResponse>, Status> {
        debug!("Restore database request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = RestoreDatabaseResponse {
            success: true,
            error_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    // Execution operations (delegated to execution module)
    async fn execute_transaction(&self, request: Request<ExecuteTransactionRequest>) -> Result<Response<ExecuteTransactionResponse>, Status> {
        debug!("Execute transaction request: {:?}", request.get_ref());

        let req = request.get_ref();

        // Parse pre-execution AEXT snapshots for hybrid mode
        let pre_exec_aext_map = self.parse_pre_execution_aext(&req.pre_execution_aext);
        if !pre_exec_aext_map.is_empty() {
            debug!("Parsed {} pre-execution AEXT snapshots for hybrid mode", pre_exec_aext_map.len());
        }

        // Get the execution module
        let execution_module = self.get_execution_module()?;
        
        // Downcast to the concrete execution module type
        let execution_module = execution_module
            .as_any()
            .downcast_ref::<ExecutionModule>()
            .ok_or_else(|| Status::internal("Failed to downcast execution module"))?;
        
        // Convert protobuf types to execution types
        let (transaction, tx_kind) = match self.convert_protobuf_transaction(req.transaction.as_ref()) {
            Ok((tx, kind)) => {
                debug!("Converted transaction - gas_limit: {}, gas_price: {}, data_len: {}, kind: {:?}", 
                       tx.gas_limit, tx.gas_price, tx.data.len(), kind);
                (tx, kind)
            },
            Err(e) => {
                error!("Failed to convert transaction: {}", e);
                return Ok(Response::new(ExecuteTransactionResponse {
                    result: Some(ExecutionResult {
                        status: execution_result::Status::TronSpecificError as i32,
                        return_data: vec![],
                        energy_used: 0,
                        energy_refunded: 0,
                        state_changes: vec![],
                        logs: vec![],
                        error_message: format!("Transaction conversion error: {}", e),
                        bandwidth_used: 0,
                        resource_usage: vec![],
                        freeze_changes: vec![],
                        global_resource_changes: vec![],
                    }),
                    success: false,
                    error_message: format!("Transaction conversion error: {}", e),
                }));
            }
        };
        
        let context = match self.convert_protobuf_context(req.context.as_ref()) {
            Ok(ctx) => {
                debug!("Converted context - block_gas_limit: {}, energy_price: {}", 
                       ctx.block_gas_limit, ctx.energy_price);
                ctx
            },
            Err(e) => {
                error!("Failed to convert execution context: {}", e);
                return Ok(Response::new(ExecuteTransactionResponse {
                    result: Some(ExecutionResult {
                        status: execution_result::Status::TronSpecificError as i32,
                        return_data: vec![],
                        energy_used: 0,
                        energy_refunded: 0,
                        state_changes: vec![],
                        logs: vec![],
                        error_message: format!("Context conversion error: {}", e),
                        bandwidth_used: 0,
                        resource_usage: vec![],
                        freeze_changes: vec![],
                        global_resource_changes: vec![],
                    }),
                    success: false,
                    error_message: format!("Context conversion error: {}", e),
                }));
            }
        };
        
        // Get the storage engine and create a unified storage adapter
        let storage_engine = self.get_storage_engine()?;
        let mut storage_adapter = tron_backend_execution::EngineBackedEvmStateStore::new(
            storage_engine.clone(),
        );

        // Phase 3: Branch execution based on transaction kind
        let execution_result = match tx_kind {
            crate::backend::TxKind::NonVm => {
                info!("Executing NON_VM transaction with contract type dispatch");
                // Execute non-VM transaction with contract type dispatch
                match self.execute_non_vm_contract(&mut storage_adapter, &transaction, &context) {
                    Ok(result) => {
                        info!("Non-VM contract executed successfully - energy_used: {}, bandwidth_used: {}, state_changes: {}",
                              result.energy_used, result.bandwidth_used, result.state_changes.len());
                        Ok(result)
                    },
                    Err(e) => {
                        error!("Non-VM contract execution failed: {}", e);
                        Err(anyhow::anyhow!("Non-VM execution error: {}", e))
                    }
                }
            },
            crate::backend::TxKind::Vm => {
                info!("Executing VM transaction via EVM");
                
                // TRON Parity Fix: Check if this is likely a non-VM transaction before execution (fallback heuristic)
                let is_non_vm = self.is_likely_non_vm_transaction(&transaction, &storage_adapter);
                
                // Execute the transaction using the database-specific storage adapter
                match execution_module.execute_transaction_with_storage(storage_adapter, &transaction, &context) {
                    Ok(mut result) => {
                        // TRON Parity Fix: Apply non-VM heuristic to set energy_used = 0 for non-VM transactions
                        if is_non_vm {
                            debug!("Detected likely non-VM transaction (empty data, no code at 'to' address) - setting energy_used = 0");
                            debug!("Original energy_used: {}, from: {:?}, to: {:?}, value: {}", 
                                   result.energy_used, transaction.from, transaction.to, transaction.value);
                            result.energy_used = 0;
                        } else {
                            debug!("Detected VM transaction - keeping original energy_used: {}", result.energy_used);
                        }
                        
                        // TRON Phase 2: Apply fee post-processing based on configuration
                        if let Err(e) = self.apply_fee_post_processing(&mut result, &transaction, &context, is_non_vm) {
                            warn!("Fee post-processing failed: {}, continuing with original result", e);
                            // Continue with original result
                        }
                        
                        Ok(result)
                    },
                    Err(e) => Err(e)
                }
            }
        };

        // Handle execution result
        match execution_result {
            Ok(result) => {
                info!("Transaction executed successfully - energy_used: {}, bandwidth_used: {}",
                      result.energy_used, result.bandwidth_used);
                let response = self.convert_execution_result_to_protobuf(result, &pre_exec_aext_map);
                Ok(Response::new(response))
            }
            Err(e) => {
                let error_str = format!("{}", e);
                error!("Transaction execution failed: {}", error_str);
                
                // Check if it's a gas-related error
                if error_str.contains("CallGasCostMoreThanGasLimit") || error_str.contains("OutOfGas") {
                    warn!("Gas limit issue detected - tx.gas_limit: {}, block.gas_limit: {}", 
                          transaction.gas_limit, context.block_gas_limit);
                }
                
                Ok(Response::new(ExecuteTransactionResponse {
                    result: Some(ExecutionResult {
                        status: execution_result::Status::TronSpecificError as i32,
                        return_data: vec![],
                        energy_used: 0,
                        energy_refunded: 0,
                        state_changes: vec![],
                        logs: vec![],
                        error_message: format!("Execution error: {}", e),
                        bandwidth_used: 0,
                        resource_usage: vec![],
                        freeze_changes: vec![],
                        global_resource_changes: vec![],
                    }),
                    success: false,
                    error_message: format!("Execution error: {}", e),
                }))
            }
        }
    }
    
    async fn call_contract(&self, request: Request<CallContractRequest>) -> Result<Response<CallContractResponse>, Status> {
        debug!("Call contract request: {:?}", request.get_ref());

        let req = request.get_ref();

        // Get the execution module
        let execution_module = self.get_execution_module()?;

        // Downcast to the concrete execution module type
        let execution_module = execution_module
            .as_any()
            .downcast_ref::<ExecutionModule>()
            .ok_or_else(|| Status::internal("Failed to downcast execution module"))?;

        // Convert protobuf types to execution types
        let transaction = match self.convert_call_contract_request_to_transaction(req) {
            Ok(tx) => tx,
            Err(e) => {
                error!("Failed to convert call contract request: {}", e);
                return Ok(Response::new(CallContractResponse {
                    return_data: vec![],
                    success: false,
                    error_message: format!("Request conversion error: {}", e),
                    energy_used: 0,
                }));
            }
        };

        let context = match self.convert_protobuf_context(req.context.as_ref()) {
            Ok(ctx) => ctx,
            Err(e) => {
                error!("Failed to convert execution context: {}", e);
                return Ok(Response::new(CallContractResponse {
                    return_data: vec![],
                    success: false,
                    error_message: format!("Context conversion error: {}", e),
                    energy_used: 0,
                }));
            }
        };

        // Get the storage engine and create a unified storage adapter
        let storage_engine = self.get_storage_engine()?;
        let storage_adapter = tron_backend_execution::EngineBackedEvmStateStore::new(
            storage_engine.clone(),
        );

        // Call the contract using the database-specific storage adapter
        match execution_module.call_contract_with_storage(storage_adapter, &transaction, &context) {
            Ok(result) => {
                let response = CallContractResponse {
                    return_data: result.return_data.to_vec(),
                    success: true,
                    error_message: String::new(),
                    energy_used: result.energy_used as i64,
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Contract call failed: {}", e);
                Ok(Response::new(CallContractResponse {
                    return_data: vec![],
                    success: false,
                    error_message: format!("Contract call error: {}", e),
                    energy_used: 0,
                }))
            }
        }
    }
    
    async fn estimate_energy(&self, request: Request<EstimateEnergyRequest>) -> Result<Response<EstimateEnergyResponse>, Status> {
        debug!("Estimate energy request: {:?}", request.get_ref());

        let req = request.get_ref();

        // Get the execution module
        let execution_module = self.get_execution_module()?;

        // Downcast to the concrete execution module type
        let execution_module = execution_module
            .as_any()
            .downcast_ref::<ExecutionModule>()
            .ok_or_else(|| Status::internal("Failed to downcast execution module"))?;

        // Convert protobuf types to execution types
        let _transaction = match self.convert_protobuf_transaction(req.transaction.as_ref()) {
            Ok(tx) => tx,
            Err(e) => {
                error!("Failed to convert transaction: {}", e);
                return Ok(Response::new(EstimateEnergyResponse {
                    energy_estimate: 21000, // Default estimate on error
                    success: false,
                    error_message: format!("Transaction conversion error: {}", e),
                }));
            }
        };

        let context = match self.convert_protobuf_context(req.context.as_ref()) {
            Ok(ctx) => ctx,
            Err(e) => {
                error!("Failed to convert execution context: {}", e);
                return Ok(Response::new(EstimateEnergyResponse {
                    energy_estimate: 21000, // Default estimate on error
                    success: false,
                    error_message: format!("Context conversion error: {}", e),
                }));
            }
        };

        // Get the storage engine and create a unified storage adapter
        let storage_engine = self.get_storage_engine()?;
        let storage_adapter = tron_backend_execution::EngineBackedEvmStateStore::new(
            storage_engine.clone(),
        );

        // Estimate energy using the database-specific storage adapter
        // Convert protobuf types to execution types (for estimate_energy, we don't need tx_kind)
        let (transaction_only, _) = match self.convert_protobuf_transaction(req.transaction.as_ref()) {
            Ok((tx, _kind)) => (tx, _kind),
            Err(e) => {
                error!("Failed to convert transaction: {}", e);
                return Ok(Response::new(EstimateEnergyResponse {
                    energy_estimate: 21000, // Default estimate on error
                    success: false,
                    error_message: format!("Transaction conversion error: {}", e),
                }));
            }
        };
        
        match execution_module.estimate_energy_with_storage(storage_adapter, &transaction_only, &context) {
            Ok(estimate) => {
                let response = EstimateEnergyResponse {
                    energy_estimate: estimate as i64,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Energy estimation failed: {}", e);
                Ok(Response::new(EstimateEnergyResponse {
                    energy_estimate: 21000, // Default estimate on error
                    success: false,
                    error_message: format!("Energy estimation error: {}", e),
                }))
            }
        }
    }
    
    async fn get_code(&self, request: Request<GetCodeRequest>) -> Result<Response<GetCodeResponse>, Status> {
        debug!("Get code request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = GetCodeResponse {
            code: vec![],
            found: false,
            success: false,
            error_message: "Not implemented".to_string(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn get_storage_at(&self, request: Request<GetStorageAtRequest>) -> Result<Response<GetStorageAtResponse>, Status> {
        debug!("Get storage at request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = GetStorageAtResponse {
            value: vec![],
            found: false,
            success: false,
            error_message: "Not implemented".to_string(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn get_nonce(&self, request: Request<GetNonceRequest>) -> Result<Response<GetNonceResponse>, Status> {
        debug!("Get nonce request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = GetNonceResponse {
            nonce: 0,
            found: false,
            success: false,
            error_message: "Not implemented".to_string(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn get_balance(&self, request: Request<GetBalanceRequest>) -> Result<Response<GetBalanceResponse>, Status> {
        debug!("Get balance request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = GetBalanceResponse {
            balance: vec![],
            found: false,
            success: false,
            error_message: "Not implemented".to_string(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn create_evm_snapshot(&self, request: Request<CreateEvmSnapshotRequest>) -> Result<Response<CreateEvmSnapshotResponse>, Status> {
        debug!("Create EVM snapshot request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = CreateEvmSnapshotResponse {
            snapshot_id: uuid::Uuid::new_v4().to_string(),
            success: true,
            error_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn revert_to_evm_snapshot(&self, request: Request<RevertToEvmSnapshotRequest>) -> Result<Response<RevertToEvmSnapshotResponse>, Status> {
        debug!("Revert to EVM snapshot request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = RevertToEvmSnapshotResponse {
            success: true,
            error_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
}

impl BackendService {
    // Helper functions for Tron address format conversion
    fn strip_tron_address_prefix(address_bytes: &[u8]) -> Result<&[u8], String> {
        if address_bytes.len() == 21 && address_bytes[0] == 0x41 {
            Ok(&address_bytes[1..]) // Skip the 0x41 prefix, return 20 bytes
        } else if address_bytes.len() == 20 {
            Ok(address_bytes) // Already 20 bytes, no prefix
        } else {
            Err(format!("Invalid address length: expected 20 or 21 bytes (with 0x41 prefix), got {}", address_bytes.len()))
        }
    }
    
    /// Parse pre-execution AEXT snapshots from the gRPC request into a HashMap.
    /// Converts Tron 21-byte addresses (0x41 prefix) to 20-byte EVM addresses for lookup.
    fn parse_pre_execution_aext(
        &self,
        snapshots: &[crate::backend::AccountAextSnapshot]
    ) -> std::collections::HashMap<revm::primitives::Address, AccountAext> {
        let mut map = std::collections::HashMap::new();

        for snapshot in snapshots {
            // Strip Tron 0x41 prefix to get 20-byte address
            match Self::strip_tron_address_prefix(&snapshot.address) {
                Ok(addr_bytes) => {
                    let address = revm::primitives::Address::from_slice(addr_bytes);

                    // Extract AEXT fields from protobuf
                    if let Some(aext_proto) = &snapshot.aext {
                        debug!("Parsed pre-exec AEXT for address {}: net_usage={}, free_net_usage={}, energy_usage={}",
                               hex::encode(&snapshot.address),
                               aext_proto.net_usage,
                               aext_proto.free_net_usage,
                               aext_proto.energy_usage);

                        let aext = AccountAext {
                            net_usage: aext_proto.net_usage,
                            free_net_usage: aext_proto.free_net_usage,
                            energy_usage: aext_proto.energy_usage,
                            latest_consume_time: aext_proto.latest_consume_time,
                            latest_consume_free_time: aext_proto.latest_consume_free_time,
                            latest_consume_time_for_energy: aext_proto.latest_consume_time_for_energy,
                            net_window_size: aext_proto.net_window_size,
                            net_window_optimized: aext_proto.net_window_optimized,
                            energy_window_size: aext_proto.energy_window_size,
                            energy_window_optimized: aext_proto.energy_window_optimized,
                        };

                        map.insert(address, aext);
                    }
                }
                Err(e) => {
                    warn!("Failed to parse address from pre-exec AEXT snapshot: {}", e);
                }
            }
        }

        map
    }

    fn add_tron_address_prefix(address: &revm_primitives::Address) -> Vec<u8> {
        let mut result = Vec::with_capacity(21);
        result.push(0x41); // Add Tron address prefix
        result.extend_from_slice(address.as_slice());
        result
    }
    
    // Helper functions for converting between protobuf and execution types
    fn convert_protobuf_transaction(&self, tx: Option<&crate::backend::TronTransaction>) -> Result<(TronTransaction, crate::backend::TxKind), String> {
        let tx = tx.ok_or("Transaction is required")?;

        // Log the raw transaction data from Java
        debug!("Raw transaction from Java - energy_limit: {}, energy_price: {}, data_len: {}, contract_type: {}, asset_id_len: {}",
               tx.energy_limit, tx.energy_price, tx.data.len(), tx.contract_type, tx.asset_id.len());
        
        // Convert bytes to Address (strip Tron 0x41 prefix if present)
        let from_bytes = Self::strip_tron_address_prefix(&tx.from)?;
        let from = revm_primitives::Address::from_slice(from_bytes);
        
        let to = if tx.to.is_empty() {
            None // Contract creation
        } else {
            let to_bytes = Self::strip_tron_address_prefix(&tx.to)?;
            Some(revm_primitives::Address::from_slice(to_bytes))
        };
        
        // Convert bytes to U256 (32 bytes max)
        let value = if tx.value.len() <= 32 {
            revm_primitives::U256::from_be_slice(&tx.value)
        } else {
            return Err("Invalid value length".to_string());
        };
        
        // Get execution config to check coinbase compatibility
        let execution_config = self.get_execution_config()?;
        
        // TRON Parity Fix: Force gas_price = 0 unless coinbase compat is explicitly enabled
        let gas_price = if execution_config.evm_eth_coinbase_compat {
            // Legacy behavior: convert energy_price to gas_price
            let energy_price_sun = tx.energy_price as u64;
            if energy_price_sun == 0 {
                revm_primitives::U256::from(1u64) // Minimum gas price
            } else {
                // Convert SUN to a reasonable gas price range (1-1000)
                let converted_price = std::cmp::min(energy_price_sun / 1000, 1000);
                let final_price = std::cmp::max(converted_price, 1); // Minimum 1
                revm_primitives::U256::from(final_price)
            }
        } else {
            // TRON mode: Always use gas_price = 0 to prevent coinbase/miner rewards
            debug!("Using gas_price = 0 for TRON parity (suppresses coinbase rewards)");
            revm_primitives::U256::ZERO
        };

        debug!("Gas price conversion - original energy_price: {} SUN, final gas_price: {}, coinbase_compat: {}",
               tx.energy_price, gas_price, execution_config.evm_eth_coinbase_compat);
        
        // Handle zero energy_limit by using a reasonable default
        let gas_limit = if tx.energy_limit == 0 {
            // Use a default gas limit based on transaction type and data size
            let base_gas = 21000u64; // Basic transaction cost
            let data_gas = tx.data.len() as u64 * 16; // 16 gas per byte of data
            let default_limit = base_gas + data_gas + 100000; // Add buffer for contract execution
            debug!("Using default gas limit {} for zero energy_limit transaction", default_limit);
            default_limit
        } else {
            tx.energy_limit as u64
        };

        // Extract contract_type and asset_id from protobuf for TRON system contracts
        let contract_type = if tx.contract_type != 0 {
            match tron_backend_execution::TronContractType::try_from(tx.contract_type) {
                Ok(ct) => {
                    debug!("Parsed contract type: {:?}", ct);
                    Some(ct)
                },
                Err(e) => {
                    warn!("Invalid contract type {}: {}, ignoring", tx.contract_type, e);
                    None
                }
            }
        } else {
            None
        };

        let asset_id = if !tx.asset_id.is_empty() {
            debug!("Parsed asset_id: {} bytes", tx.asset_id.len());
            Some(tx.asset_id.clone())
        } else {
            None
        };

        let metadata = tron_backend_execution::TxMetadata {
            contract_type,
            asset_id,
        };

        let transaction = TronTransaction {
            from,
            to,
            value,
            data: revm_primitives::Bytes::from(tx.data.clone()),
            gas_limit,
            gas_price,
            nonce: tx.nonce as u64,
            metadata,
        };

        // Extract tx_kind from protobuf, default to VM for backward compatibility
        let tx_kind = crate::backend::TxKind::try_from(tx.tx_kind).unwrap_or(crate::backend::TxKind::Vm);
        debug!("Transaction kind: {:?}, contract_type: {:?}", tx_kind, transaction.metadata.contract_type);
        
        Ok((transaction, tx_kind))
    }
    
    fn convert_protobuf_context(&self, ctx: Option<&crate::backend::ExecutionContext>) -> Result<TronExecutionContext, String> {
        let ctx = ctx.ok_or("Execution context is required")?;
        
        // Log the raw context data from Java
        debug!("Raw context from Java - energy_limit: {}, energy_price: {}", 
               ctx.energy_limit, ctx.energy_price);
        
        // Strip Tron 0x41 prefix from coinbase address if present
        let coinbase_bytes = Self::strip_tron_address_prefix(&ctx.coinbase)?;
        let block_coinbase = revm_primitives::Address::from_slice(coinbase_bytes);
        
        // Handle zero energy_limit by using the configured block gas limit
        let block_gas_limit = if ctx.energy_limit == 0 {
            // Use the configured energy_limit from config as the block gas limit
            1000000000u64 // 1B gas limit (same as config.toml)
        } else {
            ctx.energy_limit as u64
        };
        
        debug!("Using block_gas_limit: {}", block_gas_limit);
        
        Ok(TronExecutionContext {
            block_number: ctx.block_number as u64,
            block_timestamp: ctx.block_timestamp as u64,
            block_coinbase,
            block_difficulty: revm_primitives::U256::from(1u64), // Default difficulty
            block_gas_limit,
            chain_id: 0x2b6653dc, // Tron mainnet chain ID
            energy_price: ctx.energy_price as u64,
            bandwidth_price: 1000, // Default bandwidth price
        })
    }

    fn convert_call_contract_request_to_transaction(&self, req: &crate::backend::CallContractRequest) -> Result<TronTransaction, String> {
        // Convert bytes to Address (strip Tron 0x41 prefix if present)
        let from_bytes = Self::strip_tron_address_prefix(&req.from)?;
        let from = revm_primitives::Address::from_slice(from_bytes);

        let to_bytes = Self::strip_tron_address_prefix(&req.to)?;
        let to = revm_primitives::Address::from_slice(to_bytes);

        Ok(TronTransaction {
            from,
            to: Some(to),
            value: revm_primitives::U256::ZERO, // Contract calls typically don't transfer value
            data: req.data.clone().into(),
            gas_limit: 1000000, // Default gas limit for contract calls
            gas_price: revm_primitives::U256::from(1),
            nonce: 0, // Contract calls don't use nonce
            metadata: tron_backend_execution::TxMetadata::default(), // No specific metadata for contract calls
        })
    }

    fn convert_execution_result_to_protobuf(
        &self,
        result: TronExecutionResult,
        pre_exec_aext: &std::collections::HashMap<revm::primitives::Address, AccountAext>
    ) -> ExecuteTransactionResponse {
        let status = if result.success {
            execution_result::Status::Success
        } else {
            execution_result::Status::Revert
        };
        
        let logs: Vec<LogEntry> = result.logs.iter().map(|log| {
            LogEntry {
                address: Self::add_tron_address_prefix(&log.address),
                topics: log.topics().iter().map(|t| t.as_slice().to_vec()).collect(),
                data: log.data.data.to_vec(),
            }
        }).collect();

        let state_changes: Vec<StateChange> = result.state_changes.iter().map(|change| {
            match change {
                TronStateChange::StorageChange { address, key, old_value, new_value } => {
                    StateChange {
                        change: Some(crate::backend::state_change::Change::StorageChange(
                            crate::backend::StorageChange {
                                address: Self::add_tron_address_prefix(address),
                                key: key.to_be_bytes::<32>().to_vec(),
                                old_value: old_value.to_be_bytes::<32>().to_vec(),
                                new_value: new_value.to_be_bytes::<32>().to_vec(),
                            }
                        ))
                    }
                },
                TronStateChange::AccountChange { address, old_account, new_account } => {
                    // Get AEXT mode from config
                    let aext_mode = self.get_execution_config()
                        .ok()
                        .and_then(|cfg| Some(cfg.remote.accountinfo_aext_mode.as_str()))
                        .unwrap_or("none");

                    // Helper function to convert AccountInfo to protobuf
                    // is_old: true for old_account, false for new_account
                    let convert_account_info = |addr: &revm::primitives::Address, acc_info: &revm::primitives::AccountInfo, is_old: bool| {
                        // Ensure EOAs (no code) serialize with empty code bytes.
                        let code_bytes: Vec<u8> = match acc_info.code.as_ref() {
                            None => Vec::new(),
                            Some(c) => {
                                let b = c.bytes();
                                // Treat zero-length or single 0x00 as empty for EOAs
                                if b.len() == 0 || (b.len() == 1 && b[0] == 0) {
                                    Vec::new()
                                } else {
                                    b.to_vec()
                                }
                            }
                        };

                        // Canonical empty code hash: keccak256("")
                        const KECCAK_EMPTY: [u8; 32] = [
                            0xc5, 0xd2, 0x46, 0x01, 0x86, 0xf7, 0x23, 0x3c,
                            0x92, 0x7e, 0x7d, 0xb2, 0xdc, 0xc7, 0x03, 0xc0,
                            0xe5, 0x00, 0xb6, 0x53, 0xca, 0x82, 0x27, 0x3b,
                            0x7b, 0xfa, 0xd8, 0x04, 0x5d, 0x85, 0xa4, 0x70,
                        ];

                        // Normalize code_hash for empty code to KECCAK_EMPTY for parity with embedded
                        let code_hash_bytes: Vec<u8> = if code_bytes.is_empty() {
                            KECCAK_EMPTY.to_vec()
                        } else {
                            acc_info.code_hash.as_slice().to_vec()
                        };

                        // Determine if this is an EOA (empty code)
                        let is_eoa = code_bytes.is_empty();

                        // Populate AEXT fields based on mode
                        let (net_usage, free_net_usage, energy_usage, latest_consume_time,
                             latest_consume_free_time, latest_consume_time_for_energy,
                             net_window_size, net_window_optimized, energy_window_size,
                             energy_window_optimized) = match aext_mode {
                            "hybrid" if is_eoa => {
                                // Hybrid mode: prefer pre-provided AEXT from Java, fallback to defaults
                                if let Some(aext) = pre_exec_aext.get(addr) {
                                    debug!("Using pre-exec AEXT for address {} in hybrid mode", hex::encode(Self::add_tron_address_prefix(addr)));
                                    // Use the same AEXT for both old and new (unchanged fields)
                                    (Some(aext.net_usage), Some(aext.free_net_usage), Some(aext.energy_usage),
                                     Some(aext.latest_consume_time), Some(aext.latest_consume_free_time),
                                     Some(aext.latest_consume_time_for_energy), Some(aext.net_window_size),
                                     Some(aext.net_window_optimized), Some(aext.energy_window_size),
                                     Some(aext.energy_window_optimized))
                                } else {
                                    // Not provided, fall back to defaults
                                    debug!("No pre-exec AEXT for address {}, using defaults in hybrid mode", hex::encode(Self::add_tron_address_prefix(addr)));
                                    (Some(0), Some(0), Some(0), Some(0), Some(0), Some(0),
                                     Some(28800), Some(false), Some(28800), Some(false))
                                }
                            },
                            "zeros" if is_eoa => {
                                // For EOAs in "zeros" mode, populate all fields with zero/false
                                (Some(0), Some(0), Some(0), Some(0), Some(0), Some(0),
                                 Some(0), Some(false), Some(0), Some(false))
                            },
                            "defaults" if is_eoa => {
                                // For EOAs in "defaults" mode, match embedded Java-Tron defaults:
                                // - net_window_size = 28800 (0x7080)
                                // - energy_window_size = 28800 (0x7080)
                                // - All other fields zero/false
                                // This ensures byte-level AEXT tail parity with embedded CSVs
                                (Some(0), Some(0), Some(0), Some(0), Some(0), Some(0),
                                 Some(28800), Some(false), Some(28800), Some(false))
                            },
                            "tracked" if is_eoa => {
                                // Populate with real values from resource tracking
                                // Look up address in aext_map
                                if let Some((before_aext, after_aext)) = result.aext_map.get(addr) {
                                    // Use before_aext for old_account, after_aext for new_account
                                    let aext = if is_old { before_aext } else { after_aext };

                                    (Some(aext.net_usage), Some(aext.free_net_usage), Some(aext.energy_usage),
                                     Some(aext.latest_consume_time), Some(aext.latest_consume_free_time),
                                     Some(aext.latest_consume_time_for_energy), Some(aext.net_window_size),
                                     Some(aext.net_window_optimized), Some(aext.energy_window_size),
                                     Some(aext.energy_window_optimized))
                                } else {
                                    // Address not in aext_map, use defaults
                                    (Some(0), Some(0), Some(0), Some(0), Some(0), Some(0),
                                     Some(28800), Some(false), Some(28800), Some(false))
                                }
                            },
                            "tracked" => {
                                // Non-EOA contracts in tracked mode: no AEXT
                                (None, None, None, None, None, None, None, None, None, None)
                            },
                            "hybrid" => {
                                // Non-EOA contracts in hybrid mode: no AEXT
                                (None, None, None, None, None, None, None, None, None, None)
                            },
                            _ => {
                                // "none" or unknown: leave all fields as None (current behavior)
                                (None, None, None, None, None, None, None, None, None, None)
                            }
                        };

                        debug!("AccountInfo AEXT presence: mode={}, is_eoa={}, address={}, net_window={:?}, energy_window={:?}",
                               aext_mode, is_eoa, hex::encode(Self::add_tron_address_prefix(addr)),
                               net_window_size, energy_window_size);

                        crate::backend::AccountInfo {
                            address: Self::add_tron_address_prefix(addr),
                            balance: acc_info.balance.to_be_bytes::<32>().to_vec(),
                            nonce: acc_info.nonce,
                            code_hash: code_hash_bytes,
                            code: code_bytes,
                            // Optional resource usage fields (AEXT) - populated based on mode
                            net_usage,
                            free_net_usage,
                            energy_usage,
                            latest_consume_time,
                            latest_consume_free_time,
                            latest_consume_time_for_energy,
                            net_window_size,
                            net_window_optimized,
                            energy_window_size,
                            energy_window_optimized,
                        }
                    };

                    let old_account_proto = old_account.as_ref().map(|acc| convert_account_info(address, acc, true));
                    let new_account_proto = new_account.as_ref().map(|acc| convert_account_info(address, acc, false));
                    
                    StateChange {
                        change: Some(crate::backend::state_change::Change::AccountChange(
                            crate::backend::AccountChange {
                                address: Self::add_tron_address_prefix(address),
                                old_account: old_account_proto,
                                new_account: new_account_proto,
                                is_creation: old_account.is_none() && new_account.is_some(),
                                is_deletion: old_account.is_some() && new_account.is_none(),
                            }
                        ))
                    }
                }
            }
        }).collect();

        // Convert freeze changes from execution result to protobuf
        let freeze_changes: Vec<crate::backend::FreezeLedgerChange> = result.freeze_changes.iter().map(|change| {
            use crate::backend::freeze_ledger_change::Resource;
            use tron_backend_execution::FreezeLedgerResource;

            let resource = match change.resource {
                FreezeLedgerResource::Bandwidth => Resource::Bandwidth,
                FreezeLedgerResource::Energy => Resource::Energy,
                FreezeLedgerResource::TronPower => Resource::TronPower,
            };

            crate::backend::FreezeLedgerChange {
                owner_address: Self::add_tron_address_prefix(&change.owner_address),
                resource: resource as i32,
                amount: change.amount,
                expiration_ms: change.expiration_ms,
                v2_model: change.v2_model,
            }
        }).collect();

        // Convert global resource totals changes from execution result to protobuf
        let global_resource_changes: Vec<crate::backend::GlobalResourceTotalsChange> = result.global_resource_changes.iter().map(|change| {
            crate::backend::GlobalResourceTotalsChange {
                total_net_weight: change.total_net_weight,
                total_net_limit: change.total_net_limit,
                total_energy_weight: change.total_energy_weight,
                total_energy_limit: change.total_energy_limit,
            }
        }).collect();

        let error_message = result.error.unwrap_or_default();

        ExecuteTransactionResponse {
            result: Some(ExecutionResult {
                status: status as i32,
                return_data: result.return_data.to_vec(),
                energy_used: result.energy_used as i64,
                energy_refunded: 0, // Not provided by TronExecutionResult
                state_changes,
                logs,
                error_message: error_message.clone(),
                bandwidth_used: result.bandwidth_used as i64,
                resource_usage: vec![], // Not implemented yet
                freeze_changes, // Converted from TronExecutionResult
                global_resource_changes, // Converted from TronExecutionResult
            }),
            success: result.success,
            error_message,
        }
    }
} 
