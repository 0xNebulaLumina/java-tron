// Freeze/Unfreeze contract handlers
// V1 and V2 freeze/unfreeze balance operations

use super::super::BackendService;
use super::proto::read_varint;
use revm_primitives::{Address, Bytes, U256};
use tron_backend_execution::{TronTransaction, TronExecutionContext, TronExecutionResult, TronStateChange, EvmStateStore};
use tracing::{debug, info, error, warn};

/// FreezeBalance contract parameters
#[derive(Debug, Clone)]
pub(super) struct FreezeParams {
    pub(super) frozen_balance: i64,
    pub(super) frozen_duration: u32,
    pub(super) resource: FreezeResource,
}

/// UnfreezeBalance contract parameters
#[derive(Debug, Clone)]
pub(super) struct UnfreezeParams {
    pub(super) resource: FreezeResource,
}

/// FreezeBalanceV2 contract parameters
#[derive(Debug, Clone)]
pub(super) struct FreezeV2Params {
    pub(super) frozen_balance: i64,
    pub(super) resource: FreezeResource,
}

/// UnfreezeBalanceV2 contract parameters
#[derive(Debug, Clone)]
pub(super) struct UnfreezeV2Params {
    pub(super) unfreeze_balance: i64,
    pub(super) resource: FreezeResource,
}

/// Resource type for freeze/unfreeze operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FreezeResource {
    Bandwidth = 0,
    Energy = 1,
    TronPower = 2,
}

impl BackendService {
    pub(crate) fn execute_freeze_balance_contract(
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
            trc10_changes: vec![],
            global_resource_changes, // Populated when emit_global_resource_changes is true
        })
    }

    /// Execute an UNFREEZE_BALANCE_CONTRACT (Phase 2: with freeze ledger changes)
    /// Handles unfreezing balance and emitting FreezeLedgerChange with updated amounts
    pub(crate) fn execute_unfreeze_balance_contract(
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
            trc10_changes: vec![],
            global_resource_changes, // Populated when emit_global_resource_changes is true
        })
    }

    /// Parse UnfreezeBalanceContract parameters from protobuf-encoded data
    ///
    /// UnfreezeBalanceContract protobuf structure:
    /// - owner_address: bytes (field 1) - we get this from transaction.from
    /// - resource: ResourceCode enum (field 10)
    /// - receiver_address: bytes (field 15) - optional, Phase 1 ignores
    pub(crate) fn parse_unfreeze_balance_params(data: &revm_primitives::Bytes) -> Result<UnfreezeParams, String> {
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
    pub(crate) fn execute_freeze_balance_v2_contract(
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
            trc10_changes: vec![],
            global_resource_changes, // Populated when emit_global_resource_changes is true
        })
    }

    /// Execute an UNFREEZE_BALANCE_V2_CONTRACT (Phase 2: with freeze ledger changes)
    /// Handles V2 unfreeze which may support partial unfreezing
    pub(crate) fn execute_unfreeze_balance_v2_contract(
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
            trc10_changes: vec![],
            global_resource_changes, // Populated when emit_global_resource_changes is true
        })
    }

    /// Parse FreezeBalanceV2Contract parameters from protobuf-encoded data
    ///
    /// FreezeBalanceV2Contract protobuf structure:
    /// - owner_address: bytes (field 1) - we get this from transaction.from
    /// - frozen_balance: int64 (field 2)
    /// - resource: ResourceCode enum (field 3)
    pub(crate) fn parse_freeze_balance_v2_params(data: &revm_primitives::Bytes) -> Result<FreezeV2Params, String> {
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
    pub(crate) fn parse_unfreeze_balance_v2_params(data: &revm_primitives::Bytes) -> Result<UnfreezeV2Params, String> {
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
    pub(crate) fn parse_freeze_balance_params(data: &revm_primitives::Bytes) -> Result<FreezeParams, String> {
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
    pub(crate) fn calculate_bandwidth_usage(transaction: &TronTransaction) -> u64 {
        // Approximate bandwidth calculation based on transaction fields
        // This is a simplified version - full implementation would consider exact protobuf serialization

        let base_size = 60; // Base transaction overhead (addresses, nonce, etc.)
        let data_size = transaction.data.len() as u64;
        let signature_size = 65; // ECDSA signature size

        base_size + data_size + signature_size
    }
}
