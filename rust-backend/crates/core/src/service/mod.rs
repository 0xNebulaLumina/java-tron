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
use revm_primitives::AccountInfo;

// Module declarations
pub mod grpc;
pub mod contracts;

// Import utilities from submodules
use contracts::proto::read_varint;
use grpc::address::add_tron_address_prefix;

/// Vote witness contract constants
const MAX_VOTE_NUMBER: usize = 30;
const TRX_PRECISION: u64 = 1_000_000; // 1 TRX = 1,000,000 SUN

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
                self.execute_trc10_transfer_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::AssetIssueContract) => {
                if !remote_config.trc10_enabled {
                    return Err("ASSET_ISSUE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing ASSET_ISSUE_CONTRACT");
                self.execute_asset_issue_contract(storage_adapter, transaction, context)
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

        // Load sender account (track existence)
        let sender_opt = storage_adapter
            .get_account(&transaction.from)
            .map_err(|e| format!("Failed to load sender account: {}", e))?;
        let sender_account = sender_opt.clone().unwrap_or_default();

        // Load recipient account (track existence)
        let recipient_opt = storage_adapter
            .get_account(&to_address)
            .map_err(|e| format!("Failed to load recipient account: {}", e))?;
        let recipient_account = recipient_opt.clone().unwrap_or_default();
        
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
                                
                                // Load blackhole account (track existence)
                                let blackhole_opt = storage_adapter.get_account(&blackhole_address)
                                    .map_err(|e| format!("Failed to load blackhole account: {}", e))?;
                                let blackhole_account = blackhole_opt.clone().unwrap_or_default();
                                
                                // Credit blackhole account with fee
                                let new_blackhole_balance = blackhole_account.balance + revm_primitives::U256::from(fee_amount);
                                let new_blackhole_account = revm_primitives::AccountInfo {
                                    balance: new_blackhole_balance,
                                    nonce: blackhole_account.nonce,
                                    code_hash: blackhole_account.code_hash,
                                    code: blackhole_account.code.clone(),
                                };
                                
                                // Add blackhole account change
                                let old_blackhole_account = if blackhole_opt.is_none() {
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
            trc10_changes: vec![], // Not applicable for value transfers
            vote_changes: vec![], // Not applicable for value transfers
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

                let bh_tron = revm_primitives::hex::encode(add_tron_address_prefix(&blackhole_addr));
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
            aext_map, // Populated with tracked AEXT if mode is "tracked"
            freeze_changes: vec![], // Will be populated by freeze-related contracts
            global_resource_changes: vec![], // Not applicable for witness creation
            trc10_changes: vec![], // Not applicable for witness creation
            vote_changes: vec![], // Not applicable for witness creation
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
        use tron_backend_execution::{TronExecutionResult, TronStateChange, WitnessInfo};

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        debug!("Executing WITNESS_UPDATE_CONTRACT for owner {}", owner_tron);

        // 1. Extract and validate URL from transaction.data
        let url_bytes = &transaction.data;

        // Validate: non-empty and ≤256 bytes (mirror TransactionUtil.validUrl with allowEmpty=false)
        if url_bytes.is_empty() || url_bytes.len() > 256 {
            warn!("WITNESS_UPDATE_CONTRACT: Invalid url (empty={}, len={})", url_bytes.is_empty(), url_bytes.len());
            return Err("Invalid url".to_string());
        }

        // Decode URL as UTF-8 (consistent with existing WitnessCreate handler style)
        let new_url = String::from_utf8(url_bytes.to_vec())
            .map_err(|e| format!("Invalid UTF-8 in witness URL: {}", e))?;

        debug!("WitnessUpdate: new URL = {}", new_url);

        // 2. Load owner account (required)
        let owner_account = storage_adapter.get_account(&owner)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or_else(|| {
                warn!("WITNESS_UPDATE_CONTRACT: account does not exist for {}", owner_tron);
                "account does not exist".to_string()
            })?;

        // 3. Load existing witness (required)
        let existing_witness = storage_adapter.get_witness(&owner)
            .map_err(|e| format!("Failed to load witness: {}", e))?
            .ok_or_else(|| {
                warn!("WITNESS_UPDATE_CONTRACT: Witness does not exist for {}", owner_tron);
                "Witness does not exist".to_string()
            })?;

        let old_url = existing_witness.url.clone();

        // 4. Create updated witness entry with new URL, preserving address and vote_count
        let updated_witness = WitnessInfo::new(
            existing_witness.address,
            new_url.clone(),
            existing_witness.vote_count,
        );

        // 5. Persist updated witness only if URL actually changes to avoid no-op writes
        if new_url != old_url {
            storage_adapter
                .put_witness(&updated_witness)
                .map_err(|e| format!("Failed to update witness: {}", e))?;
            info!(
                "Updated witness URL: owner={}, old_url='{}', new_url='{}'",
                owner_tron, old_url, new_url
            );
        } else {
            info!(
                "Witness update is a no-op (URL unchanged): owner={}, url='{}'",
                owner_tron, new_url
            );
        }

        // 6. Do not emit state changes for WitnessUpdateContract to match embedded semantics
        // (Java embedded CSV logs 0 state changes and empty digest for witness updates)
        let state_changes: Vec<TronStateChange> = Vec::new();

        // 7. Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // 8. Handle AEXT tracking if enabled
        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();
        let mut aext_map = std::collections::HashMap::new();

        if aext_mode == "tracked" {
            debug!("AEXT tracking enabled for WITNESS_UPDATE_CONTRACT");

            // Load current AEXT or default
            let current_aext = storage_adapter.get_account_aext(&owner)
                .map_err(|e| format!("Failed to load AEXT: {}", e))?
                .unwrap_or_default();

            // Load FREE_NET_LIMIT from dynamic properties
            let free_net_limit = storage_adapter.get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;

            // Track bandwidth usage (returns path, before_aext, after_aext)
            let (_path, before_aext, after_aext) = tron_backend_execution::ResourceTracker::track_bandwidth(
                &owner,
                bandwidth_used as i64,
                context.block_number as i64,
                &current_aext,
                free_net_limit,
            ).map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist updated AEXT
            storage_adapter.set_account_aext(&owner, &after_aext)
                .map_err(|e| format!("Failed to persist AEXT: {}", e))?;

            // Populate aext_map
            aext_map.insert(owner, (before_aext, after_aext));

            debug!("AEXT tracked for owner {}: bandwidth_used={}", owner_tron, bandwidth_used);
        }

        // 9. Return success result
        debug!("WITNESS_UPDATE_CONTRACT completed successfully for {}", owner_tron);

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,     // Witness update uses zero energy
            bandwidth_used,
            logs: Vec::new(),   // No logs for witness update
            state_changes,
            error: None,
            aext_map,
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![], // Not applicable for witness update
        })
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
        // When creating a new record (no existing VotesRecord), seed old_votes from Account.votes
        // to match embedded behavior. This ensures correct delta computation in maintenance.
        let seed_from_account = execution_config.remote.vote_witness_seed_old_from_account;

        let mut votes_record = match storage_adapter.get_votes(&owner) {
            Ok(Some(record)) => {
                info!("Found existing votes for {}: old_votes={}, new_votes={}",
                      owner_tron, record.old_votes.len(), record.new_votes.len());
                // Update old_votes to current new_votes
                VotesRecord::new(owner, record.new_votes.clone(), Vec::new())
            },
            Ok(None) => {
                // No existing VotesRecord - this is the first vote for this account in this epoch
                if seed_from_account {
                    // Seed old_votes from Account.votes field (matches embedded behavior)
                    let prior_votes_tuples = storage_adapter.get_account_votes_list(&owner)
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

        // Build VoteChange for Java to update Account.votes
        // This ensures correct old_votes seeding in subsequent epochs
        use tron_backend_execution::{VoteChange, VoteEntry};
        let vote_change = VoteChange {
            owner_address: owner,
            votes: votes_record.new_votes.iter().map(|v| VoteEntry {
                vote_address: v.vote_address.clone(),
                vote_count: v.vote_count,
            }).collect(),
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
            aext_map, // Populated with tracked AEXT if mode is "tracked"
            freeze_changes: vec![], // Will be populated by freeze-related contracts
            global_resource_changes: vec![], // Not applicable for vote witness
            trc10_changes: vec![], // Not applicable for vote witness
            vote_changes: vec![vote_change], // VoteChange for Account.votes update
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
            trc10_changes: vec![], // Not applicable for account update
            vote_changes: vec![], // Not applicable for account update
        })
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
        use tron_backend_execution::{TronExecutionResult, TronStateChange};
        use revm_primitives::Address;

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        debug!("Executing TRANSFER_ASSET_CONTRACT for owner {}", owner_tron);

        // 1. Extract required fields from transaction
        let to_address = transaction.to.ok_or("to address is required for TransferAssetContract")?;
        let to_tron = tron_backend_common::to_tron_address(&to_address);

        // Get asset_id from metadata (Java passes it as metadata.asset_id)
        let asset_id = transaction.metadata.asset_id.as_ref()
            .ok_or("asset_id is required for TransferAssetContract")?
            .clone();

        if asset_id.is_empty() {
            return Err("asset_id cannot be empty".to_string());
        }

        // Convert value (U256) to i64 for TRC-10 amount
        // TransferAssetContract amounts are typically i64
        let amount_u64: u64 = transaction.value.try_into()
            .map_err(|_| "TransferAssetContract amount overflow: value too large for i64")?;
        let amount = amount_u64 as i64;

        if amount <= 0 {
            return Err("Invalid amount: must be positive".to_string());
        }

        info!(
            "TRC-10 Transfer: owner={}, to={}, asset_id_len={}, amount={}",
            owner_tron, to_tron, asset_id.len(), amount
        );

        // 2. Get execution configuration
        let execution_config = self.get_execution_config()?;
        let fee_config = &execution_config.fees;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();

        // 3. Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // 4. Track AEXT for bandwidth if in tracked mode
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
            let (_path, before_aext, after_aext) = ResourceTracker::track_bandwidth(
                &owner,
                bandwidth_used as i64,
                context.block_number as i64,
                &current_aext,
                free_net_limit,
            ).map_err(|e| format!("Failed to track bandwidth: {}", e))?;

            // Persist after AEXT to storage
            storage_adapter.set_account_aext(&owner, &after_aext)
                .map_err(|e| format!("Failed to persist account AEXT: {}", e))?;

            // Add to aext_map
            aext_map.insert(owner, (before_aext.clone(), after_aext.clone()));

            debug!(
                "AEXT tracked for TRC-10 transfer: owner={}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}",
                owner_tron, before_aext.net_usage, after_aext.net_usage,
                before_aext.free_net_usage, after_aext.free_net_usage
            );
        }

        // 5. Build state changes
        let mut state_changes = Vec::new();

        // Load owner account for AccountChange (needed for AEXT passthrough)
        let owner_account = storage_adapter.get_account(&owner)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or("Owner account does not exist".to_string())?;

        // Check if there's a TRX fee configured for non-VM transactions
        let fee_amount = match fee_config.non_vm_blackhole_credit_flat {
            Some(flat_fee) => {
                debug!("Using configured flat fee for TRC-10 transfer: {} SUN", flat_fee);
                flat_fee
            },
            None => {
                // Default: no TRX fee for TRC-10 transfers (TRON free bandwidth semantics)
                debug!("No flat fee configured for TRC-10 transfer, using 0 (TRON free bandwidth semantics)");
                0
            }
        };

        if fee_amount > 0 {
            // Validate owner has enough TRX for fee
            let fee_u256 = revm_primitives::U256::from(fee_amount);
            if owner_account.balance < fee_u256 {
                return Err(format!(
                    "Insufficient TRX balance for fee: owner has {} SUN, fee is {} SUN",
                    owner_account.balance, fee_amount
                ));
            }

            // Deduct fee from owner
            let new_owner_balance = owner_account.balance - fee_u256;
            let new_owner_account = revm_primitives::AccountInfo {
                balance: new_owner_balance,
                nonce: owner_account.nonce, // Do NOT increment nonce for non-VM
                code_hash: owner_account.code_hash,
                code: owner_account.code.clone(),
            };

            // Emit owner account change
            state_changes.push(TronStateChange::AccountChange {
                address: owner,
                old_account: Some(owner_account.clone()),
                new_account: Some(new_owner_account.clone()),
            });

            // Persist owner account update
            storage_adapter
                .set_account(owner, new_owner_account.clone())
                .map_err(|e| format!("Failed to persist owner account: {}", e))?;

            // Handle fee crediting based on mode
            match fee_config.mode.as_str() {
                "burn" => {
                    debug!("Burning fee {} SUN (no account delta for burn)", fee_amount);
                },
                "blackhole" => {
                    if !fee_config.blackhole_address_base58.is_empty() {
                        match tron_backend_common::from_tron_address(&fee_config.blackhole_address_base58) {
                            Ok(blackhole_bytes) => {
                                let blackhole_address = revm_primitives::Address::from_slice(&blackhole_bytes);

                                let blackhole_account = storage_adapter.get_account(&blackhole_address)
                                    .map_err(|e| format!("Failed to load blackhole account: {}", e))?
                                    .unwrap_or_default();

                                let new_blackhole_balance = blackhole_account.balance + fee_u256;
                                let new_blackhole_account = revm_primitives::AccountInfo {
                                    balance: new_blackhole_balance,
                                    nonce: blackhole_account.nonce,
                                    code_hash: blackhole_account.code_hash,
                                    code: blackhole_account.code.clone(),
                                };

                                let old_blackhole_account = if blackhole_account.balance.is_zero() && blackhole_account.nonce == 0 {
                                    None
                                } else {
                                    Some(blackhole_account)
                                };

                                state_changes.push(TronStateChange::AccountChange {
                                    address: blackhole_address,
                                    old_account: old_blackhole_account,
                                    new_account: Some(new_blackhole_account.clone()),
                                });

                                storage_adapter
                                    .set_account(blackhole_address, new_blackhole_account.clone())
                                    .map_err(|e| format!("Failed to persist blackhole account: {}", e))?;

                                debug!("Credited fee {} SUN to blackhole address {}",
                                       fee_amount, fee_config.blackhole_address_base58);
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
            // No TRX fee - emit a no-op AccountChange for owner to carry AEXT
            // This ensures the owner account is included in state changes for CSV parity
            state_changes.push(TronStateChange::AccountChange {
                address: owner,
                old_account: Some(owner_account.clone()),
                new_account: Some(owner_account.clone()), // Same account (no-op)
            });
        }

        // 6. Also include a no-op AccountChange for recipient to mirror embedded journaling
        //    so that both sender and recipient appear in account_changes/state_changes.
        //    This carries AEXT if present and stabilizes CSV parity.
        if let Ok(Some(recipient_account)) = storage_adapter.get_account(&to_address) {
            state_changes.push(TronStateChange::AccountChange {
                address: to_address,
                old_account: Some(recipient_account.clone()),
                new_account: Some(recipient_account), // No-op (ledger updates are tracked separately)
            });
        } else {
            // If recipient account does not exist yet, fabricate a minimal placeholder
            // with zero fields so Java side still sees an account-level entry for CSV parity.
            let placeholder = AccountInfo::default();
            state_changes.push(TronStateChange::AccountChange {
                address: to_address,
                old_account: Some(placeholder.clone()),
                new_account: Some(placeholder),
            });
        }

        // 7. Sort state changes deterministically by address for CSV parity
        state_changes.sort_by_key(|change| match change {
            TronStateChange::AccountChange { address, .. } => *address,
            _ => Address::ZERO,
        });

        // 8. Determine token_id if asset_id bytes are ASCII digits (V2 path)
        let token_id = if asset_id.iter().all(|&b| b.is_ascii_digit()) {
            match String::from_utf8(asset_id.clone()) {
                Ok(id_str) => Some(id_str),
                Err(_) => None,
            }
        } else {
            None
        };

        // 9. Build TRC-10 Asset Transferred change for Phase 2
        let trc10_change = tron_backend_execution::Trc10Change::AssetTransferred(
            tron_backend_execution::Trc10AssetTransferred {
                owner_address: owner,
                to_address,
                asset_name: asset_id.clone(),
                token_id,
                amount,
            }
        );

        info!(
            "TRC-10 Transfer completed: owner={}, to={}, asset_id_len={}, amount={}, fee={} SUN, state_changes={}, bandwidth={}",
            owner_tron, to_tron, asset_id.len(), amount, fee_amount, state_changes.len(), bandwidth_used
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(), // No return data for TRC-10 transfers
            energy_used: 0, // Non-VM transactions use 0 energy
            bandwidth_used,
            state_changes,
            logs: Vec::new(), // No logs for TRC-10 transfers
            error: None,
            aext_map,
            freeze_changes: vec![], // Not applicable for TRC-10 transfers
            global_resource_changes: vec![], // Not applicable for TRC-10 transfers
            trc10_changes: vec![trc10_change], // Phase 2: emit TRC-10 semantic change
            vote_changes: vec![], // Not applicable for TRC-10 transfers
        })
    }

    fn execute_asset_issue_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::{TronExecutionResult, TronStateChange};
        use revm_primitives::Address;

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        debug!("Executing ASSET_ISSUE_CONTRACT for owner {}", owner_tron);

        // 1. Parse AssetIssueContract proto from transaction.data
        let asset_info = Self::parse_asset_issue_contract(&transaction.data)?;

        info!(
            "AssetIssue: owner={}, name={}, total_supply={}, precision={}",
            owner_tron, asset_info.name, asset_info.total_supply, asset_info.precision
        );

        // 2. Get execution configuration
        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();

        // 3. Load owner account
        let owner_account = storage_adapter.get_account(&owner)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or("Owner account does not exist".to_string())?;

        // 4. Get asset issue fee from dynamic properties
        let asset_issue_fee = storage_adapter.get_asset_issue_fee()
            .map_err(|e| format!("Failed to get AssetIssueFee: {}", e))?;

        debug!("AssetIssueFee: {} SUN", asset_issue_fee);

        // 5. Validate owner balance >= fee
        let owner_balance_u256 = owner_account.balance;
        let fee_u256 = revm_primitives::U256::from(asset_issue_fee);

        if owner_balance_u256 < fee_u256 {
            return Err(format!(
                "Insufficient balance for asset issue fee: owner has {} SUN, fee is {} SUN",
                owner_balance_u256, asset_issue_fee
            ));
        }

        // 6. Deduct fee from owner
        let new_owner_balance = owner_balance_u256 - fee_u256;
        let new_owner_account = revm_primitives::AccountInfo {
            balance: new_owner_balance,
            nonce: owner_account.nonce,
            code_hash: owner_account.code_hash,
            code: owner_account.code.clone(),
        };

        // 7. Emit state changes (deterministic ordering by address)
        let mut state_changes = Vec::new();

        // Always emit owner account change
        state_changes.push(TronStateChange::AccountChange {
            address: owner,
            old_account: Some(owner_account.clone()),
            new_account: Some(new_owner_account.clone()),
        });

        // Persist owner account update
        storage_adapter
            .set_account(owner, new_owner_account.clone())
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;

        // 8. Handle fee burning/crediting
        let support_blackhole = storage_adapter.support_black_hole_optimization()
            .map_err(|e| format!("Failed to get blackhole optimization flag: {}", e))?;

        if support_blackhole {
            // Burn mode - no additional account change needed
            info!("Burning {} SUN asset issue fee (blackhole optimization)", asset_issue_fee);
        } else {
            // Credit blackhole account
            if let Some(blackhole_addr) = storage_adapter.get_blackhole_address()
                .map_err(|e| format!("Failed to get blackhole address: {}", e))? {

                let blackhole_account = storage_adapter.get_account(&blackhole_addr)
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
            let current_aext = storage_adapter.get_account_aext(&owner)
                .map_err(|e| format!("Failed to get account AEXT: {}", e))?
                .unwrap_or_else(|| AccountAext::with_defaults());

            // Get FREE_NET_LIMIT from dynamic properties
            let free_net_limit = storage_adapter.get_free_net_limit()
                .map_err(|e| format!("Failed to get FREE_NET_LIMIT: {}", e))?;

            // Track bandwidth usage (returns path, before_aext, after_aext)
            let (_path, before_aext, after_aext) = ResourceTracker::track_bandwidth(
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

            debug!(
                "AEXT tracked for asset_issue: owner={}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}",
                owner_tron, before_aext.net_usage, after_aext.net_usage,
                before_aext.free_net_usage, after_aext.free_net_usage
            );
        }

        info!(
            "AssetIssue completed: owner={}, name={}, fee={} SUN, state_changes={}, bandwidth={}",
            owner_tron, asset_info.name, asset_issue_fee, state_changes.len(), bandwidth_used
        );

        // 12. Build TRC-10 Asset Issued change for Phase 2
        let trc10_change = tron_backend_execution::Trc10Change::AssetIssued(
            tron_backend_execution::Trc10AssetIssued {
                owner_address: owner,
                name: asset_info.name.as_bytes().to_vec(),
                abbr: asset_info.abbr.as_bytes().to_vec(),
                total_supply: asset_info.total_supply,
                trx_num: asset_info.trx_num,
                precision: asset_info.precision,
                num: asset_info.num,
                start_time: asset_info.start_time,
                end_time: asset_info.end_time,
                description: asset_info.description.as_bytes().to_vec(),
                url: asset_info.url.as_bytes().to_vec(),
                free_asset_net_limit: asset_info.free_asset_net_limit,
                public_free_asset_net_limit: asset_info.public_free_asset_net_limit,
                public_free_asset_net_usage: asset_info.public_free_asset_net_usage,
                public_latest_free_net_time: asset_info.public_latest_free_net_time,
                token_id: None, // Java will compute via TOKEN_ID_NUM
            }
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(), // No return data for asset issue
            energy_used: 0, // System contracts use 0 energy
            bandwidth_used,
            state_changes,
            logs: Vec::new(), // No logs for asset issue
            error: None,
            aext_map,
            freeze_changes: vec![], // Not applicable for asset issue
            global_resource_changes: vec![], // Not applicable for asset issue
            trc10_changes: vec![trc10_change], // Phase 2: emit TRC-10 semantic change
            vote_changes: vec![], // Not applicable for asset issue
        })
    }

    /// Parse AssetIssueContract protobuf from transaction data
    /// Phase 1: Parse minimal fields (name, total_supply, precision, etc.)
    /// Returns basic asset information without full validation
    fn parse_asset_issue_contract(data: &[u8]) -> Result<AssetIssueInfo, String> {
        use crate::service::contracts::proto::read_varint;

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
                (2, 2) => { // name (bytes)
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read name length: {}", e))?;
                    pos += bytes_read;

                    if pos + length as usize > data.len() {
                        return Err("Invalid name length".to_string());
                    }

                    let name_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    name = String::from_utf8_lossy(name_bytes).to_string();
                },
                (3, 2) => { // abbr (bytes)
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read abbr length: {}", e))?;
                    pos += bytes_read;

                    if pos + length as usize > data.len() {
                        return Err("Invalid abbr length".to_string());
                    }

                    let abbr_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    abbr = String::from_utf8_lossy(abbr_bytes).to_string();
                },
                (4, 0) => { // total_supply (int64, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read total_supply: {}", e))?;
                    pos += bytes_read;
                    total_supply = value as i64;
                },
                (6, 0) => { // trx_num (int32, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read trx_num: {}", e))?;
                    pos += bytes_read;
                    trx_num = value as i32;
                },
                (7, 0) => { // precision (int32, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read precision: {}", e))?;
                    pos += bytes_read;
                    precision = value as i32;
                },
                (8, 0) => { // num (int32, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read num: {}", e))?;
                    pos += bytes_read;
                    num = value as i32;
                },
                (9, 0) => { // start_time (int64, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read start_time: {}", e))?;
                    pos += bytes_read;
                    start_time = value as i64;
                },
                (10, 0) => { // end_time (int64, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read end_time: {}", e))?;
                    pos += bytes_read;
                    end_time = value as i64;
                },
                (20, 2) => { // description (bytes)
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read description length: {}", e))?;
                    pos += bytes_read;

                    if pos + length as usize > data.len() {
                        return Err("Invalid description length".to_string());
                    }

                    let desc_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    description = String::from_utf8_lossy(desc_bytes).to_string();
                },
                (21, 2) => { // url (bytes)
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read url length: {}", e))?;
                    pos += bytes_read;

                    if pos + length as usize > data.len() {
                        return Err("Invalid url length".to_string());
                    }

                    let url_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    url = String::from_utf8_lossy(url_bytes).to_string();
                },
                (22, 0) => { // free_asset_net_limit (int64, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read free_asset_net_limit: {}", e))?;
                    pos += bytes_read;
                    free_asset_net_limit = value as i64;
                },
                (23, 0) => { // public_free_asset_net_limit (int64, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read public_free_asset_net_limit: {}", e))?;
                    pos += bytes_read;
                    public_free_asset_net_limit = value as i64;
                },
                (24, 0) => { // public_free_asset_net_usage (int64, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read public_free_asset_net_usage: {}", e))?;
                    pos += bytes_read;
                    public_free_asset_net_usage = value as i64;
                },
                (25, 0) => { // public_latest_free_net_time (int64, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read public_latest_free_net_time: {}", e))?;
                    pos += bytes_read;
                    public_latest_free_net_time = value as i64;
                },
                _ => {
                    // Skip unknown fields
                    let bytes_skipped = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
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

#[cfg(test)]
mod tests;
