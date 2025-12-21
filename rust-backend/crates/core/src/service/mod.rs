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
            Some(tron_backend_execution::TronContractType::WithdrawBalanceContract) => {
                if !remote_config.withdraw_balance_enabled {
                    return Err("WITHDRAW_BALANCE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing WITHDRAW_BALANCE_CONTRACT");
                self.execute_withdraw_balance_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::AccountCreateContract) => {
                if !remote_config.account_create_enabled {
                    return Err("ACCOUNT_CREATE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing ACCOUNT_CREATE_CONTRACT");
                self.execute_account_create_contract(storage_adapter, transaction, context)
            },
            // Phase 2.A: Proposal contracts (16/17/18)
            Some(tron_backend_execution::TronContractType::ProposalCreateContract) => {
                if !remote_config.proposal_create_enabled {
                    return Err("PROPOSAL_CREATE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing PROPOSAL_CREATE_CONTRACT");
                self.execute_proposal_create_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::ProposalApproveContract) => {
                if !remote_config.proposal_approve_enabled {
                    return Err("PROPOSAL_APPROVE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing PROPOSAL_APPROVE_CONTRACT");
                self.execute_proposal_approve_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::ProposalDeleteContract) => {
                if !remote_config.proposal_delete_enabled {
                    return Err("PROPOSAL_DELETE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing PROPOSAL_DELETE_CONTRACT");
                self.execute_proposal_delete_contract(storage_adapter, transaction, context)
            },
            // Phase 2.B: Account management contracts (19/46)
            Some(tron_backend_execution::TronContractType::SetAccountIdContract) => {
                if !remote_config.set_account_id_enabled {
                    return Err("SET_ACCOUNT_ID_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing SET_ACCOUNT_ID_CONTRACT");
                self.execute_set_account_id_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::AccountPermissionUpdateContract) => {
                if !remote_config.account_permission_update_enabled {
                    return Err("ACCOUNT_PERMISSION_UPDATE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing ACCOUNT_PERMISSION_UPDATE_CONTRACT");
                self.execute_account_permission_update_contract(storage_adapter, transaction, context)
            },
            // Phase 2.C: Contract metadata contracts (33/45/48)
            Some(tron_backend_execution::TronContractType::UpdateSettingContract) => {
                if !remote_config.update_setting_enabled {
                    return Err("UPDATE_SETTING_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing UPDATE_SETTING_CONTRACT");
                self.execute_update_setting_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::UpdateEnergyLimitContract) => {
                if !remote_config.update_energy_limit_enabled {
                    return Err("UPDATE_ENERGY_LIMIT_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing UPDATE_ENERGY_LIMIT_CONTRACT");
                self.execute_update_energy_limit_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::ClearAbiContract) => {
                if !remote_config.clear_abi_enabled {
                    return Err("CLEAR_ABI_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing CLEAR_ABI_CONTRACT");
                self.execute_clear_abi_contract(storage_adapter, transaction, context)
            },
            // Phase 2.C2: UpdateBrokerage contract (49)
            Some(tron_backend_execution::TronContractType::UpdateBrokerageContract) => {
                if !remote_config.update_brokerage_enabled {
                    return Err("UPDATE_BROKERAGE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing UPDATE_BROKERAGE_CONTRACT");
                self.execute_update_brokerage_contract(storage_adapter, transaction, context)
            },
            // Phase 2.D: Resource/Freeze/Delegation contracts (56/57/58/59)
            Some(tron_backend_execution::TronContractType::WithdrawExpireUnfreezeContract) => {
                if !remote_config.withdraw_expire_unfreeze_enabled {
                    return Err("WITHDRAW_EXPIRE_UNFREEZE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing WITHDRAW_EXPIRE_UNFREEZE_CONTRACT");
                self.execute_withdraw_expire_unfreeze_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::DelegateResourceContract) => {
                if !remote_config.delegate_resource_enabled {
                    return Err("DELEGATE_RESOURCE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing DELEGATE_RESOURCE_CONTRACT");
                self.execute_delegate_resource_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::UndelegateResourceContract) => {
                if !remote_config.undelegate_resource_enabled {
                    return Err("UNDELEGATE_RESOURCE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing UNDELEGATE_RESOURCE_CONTRACT");
                self.execute_undelegate_resource_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract) => {
                if !remote_config.cancel_all_unfreeze_v2_enabled {
                    return Err("CANCEL_ALL_UNFREEZE_V2_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing CANCEL_ALL_UNFREEZE_V2_CONTRACT");
                self.execute_cancel_all_unfreeze_v2_contract(storage_adapter, transaction, context)
            },
            // Phase 2.E: TRC-10 Extension contracts (9/14/15)
            Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract) => {
                if !remote_config.participate_asset_issue_enabled {
                    return Err("PARTICIPATE_ASSET_ISSUE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing PARTICIPATE_ASSET_ISSUE_CONTRACT");
                self.execute_participate_asset_issue_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::UnfreezeAssetContract) => {
                if !remote_config.unfreeze_asset_enabled {
                    return Err("UNFREEZE_ASSET_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing UNFREEZE_ASSET_CONTRACT");
                self.execute_unfreeze_asset_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::UpdateAssetContract) => {
                if !remote_config.update_asset_enabled {
                    return Err("UPDATE_ASSET_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing UPDATE_ASSET_CONTRACT");
                self.execute_update_asset_contract(storage_adapter, transaction, context)
            },
            // Phase 2.F: Exchange contracts (41/42/43/44)
            Some(tron_backend_execution::TronContractType::ExchangeCreateContract) => {
                if !remote_config.exchange_create_enabled {
                    return Err("EXCHANGE_CREATE_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing EXCHANGE_CREATE_CONTRACT");
                self.execute_exchange_create_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::ExchangeInjectContract) => {
                if !remote_config.exchange_inject_enabled {
                    return Err("EXCHANGE_INJECT_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing EXCHANGE_INJECT_CONTRACT");
                self.execute_exchange_inject_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::ExchangeWithdrawContract) => {
                if !remote_config.exchange_withdraw_enabled {
                    return Err("EXCHANGE_WITHDRAW_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing EXCHANGE_WITHDRAW_CONTRACT");
                self.execute_exchange_withdraw_contract(storage_adapter, transaction, context)
            },
            Some(tron_backend_execution::TronContractType::ExchangeTransactionContract) => {
                if !remote_config.exchange_transaction_enabled {
                    return Err("EXCHANGE_TRANSACTION_CONTRACT execution is disabled - falling back to Java".to_string());
                }
                debug!("Executing EXCHANGE_TRANSACTION_CONTRACT");
                self.execute_exchange_transaction_contract(storage_adapter, transaction, context)
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
            withdraw_changes: vec![], // Not applicable for value transfers
            tron_transaction_result: None,
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
            withdraw_changes: vec![], // Not applicable for witness creation
            tron_transaction_result: None,
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
            withdraw_changes: vec![], // Not applicable for witness update
            tron_transaction_result: None,
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
            withdraw_changes: vec![], // Not applicable for vote witness
            tron_transaction_result: None,
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
            withdraw_changes: vec![], // Not applicable for account update
            tron_transaction_result: None,
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
        use crate::service::grpc::address::strip_tron_address_prefix;

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("AccountCreate owner={}", owner_tron);

        // 1. Parse AccountCreateContract from transaction.data
        // AccountCreateContract protobuf:
        //   bytes owner_address = 1;   (ignored - use transaction.from)
        //   bytes account_address = 2; (target account to create)
        //   AccountType type = 3;      (ignored - always Normal)
        let target_address = self.parse_account_create_contract(&transaction.data)?;
        let target_tron = tron_backend_common::to_tron_address(&target_address);

        info!(
            "AccountCreate: owner={}, target={}",
            owner_tron, target_tron
        );

        // 2. Validate owner account exists
        let owner_account = storage_adapter.get_account(&owner)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or_else(|| {
                let msg = format!("Account {} does not exist", owner_tron);
                warn!("{}", msg);
                msg
            })?;

        // 3. Validate target account does NOT exist
        let target_exists = storage_adapter.get_account(&target_address)
            .map_err(|e| format!("Failed to check target account: {}", e))?
            .is_some();

        if target_exists {
            warn!("Account has existed: {}", target_tron);
            return Err("Account has existed".to_string());
        }

        // 4. Get fee from dynamic properties
        let fee = storage_adapter.get_create_new_account_fee_in_system_contract()
            .map_err(|e| format!("Failed to get CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT: {}", e))?;

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
        let support_blackhole = storage_adapter.support_black_hole_optimization()
            .map_err(|e| format!("Failed to get SupportBlackHoleOptimization: {}", e))?;

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

        // Persist new account
        storage_adapter
            .set_account(target_address, new_target_account)
            .map_err(|e| format!("Failed to persist new account: {}", e))?;

        // 10. Handle fee burning/crediting (only if fee > 0)
        let fee_destination: String;
        if fee == 0 {
            // No fee to process
            fee_destination = String::from("none(fee=0)");
        } else if support_blackhole {
            // Burn mode - no additional account change needed
            info!("Burning {} SUN (blackhole optimization)", fee);
            fee_destination = String::from("burn");
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
                    .set_account(blackhole_addr, new_blackhole_account)
                    .map_err(|e| format!("Failed to persist blackhole account: {}", e))?;

                let bh_tron = tron_backend_common::to_tron_address(&blackhole_addr);
                info!(
                    "Credited {} SUN to blackhole address {}",
                    fee, bh_tron
                );
                fee_destination = format!("blackhole:{}", bh_tron);
            } else {
                warn!("No blackhole address configured, burning {} SUN", fee);
                fee_destination = String::from("burn(no_addr)");
            }
        }

        // 11. Sort state changes deterministically for CSV parity
        state_changes.sort_by(|a, b| {
            match (a, b) {
                (TronStateChange::AccountChange { address: addr_a, .. },
                 TronStateChange::AccountChange { address: addr_b, .. }) => {
                    addr_a.cmp(addr_b)
                },
                _ => std::cmp::Ordering::Equal,
            }
        });

        // 12. Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // 13. Track AEXT for bandwidth if in tracked mode
        let execution_config = self.get_execution_config()?;
        let aext_mode = execution_config.remote.accountinfo_aext_mode.as_str();
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
                "AEXT tracked for account_create: owner={:?}, path={:?}, before_net_usage={}, after_net_usage={}",
                owner, path, before_aext.net_usage, after_aext.net_usage
            );
        }

        info!(
            "AccountCreate completed: fee={} SUN, state_changes={}, owner={}, target={}, fee_dest={}",
            fee, state_changes.len(), owner_tron, target_tron, fee_destination
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0, // System contracts use 0 energy
            bandwidth_used,
            logs: Vec::new(), // No logs for account creation
            state_changes,
            error: None,
            aext_map,
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: None,
        })
    }

    /// Parse AccountCreateContract from protobuf bytes
    /// AccountCreateContract structure:
    ///   bytes owner_address = 1;   (field 1, length-delimited) - ignored, use tx.from
    ///   bytes account_address = 2; (field 2, length-delimited) - target account
    ///   AccountType type = 3;      (field 3, varint) - ignored, always Normal
    fn parse_account_create_contract(&self, data: &[u8]) -> Result<revm::primitives::Address, String> {
        use crate::service::grpc::address::strip_tron_address_prefix;

        let mut account_address: Option<revm::primitives::Address> = None;
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
                (2, 2) => { // account_address (length-delimited) - the target account to create
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read account_address length: {}", e))?;
                    pos += bytes_read;

                    if pos + length as usize > data.len() {
                        return Err("Invalid account_address length".to_string());
                    }

                    let addr_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    // Handle 21-byte Tron address (0x41 prefix) or 20-byte EVM address
                    let evm_addr_bytes = if addr_bytes.len() == 21 && addr_bytes[0] == 0x41 {
                        // Strip 0x41 prefix
                        &addr_bytes[1..]
                    } else if addr_bytes.len() == 20 {
                        addr_bytes
                    } else {
                        return Err(format!("Invalid account_address length: {}", addr_bytes.len()));
                    };

                    if evm_addr_bytes.len() != 20 {
                        return Err(format!("Invalid EVM address length: {}", evm_addr_bytes.len()));
                    }

                    let mut addr = [0u8; 20];
                    addr.copy_from_slice(evm_addr_bytes);
                    account_address = Some(revm::primitives::Address::from(addr));
                },
                (3, 0) => { // type (varint) - ignored, always use Normal
                    let (_, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read type: {}", e))?;
                    pos += bytes_read;
                },
                _ => {
                    // Skip unknown field
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        account_address.ok_or_else(|| "Missing account_address in AccountCreateContract".to_string())
    }

    // =========================================================================
    // Phase 2.A: Proposal Contracts (16/17/18)
    // =========================================================================
    // These contracts handle TRON governance proposals (parameter changes).
    // Java reference: ProposalCreateActuator, ProposalApproveActuator, ProposalDeleteActuator

    /// Execute a PROPOSAL_CREATE_CONTRACT
    /// Creates a new proposal with specified parameters
    ///
    /// Java reference: ProposalCreateActuator.java
    fn execute_proposal_create_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::TronExecutionResult;
        use tron_backend_execution::protocol::Proposal;
        use prost::Message;

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("ProposalCreate owner={}", owner_tron);

        // 1. Validate owner is a witness
        let is_witness = storage_adapter.is_witness(&owner)
            .map_err(|e| format!("Failed to check witness status: {}", e))?;

        if !is_witness {
            warn!("Account {} is not a witness", owner_tron);
            return Err("Account is not a witness".to_string());
        }

        // 2. Parse ProposalCreateContract from transaction.data
        // ProposalCreateContract:
        //   bytes owner_address = 1;
        //   map<int64, int64> parameters = 2;
        let parameters = self.parse_proposal_create_contract(&transaction.data)?;

        if parameters.is_empty() {
            warn!("ProposalCreate: empty parameters");
            return Err("This proposal has no parameter.".to_string());
        }

        info!("ProposalCreate: {} parameters", parameters.len());

        // 3. Get next proposal ID
        let latest_proposal_num = storage_adapter.get_latest_proposal_num()
            .map_err(|e| format!("Failed to get LATEST_PROPOSAL_NUM: {}", e))?;
        let new_proposal_id = latest_proposal_num + 1;

        // 4. Calculate expiration time
        // Java: now + CommonParameter.getInstance().getProposalExpireTime()
        let execution_config = self.get_execution_config()?;
        let expire_time_ms = execution_config.remote.proposal_expire_time_ms;

        let now = context.block_timestamp as i64;
        let expiration_time = now + (expire_time_ms as i64);

        // 5. Build owner address in 21-byte format
        let mut owner_address_bytes = Vec::with_capacity(21);
        owner_address_bytes.push(0x41);
        owner_address_bytes.extend_from_slice(owner.as_slice());

        // 6. Create new Proposal
        let proposal = Proposal {
            proposal_id: new_proposal_id,
            proposer_address: owner_address_bytes,
            parameters,
            expiration_time,
            create_time: now,
            approvals: Vec::new(),
            state: 0, // PENDING
        };

        // 7. Persist proposal
        storage_adapter.put_proposal(&proposal)
            .map_err(|e| format!("Failed to persist proposal: {}", e))?;

        // 8. Update LATEST_PROPOSAL_NUM
        storage_adapter.set_latest_proposal_num(new_proposal_id)
            .map_err(|e| format!("Failed to update LATEST_PROPOSAL_NUM: {}", e))?;

        info!(
            "ProposalCreate completed: id={}, expiration={}, params={}",
            new_proposal_id, expiration_time, proposal.parameters.len()
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
        })
    }

    /// Parse ProposalCreateContract from protobuf bytes
    /// ProposalCreateContract:
    ///   bytes owner_address = 1;
    ///   map<int64, int64> parameters = 2;
    fn parse_proposal_create_contract(&self, data: &[u8]) -> Result<std::collections::HashMap<i64, i64>, String> {
        let mut parameters = std::collections::HashMap::new();
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address (bytes) - skip, use transaction.from
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 2) => {
                    // parameters (map<int64, int64>) - each entry is length-delimited
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read map entry length: {}", e))?;
                    pos += bytes_read;

                    if pos + length as usize > data.len() {
                        return Err("Invalid map entry length".to_string());
                    }

                    // Parse map entry (key=1, value=2)
                    let entry_data = &data[pos..pos + length as usize];
                    pos += length as usize;

                    let (key, value) = self.parse_map_entry_i64_i64(entry_data)?;
                    parameters.insert(key, value);
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        Ok(parameters)
    }

    /// Parse a map entry with int64 key and int64 value
    fn parse_map_entry_i64_i64(&self, data: &[u8]) -> Result<(i64, i64), String> {
        let mut key: Option<i64> = None;
        let mut value: Option<i64> = None;
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read map entry field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 0) => {
                    // key (int64, varint)
                    let (v, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read map key: {}", e))?;
                    pos += bytes_read;
                    key = Some(v as i64);
                }
                (2, 0) => {
                    // value (int64, varint)
                    let (v, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read map value: {}", e))?;
                    pos += bytes_read;
                    value = Some(v as i64);
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip map entry field: {}", e))?;
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
        use prost::Message;

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("ProposalApprove owner={}", owner_tron);

        // 1. Validate owner is a witness
        let is_witness = storage_adapter.is_witness(&owner)
            .map_err(|e| format!("Failed to check witness status: {}", e))?;

        if !is_witness {
            warn!("Account {} is not a witness", owner_tron);
            return Err("Account is not a witness".to_string());
        }

        // 2. Parse ProposalApproveContract
        // ProposalApproveContract:
        //   bytes owner_address = 1;
        //   int64 proposal_id = 2;
        //   bool is_add_approval = 3;
        let (proposal_id, is_add_approval) = self.parse_proposal_approve_contract(&transaction.data)?;

        info!(
            "ProposalApprove: id={}, is_add={}",
            proposal_id, is_add_approval
        );

        // 3. Get proposal
        let mut proposal = storage_adapter.get_proposal(proposal_id)
            .map_err(|e| format!("Failed to get proposal: {}", e))?
            .ok_or_else(|| format!("Proposal {} does not exist", proposal_id))?;

        // 4. Validate proposal state is PENDING (0)
        if proposal.state != 0 {
            warn!("Proposal {} has already been processed, state={}", proposal_id, proposal.state);
            return Err("Proposal has already been processed".to_string());
        }

        // 5. Build owner address in 21-byte format for comparison
        let mut owner_address_bytes = Vec::with_capacity(21);
        owner_address_bytes.push(0x41);
        owner_address_bytes.extend_from_slice(owner.as_slice());

        // 6. Add or remove approval
        if is_add_approval {
            // Check if already approved
            if proposal.approvals.iter().any(|a| a == &owner_address_bytes) {
                warn!("Witness {} has already approved proposal {}", owner_tron, proposal_id);
                return Err("Witness has already approved".to_string());
            }
            proposal.approvals.push(owner_address_bytes);
            info!("Added approval from {} to proposal {}", owner_tron, proposal_id);
        } else {
            // Remove approval
            let original_len = proposal.approvals.len();
            proposal.approvals.retain(|a| a != &owner_address_bytes);
            if proposal.approvals.len() == original_len {
                warn!("Witness {} has not approved proposal {}", owner_tron, proposal_id);
                return Err("Witness has not approved".to_string());
            }
            info!("Removed approval from {} to proposal {}", owner_tron, proposal_id);
        }

        // 7. Persist updated proposal
        storage_adapter.put_proposal(&proposal)
            .map_err(|e| format!("Failed to persist proposal: {}", e))?;

        info!(
            "ProposalApprove completed: id={}, approvals={}",
            proposal_id, proposal.approvals.len()
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
        })
    }

    /// Parse ProposalApproveContract from protobuf bytes
    /// ProposalApproveContract:
    ///   bytes owner_address = 1;
    ///   int64 proposal_id = 2;
    ///   bool is_add_approval = 3;
    fn parse_proposal_approve_contract(&self, data: &[u8]) -> Result<(i64, bool), String> {
        let mut proposal_id: Option<i64> = None;
        let mut is_add_approval = true; // Default to true
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address - skip
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 0) => {
                    // proposal_id (int64, varint)
                    let (v, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read proposal_id: {}", e))?;
                    pos += bytes_read;
                    proposal_id = Some(v as i64);
                }
                (3, 0) => {
                    // is_add_approval (bool, varint)
                    let (v, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read is_add_approval: {}", e))?;
                    pos += bytes_read;
                    is_add_approval = v != 0;
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        let id = proposal_id.ok_or("Missing proposal_id")?;
        Ok((id, is_add_approval))
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
        use prost::Message;

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("ProposalDelete owner={}", owner_tron);

        // 1. Parse ProposalDeleteContract
        // ProposalDeleteContract:
        //   bytes owner_address = 1;
        //   int64 proposal_id = 2;
        let proposal_id = self.parse_proposal_delete_contract(&transaction.data)?;

        info!("ProposalDelete: id={}", proposal_id);

        // 2. Get proposal
        let mut proposal = storage_adapter.get_proposal(proposal_id)
            .map_err(|e| format!("Failed to get proposal: {}", e))?
            .ok_or_else(|| format!("Proposal {} does not exist", proposal_id))?;

        // 3. Validate owner is the proposer
        let mut owner_address_bytes = Vec::with_capacity(21);
        owner_address_bytes.push(0x41);
        owner_address_bytes.extend_from_slice(owner.as_slice());

        if proposal.proposer_address != owner_address_bytes {
            warn!(
                "Account {} is not the proposer of proposal {}",
                owner_tron, proposal_id
            );
            return Err("Only the proposer can delete the proposal".to_string());
        }

        // 4. Validate proposal state is PENDING (0)
        if proposal.state != 0 {
            warn!("Proposal {} has already been processed, state={}", proposal_id, proposal.state);
            return Err("Proposal has already been processed".to_string());
        }

        // 5. Set state to CANCELED (3)
        proposal.state = 3;

        // 6. Persist updated proposal
        storage_adapter.put_proposal(&proposal)
            .map_err(|e| format!("Failed to persist proposal: {}", e))?;

        info!("ProposalDelete completed: id={}, state=CANCELED", proposal_id);

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
        })
    }

    /// Parse ProposalDeleteContract from protobuf bytes
    /// ProposalDeleteContract:
    ///   bytes owner_address = 1;
    ///   int64 proposal_id = 2;
    fn parse_proposal_delete_contract(&self, data: &[u8]) -> Result<i64, String> {
        let mut proposal_id: Option<i64> = None;
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address - skip
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 0) => {
                    // proposal_id (int64, varint)
                    let (v, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read proposal_id: {}", e))?;
                    pos += bytes_read;
                    proposal_id = Some(v as i64);
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        proposal_id.ok_or_else(|| "Missing proposal_id".to_string())
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

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("SetAccountId owner={}", owner_tron);

        // 1. Parse SetAccountIdContract
        // SetAccountIdContract:
        //   bytes account_id = 1;
        //   bytes owner_address = 2;
        let account_id = self.parse_set_account_id_contract(&transaction.data)?;

        info!("SetAccountId: owner={}, account_id={:?}",
              owner_tron, String::from_utf8_lossy(&account_id));

        // 2. Validate account ID format (8-32 bytes, valid characters)
        if !self.validate_account_id(&account_id) {
            return Err("Invalid account ID format".to_string());
        }

        // 3. Check if account ID is already taken
        if storage_adapter.has_account_id(&account_id)
            .map_err(|e| format!("Failed to check account id: {}", e))? {
            return Err("This account ID has already been used".to_string());
        }

        // 4. Get owner account
        let mut account_proto = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or_else(|| "Account does not exist".to_string())?;

        // 5. Check if account already has an ID
        if !account_proto.account_id.is_empty() {
            return Err("This account has already set an account ID".to_string());
        }

        // 6. Set account ID
        account_proto.account_id = account_id.clone();

        // 7. Persist account
        storage_adapter.put_account_proto(&owner, &account_proto)
            .map_err(|e| format!("Failed to persist account: {}", e))?;

        // 8. Add to account ID index
        let mut owner_address_bytes = Vec::with_capacity(21);
        owner_address_bytes.push(0x41);
        owner_address_bytes.extend_from_slice(owner.as_slice());

        storage_adapter.put_account_id_index(&account_id, &owner_address_bytes)
            .map_err(|e| format!("Failed to persist account id index: {}", e))?;

        info!("SetAccountId completed: owner={}, account_id={:?}",
              owner_tron, String::from_utf8_lossy(&account_id));

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
        })
    }

    /// Parse SetAccountIdContract from protobuf bytes
    /// SetAccountIdContract:
    ///   bytes account_id = 1;
    ///   bytes owner_address = 2;
    fn parse_set_account_id_contract(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let mut account_id: Option<Vec<u8>> = None;
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // account_id (bytes)
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read;
                    let end = pos + length as usize;
                    if end > data.len() {
                        return Err("Invalid account_id length".to_string());
                    }
                    account_id = Some(data[pos..end].to_vec());
                    pos = end;
                }
                (2, 2) => {
                    // owner_address - skip
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        account_id.ok_or_else(|| "Missing account_id".to_string())
    }

    /// Validate account ID format
    /// Java: TransactionUtil.validAccountId(accountId)
    /// Rules:
    /// - Length: 8-32 bytes
    /// - Valid ASCII characters only (no spaces, no Chinese chars)
    fn validate_account_id(&self, account_id: &[u8]) -> bool {
        // Length check: 8-32 bytes
        if account_id.len() < 8 || account_id.len() > 32 {
            return false;
        }

        // Check for valid ASCII characters (printable, no spaces)
        for &b in account_id {
            // Allow: a-z, A-Z, 0-9, underscore
            let valid = (b >= b'a' && b <= b'z')
                || (b >= b'A' && b <= b'Z')
                || (b >= b'0' && b <= b'9')
                || b == b'_';
            if !valid {
                return false;
            }
        }

        true
    }

    /// Execute an ACCOUNT_PERMISSION_UPDATE_CONTRACT (type 46)
    /// Updates owner/witness/active permissions for multi-sig functionality
    ///
    /// Note: This is a complex contract that requires full Account proto support.
    /// For now, we implement a minimal version that:
    /// 1. Validates multi-sign is enabled
    /// 2. Charges the update permission fee
    /// 3. Updates the permissions on the account
    ///
    /// Java reference: AccountPermissionUpdateActuator.java
    fn execute_account_permission_update_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::TronExecutionResult;

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("AccountPermissionUpdate owner={}", owner_tron);

        // 1. Check if multi-sign is enabled
        let allow_multi_sign = storage_adapter.get_allow_multi_sign()
            .map_err(|e| format!("Failed to get allow_multi_sign: {}", e))?;

        if !allow_multi_sign {
            return Err("Multi-sign is not allowed, need to be opened by the committee".to_string());
        }

        // 2. Get fee
        let fee = storage_adapter.get_update_account_permission_fee()
            .map_err(|e| format!("Failed to get update_account_permission_fee: {}", e))?;

        info!("AccountPermissionUpdate: owner={}, fee={}", owner_tron, fee);

        // 3. Get owner account
        let mut account_proto = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or_else(|| "Account does not exist".to_string())?;

        // 4. Parse the permission update contract
        let (owner_permission, witness_permission, active_permissions) =
            self.parse_account_permission_update_contract(&transaction.data)?;

        // 5. Validate permissions (simplified - full validation would be complex)
        // For now, just check that owner permission is present
        if owner_permission.is_none() {
            return Err("Owner permission is required".to_string());
        }

        // 6. Check if account has enough balance for fee
        let current_balance = account_proto.balance;
        if current_balance < fee {
            return Err(format!("Insufficient balance for fee: {} < {}", current_balance, fee));
        }

        // 7. Deduct fee from account
        account_proto.balance = current_balance - fee;

        // 8. Update permissions on account
        if let Some(owner_perm) = owner_permission {
            account_proto.owner_permission = Some(owner_perm);
        }
        if let Some(witness_perm) = witness_permission {
            account_proto.witness_permission = Some(witness_perm);
        }
        // Replace active permissions
        account_proto.active_permission.clear();
        for active in active_permissions {
            account_proto.active_permission.push(active);
        }

        // 9. Persist updated account
        storage_adapter.put_account_proto(&owner, &account_proto)
            .map_err(|e| format!("Failed to persist account: {}", e))?;

        // 10. Handle fee: burn or credit to blackhole
        let support_blackhole_optimization = storage_adapter.support_black_hole_optimization()
            .map_err(|e| format!("Failed to get blackhole optimization flag: {}", e))?;

        if !support_blackhole_optimization {
            // Credit fee to blackhole account
            let blackhole_addr = storage_adapter.get_blackhole_address_evm();
            if let Ok(Some(mut blackhole_account)) = storage_adapter.get_account_proto(&blackhole_addr) {
                blackhole_account.balance += fee;
                let _ = storage_adapter.put_account_proto(&blackhole_addr, &blackhole_account);
            }
        }
        // If blackhole optimization is enabled, fee is just burned (not credited anywhere)

        info!("AccountPermissionUpdate completed: owner={}, fee={}", owner_tron, fee);

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
        })
    }

    /// Parse AccountPermissionUpdateContract from protobuf bytes
    /// AccountPermissionUpdateContract:
    ///   bytes owner_address = 1;
    ///   Permission owner = 2;
    ///   Permission witness = 3;
    ///   repeated Permission actives = 4;
    ///
    /// Returns a tuple: (owner_permission, witness_permission, active_permissions)
    fn parse_account_permission_update_contract(&self, data: &[u8]) -> Result<(
        Option<tron_backend_execution::protocol::Permission>,
        Option<tron_backend_execution::protocol::Permission>,
        Vec<tron_backend_execution::protocol::Permission>,
    ), String> {
        use prost::Message;
        use tron_backend_execution::protocol::Permission;

        let mut owner_permission: Option<Permission> = None;
        let mut witness_permission: Option<Permission> = None;
        let mut active_permissions: Vec<Permission> = Vec::new();
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address - skip
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 2) => {
                    // owner permission
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read;
                    let end = pos + length as usize;
                    if end > data.len() {
                        return Err("Invalid owner permission length".to_string());
                    }
                    owner_permission = Some(Permission::decode(&data[pos..end])
                        .map_err(|e| format!("Failed to decode owner permission: {}", e))?);
                    pos = end;
                }
                (3, 2) => {
                    // witness permission
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read;
                    let end = pos + length as usize;
                    if end > data.len() {
                        return Err("Invalid witness permission length".to_string());
                    }
                    witness_permission = Some(Permission::decode(&data[pos..end])
                        .map_err(|e| format!("Failed to decode witness permission: {}", e))?);
                    pos = end;
                }
                (4, 2) => {
                    // active permission (repeated)
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read;
                    let end = pos + length as usize;
                    if end > data.len() {
                        return Err("Invalid active permission length".to_string());
                    }
                    let perm = Permission::decode(&data[pos..end])
                        .map_err(|e| format!("Failed to decode active permission: {}", e))?;
                    active_permissions.push(perm);
                    pos = end;
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        Ok((owner_permission, witness_permission, active_permissions))
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
            withdraw_changes: vec![], // Not applicable for TRC-10 transfers
            tron_transaction_result: None,
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
            withdraw_changes: vec![], // Not applicable for asset issue
            tron_transaction_result: None,
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
        use tron_backend_execution::TronExecutionResult;
        use contracts::proto::read_varint;

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        debug!("Executing UPDATE_SETTING_CONTRACT for owner {}", owner_tron);

        // 1. Parse the contract data
        let (contract_address, new_percent) = self.parse_update_setting_contract(&transaction.data)?;

        debug!("Parsed UpdateSettingContract: contract_address={}, new_percent={}",
               hex::encode(&contract_address), new_percent);

        // 2. Validate owner exists
        // Build owner key as 21-byte TRON address (0x41 prefix + 20-byte EVM address)
        let mut owner_key = Vec::with_capacity(21);
        owner_key.push(0x41);
        owner_key.extend_from_slice(owner.as_slice());

        let _owner_account = storage_adapter.get_account(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("Owner account {} does not exist", owner_tron))?;

        // 3. Validate new_percent is in [0, 100]
        if new_percent < 0 || new_percent > 100 {
            return Err(format!("percent not in [0, 100]: {}", new_percent));
        }

        // 4. Get the smart contract
        let mut smart_contract = storage_adapter.get_smart_contract(&contract_address)
            .map_err(|e| format!("Failed to get contract: {}", e))?
            .ok_or_else(|| "Contract does not exist".to_string())?;

        // 5. Validate owner is the contract's origin_address
        if smart_contract.origin_address != owner_key {
            return Err(format!("Account {} is not the owner of the contract", owner_tron));
        }

        // 6. Update the consume_user_resource_percent field
        let old_percent = smart_contract.consume_user_resource_percent;
        smart_contract.consume_user_resource_percent = new_percent;

        debug!("Updating consume_user_resource_percent: {} -> {}", old_percent, new_percent);

        // 7. Write back to ContractStore
        storage_adapter.put_smart_contract(&smart_contract)
            .map_err(|e| format!("Failed to update contract: {}", e))?;

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
        })
    }

    /// Parse UpdateSettingContract from protobuf bytes
    /// UpdateSettingContract:
    ///   bytes owner_address = 1;
    ///   bytes contract_address = 2;
    ///   int64 consume_user_resource_percent = 3;
    fn parse_update_setting_contract(&self, data: &[u8]) -> Result<(Vec<u8>, i64), String> {
        use contracts::proto::read_varint;

        let mut contract_address: Vec<u8> = vec![];
        let mut consume_user_resource_percent: i64 = 0;
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address - skip
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 2) => {
                    // contract_address
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read;
                    let end = pos + length as usize;
                    if end > data.len() {
                        return Err("Invalid contract_address length".to_string());
                    }
                    contract_address = data[pos..end].to_vec();
                    pos = end;
                }
                (3, 0) => {
                    // consume_user_resource_percent (varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read percent: {}", e))?;
                    pos += bytes_read;
                    consume_user_resource_percent = value as i64;
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        if contract_address.is_empty() {
            return Err("contract_address is required".to_string());
        }

        Ok((contract_address, consume_user_resource_percent))
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

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        debug!("Executing UPDATE_ENERGY_LIMIT_CONTRACT for owner {}", owner_tron);

        // 1. Check if energy limit feature is enabled
        // This is equivalent to ReceiptCapsule.checkForEnergyLimit()
        let energy_limit_enabled = storage_adapter.check_for_energy_limit()
            .map_err(|e| format!("Failed to check energy limit: {}", e))?;

        if !energy_limit_enabled {
            return Err("contract type error, unexpected type [UpdateEnergyLimitContract]".to_string());
        }

        // 2. Parse the contract data
        let (contract_address, new_origin_energy_limit) = self.parse_update_energy_limit_contract(&transaction.data)?;

        debug!("Parsed UpdateEnergyLimitContract: contract_address={}, new_origin_energy_limit={}",
               hex::encode(&contract_address), new_origin_energy_limit);

        // 3. Validate owner exists
        // Build owner key as 21-byte TRON address (0x41 prefix + 20-byte EVM address)
        let mut owner_key = Vec::with_capacity(21);
        owner_key.push(0x41);
        owner_key.extend_from_slice(owner.as_slice());

        let _owner_account = storage_adapter.get_account(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("Owner account {} does not exist", owner_tron))?;

        // 4. Validate new_origin_energy_limit > 0
        if new_origin_energy_limit <= 0 {
            return Err("origin energy limit must be > 0".to_string());
        }

        // 5. Get the smart contract
        let mut smart_contract = storage_adapter.get_smart_contract(&contract_address)
            .map_err(|e| format!("Failed to get contract: {}", e))?
            .ok_or_else(|| "Contract does not exist".to_string())?;

        // 6. Validate owner is the contract's origin_address
        if smart_contract.origin_address != owner_key {
            return Err(format!("Account {} is not the owner of the contract", owner_tron));
        }

        // 7. Update the origin_energy_limit field
        let old_limit = smart_contract.origin_energy_limit;
        smart_contract.origin_energy_limit = new_origin_energy_limit;

        debug!("Updating origin_energy_limit: {} -> {}", old_limit, new_origin_energy_limit);

        // 8. Write back to ContractStore
        storage_adapter.put_smart_contract(&smart_contract)
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
        })
    }

    /// Parse UpdateEnergyLimitContract from protobuf bytes
    /// UpdateEnergyLimitContract:
    ///   bytes owner_address = 1;
    ///   bytes contract_address = 2;
    ///   int64 origin_energy_limit = 3;
    fn parse_update_energy_limit_contract(&self, data: &[u8]) -> Result<(Vec<u8>, i64), String> {
        use contracts::proto::read_varint;

        let mut contract_address: Vec<u8> = vec![];
        let mut origin_energy_limit: i64 = 0;
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address - skip
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 2) => {
                    // contract_address
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read;
                    let end = pos + length as usize;
                    if end > data.len() {
                        return Err("Invalid contract_address length".to_string());
                    }
                    contract_address = data[pos..end].to_vec();
                    pos = end;
                }
                (3, 0) => {
                    // origin_energy_limit (varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read origin_energy_limit: {}", e))?;
                    pos += bytes_read;
                    origin_energy_limit = value as i64;
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        if contract_address.is_empty() {
            return Err("contract_address is required".to_string());
        }

        Ok((contract_address, origin_energy_limit))
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

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        debug!("Executing CLEAR_ABI_CONTRACT for owner {}", owner_tron);

        // 1. Check if Constantinople is enabled
        let allow_constantinople = storage_adapter.get_allow_tvm_constantinople()
            .map_err(|e| format!("Failed to get Constantinople status: {}", e))?;

        if allow_constantinople == 0 {
            return Err("contract type error,unexpected type [ClearABIContract]".to_string());
        }

        // 2. Parse the contract data
        let contract_address = self.parse_clear_abi_contract(&transaction.data)?;

        debug!("Parsed ClearABIContract: contract_address={}", hex::encode(&contract_address));

        // 3. Validate owner exists
        // Build owner key as 21-byte TRON address (0x41 prefix + 20-byte EVM address)
        let mut owner_key = Vec::with_capacity(21);
        owner_key.push(0x41);
        owner_key.extend_from_slice(owner.as_slice());

        let _owner_account = storage_adapter.get_account(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("Owner account {} does not exist", owner_tron))?;

        // 4. Get the smart contract (to validate ownership)
        let smart_contract = storage_adapter.get_smart_contract(&contract_address)
            .map_err(|e| format!("Failed to get contract: {}", e))?
            .ok_or_else(|| "Contract not exists".to_string())?;

        // 5. Validate owner is the contract's origin_address
        if smart_contract.origin_address != owner_key {
            return Err(format!("Account {} is not the owner of the contract", owner_tron));
        }

        // 6. Clear ABI by writing default empty ABI to AbiStore
        storage_adapter.clear_abi(&contract_address)
            .map_err(|e| format!("Failed to clear ABI: {}", e))?;

        debug!("ABI cleared for contract {}", hex::encode(&contract_address));

        // 7. Build result - no state changes for account balances, fee = 0
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
        use tron_backend_execution::TronExecutionResult;

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        debug!("Executing UPDATE_BROKERAGE_CONTRACT for owner {}", owner_tron);

        // 1. Check if delegation changes are allowed
        // Java: dynamicStore.allowChangeDelegation()
        let allow_delegation = storage_adapter.allow_change_delegation()
            .map_err(|e| format!("Failed to check delegation status: {}", e))?;

        if !allow_delegation {
            return Err("contract type error, unexpected type [UpdateBrokerageContract]".to_string());
        }

        // 2. Parse the contract data to get brokerage value
        let brokerage = self.parse_update_brokerage_contract(&transaction.data)?;

        debug!("Parsed UpdateBrokerageContract: brokerage={}%", brokerage);

        // 3. Validate brokerage range: 0-100
        // Java: if (brokerage < 0 || brokerage > ActuatorConstant.ONE_HUNDRED)
        if brokerage < 0 || brokerage > 100 {
            return Err("Invalid brokerage".to_string());
        }

        // 4. Validate owner exists in AccountStore
        let _owner_account = storage_adapter.get_account(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| "Account does not exist".to_string())?;

        // 5. Validate owner is a witness
        // Java: WitnessCapsule witnessCapsule = witnessStore.get(ownerAddress);
        //       if (witnessCapsule == null) throw "Not existed witness"
        let is_witness = storage_adapter.is_witness(&owner)
            .map_err(|e| format!("Failed to check witness: {}", e))?;

        if !is_witness {
            // Build 21-byte TRON address for error message
            let mut owner_key = Vec::with_capacity(21);
            owner_key.push(0x41);
            owner_key.extend_from_slice(owner.as_slice());
            return Err(format!("Not existed witness:{}", hex::encode(&owner_key)));
        }

        // 6. Set brokerage in DelegationStore
        // Java: delegationStore.setBrokerage(ownerAddress, brokerage)
        // This is equivalent to setBrokerage(-1, ownerAddress, brokerage)
        storage_adapter.set_delegation_brokerage(-1, &owner, brokerage)
            .map_err(|e| format!("Failed to set brokerage: {}", e))?;

        debug!("Brokerage set to {}% for witness {}", brokerage, owner_tron);

        // 7. Build result - no fee for this contract, no account balance changes
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
        })
    }

    /// Parse UpdateBrokerageContract from protobuf bytes
    /// UpdateBrokerageContract:
    ///   bytes owner_address = 1;
    ///   int32 brokerage = 2;
    fn parse_update_brokerage_contract(&self, data: &[u8]) -> Result<i32, String> {
        use contracts::proto::read_varint;

        let mut brokerage: i32 = 0;
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address - skip (we already have it from transaction.from)
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 0) => {
                    // brokerage (int32, wire type 0 = varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read brokerage: {}", e))?;
                    pos += bytes_read;
                    brokerage = value as i32;
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        Ok(brokerage)
    }

    /// Parse ClearABIContract from protobuf bytes
    /// ClearABIContract:
    ///   bytes owner_address = 1;
    ///   bytes contract_address = 2;
    fn parse_clear_abi_contract(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        use contracts::proto::read_varint;

        let mut contract_address: Vec<u8> = vec![];
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address - skip
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 2) => {
                    // contract_address
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read;
                    let end = pos + length as usize;
                    if end > data.len() {
                        return Err("Invalid contract_address length".to_string());
                    }
                    contract_address = data[pos..end].to_vec();
                    pos = end;
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        if contract_address.is_empty() {
            return Err("contract_address is required".to_string());
        }

        Ok(contract_address)
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
        use contracts::proto::TransactionResultBuilder;

        let owner = transaction.from;
        let owner_tron = add_tron_address_prefix(&owner);

        debug!("WithdrawExpireUnfreeze: owner={}", hex::encode(&owner_tron));

        // 1. Gate check: supportUnfreezeDelay() must be true
        let support_unfreeze_delay = storage_adapter.support_unfreeze_delay()
            .map_err(|e| format!("Failed to check supportUnfreezeDelay: {}", e))?;
        if !support_unfreeze_delay {
            return Err("Not support WithdrawExpireUnfreeze transaction, need to be opened by the committee".to_string());
        }

        // 2. Validate owner account exists
        let account_proto = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or_else(|| format!("Account[{}] does not exist", hex::encode(&owner_tron)))?;

        // 3. Get latest block timestamp
        let now = storage_adapter.get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest timestamp: {}", e))?;

        // 4. Calculate total withdrawable amount from expired unfrozenV2 entries
        let unfrozen_v2_list = &account_proto.unfrozen_v2;
        let mut total_withdraw: i64 = 0;
        let mut remaining_unfrozen: Vec<tron_backend_execution::protocol::account::UnFreezeV2> = Vec::new();

        for entry in unfrozen_v2_list.iter() {
            if entry.unfreeze_expire_time <= now as i64 {
                // Expired - add to withdraw amount
                total_withdraw = total_withdraw.checked_add(entry.unfreeze_amount)
                    .ok_or("Overflow calculating withdraw amount")?;
            } else {
                // Not expired - keep in list
                remaining_unfrozen.push(entry.clone());
            }
        }

        // 5. Validate there's something to withdraw
        if total_withdraw <= 0 {
            return Err("no unFreeze balance to withdraw".to_string());
        }

        // 6. Check for overflow
        let new_balance = account_proto.balance.checked_add(total_withdraw)
            .ok_or("Balance overflow")?;

        // 7. Update account: balance += total_withdraw, clear and replace unfrozenV2 list
        let mut updated_account = account_proto.clone();
        updated_account.balance = new_balance;
        updated_account.unfrozen_v2.clear();
        for entry in remaining_unfrozen {
            updated_account.unfrozen_v2.push(entry);
        }

        // 8. Persist updated account
        storage_adapter.put_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to persist account: {}", e))?;

        // 9. Build state change for CSV parity
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

        // 10. Build receipt with withdraw_expire_amount
        let receipt_bytes = TransactionResultBuilder::new()
            .with_withdraw_expire_amount(total_withdraw)
            .build();

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!("WithdrawExpireUnfreeze: withdrew {} SUN, remaining entries: {}",
               total_withdraw, updated_account.unfrozen_v2.len());

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
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use contracts::proto::TransactionResultBuilder;

        let owner = transaction.from;
        let owner_tron = add_tron_address_prefix(&owner);

        debug!("CancelAllUnfreezeV2: owner={}", hex::encode(&owner_tron));

        // 1. Gate check: supportAllowCancelAllUnfreezeV2() must be true
        let allow_cancel = storage_adapter.support_allow_cancel_all_unfreeze_v2()
            .map_err(|e| format!("Failed to check supportAllowCancelAllUnfreezeV2: {}", e))?;
        if !allow_cancel {
            return Err("Not support CancelAllUnfreezeV2 transaction, need to be opened by the committee".to_string());
        }

        // 2. Validate owner account exists
        let account_proto = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or_else(|| format!("Account[{}] does not exist", hex::encode(&owner_tron)))?;

        // 3. Get latest block timestamp
        let now = storage_adapter.get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest timestamp: {}", e))?;

        // 4. Validate there are unfrozenV2 entries to process
        let unfrozen_v2_list = &account_proto.unfrozen_v2;
        if unfrozen_v2_list.is_empty() {
            return Err("No unfreezeV2 list to cancel".to_string());
        }

        // 5. Process each unfrozenV2 entry:
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
                withdraw_expire_amount = withdraw_expire_amount.checked_add(entry.unfreeze_amount)
                    .ok_or("Overflow calculating withdraw amount")?;
            } else {
                // Unexpired - re-freeze
                let resource_type = entry.r#type; // 0=BANDWIDTH, 1=ENERGY, 2=TRON_POWER
                let amount = entry.unfreeze_amount;

                match resource_type {
                    0 => {
                        // BANDWIDTH
                        cancel_bandwidth += amount;
                        // Re-freeze: add to frozenV2 bandwidth
                        Self::add_frozen_v2_bandwidth(&mut updated_account, amount);
                        // Update weight delta (amount / TRX_PRECISION)
                        let weight_before = Self::get_frozen_v2_balance_with_delegated_bandwidth(&account_proto) / TRX_PRECISION as i64;
                        let weight_after = Self::get_frozen_v2_balance_with_delegated_bandwidth(&updated_account) / TRX_PRECISION as i64;
                        net_weight_delta += weight_after - weight_before;
                    }
                    1 => {
                        // ENERGY
                        cancel_energy += amount;
                        Self::add_frozen_v2_energy(&mut updated_account, amount);
                        let weight_before = Self::get_frozen_v2_balance_with_delegated_energy(&account_proto) / TRX_PRECISION as i64;
                        let weight_after = Self::get_frozen_v2_balance_with_delegated_energy(&updated_account) / TRX_PRECISION as i64;
                        energy_weight_delta += weight_after - weight_before;
                    }
                    2 => {
                        // TRON_POWER
                        cancel_tron_power += amount;
                        Self::add_frozen_v2_tron_power(&mut updated_account, amount);
                        let weight_before = Self::get_tron_power_frozen_v2_balance(&account_proto) / TRX_PRECISION as i64;
                        let weight_after = Self::get_tron_power_frozen_v2_balance(&updated_account) / TRX_PRECISION as i64;
                        tp_weight_delta += weight_after - weight_before;
                    }
                    _ => {
                        warn!("Unknown resource type {} in unfrozenV2", resource_type);
                    }
                }
            }
        }

        // 6. Clear unfrozenV2 list
        updated_account.unfrozen_v2.clear();

        // 7. Add expired amount to balance
        if withdraw_expire_amount > 0 {
            updated_account.balance = updated_account.balance.checked_add(withdraw_expire_amount)
                .ok_or("Balance overflow")?;
        }

        // 8. Update total resource weights in DynamicPropertiesStore
        if net_weight_delta != 0 {
            storage_adapter.add_total_net_weight(net_weight_delta)
                .map_err(|e| format!("Failed to update total net weight: {}", e))?;
        }
        if energy_weight_delta != 0 {
            storage_adapter.add_total_energy_weight(energy_weight_delta)
                .map_err(|e| format!("Failed to update total energy weight: {}", e))?;
        }
        if tp_weight_delta != 0 {
            storage_adapter.add_total_tron_power_weight(tp_weight_delta)
                .map_err(|e| format!("Failed to update total tron power weight: {}", e))?;
        }

        // 9. Persist updated account
        storage_adapter.put_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to persist account: {}", e))?;

        // 10. Build state change for CSV parity
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

        // 11. Build receipt with withdraw_expire_amount and cancel_unfreezeV2_amount map
        let receipt_bytes = TransactionResultBuilder::new()
            .with_withdraw_expire_amount(withdraw_expire_amount)
            .with_cancel_unfreeze_v2_amounts(cancel_bandwidth, cancel_energy, cancel_tron_power)
            .build();

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!("CancelAllUnfreezeV2: withdrew={}, cancel_bw={}, cancel_energy={}, cancel_tp={}",
               withdraw_expire_amount, cancel_bandwidth, cancel_energy, cancel_tron_power);

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
        let owner = transaction.from;
        let owner_tron = add_tron_address_prefix(&owner);

        // Parse contract data
        let delegate_info = self.parse_delegate_resource_contract(&transaction.data)?;

        let receiver_address = if delegate_info.receiver_address.len() == 21 {
            revm_primitives::Address::from_slice(&delegate_info.receiver_address[1..])
        } else if delegate_info.receiver_address.len() == 20 {
            revm_primitives::Address::from_slice(&delegate_info.receiver_address)
        } else {
            return Err("Invalid receiver address length".to_string());
        };
        let receiver_tron = add_tron_address_prefix(&receiver_address);

        debug!("DelegateResource: owner={}, receiver={}, balance={}, resource={}, lock={}, lock_period={}",
               hex::encode(&owner_tron), hex::encode(&receiver_tron),
               delegate_info.balance, delegate_info.resource, delegate_info.lock, delegate_info.lock_period);

        // 1. Gate check: supportDR() must be true
        let support_dr = storage_adapter.support_dr()
            .map_err(|e| format!("Failed to check supportDR: {}", e))?;
        if !support_dr {
            return Err("No support for resource delegate".to_string());
        }

        // 2. Gate check: supportUnfreezeDelay() must be true
        let support_unfreeze_delay = storage_adapter.support_unfreeze_delay()
            .map_err(|e| format!("Failed to check supportUnfreezeDelay: {}", e))?;
        if !support_unfreeze_delay {
            return Err("Not support Delegate resource transaction, need to be opened by the committee".to_string());
        }

        // 3. Validate owner account exists
        let owner_account = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("Account[{}] does not exist", hex::encode(&owner_tron)))?;

        // 4. Validate delegate balance >= 1 TRX
        if delegate_info.balance < TRX_PRECISION as i64 {
            return Err("delegateBalance must be greater than or equal to 1 TRX".to_string());
        }

        // 5. Validate receiver is different from owner
        if owner == receiver_address {
            return Err("receiverAddress must not be the same as ownerAddress".to_string());
        }

        // 6. Validate receiver exists
        let receiver_account = storage_adapter.get_account_proto(&receiver_address)
            .map_err(|e| format!("Failed to get receiver account: {}", e))?
            .ok_or_else(|| format!("Account[{}] does not exist", hex::encode(&receiver_tron)))?;

        // 7. Validate receiver is not a contract
        if receiver_account.r#type == 2 { // Contract type
            return Err("Do not allow delegate resources to contract addresses".to_string());
        }

        // 8. Validate sufficient frozen balance for the resource type
        match delegate_info.resource {
            0 => { // BANDWIDTH
                let frozen_v2_bandwidth = Self::get_frozen_v2_balance_for_bandwidth(&owner_account);
                if frozen_v2_bandwidth < delegate_info.balance {
                    return Err("delegateBalance must be less than or equal to available FreezeBandwidthV2 balance".to_string());
                }
            }
            1 => { // ENERGY
                let frozen_v2_energy = Self::get_frozen_v2_balance_for_energy(&owner_account);
                if frozen_v2_energy < delegate_info.balance {
                    return Err("delegateBalance must be less than or equal to available FreezeEnergyV2 balance".to_string());
                }
            }
            _ => {
                return Err("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]".to_string());
            }
        }

        // 9. Get timestamp and calculate expiration
        let now = storage_adapter.get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest timestamp: {}", e))?;

        let lock_period = if delegate_info.lock {
            if delegate_info.lock_period == 0 {
                // Default lock period: DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL
                // DELEGATE_PERIOD = 3 days in ms, BLOCK_PRODUCED_INTERVAL = 3000 ms
                86400 // 3 days worth of blocks
            } else {
                delegate_info.lock_period
            }
        } else {
            0
        };
        let expire_time = if delegate_info.lock {
            now as i64 + lock_period * 3000 // BLOCK_PRODUCED_INTERVAL = 3000ms
        } else {
            0
        };

        // 10. Update owner account
        let mut updated_owner = owner_account.clone();
        match delegate_info.resource {
            0 => { // BANDWIDTH
                Self::add_delegated_frozen_v2_balance_for_bandwidth(&mut updated_owner, delegate_info.balance);
                Self::add_frozen_v2_bandwidth(&mut updated_owner, -delegate_info.balance);
            }
            1 => { // ENERGY
                Self::add_delegated_frozen_v2_balance_for_energy(&mut updated_owner, delegate_info.balance);
                Self::add_frozen_v2_energy(&mut updated_owner, -delegate_info.balance);
            }
            _ => {}
        }

        // 11. Update receiver account
        let mut updated_receiver = receiver_account.clone();
        match delegate_info.resource {
            0 => { // BANDWIDTH
                Self::add_acquired_delegated_frozen_v2_balance_for_bandwidth(&mut updated_receiver, delegate_info.balance);
            }
            1 => { // ENERGY
                Self::add_acquired_delegated_frozen_v2_balance_for_energy(&mut updated_receiver, delegate_info.balance);
            }
            _ => {}
        }

        // 12. Update/Create DelegatedResource record
        storage_adapter.delegate_resource(
            &owner,
            &receiver_address,
            delegate_info.resource == 0, // isBandwidth
            delegate_info.balance,
            delegate_info.lock,
            expire_time,
        ).map_err(|e| format!("Failed to update DelegatedResource: {}", e))?;

        // 13. Update DelegatedResourceAccountIndex
        storage_adapter.delegate_resource_account_index(
            &owner,
            &receiver_address,
            now as i64,
        ).map_err(|e| format!("Failed to update DelegatedResourceAccountIndex: {}", e))?;

        // 14. Persist accounts
        storage_adapter.put_account_proto(&owner, &updated_owner)
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;
        storage_adapter.put_account_proto(&receiver_address, &updated_receiver)
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

        debug!("DelegateResource: delegated {} SUN of resource {} from {} to {}",
               delegate_info.balance, delegate_info.resource,
               hex::encode(&owner_tron), hex::encode(&receiver_tron));

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
        let owner = transaction.from;
        let owner_tron = add_tron_address_prefix(&owner);

        // Parse contract data
        let undelegate_info = self.parse_undelegate_resource_contract(&transaction.data)?;

        let receiver_address = if undelegate_info.receiver_address.len() == 21 {
            revm_primitives::Address::from_slice(&undelegate_info.receiver_address[1..])
        } else if undelegate_info.receiver_address.len() == 20 {
            revm_primitives::Address::from_slice(&undelegate_info.receiver_address)
        } else {
            return Err("Invalid receiver address length".to_string());
        };
        let receiver_tron = add_tron_address_prefix(&receiver_address);

        debug!("UnDelegateResource: owner={}, receiver={}, balance={}, resource={}",
               hex::encode(&owner_tron), hex::encode(&receiver_tron),
               undelegate_info.balance, undelegate_info.resource);

        // 1. Gate checks
        let support_dr = storage_adapter.support_dr()
            .map_err(|e| format!("Failed to check supportDR: {}", e))?;
        if !support_dr {
            return Err("No support for resource delegate".to_string());
        }

        let support_unfreeze_delay = storage_adapter.support_unfreeze_delay()
            .map_err(|e| format!("Failed to check supportUnfreezeDelay: {}", e))?;
        if !support_unfreeze_delay {
            return Err("Not support unDelegate resource transaction, need to be opened by the committee".to_string());
        }

        // 2. Validate owner exists
        let owner_account = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or_else(|| format!("Account[{}] does not exist", hex::encode(&owner_tron)))?;

        // 3. Validate balance > 0
        if undelegate_info.balance <= 0 {
            return Err("unDelegateBalance must be more than 0 TRX".to_string());
        }

        // 4. Validate receiver different from owner
        if owner == receiver_address {
            return Err("receiverAddress must not be the same as ownerAddress".to_string());
        }

        // 5. Get timestamp
        let now = storage_adapter.get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get latest timestamp: {}", e))?;

        // 6. Check DelegatedResource exists and has sufficient balance
        let delegate_balance = storage_adapter.get_available_delegate_balance(
            &owner,
            &receiver_address,
            undelegate_info.resource == 0,
            now as i64,
        ).map_err(|e| format!("Failed to get delegated balance: {}", e))?;

        if delegate_balance < undelegate_info.balance {
            let resource_name = if undelegate_info.resource == 0 { "BANDWIDTH" } else { "Energy" };
            return Err(format!("insufficient delegatedFrozenBalance({}), request={}, unlock_balance={}",
                               resource_name, undelegate_info.balance, delegate_balance));
        }

        // 7. Get receiver account (might not exist if contract was destroyed)
        let receiver_account_opt = storage_adapter.get_account_proto(&receiver_address)
            .map_err(|e| format!("Failed to get receiver account: {}", e))?;

        // 8. Update receiver if exists (reduce acquired balance)
        let mut updated_receiver_opt = None;
        if let Some(receiver_account) = receiver_account_opt.as_ref() {
            let mut updated_receiver = receiver_account.clone();
            match undelegate_info.resource {
                0 => { // BANDWIDTH
                    let current = Self::get_acquired_delegated_frozen_v2_balance_for_bandwidth(&updated_receiver);
                    if current < undelegate_info.balance {
                        // Edge case: contract suicide/re-create
                        Self::set_acquired_delegated_frozen_v2_balance_for_bandwidth(&mut updated_receiver, 0);
                    } else {
                        Self::add_acquired_delegated_frozen_v2_balance_for_bandwidth(&mut updated_receiver, -undelegate_info.balance);
                    }
                }
                1 => { // ENERGY
                    let current = Self::get_acquired_delegated_frozen_v2_balance_for_energy(&updated_receiver);
                    if current < undelegate_info.balance {
                        Self::set_acquired_delegated_frozen_v2_balance_for_energy(&mut updated_receiver, 0);
                    } else {
                        Self::add_acquired_delegated_frozen_v2_balance_for_energy(&mut updated_receiver, -undelegate_info.balance);
                    }
                }
                _ => {}
            }
            updated_receiver_opt = Some(updated_receiver);
        }

        // 9. Update DelegatedResourceStore
        storage_adapter.undelegate_resource(
            &owner,
            &receiver_address,
            undelegate_info.resource == 0,
            undelegate_info.balance,
            now as i64,
        ).map_err(|e| format!("Failed to update DelegatedResource: {}", e))?;

        // 10. Update owner account (add back to frozen, reduce delegated)
        let mut updated_owner = owner_account.clone();
        match undelegate_info.resource {
            0 => { // BANDWIDTH
                Self::add_delegated_frozen_v2_balance_for_bandwidth(&mut updated_owner, -undelegate_info.balance);
                Self::add_frozen_v2_bandwidth(&mut updated_owner, undelegate_info.balance);
            }
            1 => { // ENERGY
                Self::add_delegated_frozen_v2_balance_for_energy(&mut updated_owner, -undelegate_info.balance);
                Self::add_frozen_v2_energy(&mut updated_owner, undelegate_info.balance);
            }
            _ => {}
        }

        // 11. Persist accounts
        storage_adapter.put_account_proto(&owner, &updated_owner)
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;

        if let Some(updated_receiver) = updated_receiver_opt.as_ref() {
            storage_adapter.put_account_proto(&receiver_address, updated_receiver)
                .map_err(|e| format!("Failed to persist receiver account: {}", e))?;
        }

        // 12. Build state changes
        let mut state_changes = vec![
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
        ];

        if let (Some(receiver_account), Some(updated_receiver)) = (receiver_account_opt.as_ref(), updated_receiver_opt.as_ref()) {
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

        debug!("UnDelegateResource: undelegated {} SUN of resource {} from {} back to {}",
               undelegate_info.balance, undelegate_info.resource,
               hex::encode(&receiver_tron), hex::encode(&owner_tron));

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
    fn parse_delegate_resource_contract(&self, data: &[u8]) -> Result<DelegateResourceInfo, String> {
        use contracts::proto::read_varint;

        let mut receiver_address: Vec<u8> = vec![];
        let mut balance: i64 = 0;
        let mut resource: i32 = 0;
        let mut lock: bool = false;
        let mut lock_period: i64 = 0;
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address - skip
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 0) => {
                    // resource (ResourceCode enum, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read resource: {}", e))?;
                    pos += bytes_read;
                    resource = value as i32;
                }
                (3, 0) => {
                    // balance (int64, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read balance: {}", e))?;
                    pos += bytes_read;
                    balance = value as i64;
                }
                (4, 2) => {
                    // receiver_address
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read;
                    let end = pos + length as usize;
                    if end > data.len() {
                        return Err("Invalid receiver_address length".to_string());
                    }
                    receiver_address = data[pos..end].to_vec();
                    pos = end;
                }
                (5, 0) => {
                    // lock (bool, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read lock: {}", e))?;
                    pos += bytes_read;
                    lock = value != 0;
                }
                (6, 0) => {
                    // lock_period (int64, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read lock_period: {}", e))?;
                    pos += bytes_read;
                    lock_period = value as i64;
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        if receiver_address.is_empty() {
            return Err("receiver_address is required".to_string());
        }

        Ok(DelegateResourceInfo {
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
    fn parse_undelegate_resource_contract(&self, data: &[u8]) -> Result<UnDelegateResourceInfo, String> {
        use contracts::proto::read_varint;

        let mut receiver_address: Vec<u8> = vec![];
        let mut balance: i64 = 0;
        let mut resource: i32 = 0;
        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // owner_address - skip
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 0) => {
                    // resource (ResourceCode enum, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read resource: {}", e))?;
                    pos += bytes_read;
                    resource = value as i32;
                }
                (3, 0) => {
                    // balance (int64, varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read balance: {}", e))?;
                    pos += bytes_read;
                    balance = value as i64;
                }
                (4, 2) => {
                    // receiver_address
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read length: {}", e))?;
                    pos += bytes_read;
                    let end = pos + length as usize;
                    if end > data.len() {
                        return Err("Invalid receiver_address length".to_string());
                    }
                    receiver_address = data[pos..end].to_vec();
                    pos = end;
                }
                _ => {
                    let skip_len = Self::skip_protobuf_field(&data[pos..], wire_type)
                        .map_err(|e| format!("Failed to skip field: {}", e))?;
                    pos += skip_len;
                }
            }
        }

        if receiver_address.is_empty() {
            return Err("receiver_address is required".to_string());
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
    fn get_frozen_v2_balance_for_bandwidth(account: &tron_backend_execution::protocol::Account) -> i64 {
        account.frozen_v2.iter()
            .filter(|f| f.r#type == 0) // BANDWIDTH
            .map(|f| f.amount)
            .sum()
    }

    /// Get frozenV2 balance for energy
    fn get_frozen_v2_balance_for_energy(account: &tron_backend_execution::protocol::Account) -> i64 {
        account.frozen_v2.iter()
            .filter(|f| f.r#type == 1) // ENERGY
            .map(|f| f.amount)
            .sum()
    }

    /// Get frozenV2 balance with delegated for bandwidth
    fn get_frozen_v2_balance_with_delegated_bandwidth(account: &tron_backend_execution::protocol::Account) -> i64 {
        Self::get_frozen_v2_balance_for_bandwidth(account) + account.delegated_frozen_v2_balance_for_bandwidth
    }

    /// Get frozenV2 balance with delegated for energy
    fn get_frozen_v2_balance_with_delegated_energy(account: &tron_backend_execution::protocol::Account) -> i64 {
        Self::get_frozen_v2_balance_for_energy(account) +
            account.account_resource.as_ref().map(|r| r.delegated_frozen_v2_balance_for_energy).unwrap_or(0)
    }

    /// Get tron power frozenV2 balance
    fn get_tron_power_frozen_v2_balance(account: &tron_backend_execution::protocol::Account) -> i64 {
        account.frozen_v2.iter()
            .filter(|f| f.r#type == 2) // TRON_POWER
            .map(|f| f.amount)
            .sum()
    }

    /// Get acquired delegated frozenV2 balance for bandwidth
    fn get_acquired_delegated_frozen_v2_balance_for_bandwidth(account: &tron_backend_execution::protocol::Account) -> i64 {
        account.acquired_delegated_frozen_v2_balance_for_bandwidth
    }

    /// Get acquired delegated frozenV2 balance for energy
    fn get_acquired_delegated_frozen_v2_balance_for_energy(account: &tron_backend_execution::protocol::Account) -> i64 {
        account.account_resource.as_ref()
            .map(|r| r.acquired_delegated_frozen_v2_balance_for_energy)
            .unwrap_or(0)
    }

    /// Add to frozenV2 bandwidth amount
    fn add_frozen_v2_bandwidth(account: &mut tron_backend_execution::protocol::Account, amount: i64) {
        let mut found = false;
        for f in account.frozen_v2.iter_mut() {
            if f.r#type == 0 { // BANDWIDTH
                f.amount += amount;
                found = true;
                break;
            }
        }
        if !found && amount > 0 {
            account.frozen_v2.push(tron_backend_execution::protocol::account::FreezeV2 {
                r#type: 0,
                amount,
            });
        }
    }

    /// Add to frozenV2 energy amount
    fn add_frozen_v2_energy(account: &mut tron_backend_execution::protocol::Account, amount: i64) {
        let mut found = false;
        for f in account.frozen_v2.iter_mut() {
            if f.r#type == 1 { // ENERGY
                f.amount += amount;
                found = true;
                break;
            }
        }
        if !found && amount > 0 {
            account.frozen_v2.push(tron_backend_execution::protocol::account::FreezeV2 {
                r#type: 1,
                amount,
            });
        }
    }

    /// Add to frozenV2 tron power amount
    fn add_frozen_v2_tron_power(account: &mut tron_backend_execution::protocol::Account, amount: i64) {
        let mut found = false;
        for f in account.frozen_v2.iter_mut() {
            if f.r#type == 2 { // TRON_POWER
                f.amount += amount;
                found = true;
                break;
            }
        }
        if !found && amount > 0 {
            account.frozen_v2.push(tron_backend_execution::protocol::account::FreezeV2 {
                r#type: 2,
                amount,
            });
        }
    }

    /// Add to delegated frozenV2 balance for bandwidth
    fn add_delegated_frozen_v2_balance_for_bandwidth(account: &mut tron_backend_execution::protocol::Account, amount: i64) {
        account.delegated_frozen_v2_balance_for_bandwidth += amount;
    }

    /// Add to delegated frozenV2 balance for energy
    fn add_delegated_frozen_v2_balance_for_energy(account: &mut tron_backend_execution::protocol::Account, amount: i64) {
        if account.account_resource.is_none() {
            account.account_resource = Some(tron_backend_execution::protocol::account::AccountResource::default());
        }
        if let Some(ref mut res) = account.account_resource {
            res.delegated_frozen_v2_balance_for_energy += amount;
        }
    }

    /// Add to acquired delegated frozenV2 balance for bandwidth
    fn add_acquired_delegated_frozen_v2_balance_for_bandwidth(account: &mut tron_backend_execution::protocol::Account, amount: i64) {
        account.acquired_delegated_frozen_v2_balance_for_bandwidth += amount;
    }

    /// Add to acquired delegated frozenV2 balance for energy
    fn add_acquired_delegated_frozen_v2_balance_for_energy(account: &mut tron_backend_execution::protocol::Account, amount: i64) {
        if account.account_resource.is_none() {
            account.account_resource = Some(tron_backend_execution::protocol::account::AccountResource::default());
        }
        if let Some(ref mut res) = account.account_resource {
            res.acquired_delegated_frozen_v2_balance_for_energy += amount;
        }
    }

    /// Set acquired delegated frozenV2 balance for bandwidth
    fn set_acquired_delegated_frozen_v2_balance_for_bandwidth(account: &mut tron_backend_execution::protocol::Account, amount: i64) {
        account.acquired_delegated_frozen_v2_balance_for_bandwidth = amount;
    }

    /// Set acquired delegated frozenV2 balance for energy
    fn set_acquired_delegated_frozen_v2_balance_for_energy(account: &mut tron_backend_execution::protocol::Account, amount: i64) {
        if account.account_resource.is_none() {
            account.account_resource = Some(tron_backend_execution::protocol::account::AccountResource::default());
        }
        if let Some(ref mut res) = account.account_resource {
            res.acquired_delegated_frozen_v2_balance_for_energy = amount;
        }
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
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        let owner = transaction.from;
        let owner_tron = add_tron_address_prefix(&owner);

        // Parse contract data
        let participate_info = self.parse_participate_asset_issue_contract(&transaction.data)?;

        debug!("ParticipateAssetIssue: owner={}, to={}, asset={}, amount={}",
               hex::encode(&owner_tron),
               hex::encode(&participate_info.to_address),
               String::from_utf8_lossy(&participate_info.asset_name),
               participate_info.amount);

        // 1. Validate addresses
        let to_address = if participate_info.to_address.len() == 21 {
            revm_primitives::Address::from_slice(&participate_info.to_address[1..])
        } else if participate_info.to_address.len() == 20 {
            revm_primitives::Address::from_slice(&participate_info.to_address)
        } else {
            return Err("Invalid to address length".to_string());
        };
        let to_tron = add_tron_address_prefix(&to_address);

        if owner == to_address {
            return Err("Cannot participate asset Issue yourself !".to_string());
        }

        // 2. Validate amount > 0
        if participate_info.amount <= 0 {
            return Err("Amount must greater than 0!".to_string());
        }

        // 3. Validate owner account exists
        let owner_account = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or("Account does not exist!")?;

        // 4. Validate to account exists (asset issuer)
        let to_account = storage_adapter.get_account_proto(&to_address)
            .map_err(|e| format!("Failed to get to account: {}", e))?
            .ok_or("To account does not exist!")?;

        // 5. Get asset issue (using asset name as key)
        let allow_same_token_name = storage_adapter.get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;

        let asset_issue = storage_adapter.get_asset_issue(&participate_info.asset_name, allow_same_token_name)
            .map_err(|e| format!("Failed to get asset issue: {}", e))?
            .ok_or_else(|| format!("No asset named {}", String::from_utf8_lossy(&participate_info.asset_name)))?;

        // 6. Validate to_address is the asset owner
        let asset_owner_address = if asset_issue.owner_address.len() == 21 {
            &asset_issue.owner_address[1..]
        } else {
            &asset_issue.owner_address[..]
        };
        if to_address.as_slice() != asset_owner_address {
            return Err(format!("The asset is not issued by {}", hex::encode(&to_tron)));
        }

        // 7. Validate time window
        let now = storage_adapter.get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get timestamp: {}", e))?;
        if now >= asset_issue.end_time || now < asset_issue.start_time {
            return Err("No longer valid period!".to_string());
        }

        // 8. Calculate exchange amount
        let exchange_amount = Self::safe_multiply_divide(
            participate_info.amount,
            asset_issue.num as i64,
            asset_issue.trx_num as i64,
        )?;
        if exchange_amount <= 0 {
            return Err("Can not process the exchange!".to_string());
        }

        // 9. Validate owner has enough balance (amount + fee)
        let fee = 0i64; // ParticipateAssetIssue has no fee
        if owner_account.balance < participate_info.amount + fee {
            return Err("No enough balance !".to_string());
        }

        // 10. Validate issuer has enough tokens
        let issuer_asset_balance = Self::get_asset_balance_v2(&to_account, &participate_info.asset_name, allow_same_token_name);
        if issuer_asset_balance < exchange_amount {
            return Err("Asset balance is not enough !".to_string());
        }

        // 11. Execute the exchange
        let mut updated_owner = owner_account.clone();
        let mut updated_to = to_account.clone();

        // Subtract TRX from owner
        updated_owner.balance = updated_owner.balance.checked_sub(participate_info.amount)
            .ok_or("Balance underflow")?;

        // Add TRX to issuer
        updated_to.balance = updated_to.balance.checked_add(participate_info.amount)
            .ok_or("Balance overflow")?;

        // Add tokens to owner
        Self::add_asset_amount_v2(&mut updated_owner, &participate_info.asset_name, exchange_amount, allow_same_token_name);

        // Subtract tokens from issuer
        Self::reduce_asset_amount_v2(&mut updated_to, &participate_info.asset_name, exchange_amount, allow_same_token_name)?;

        // 12. Persist updated accounts
        storage_adapter.put_account_proto(&owner, &updated_owner)
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;
        storage_adapter.put_account_proto(&to_address, &updated_to)
            .map_err(|e| format!("Failed to persist to account: {}", e))?;

        // 13. Build state changes for CSV parity
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

        debug!("ParticipateAssetIssue: exchanged {} TRX for {} tokens", participate_info.amount, exchange_amount);

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
        })
    }

    /// Execute UNFREEZE_ASSET_CONTRACT (type 14)
    /// Unfreezes frozen TRC-10 supply and returns it to the asset issuer's balance
    ///
    /// Java oracle: UnfreezeAssetActuator.java
    fn execute_unfreeze_asset_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        let owner = transaction.from;
        let owner_tron = add_tron_address_prefix(&owner);

        debug!("UnfreezeAsset: owner={}", hex::encode(&owner_tron));

        // 1. Validate owner account exists
        let account = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or_else(|| format!("Account[{}] does not exist", hex::encode(&owner_tron)))?;

        // 2. Validate account has frozen supply
        if account.frozen_supply.is_empty() {
            return Err("no frozen supply balance".to_string());
        }

        // 3. Get asset issued info
        let allow_same_token_name = storage_adapter.get_allow_same_token_name()
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

        // 4. Get current timestamp
        let now = storage_adapter.get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to get timestamp: {}", e))?;

        // 5. Calculate how many entries have expired
        let expired_count = account.frozen_supply.iter()
            .filter(|frozen| frozen.expire_time <= now as i64)
            .count();
        if expired_count == 0 {
            return Err("It's not time to unfreeze asset supply".to_string());
        }

        // 6. Process frozen supply - separate expired from non-expired
        let mut unfreeze_asset: i64 = 0;
        let mut remaining_frozen = Vec::new();

        for frozen in account.frozen_supply.iter() {
            if frozen.expire_time <= now as i64 {
                // Expired - add to unfreeze amount
                unfreeze_asset = unfreeze_asset.checked_add(frozen.frozen_balance)
                    .ok_or("Overflow calculating unfreeze amount")?;
            } else {
                // Not expired - keep in frozen list
                remaining_frozen.push(frozen.clone());
            }
        }

        // 7. Update account
        let mut updated_account = account.clone();
        updated_account.frozen_supply = remaining_frozen;

        // Add unfrozen assets back to balance
        Self::add_asset_amount_v2(&mut updated_account, &asset_key, unfreeze_asset, allow_same_token_name);

        // 8. Persist updated account
        storage_adapter.put_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to persist account: {}", e))?;

        // 9. Build state change for CSV parity (balance unchanged, but for consistency)
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
        })
    }

    /// Execute UPDATE_ASSET_CONTRACT (type 15)
    /// Updates TRC-10 asset metadata: url, description, free_asset_net_limit, public_free_asset_net_limit
    ///
    /// Java oracle: UpdateAssetActuator.java
    fn execute_update_asset_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        _context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        let owner = transaction.from;
        let owner_tron = add_tron_address_prefix(&owner);

        // Parse contract data
        let update_info = self.parse_update_asset_contract(&transaction.data)?;

        debug!("UpdateAsset: owner={}, new_limit={}, new_public_limit={}",
               hex::encode(&owner_tron), update_info.new_limit, update_info.new_public_limit);

        // 1. Validate owner account exists
        let account = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get account: {}", e))?
            .ok_or("Account does not exist")?;

        // 2. Get asset info and validate ownership
        let allow_same_token_name = storage_adapter.get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;

        // Get asset key based on allowSameTokenName flag
        let asset_key = if allow_same_token_name == 0 {
            if account.asset_issued_name.is_empty() {
                return Err("Account has not issued any asset".to_string());
            }
            account.asset_issued_name.clone()
        } else {
            if account.asset_issued_id.is_empty() {
                return Err("Account has not issued any asset".to_string());
            }
            account.asset_issued_id.clone()
        };

        // 3. Validate URL
        if !Self::valid_url(&update_info.url) {
            return Err("Invalid url".to_string());
        }

        // 4. Validate description
        if !Self::valid_asset_description(&update_info.description) {
            return Err("Invalid description".to_string());
        }

        // 5. Validate limits
        let one_day_net_limit = storage_adapter.get_one_day_net_limit()
            .map_err(|e| format!("Failed to get oneDayNetLimit: {}", e))?;

        if update_info.new_limit < 0 || update_info.new_limit >= one_day_net_limit {
            return Err("Invalid FreeAssetNetLimit".to_string());
        }

        if update_info.new_public_limit < 0 || update_info.new_public_limit >= one_day_net_limit {
            return Err("Invalid PublicFreeAssetNetLimit".to_string());
        }

        // 6. Get and update asset issue
        let asset_issue = storage_adapter.get_asset_issue(&asset_key, allow_same_token_name)
            .map_err(|e| format!("Failed to get asset issue: {}", e))?
            .ok_or_else(|| format!("Asset is not existed in AssetIssue{}Store",
                                   if allow_same_token_name == 0 { "" } else { "V2" }))?;

        let mut updated_asset = asset_issue.clone();
        updated_asset.free_asset_net_limit = update_info.new_limit;
        updated_asset.public_free_asset_net_limit = update_info.new_public_limit;
        updated_asset.url = update_info.url.clone();
        updated_asset.description = update_info.description.clone();

        // 7. Persist updated asset issue
        // If allowSameTokenName == 0, update both stores
        if allow_same_token_name == 0 {
            // Update AssetIssueStore (by name)
            storage_adapter.put_asset_issue(&account.asset_issued_name, &updated_asset, false)
                .map_err(|e| format!("Failed to persist asset issue: {}", e))?;
            // Update AssetIssueV2Store (by id) if account has issued ID
            if !account.asset_issued_id.is_empty() {
                storage_adapter.put_asset_issue(&account.asset_issued_id, &updated_asset, true)
                    .map_err(|e| format!("Failed to persist asset issue V2: {}", e))?;
            }
        } else {
            // Only update AssetIssueV2Store
            storage_adapter.put_asset_issue(&account.asset_issued_id, &updated_asset, true)
                .map_err(|e| format!("Failed to persist asset issue V2: {}", e))?;
        }

        // 8. Build minimal state change (no TRX balance change)
        let account_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(account.balance as u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        let state_changes = vec![TronStateChange::AccountChange {
            address: owner,
            old_account: Some(account_info.clone()),
            new_account: Some(account_info),
        }];

        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        debug!("UpdateAsset: updated asset {}", String::from_utf8_lossy(&asset_key));

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
        })
    }

    /// Parse ParticipateAssetIssueContract protobuf bytes
    fn parse_participate_asset_issue_contract(&self, data: &[u8]) -> Result<ParticipateAssetIssueInfo, String> {
        use contracts::proto::read_varint;

        let mut to_address = Vec::new();
        let mut asset_name = Vec::new();
        let mut amount: i64 = 0;

        let mut pos = 0;
        while pos < data.len() {
            // Read tag
            let (tag, new_pos) = read_varint(&data[pos..])?;
            pos += new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            match field_number {
                // owner_address = 1 (skip, we use transaction.from)
                1 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for owner_address".to_string());
                    }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos + len as usize;
                }
                // to_address = 2
                2 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for to_address".to_string());
                    }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    if pos + len as usize > data.len() {
                        return Err("Data truncated reading to_address".to_string());
                    }
                    to_address = data[pos..pos + len as usize].to_vec();
                    pos += len as usize;
                }
                // asset_name = 3
                3 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for asset_name".to_string());
                    }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    if pos + len as usize > data.len() {
                        return Err("Data truncated reading asset_name".to_string());
                    }
                    asset_name = data[pos..pos + len as usize].to_vec();
                    pos += len as usize;
                }
                // amount = 4
                4 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for amount".to_string());
                    }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    amount = value as i64;
                    pos += new_pos;
                }
                _ => {
                    // Skip unknown fields
                    match wire_type {
                        0 => { let (_, new_pos) = read_varint(&data[pos..])?; pos += new_pos; }
                        2 => { let (len, new_pos) = read_varint(&data[pos..])?; pos = pos + new_pos + len as usize; }
                        _ => return Err(format!("Unknown wire type {}", wire_type)),
                    }
                }
            }
        }

        Ok(ParticipateAssetIssueInfo {
            to_address,
            asset_name,
            amount,
        })
    }

    /// Parse UpdateAssetContract protobuf bytes
    fn parse_update_asset_contract(&self, data: &[u8]) -> Result<UpdateAssetInfo, String> {
        use contracts::proto::read_varint;

        let mut description = Vec::new();
        let mut url = Vec::new();
        let mut new_limit: i64 = 0;
        let mut new_public_limit: i64 = 0;

        let mut pos = 0;
        while pos < data.len() {
            // Read tag
            let (tag, new_pos) = read_varint(&data[pos..])?;
            pos += new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            match field_number {
                // owner_address = 1 (skip, we use transaction.from)
                1 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for owner_address".to_string());
                    }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos + len as usize;
                }
                // description = 2
                2 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for description".to_string());
                    }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    if pos + len as usize > data.len() {
                        return Err("Data truncated reading description".to_string());
                    }
                    description = data[pos..pos + len as usize].to_vec();
                    pos += len as usize;
                }
                // url = 3
                3 => {
                    if wire_type != 2 {
                        return Err("Invalid wire type for url".to_string());
                    }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    if pos + len as usize > data.len() {
                        return Err("Data truncated reading url".to_string());
                    }
                    url = data[pos..pos + len as usize].to_vec();
                    pos += len as usize;
                }
                // new_limit = 4
                4 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for new_limit".to_string());
                    }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    new_limit = value as i64;
                    pos += new_pos;
                }
                // new_public_limit = 5
                5 => {
                    if wire_type != 0 {
                        return Err("Invalid wire type for new_public_limit".to_string());
                    }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    new_public_limit = value as i64;
                    pos += new_pos;
                }
                _ => {
                    // Skip unknown fields
                    match wire_type {
                        0 => { let (_, new_pos) = read_varint(&data[pos..])?; pos += new_pos; }
                        2 => { let (len, new_pos) = read_varint(&data[pos..])?; pos = pos + new_pos + len as usize; }
                        _ => return Err(format!("Unknown wire type {}", wire_type)),
                    }
                }
            }
        }

        Ok(UpdateAssetInfo {
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
        let product = value.checked_mul(multiplier)
            .ok_or("Overflow in multiplication")?;
        // Floor division (rounds toward negative infinity for negative results)
        let result = product / divisor;
        Ok(result)
    }

    /// Get asset balance from account (V2 style)
    fn get_asset_balance_v2(account: &tron_backend_execution::protocol::Account, asset_key: &[u8], allow_same_token_name: i64) -> i64 {
        let key_str = String::from_utf8_lossy(asset_key).to_string();

        if allow_same_token_name == 0 {
            // Check assetV2 first, fall back to asset
            if let Some(balance) = account.asset_v2.get(&key_str) {
                return *balance;
            }
            if let Some(balance) = account.asset.get(&key_str) {
                return *balance;
            }
        } else {
            // Only check assetV2
            if let Some(balance) = account.asset_v2.get(&key_str) {
                return *balance;
            }
        }
        0
    }

    /// Add asset amount to account (V2 style)
    fn add_asset_amount_v2(account: &mut tron_backend_execution::protocol::Account, asset_key: &[u8], amount: i64, allow_same_token_name: i64) {
        let key_str = String::from_utf8_lossy(asset_key).to_string();

        let entry = account.asset_v2.entry(key_str.clone()).or_insert(0);
        *entry += amount;

        // Also update asset map if allowSameTokenName == 0
        if allow_same_token_name == 0 {
            let entry = account.asset.entry(key_str).or_insert(0);
            *entry += amount;
        }
    }

    /// Reduce asset amount from account (V2 style)
    fn reduce_asset_amount_v2(account: &mut tron_backend_execution::protocol::Account, asset_key: &[u8], amount: i64, allow_same_token_name: i64) -> Result<(), String> {
        let key_str = String::from_utf8_lossy(asset_key).to_string();

        // Check if has enough balance
        let current = *account.asset_v2.get(&key_str).unwrap_or(&0);
        if current < amount {
            return Err("Insufficient asset balance".to_string());
        }

        // Reduce from assetV2
        let entry = account.asset_v2.entry(key_str.clone()).or_insert(0);
        *entry -= amount;

        // Also reduce from asset map if allowSameTokenName == 0
        if allow_same_token_name == 0 {
            if let Some(entry) = account.asset.get_mut(&key_str) {
                *entry -= amount;
            }
        }

        Ok(())
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
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use contracts::exchange::{TRX_SYMBOL, is_number, is_trx};
        use contracts::proto::TransactionResultBuilder;

        debug!("Executing EXCHANGE_CREATE_CONTRACT: owner={:?}", transaction.from);

        // 1. Parse contract data
        let create_info = self.parse_exchange_create_contract(&transaction.data)?;
        debug!("Parsed ExchangeCreate: first_token={}, second_token={}, balances={}/{}",
            String::from_utf8_lossy(&create_info.first_token_id),
            String::from_utf8_lossy(&create_info.second_token_id),
            create_info.first_token_balance,
            create_info.second_token_balance
        );

        // 2. Get owner account
        let owner = transaction.from;
        let owner_tron = storage_adapter.to_tron_address_21(&owner).to_vec();
        let account = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or("Owner account not found")?;

        // 3. Get dynamic properties
        let allow_same_token_name = storage_adapter.get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;
        let exchange_create_fee = storage_adapter.get_exchange_create_fee()
            .map_err(|e| format!("Failed to get exchange create fee: {}", e))?;
        let balance_limit = storage_adapter.get_exchange_balance_limit()
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
        if create_info.first_token_balance > balance_limit || create_info.second_token_balance > balance_limit {
            return Err(format!("token balance must less than {}", balance_limit));
        }

        // - Sufficient balance for first token
        if is_trx(&create_info.first_token_id) {
            if account.balance < create_info.first_token_balance + exchange_create_fee {
                return Err("balance is not enough".to_string());
            }
        } else {
            let asset_balance = storage_adapter.get_asset_balance_v2(&owner, &create_info.first_token_id)
                .map_err(|e| format!("Failed to get first token balance: {}", e))?;
            if asset_balance < create_info.first_token_balance {
                return Err("first token balance is not enough".to_string());
            }
        }

        // - Sufficient balance for second token
        if is_trx(&create_info.second_token_id) {
            if account.balance < create_info.second_token_balance + exchange_create_fee {
                return Err("balance is not enough".to_string());
            }
        } else {
            let asset_balance = storage_adapter.get_asset_balance_v2(&owner, &create_info.second_token_id)
                .map_err(|e| format!("Failed to get second token balance: {}", e))?;
            if asset_balance < create_info.second_token_balance {
                return Err("second token balance is not enough".to_string());
            }
        }

        // 5. Execute
        let mut updated_account = account.clone();

        // Deduct fee
        updated_account.balance -= exchange_create_fee;

        // Deduct first token
        if is_trx(&create_info.first_token_id) {
            updated_account.balance -= create_info.first_token_balance;
        } else {
            storage_adapter.reduce_asset_amount_v2(&owner, &create_info.first_token_id, create_info.first_token_balance)
                .map_err(|e| format!("Failed to reduce first token: {}", e))?;
        }

        // Deduct second token
        if is_trx(&create_info.second_token_id) {
            updated_account.balance -= create_info.second_token_balance;
        } else {
            storage_adapter.reduce_asset_amount_v2(&owner, &create_info.second_token_id, create_info.second_token_balance)
                .map_err(|e| format!("Failed to reduce second token: {}", e))?;
        }

        // Create exchange
        let exchange_id = storage_adapter.get_latest_exchange_num()
            .map_err(|e| format!("Failed to get latest exchange num: {}", e))? + 1;
        let now = context.block_timestamp as i64;

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
            storage_adapter.put_exchange_to_store(&exchange_v1, false)
                .map_err(|e| format!("Failed to store exchange V1: {}", e))?;

            // Resolve to real IDs for V2 store
            if !is_trx(&create_info.first_token_id) {
                if let Ok(Some(asset)) = storage_adapter.get_asset_issue(&create_info.first_token_id, 0) {
                    first_token_id_v2 = asset.id.as_bytes().to_vec();
                }
            }
            if !is_trx(&create_info.second_token_id) {
                if let Ok(Some(asset)) = storage_adapter.get_asset_issue(&create_info.second_token_id, 0) {
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
        storage_adapter.put_exchange(&exchange_v2)
            .map_err(|e| format!("Failed to store exchange V2: {}", e))?;

        // Update latest exchange num
        storage_adapter.set_latest_exchange_num(exchange_id)
            .map_err(|e| format!("Failed to update latest exchange num: {}", e))?;

        // Update account
        storage_adapter.set_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to update account: {}", e))?;

        // Handle fee (burn or blackhole)
        let support_black_hole = storage_adapter.support_black_hole_optimization()
            .unwrap_or(true);
        if !support_black_hole {
            // Credit blackhole account
            let blackhole_addr = storage_adapter.get_blackhole_address_evm();
            storage_adapter.add_balance(&blackhole_addr, exchange_create_fee as u64)
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

        // Build receipt with exchange_id
        let receipt = TransactionResultBuilder::new()
            .with_exchange_id(exchange_id)
            .build();

        debug!("ExchangeCreate: created exchange {} with tokens {}/{}",
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
        use contracts::exchange::{is_number, is_trx, calculate_inject_another_amount};
        use contracts::proto::TransactionResultBuilder;

        debug!("Executing EXCHANGE_INJECT_CONTRACT: owner={:?}", transaction.from);

        // 1. Parse contract data
        let inject_info = self.parse_exchange_inject_contract(&transaction.data)?;
        debug!("Parsed ExchangeInject: exchange_id={}, token={}, quant={}",
            inject_info.exchange_id,
            String::from_utf8_lossy(&inject_info.token_id),
            inject_info.quant
        );

        // 2. Get owner account
        let owner = transaction.from;
        let owner_tron = storage_adapter.to_tron_address_21(&owner).to_vec();
        let account = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or("Owner account not found")?;

        // 3. Get exchange
        let allow_same_token_name = storage_adapter.get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;
        let exchange = storage_adapter.get_exchange(inject_info.exchange_id)
            .map_err(|e| format!("Failed to get exchange: {}", e))?
            .ok_or(format!("Exchange[{}] not exists", inject_info.exchange_id))?;

        // 4. Validate
        // - Must be creator
        if owner_tron != exchange.creator_address {
            return Err(format!("account[{}] is not creator", hex::encode(&owner_tron)));
        }

        // - Token ID format validation
        if allow_same_token_name == 1 && !is_trx(&inject_info.token_id) && !is_number(&inject_info.token_id) {
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
            return Err("Token balance in exchange is equal with 0,the exchange has been closed".to_string());
        }

        // - Quant must be positive
        if inject_info.quant <= 0 {
            return Err("injected token quant must greater than zero".to_string());
        }

        // Calculate another token amount
        let (another_token_id, another_token_quant, new_first_balance, new_second_balance) = if is_first_token {
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
        if another_token_quant <= 0 {
            return Err("the calculated token quant must be greater than 0".to_string());
        }

        // - Balance limits
        let balance_limit = storage_adapter.get_exchange_balance_limit()
            .map_err(|e| format!("Failed to get balance limit: {}", e))?;
        if new_first_balance > balance_limit || new_second_balance > balance_limit {
            return Err(format!("token balance must less than {}", balance_limit));
        }

        // - Sufficient balance for token
        if is_trx(&inject_info.token_id) {
            if account.balance < inject_info.quant {
                return Err("balance is not enough".to_string());
            }
        } else {
            let balance = storage_adapter.get_asset_balance_v2(&owner, &inject_info.token_id)
                .map_err(|e| format!("Failed to get token balance: {}", e))?;
            if balance < inject_info.quant {
                return Err("token balance is not enough".to_string());
            }
        }

        // - Sufficient balance for another token
        if is_trx(&another_token_id) {
            if account.balance < another_token_quant {
                return Err("balance is not enough".to_string());
            }
        } else {
            let balance = storage_adapter.get_asset_balance_v2(&owner, &another_token_id)
                .map_err(|e| format!("Failed to get another token balance: {}", e))?;
            if balance < another_token_quant {
                return Err("another token balance is not enough".to_string());
            }
        }

        // 5. Execute
        let mut updated_account = account.clone();

        // Deduct token
        if is_trx(&inject_info.token_id) {
            updated_account.balance -= inject_info.quant;
        } else {
            storage_adapter.reduce_asset_amount_v2(&owner, &inject_info.token_id, inject_info.quant)
                .map_err(|e| format!("Failed to reduce token: {}", e))?;
        }

        // Deduct another token
        if is_trx(&another_token_id) {
            updated_account.balance -= another_token_quant;
        } else {
            storage_adapter.reduce_asset_amount_v2(&owner, &another_token_id, another_token_quant)
                .map_err(|e| format!("Failed to reduce another token: {}", e))?;
        }

        // Update exchange
        let mut updated_exchange = exchange.clone();
        updated_exchange.first_token_balance = new_first_balance;
        updated_exchange.second_token_balance = new_second_balance;
        storage_adapter.put_exchange(&updated_exchange)
            .map_err(|e| format!("Failed to update exchange: {}", e))?;

        // Update account
        storage_adapter.set_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to update account: {}", e))?;

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

        // Build receipt with inject_another_amount
        let receipt = TransactionResultBuilder::new()
            .with_exchange_inject_another_amount(another_token_quant)
            .build();

        debug!("ExchangeInject: injected {} of token, calculated {} of another token",
            inject_info.quant, another_token_quant);

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
        use contracts::exchange::{is_number, is_trx, calculate_withdraw_another_amount, is_withdraw_precise_enough};
        use contracts::proto::TransactionResultBuilder;

        debug!("Executing EXCHANGE_WITHDRAW_CONTRACT: owner={:?}", transaction.from);

        // 1. Parse contract data
        let withdraw_info = self.parse_exchange_withdraw_contract(&transaction.data)?;
        debug!("Parsed ExchangeWithdraw: exchange_id={}, token={}, quant={}",
            withdraw_info.exchange_id,
            String::from_utf8_lossy(&withdraw_info.token_id),
            withdraw_info.quant
        );

        // 2. Get owner account
        let owner = transaction.from;
        let owner_tron = storage_adapter.to_tron_address_21(&owner).to_vec();
        let account = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or("Owner account not found")?;

        // 3. Get exchange
        let allow_same_token_name = storage_adapter.get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;
        let exchange = storage_adapter.get_exchange(withdraw_info.exchange_id)
            .map_err(|e| format!("Failed to get exchange: {}", e))?
            .ok_or(format!("Exchange[{}] not exists", withdraw_info.exchange_id))?;

        // 4. Validate
        // - Must be creator
        if owner_tron != exchange.creator_address {
            return Err(format!("account[{}] is not creator", hex::encode(&owner_tron)));
        }

        // - Token ID format validation
        if allow_same_token_name == 1 && !is_trx(&withdraw_info.token_id) && !is_number(&withdraw_info.token_id) {
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
            return Err("Token balance in exchange is equal with 0,the exchange has been closed".to_string());
        }

        // Calculate another token amount and validate
        let (another_token_id, another_token_quant, new_first_balance, new_second_balance, token_balance, other_balance) = if is_first_token {
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

        // Add token to account
        if is_trx(&withdraw_info.token_id) {
            updated_account.balance += withdraw_info.quant;
        } else {
            storage_adapter.add_asset_amount_v2(&owner, &withdraw_info.token_id, withdraw_info.quant)
                .map_err(|e| format!("Failed to add token: {}", e))?;
        }

        // Add another token to account
        if is_trx(&another_token_id) {
            updated_account.balance += another_token_quant;
        } else {
            storage_adapter.add_asset_amount_v2(&owner, &another_token_id, another_token_quant)
                .map_err(|e| format!("Failed to add another token: {}", e))?;
        }

        // Update exchange
        let mut updated_exchange = exchange.clone();
        updated_exchange.first_token_balance = new_first_balance;
        updated_exchange.second_token_balance = new_second_balance;
        storage_adapter.put_exchange(&updated_exchange)
            .map_err(|e| format!("Failed to update exchange: {}", e))?;

        // Update account
        storage_adapter.set_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to update account: {}", e))?;

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

        // Build receipt with withdraw_another_amount
        let receipt = TransactionResultBuilder::new()
            .with_exchange_withdraw_another_amount(another_token_quant)
            .build();

        debug!("ExchangeWithdraw: withdrew {} of token, plus {} of another token",
            withdraw_info.quant, another_token_quant);

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
        use contracts::exchange::{ExchangeProcessor, is_number, is_trx};
        use contracts::proto::TransactionResultBuilder;

        debug!("Executing EXCHANGE_TRANSACTION_CONTRACT: owner={:?}", transaction.from);

        // 1. Parse contract data
        let tx_info = self.parse_exchange_transaction_contract(&transaction.data)?;
        debug!("Parsed ExchangeTransaction: exchange_id={}, token={}, quant={}, expected={}",
            tx_info.exchange_id,
            String::from_utf8_lossy(&tx_info.token_id),
            tx_info.quant,
            tx_info.expected
        );

        // 2. Get owner account
        let owner = transaction.from;
        let account = storage_adapter.get_account_proto(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or("Owner account not found")?;

        // 3. Get exchange and properties
        let allow_same_token_name = storage_adapter.get_allow_same_token_name()
            .map_err(|e| format!("Failed to get allowSameTokenName: {}", e))?;
        let use_strict_math = storage_adapter.allow_strict_math()
            .map_err(|e| format!("Failed to get allowStrictMath: {}", e))?;
        let mut exchange = storage_adapter.get_exchange(tx_info.exchange_id)
            .map_err(|e| format!("Failed to get exchange: {}", e))?
            .ok_or(format!("Exchange[{}] not exists", tx_info.exchange_id))?;

        // 4. Validate
        // - Token ID format validation
        if allow_same_token_name == 1 && !is_trx(&tx_info.token_id) && !is_number(&tx_info.token_id) {
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
            return Err("Token balance in exchange is equal with 0,the exchange has been closed".to_string());
        }

        // - Balance limit check
        let balance_limit = storage_adapter.get_exchange_balance_limit()
            .map_err(|e| format!("Failed to get balance limit: {}", e))?;
        let token_balance = if is_first_token { exchange.first_token_balance } else { exchange.second_token_balance };
        if token_balance + tx_info.quant > balance_limit {
            return Err(format!("token balance must less than {}", balance_limit));
        }

        // - Sufficient balance for token
        if is_trx(&tx_info.token_id) {
            if account.balance < tx_info.quant {
                return Err("balance is not enough".to_string());
            }
        } else {
            let balance = storage_adapter.get_asset_balance_v2(&owner, &tx_info.token_id)
                .map_err(|e| format!("Failed to get token balance: {}", e))?;
            if balance < tx_info.quant {
                return Err("token balance is not enough".to_string());
            }
        }

        // Calculate received amount using AMM
        let (another_token_id, another_token_quant) = {
            let mut processor = ExchangeProcessor::new(use_strict_math);

            let (sell_balance, buy_balance, another_id) = if is_first_token {
                (exchange.first_token_balance, exchange.second_token_balance, exchange.second_token_id.clone())
            } else {
                (exchange.second_token_balance, exchange.first_token_balance, exchange.first_token_id.clone())
            };

            let buy_quant = processor.exchange(sell_balance, buy_balance, tx_info.quant);
            (another_id, buy_quant)
        };

        // - Check expected amount
        if another_token_quant < tx_info.expected {
            return Err("token required must greater than expected".to_string());
        }

        // 5. Execute
        let mut updated_account = account.clone();

        // Deduct sold token
        if is_trx(&tx_info.token_id) {
            updated_account.balance -= tx_info.quant;
        } else {
            storage_adapter.reduce_asset_amount_v2(&owner, &tx_info.token_id, tx_info.quant)
                .map_err(|e| format!("Failed to reduce sold token: {}", e))?;
        }

        // Add bought token
        if is_trx(&another_token_id) {
            updated_account.balance += another_token_quant;
        } else {
            storage_adapter.add_asset_amount_v2(&owner, &another_token_id, another_token_quant)
                .map_err(|e| format!("Failed to add bought token: {}", e))?;
        }

        // Update exchange balances
        if is_first_token {
            exchange.first_token_balance += tx_info.quant;
            exchange.second_token_balance -= another_token_quant;
        } else {
            exchange.first_token_balance -= another_token_quant;
            exchange.second_token_balance += tx_info.quant;
        }
        storage_adapter.put_exchange(&exchange)
            .map_err(|e| format!("Failed to update exchange: {}", e))?;

        // Update account
        storage_adapter.set_account_proto(&owner, &updated_account)
            .map_err(|e| format!("Failed to update account: {}", e))?;

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

        // Build receipt with received_amount
        let receipt = TransactionResultBuilder::new()
            .with_exchange_received_amount(another_token_quant)
            .build();

        debug!("ExchangeTransaction: sold {} of token, received {} of another token",
            tx_info.quant, another_token_quant);

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
        })
    }

    /// Parse ExchangeCreateContract protobuf bytes
    fn parse_exchange_create_contract(&self, data: &[u8]) -> Result<ExchangeCreateInfo, String> {
        use contracts::proto::read_varint;

        let mut first_token_id = Vec::new();
        let mut first_token_balance: i64 = 0;
        let mut second_token_id = Vec::new();
        let mut second_token_balance: i64 = 0;

        let mut pos = 0;
        while pos < data.len() {
            let (tag, new_pos) = read_varint(&data[pos..])?;
            pos += new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            match field_number {
                // owner_address = 1 (skip)
                1 => {
                    if wire_type != 2 { return Err("Invalid wire type for owner_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos + len as usize;
                }
                // first_token_id = 2
                2 => {
                    if wire_type != 2 { return Err("Invalid wire type for first_token_id".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    first_token_id = data[pos..pos + len as usize].to_vec();
                    pos += len as usize;
                }
                // first_token_balance = 3
                3 => {
                    if wire_type != 0 { return Err("Invalid wire type for first_token_balance".to_string()); }
                    let (val, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    first_token_balance = val as i64;
                }
                // second_token_id = 4
                4 => {
                    if wire_type != 2 { return Err("Invalid wire type for second_token_id".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    second_token_id = data[pos..pos + len as usize].to_vec();
                    pos += len as usize;
                }
                // second_token_balance = 5
                5 => {
                    if wire_type != 0 { return Err("Invalid wire type for second_token_balance".to_string()); }
                    let (val, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    second_token_balance = val as i64;
                }
                _ => {
                    // Skip unknown fields
                    match wire_type {
                        0 => { let (_, new_pos) = read_varint(&data[pos..])?; pos += new_pos; }
                        2 => { let (len, new_pos) = read_varint(&data[pos..])?; pos = pos + new_pos + len as usize; }
                        _ => return Err(format!("Unsupported wire type: {}", wire_type)),
                    }
                }
            }
        }

        Ok(ExchangeCreateInfo {
            first_token_id,
            first_token_balance,
            second_token_id,
            second_token_balance,
        })
    }

    /// Parse ExchangeInjectContract protobuf bytes
    fn parse_exchange_inject_contract(&self, data: &[u8]) -> Result<ExchangeInjectInfo, String> {
        use contracts::proto::read_varint;

        let mut exchange_id: i64 = 0;
        let mut token_id = Vec::new();
        let mut quant: i64 = 0;

        let mut pos = 0;
        while pos < data.len() {
            let (tag, new_pos) = read_varint(&data[pos..])?;
            pos += new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            match field_number {
                // owner_address = 1 (skip)
                1 => {
                    if wire_type != 2 { return Err("Invalid wire type for owner_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos + len as usize;
                }
                // exchange_id = 2
                2 => {
                    if wire_type != 0 { return Err("Invalid wire type for exchange_id".to_string()); }
                    let (val, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    exchange_id = val as i64;
                }
                // token_id = 3
                3 => {
                    if wire_type != 2 { return Err("Invalid wire type for token_id".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    token_id = data[pos..pos + len as usize].to_vec();
                    pos += len as usize;
                }
                // quant = 4
                4 => {
                    if wire_type != 0 { return Err("Invalid wire type for quant".to_string()); }
                    let (val, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    quant = val as i64;
                }
                _ => {
                    match wire_type {
                        0 => { let (_, new_pos) = read_varint(&data[pos..])?; pos += new_pos; }
                        2 => { let (len, new_pos) = read_varint(&data[pos..])?; pos = pos + new_pos + len as usize; }
                        _ => return Err(format!("Unsupported wire type: {}", wire_type)),
                    }
                }
            }
        }

        Ok(ExchangeInjectInfo { exchange_id, token_id, quant })
    }

    /// Parse ExchangeWithdrawContract protobuf bytes
    fn parse_exchange_withdraw_contract(&self, data: &[u8]) -> Result<ExchangeWithdrawInfo, String> {
        // Same structure as inject
        let inject_info = self.parse_exchange_inject_contract(data)?;
        Ok(ExchangeWithdrawInfo {
            exchange_id: inject_info.exchange_id,
            token_id: inject_info.token_id,
            quant: inject_info.quant,
        })
    }

    /// Parse ExchangeTransactionContract protobuf bytes
    fn parse_exchange_transaction_contract(&self, data: &[u8]) -> Result<ExchangeTransactionInfo, String> {
        use contracts::proto::read_varint;

        let mut exchange_id: i64 = 0;
        let mut token_id = Vec::new();
        let mut quant: i64 = 0;
        let mut expected: i64 = 0;

        let mut pos = 0;
        while pos < data.len() {
            let (tag, new_pos) = read_varint(&data[pos..])?;
            pos += new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            match field_number {
                // owner_address = 1 (skip)
                1 => {
                    if wire_type != 2 { return Err("Invalid wire type for owner_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos + len as usize;
                }
                // exchange_id = 2
                2 => {
                    if wire_type != 0 { return Err("Invalid wire type for exchange_id".to_string()); }
                    let (val, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    exchange_id = val as i64;
                }
                // token_id = 3
                3 => {
                    if wire_type != 2 { return Err("Invalid wire type for token_id".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    token_id = data[pos..pos + len as usize].to_vec();
                    pos += len as usize;
                }
                // quant = 4
                4 => {
                    if wire_type != 0 { return Err("Invalid wire type for quant".to_string()); }
                    let (val, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    quant = val as i64;
                }
                // expected = 5
                5 => {
                    if wire_type != 0 { return Err("Invalid wire type for expected".to_string()); }
                    let (val, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    expected = val as i64;
                }
                _ => {
                    match wire_type {
                        0 => { let (_, new_pos) = read_varint(&data[pos..])?; pos += new_pos; }
                        2 => { let (len, new_pos) = read_varint(&data[pos..])?; pos = pos + new_pos + len as usize; }
                        _ => return Err(format!("Unsupported wire type: {}", wire_type)),
                    }
                }
            }
        }

        Ok(ExchangeTransactionInfo { exchange_id, token_id, quant, expected })
    }
}

/// Parsed ExchangeCreateContract information
#[derive(Debug, Clone)]
struct ExchangeCreateInfo {
    first_token_id: Vec<u8>,
    first_token_balance: i64,
    second_token_id: Vec<u8>,
    second_token_balance: i64,
}

/// Parsed ExchangeInjectContract information
#[derive(Debug, Clone)]
struct ExchangeInjectInfo {
    exchange_id: i64,
    token_id: Vec<u8>,
    quant: i64,
}

/// Parsed ExchangeWithdrawContract information
#[derive(Debug, Clone)]
struct ExchangeWithdrawInfo {
    exchange_id: i64,
    token_id: Vec<u8>,
    quant: i64,
}

/// Parsed ExchangeTransactionContract information
#[derive(Debug, Clone)]
struct ExchangeTransactionInfo {
    exchange_id: i64,
    token_id: Vec<u8>,
    quant: i64,
    expected: i64,
}

/// Parsed ParticipateAssetIssueContract information
#[derive(Debug, Clone)]
struct ParticipateAssetIssueInfo {
    to_address: Vec<u8>,
    asset_name: Vec<u8>,
    amount: i64,
}

/// Parsed UpdateAssetContract information
#[derive(Debug, Clone)]
struct UpdateAssetInfo {
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

#[cfg(test)]
mod tests;
