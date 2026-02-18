// Freeze/Unfreeze contract handlers
// V1 and V2 freeze/unfreeze balance operations

use super::super::BackendService;
use super::proto::{read_varint, TransactionResultBuilder};
use revm_primitives::{Address, Bytes, U256};
use tron_backend_execution::{
    EvmStateStore, TronExecutionContext, TronExecutionResult, TronStateChange, TronTransaction,
    VotesRecord,
};
use tracing::{debug, info, error, warn};

/// FreezeBalance contract parameters
#[derive(Debug, Clone)]
pub(super) struct FreezeParams {
    pub(super) frozen_balance: i64,
    pub(super) frozen_duration: i64,
    pub(super) resource: FreezeResource,
    /// Raw resource code from protobuf (for Java-parity error messages on unknown values).
    pub(super) resource_raw: i64,
    pub(super) receiver_address: Vec<u8>,
}

/// UnfreezeBalance contract parameters
#[derive(Debug, Clone)]
pub(super) struct UnfreezeParams {
    pub(super) resource: FreezeResource,
    /// Raw resource code from protobuf (for Java-parity error messages on unknown values).
    pub(super) resource_raw: i64,
    pub(super) receiver_address: Vec<u8>,
}

/// FreezeBalanceV2 contract parameters
#[derive(Debug, Clone)]
pub(super) struct FreezeV2Params {
    pub(super) owner_address: Vec<u8>,
    pub(super) frozen_balance: i64,
    pub(super) resource: Option<FreezeResource>,
}

/// UnfreezeBalanceV2 contract parameters
#[derive(Debug, Clone)]
pub(super) struct UnfreezeV2Params {
    pub(super) unfreeze_balance: i64,
    pub(super) resource: Option<FreezeResource>,
}

/// Resource type for freeze/unfreeze operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FreezeResource {
    Bandwidth = 0,
    Energy = 1,
    TronPower = 2,
    /// Unknown resource code (for deferred validation matching Java behavior).
    Unknown = 255,
}

/// Expected type_url for FreezeBalanceContract in protobuf Any wrapper.
const FREEZE_BALANCE_TYPE_URL: &str = "type.googleapis.com/protocol.FreezeBalanceContract";

/// Expected type_url for UnfreezeBalanceContract in protobuf Any wrapper.
const UNFREEZE_BALANCE_TYPE_URL: &str = "type.googleapis.com/protocol.UnfreezeBalanceContract";

impl BackendService {
    pub(crate) fn execute_freeze_balance_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        use tron_backend_execution::{TronExecutionResult, TronStateChange};

        // Java parity: Check type_url in Any wrapper (FreezeBalanceActuator.validate() line 194-198).
        // If contract_parameter is provided, validate it matches the expected type.
        if let Some(ref param) = transaction.metadata.contract_parameter {
            if !param.type_url.ends_with("FreezeBalanceContract") {
                return Err(format!(
                    "contract type error,expected type [FreezeBalanceContract],real type[{}]",
                    param.type_url
                ));
            }
        }

        // Parse freeze parameters from transaction data
        let params = Self::parse_freeze_balance_params(&transaction.data)?;

        info!("FreezeBalance owner={} amount={} resource={:?} duration={}",
              tron_backend_common::to_tron_address(&transaction.from),
              params.frozen_balance,
              params.resource,
              params.frozen_duration);

        // === Validation (match java-tron FreezeBalanceActuator#validate) ===
        let prefix = storage_adapter.address_prefix();
        let owner_raw = transaction.metadata.from_raw.as_deref().unwrap_or(&[]);
        if owner_raw.len() != 21 || owner_raw[0] != prefix {
            return Err("Invalid address".to_string());
        }
        let readable_owner_address = hex::encode(owner_raw);

        // Load owner proto (we must update frozen fields for fixture parity).
        let owner_proto = storage_adapter
            .get_account_proto(&transaction.from)
            .map_err(|e| format!("Failed to load owner account proto: {}", e))?
            .ok_or_else(|| format!("Account[{}] not exists", readable_owner_address))?;

        // Snapshot AccountInfo view for state_changes output (not used by fixtures).
        let owner_account = storage_adapter
            .get_account(&transaction.from)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .unwrap_or_default();

        if params.frozen_balance <= 0 {
            return Err("frozenBalance must be positive".to_string());
        }

        if params.frozen_balance < super::super::TRX_PRECISION as i64 {
            return Err("frozenBalance must be greater than or equal to 1 TRX".to_string());
        }

        let frozen_count = owner_proto.frozen.len() as i64;
        if !(frozen_count == 0 || frozen_count == 1) {
            return Err("frozenCount must be 0 or 1".to_string());
        }

        if params.frozen_balance > owner_proto.balance {
            return Err("frozenBalance must be less than or equal to accountBalance".to_string());
        }

        let min_frozen_time = storage_adapter
            .get_min_frozen_time()
            .map_err(|e| format!("Failed to get MIN_FROZEN_TIME: {}", e))?;
        let max_frozen_time = storage_adapter
            .get_max_frozen_time()
            .map_err(|e| format!("Failed to get MAX_FROZEN_TIME: {}", e))?;

        // Java gating: CommonParameter.checkFrozenTime defaults to 1 (enabled).
        if !(params.frozen_duration >= min_frozen_time && params.frozen_duration <= max_frozen_time) {
            return Err(format!(
                "frozenDuration must be less than {} days and more than {} days",
                max_frozen_time, min_frozen_time
            ));
        }

        // ResourceCode validation.
        // Java reference: FreezeBalanceActuator.validate() switch (contract.getResource())
        let support_allow_new_resource_model = storage_adapter
            .support_allow_new_resource_model()
            .map_err(|e| format!("Failed to read ALLOW_NEW_RESOURCE_MODEL: {}", e))?;
        match params.resource {
            FreezeResource::Bandwidth | FreezeResource::Energy => {}
            FreezeResource::TronPower => {
                if support_allow_new_resource_model {
                    if !params.receiver_address.is_empty() {
                        return Err("TRON_POWER is not allowed to delegate to other accounts.".to_string());
                    }
                } else {
                    return Err("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]".to_string());
                }
            }
            FreezeResource::Unknown => {
                // Unknown resource codes: match Java's default case exactly.
                // Java uses fullwidth comma (U+3001 "、") in error messages.
                if support_allow_new_resource_model {
                    return Err("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY、TRON_POWER]".to_string());
                } else {
                    return Err("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]".to_string());
                }
            }
        }

        // Receiver validation: applies only when receiver_address is set and supportDR() is enabled.
        let support_dr = storage_adapter
            .support_dr()
            .map_err(|e| format!("Failed to check supportDR: {}", e))?;

        let mut receiver_address: Option<Address> = None;
        let mut readable_receiver_address = String::new();
        if !params.receiver_address.is_empty() && support_dr {
            if params.receiver_address.as_slice() == owner_raw {
                return Err("receiverAddress must not be the same as ownerAddress".to_string());
            }

            if params.receiver_address.len() != 21 || params.receiver_address[0] != prefix {
                return Err("Invalid receiverAddress".to_string());
            }

            readable_receiver_address = hex::encode(&params.receiver_address);
            let receiver = Address::from_slice(&params.receiver_address[1..]);

            let receiver_proto = storage_adapter
                .get_account_proto(&receiver)
                .map_err(|e| format!("Failed to load receiver account proto: {}", e))?
                .ok_or_else(|| format!("Account[{}] not exists", readable_receiver_address))?;

            // Constantinople gate: disallow delegating to contract accounts.
            let allow_tvm_constantinople = storage_adapter
                .get_allow_tvm_constantinople()
                .map_err(|e| format!("Failed to get ALLOW_TVM_CONSTANTINOPLE: {}", e))?;
            if allow_tvm_constantinople == 1 && receiver_proto.r#type == 2 {
                return Err("Do not allow delegate resources to contract addresses".to_string());
            }

            receiver_address = Some(receiver);
        }

        // V1 freeze is closed when V2 (unfreeze delay) is enabled.
        if storage_adapter
            .support_unfreeze_delay()
            .map_err(|e| format!("Failed to read UNFREEZE_DELAY_DAYS: {}", e))?
        {
            warn!("freeze v2 is open, old freeze is closed");
            return Err("freeze v2 is open, old freeze is closed".to_string());
        }

        // Calculate expiration timestamp (milliseconds since epoch).
        //
        // java-tron uses `DynamicPropertiesStore.getLatestBlockHeaderTimestamp()` as "now".
        // During execution of block N, this still points at block N-1 because the dynamic
        // property is updated only after the block is committed.
        let now_ms = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to read latest_block_header_timestamp: {}", e))?;
        let now_ms_u64: u64 = now_ms.try_into().unwrap_or(context.block_timestamp);

        let duration_days: u64 = params
            .frozen_duration
            .try_into()
            .map_err(|_| "frozenDuration must be non-negative".to_string())?;
        let duration_millis = duration_days
            .checked_mul(86_400_000u64) // days to milliseconds
            .ok_or("Duration millis overflow")?;
        let expiration_timestamp = now_ms_u64
            .checked_add(duration_millis)
            .ok_or("Expiration timestamp overflow")? as i64;

        debug!(
            "Freeze record: amount={}, expiration={}, resource={:?}",
            params.frozen_balance, expiration_timestamp, params.resource
        );

        // Determine whether to use the new reward algorithm for weight deltas.
        // Java reference: DynamicPropertiesStore.allowNewReward() -> ALLOW_NEW_REWARD == 1
        let allow_new_reward = storage_adapter
            .allow_new_reward()
            .unwrap_or(false);

        // Java: initialize oldTronPower when the new resource model is enabled and oldTronPower==0.
        // This must be done BEFORE applying the freeze mutation.
        // Java reference: FreezeBalanceActuator.execute() -> initializeOldTronPower()
        let mut owner_proto = owner_proto;
        if support_allow_new_resource_model && owner_proto.old_tron_power == 0 {
            let tron_power = storage_adapter
                .get_tron_power_in_sun(&transaction.from, false)
                .map_err(|e| format!("Failed to compute tron power: {}", e))?;
            owner_proto.old_tron_power = if tron_power == 0 {
                -1
            } else {
                tron_power
                    .try_into()
                    .map_err(|_| "tron power exceeds i64::MAX".to_string())?
            };
            debug!("Initialized oldTronPower to {} for owner={}",
                   owner_proto.old_tron_power,
                   tron_backend_common::to_tron_address(&transaction.from));
        }

        // Check if delegate optimization is enabled for index updates.
        let support_delegate_optimization = storage_adapter
            .support_allow_delegate_optimization()
            .unwrap_or(false);

        // Apply balance delta (common to self-freeze and delegated-freeze).
        let new_owner_balance = owner_proto
            .balance
            .checked_sub(params.frozen_balance)
            .ok_or("Balance underflow")?;

        // Delegation path: receiver_address is set and supportDR() is enabled.
        if let Some(receiver) = receiver_address {
            let receiver_proto = storage_adapter
                .get_account_proto(&receiver)
                .map_err(|e| format!("Failed to load receiver account proto: {}", e))?
                .ok_or_else(|| format!("Account[{}] not exists", readable_receiver_address))?;

            let mut new_owner_proto = owner_proto.clone();
            new_owner_proto.balance = new_owner_balance;

            let mut new_receiver_proto = receiver_proto.clone();

            let mut increment: i64 = 0;
            match params.resource {
                FreezeResource::Bandwidth => {
                    // Update DelegatedResourceStore and index stores (Java: delegateResource()).
                    storage_adapter
                        .delegate_resource_v1(
                            &transaction.from,
                            &receiver,
                            true,
                            params.frozen_balance,
                            expiration_timestamp,
                        )
                        .map_err(|e| format!("Failed to update DelegatedResource (v1): {}", e))?;

                    // Update delegation account index: use optimized layout when enabled.
                    // Java reference: FreezeBalanceActuator.delegateResource() checks supportAllowDelegateOptimization()
                    if support_delegate_optimization {
                        // Convert any legacy index entries for both addresses first
                        storage_adapter
                            .convert_delegated_resource_account_index_v1(&transaction.from)
                            .map_err(|e| format!("Failed to convert owner delegation index: {}", e))?;
                        storage_adapter
                            .convert_delegated_resource_account_index_v1(&receiver)
                            .map_err(|e| format!("Failed to convert receiver delegation index: {}", e))?;
                        // Write optimized prefix keys
                        storage_adapter
                            .delegate_v1_optimized(&transaction.from, &receiver, now_ms)
                            .map_err(|e| format!("Failed to update DelegatedResourceAccountIndex (v1 optimized): {}", e))?;
                    } else {
                        storage_adapter
                            .delegate_resource_account_index_v1(&transaction.from, &receiver)
                            .map_err(|e| format!("Failed to update DelegatedResourceAccountIndex (v1): {}", e))?;
                    }

                    // Update owner delegated balance.
                    new_owner_proto.delegated_frozen_balance_for_bandwidth = new_owner_proto
                        .delegated_frozen_balance_for_bandwidth
                        .checked_add(params.frozen_balance)
                        .ok_or("Delegated frozen balance overflow")?;

                    // Update receiver acquired delegated balance.
                    let old_weight = new_receiver_proto.acquired_delegated_frozen_balance_for_bandwidth
                        / super::super::TRX_PRECISION as i64;
                    new_receiver_proto.acquired_delegated_frozen_balance_for_bandwidth = new_receiver_proto
                        .acquired_delegated_frozen_balance_for_bandwidth
                        .checked_add(params.frozen_balance)
                        .ok_or("Acquired delegated frozen balance overflow")?;
                    let new_weight = new_receiver_proto.acquired_delegated_frozen_balance_for_bandwidth
                        / super::super::TRX_PRECISION as i64;
                    increment = new_weight - old_weight;

                    let weight = if allow_new_reward {
                        increment
                    } else {
                        params.frozen_balance / super::super::TRX_PRECISION as i64
                    };
                    storage_adapter
                        .add_total_net_weight(weight)
                        .map_err(|e| format!("Failed to update total net weight: {}", e))?;
                }
                FreezeResource::Energy => {
                    storage_adapter
                        .delegate_resource_v1(
                            &transaction.from,
                            &receiver,
                            false,
                            params.frozen_balance,
                            expiration_timestamp,
                        )
                        .map_err(|e| format!("Failed to update DelegatedResource (v1): {}", e))?;

                    // Update delegation account index: use optimized layout when enabled.
                    if support_delegate_optimization {
                        storage_adapter
                            .convert_delegated_resource_account_index_v1(&transaction.from)
                            .map_err(|e| format!("Failed to convert owner delegation index: {}", e))?;
                        storage_adapter
                            .convert_delegated_resource_account_index_v1(&receiver)
                            .map_err(|e| format!("Failed to convert receiver delegation index: {}", e))?;
                        storage_adapter
                            .delegate_v1_optimized(&transaction.from, &receiver, now_ms)
                            .map_err(|e| format!("Failed to update DelegatedResourceAccountIndex (v1 optimized): {}", e))?;
                    } else {
                        storage_adapter
                            .delegate_resource_account_index_v1(&transaction.from, &receiver)
                            .map_err(|e| format!("Failed to update DelegatedResourceAccountIndex (v1): {}", e))?;
                    }

                    // Ensure AccountResource exists on both accounts.
                    if new_owner_proto.account_resource.is_none() {
                        new_owner_proto.account_resource =
                            Some(tron_backend_execution::protocol::account::AccountResource::default());
                    }
                    if new_receiver_proto.account_resource.is_none() {
                        new_receiver_proto.account_resource =
                            Some(tron_backend_execution::protocol::account::AccountResource::default());
                    }

                    // Update owner delegated balance for energy.
                    if let Some(ref mut res) = new_owner_proto.account_resource {
                        res.delegated_frozen_balance_for_energy = res
                            .delegated_frozen_balance_for_energy
                            .checked_add(params.frozen_balance)
                            .ok_or("Delegated frozen balance overflow")?;
                    }

                    // Update receiver acquired delegated balance for energy.
                    let old_weight = new_receiver_proto
                        .account_resource
                        .as_ref()
                        .map(|r| r.acquired_delegated_frozen_balance_for_energy)
                        .unwrap_or(0)
                        / super::super::TRX_PRECISION as i64;
                    if let Some(ref mut res) = new_receiver_proto.account_resource {
                        res.acquired_delegated_frozen_balance_for_energy = res
                            .acquired_delegated_frozen_balance_for_energy
                            .checked_add(params.frozen_balance)
                            .ok_or("Acquired delegated frozen balance overflow")?;
                    }
                    let new_weight = new_receiver_proto
                        .account_resource
                        .as_ref()
                        .map(|r| r.acquired_delegated_frozen_balance_for_energy)
                        .unwrap_or(0)
                        / super::super::TRX_PRECISION as i64;
                    increment = new_weight - old_weight;

                    let weight = if allow_new_reward {
                        increment
                    } else {
                        params.frozen_balance / super::super::TRX_PRECISION as i64
                    };
                    storage_adapter
                        .add_total_energy_weight(weight)
                        .map_err(|e| format!("Failed to update total energy weight: {}", e))?;
                }
                FreezeResource::TronPower => {
                    // TRON_POWER delegation is rejected in validation.
                }
                FreezeResource::Unknown => {
                    // Unreachable: Unknown is rejected during validation
                }
            }

            storage_adapter
                .put_account_proto(&transaction.from, &new_owner_proto)
                .map_err(|e| format!("Failed to persist owner account proto: {}", e))?;
            storage_adapter
                .put_account_proto(&receiver, &new_receiver_proto)
                .map_err(|e| format!("Failed to persist receiver account proto: {}", e))?;
        } else {
            // Self-freeze path.
            let mut new_owner_proto = owner_proto.clone();
            new_owner_proto.balance = new_owner_balance;

            match params.resource {
                FreezeResource::Bandwidth => {
                    let old_frozen = new_owner_proto
                        .frozen
                        .first()
                        .map(|f| f.frozen_balance)
                        .unwrap_or(0);
                    let old_weight = old_frozen / super::super::TRX_PRECISION as i64;

                    let new_frozen = old_frozen
                        .checked_add(params.frozen_balance)
                        .ok_or("Frozen balance overflow")?;
                    new_owner_proto.frozen = vec![tron_backend_execution::protocol::account::Frozen {
                        frozen_balance: new_frozen,
                        expire_time: expiration_timestamp,
                    }];

                    let new_weight = new_frozen / super::super::TRX_PRECISION as i64;
                    let increment = new_weight - old_weight;
                    let weight = if allow_new_reward {
                        increment
                    } else {
                        params.frozen_balance / super::super::TRX_PRECISION as i64
                    };

                    storage_adapter
                        .add_total_net_weight(weight)
                        .map_err(|e| format!("Failed to update total net weight: {}", e))?;
                }
                FreezeResource::Energy => {
                    if new_owner_proto.account_resource.is_none() {
                        new_owner_proto.account_resource =
                            Some(tron_backend_execution::protocol::account::AccountResource::default());
                    }

                    let old_frozen = new_owner_proto
                        .account_resource
                        .as_ref()
                        .and_then(|r| r.frozen_balance_for_energy.as_ref())
                        .map(|f| f.frozen_balance)
                        .unwrap_or(0);
                    let old_weight = old_frozen / super::super::TRX_PRECISION as i64;

                    let new_frozen = old_frozen
                        .checked_add(params.frozen_balance)
                        .ok_or("Frozen balance overflow")?;
                    if let Some(ref mut res) = new_owner_proto.account_resource {
                        res.frozen_balance_for_energy = Some(tron_backend_execution::protocol::account::Frozen {
                            frozen_balance: new_frozen,
                            expire_time: expiration_timestamp,
                        });
                    }

                    let new_weight = new_frozen / super::super::TRX_PRECISION as i64;
                    let increment = new_weight - old_weight;
                    let weight = if allow_new_reward {
                        increment
                    } else {
                        params.frozen_balance / super::super::TRX_PRECISION as i64
                    };

                    storage_adapter
                        .add_total_energy_weight(weight)
                        .map_err(|e| format!("Failed to update total energy weight: {}", e))?;
                }
                FreezeResource::TronPower => {
                    let old_frozen = new_owner_proto
                        .tron_power
                        .as_ref()
                        .map(|f| f.frozen_balance)
                        .unwrap_or(0);
                    let old_weight = old_frozen / super::super::TRX_PRECISION as i64;

                    let new_frozen = old_frozen
                        .checked_add(params.frozen_balance)
                        .ok_or("Frozen balance overflow")?;
                    new_owner_proto.tron_power = Some(tron_backend_execution::protocol::account::Frozen {
                        frozen_balance: new_frozen,
                        expire_time: expiration_timestamp,
                    });

                    let new_weight = new_frozen / super::super::TRX_PRECISION as i64;
                    let increment = new_weight - old_weight;
                    let weight = if allow_new_reward {
                        increment
                    } else {
                        params.frozen_balance / super::super::TRX_PRECISION as i64
                    };

                    storage_adapter
                        .add_total_tron_power_weight(weight)
                        .map_err(|e| format!("Failed to update total tron power weight: {}", e))?;
                }
                FreezeResource::Unknown => {
                    // Unreachable: Unknown is rejected during validation
                }
            }

            // Persist updated owner proto.
            storage_adapter
                .put_account_proto(&transaction.from, &new_owner_proto)
                .map_err(|e| format!("Failed to persist owner account proto: {}", e))?;
        }

        // Keep the Rust-side freeze ledger updated (not part of Java DB layout).
        storage_adapter
            .add_freeze_amount(
                transaction.from,
                params.resource as u8,
                params.frozen_balance as u64,
                expiration_timestamp,
            )
            .map_err(|e| format!("Failed to persist freeze record: {}", e))?;

        // Build state change using AccountInfo view (for CSV parity).
        let mut new_owner = owner_account.clone();
        new_owner.balance = U256::from(new_owner_balance as u64);

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
                    FreezeResource::Unknown => {
                        // Unreachable: Unknown is rejected during validation
                        return Err("Unknown resource code".to_string());
                    }
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
            // Read current global totals from DynamicProperties (already updated in-buffer).
            let total_net_weight = storage_adapter.get_total_net_weight()
                .map_err(|e| format!("Failed to get total net weight: {}", e))?;
            let total_net_limit = storage_adapter.get_total_net_limit()
                .map_err(|e| format!("Failed to get total net limit: {}", e))?;
            let total_energy_weight = storage_adapter.get_total_energy_weight()
                .map_err(|e| format!("Failed to get total energy weight: {}", e))?;
            let total_energy_limit = storage_adapter.get_total_energy_limit()
                .map_err(|e| format!("Failed to get total energy limit: {}", e))?;

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
            trc10_changes: vec![], // Not applicable for freeze contracts
            vote_changes: vec![], // Not applicable for freeze contracts
            withdraw_changes: vec![], // Not applicable for freeze contracts
            tron_transaction_result: None, // Phase 0.4: Receipt passthrough
            contract_address: None, // Not applicable for freeze contracts
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
        info!(
            "Executing UNFREEZE_BALANCE_CONTRACT: owner={}, data_len={}",
            tron_backend_common::to_tron_address(&transaction.from),
            transaction.data.len()
        );

        // Java parity: Check type_url in Any wrapper (UnfreezeBalanceActuator.validate() line 339-343).
        // If contract_parameter is provided, validate it matches the expected type.
        if let Some(ref param) = transaction.metadata.contract_parameter {
            if !param.type_url.ends_with("UnfreezeBalanceContract") {
                return Err(format!(
                    "contract type error, expected type [UnfreezeBalanceContract], real type[{}]",
                    param.type_url
                ));
            }
        }

        // Parse unfreeze parameters from transaction data
        let params = Self::parse_unfreeze_balance_params(&transaction.data)?;

        debug!(
            "Parsed unfreeze params: resource={:?}, receiver_len={}",
            params.resource,
            params.receiver_address.len()
        );

        const INVALID_RESOURCE_CODE: &str = "ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]";

        // === Validation (match java-tron UnfreezeBalanceActuator messages) ===
        // Validate owner address
        let prefix = storage_adapter.address_prefix();
        let owner_raw = transaction.metadata.from_raw.as_deref().unwrap_or(&[]);
        if owner_raw.len() != 21 || owner_raw[0] != prefix {
            return Err("Invalid address".to_string());
        }
        let readable_owner_address = hex::encode(owner_raw);

        // java-tron uses `DynamicPropertiesStore.getLatestBlockHeaderTimestamp()` as "now".
        let now_ms = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to read latest_block_header_timestamp: {}", e))?;

        let allow_new_resource_model = storage_adapter
            .support_allow_new_resource_model()
            .map_err(|e| format!("Failed to read ALLOW_NEW_RESOURCE_MODEL: {}", e))?;

        let support_dr = storage_adapter
            .support_dr()
            .map_err(|e| format!("Failed to check supportDR: {}", e))?;

        let allow_tvm_constantinople = storage_adapter
            .get_allow_tvm_constantinople()
            .map_err(|e| format!("Failed to get ALLOW_TVM_CONSTANTINOPLE: {}", e))?;

        let allow_tvm_solidity059 = storage_adapter
            .get_allow_tvm_solidity059()
            .map_err(|e| format!("Failed to get ALLOW_TVM_SOLIDITY_059: {}", e))?;

        // Determine whether to use the new reward algorithm for weight deltas.
        // Java reference: DynamicPropertiesStore.allowNewReward() -> ALLOW_NEW_REWARD == 1
        let allow_new_reward = storage_adapter
            .allow_new_reward()
            .unwrap_or(false);

        // Check if delegate optimization is enabled for index updates.
        let support_delegate_optimization = storage_adapter
            .support_allow_delegate_optimization()
            .unwrap_or(false);

        // Load owner proto (we must update frozen fields for fixture parity).
        let owner_proto = storage_adapter
            .get_account_proto(&transaction.from)
            .map_err(|e| format!("Failed to load owner account proto: {}", e))?
            .ok_or_else(|| format!("Account[{}] does not exist", readable_owner_address))?;

        let owner_account = storage_adapter
            .get_account(&transaction.from)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .unwrap_or_default();

        let mut new_owner_proto = owner_proto.clone();

        // Java: initialize oldTronPower when the new resource model is enabled and oldTronPower==0.
        if allow_new_resource_model && new_owner_proto.old_tron_power == 0 {
            let tron_power = storage_adapter
                .get_tron_power_in_sun(&transaction.from, false)
                .map_err(|e| format!("Failed to compute tron power: {}", e))?;
            new_owner_proto.old_tron_power = if tron_power == 0 {
                -1
            } else {
                tron_power
                    .try_into()
                    .map_err(|_| "tron power exceeds i64::MAX".to_string())?
            };
        }

        let mut unfreeze_amount: i64 = 0;
        let mut decrease: i64 = 0;
        let mut updated_receiver: Option<(Address, tron_backend_execution::protocol::Account)> = None;

        // If receiver_address is provided and supportDR() is enabled, unfreeze delegated balance.
        if !params.receiver_address.is_empty() && support_dr {
            if params.receiver_address.as_slice() == owner_raw {
                return Err("receiverAddress must not be the same as ownerAddress".to_string());
            }

            if params.receiver_address.len() != 21 || params.receiver_address[0] != prefix {
                return Err("Invalid receiverAddress".to_string());
            }

            let readable_receiver_address = hex::encode(&params.receiver_address);
            let receiver = Address::from_slice(&params.receiver_address[1..]);

            let receiver_proto = storage_adapter
                .get_account_proto(&receiver)
                .map_err(|e| format!("Failed to load receiver account proto: {}", e))?;
            if allow_tvm_constantinople == 0 && receiver_proto.is_none() {
                return Err(format!(
                    "Receiver Account[{}] does not exist",
                    readable_receiver_address
                ));
            }

            let delegated_resource = storage_adapter
                .get_delegated_resource_v1(&transaction.from, &receiver)
                .map_err(|e| format!("Failed to load DelegatedResource (v1): {}", e))?
                .ok_or("delegated Resource does not exist")?;

            let allow_multi_sign = storage_adapter
                .get_allow_multi_sign()
                .map_err(|e| format!("Failed to read ALLOW_MULTI_SIGN: {}", e))?;

            let mut new_delegated_resource = delegated_resource.clone();

            match params.resource {
                FreezeResource::Bandwidth => {
                    let delegated_balance = delegated_resource.frozen_balance_for_bandwidth;
                    if delegated_balance <= 0 {
                        return Err("no delegatedFrozenBalance(BANDWIDTH)".to_string());
                    }

                    // Validate acquired delegated balance.
                    if allow_tvm_constantinople == 0 {
                        if let Some(ref rcvr) = receiver_proto {
                            if rcvr.acquired_delegated_frozen_balance_for_bandwidth < delegated_balance {
                                return Err(format!(
                                    "AcquiredDelegatedFrozenBalanceForBandwidth[{}] < delegatedBandwidth[{}]",
                                    rcvr.acquired_delegated_frozen_balance_for_bandwidth,
                                    delegated_balance
                                ));
                            }
                        }
                    } else if allow_tvm_solidity059 != 1 {
                        if let Some(ref rcvr) = receiver_proto {
                            let is_contract = rcvr.r#type
                                == tron_backend_execution::protocol::AccountType::Contract as i32;
                            if !is_contract
                                && rcvr.acquired_delegated_frozen_balance_for_bandwidth < delegated_balance
                            {
                                return Err(format!(
                                    "AcquiredDelegatedFrozenBalanceForBandwidth[{}] < delegatedBandwidth[{}]",
                                    rcvr.acquired_delegated_frozen_balance_for_bandwidth,
                                    delegated_balance
                                ));
                            }
                        }
                    }

                    if delegated_resource.expire_time_for_bandwidth > now_ms {
                        return Err("It's not time to unfreeze.".to_string());
                    }

                    unfreeze_amount = delegated_balance;
                    new_delegated_resource.frozen_balance_for_bandwidth = 0;
                    new_delegated_resource.expire_time_for_bandwidth = 0;

                    new_owner_proto.delegated_frozen_balance_for_bandwidth = new_owner_proto
                        .delegated_frozen_balance_for_bandwidth
                        .checked_sub(unfreeze_amount)
                        .ok_or("Delegated frozen balance underflow")?;

                    // Update receiver account when allowed.
                    let should_update_receiver = allow_tvm_constantinople == 0
                        || receiver_proto
                            .as_ref()
                            .map(|rcvr| {
                                rcvr.r#type
                                    != tron_backend_execution::protocol::AccountType::Contract as i32
                            })
                            .unwrap_or(false);

                    if should_update_receiver {
                        if let Some(rcvr) = receiver_proto {
                            let old_weight =
                                rcvr.acquired_delegated_frozen_balance_for_bandwidth
                                    / super::super::TRX_PRECISION as i64;
                            let mut new_rcvr = rcvr;

                            if allow_tvm_solidity059 == 1
                                && new_rcvr.acquired_delegated_frozen_balance_for_bandwidth
                                    < unfreeze_amount
                            {
                                new_rcvr.acquired_delegated_frozen_balance_for_bandwidth = 0;
                                decrease = -(unfreeze_amount / super::super::TRX_PRECISION as i64);
                            } else {
                                new_rcvr.acquired_delegated_frozen_balance_for_bandwidth = new_rcvr
                                    .acquired_delegated_frozen_balance_for_bandwidth
                                    .checked_sub(unfreeze_amount)
                                    .ok_or("Acquired delegated frozen balance underflow")?;
                                let new_weight =
                                    new_rcvr.acquired_delegated_frozen_balance_for_bandwidth
                                        / super::super::TRX_PRECISION as i64;
                                decrease = new_weight - old_weight;
                            }

                            updated_receiver = Some((receiver, new_rcvr));
                        }
                    } else {
                        decrease = -(unfreeze_amount / super::super::TRX_PRECISION as i64);
                    }
                }
                FreezeResource::Energy => {
                    let delegated_balance = delegated_resource.frozen_balance_for_energy;
                    if delegated_balance <= 0 {
                        return Err("no delegateFrozenBalance(Energy)".to_string());
                    }

                    // Validate acquired delegated balance.
                    if allow_tvm_constantinople == 0 {
                        if let Some(ref rcvr) = receiver_proto {
                            let acquired = rcvr
                                .account_resource
                                .as_ref()
                                .map(|r| r.acquired_delegated_frozen_balance_for_energy)
                                .unwrap_or(0);
                            if acquired < delegated_balance {
                                return Err(format!(
                                    "AcquiredDelegatedFrozenBalanceForEnergy[{}] < delegatedEnergy[{}]",
                                    acquired, delegated_balance
                                ));
                            }
                        }
                    } else if allow_tvm_solidity059 != 1 {
                        if let Some(ref rcvr) = receiver_proto {
                            let is_contract = rcvr.r#type
                                == tron_backend_execution::protocol::AccountType::Contract as i32;
                            if !is_contract {
                                let acquired = rcvr
                                    .account_resource
                                    .as_ref()
                                    .map(|r| r.acquired_delegated_frozen_balance_for_energy)
                                    .unwrap_or(0);
                                if acquired < delegated_balance {
                                    return Err(format!(
                                        "AcquiredDelegatedFrozenBalanceForEnergy[{}] < delegatedEnergy[{}]",
                                        acquired, delegated_balance
                                    ));
                                }
                            }
                        }
                    }

                    let expire_time_for_energy = if allow_multi_sign {
                        delegated_resource.expire_time_for_energy
                    } else {
                        delegated_resource.expire_time_for_bandwidth
                    };
                    if expire_time_for_energy > now_ms {
                        return Err("It's not time to unfreeze.".to_string());
                    }

                    unfreeze_amount = delegated_balance;
                    new_delegated_resource.frozen_balance_for_energy = 0;
                    new_delegated_resource.expire_time_for_energy = 0;

                    if new_owner_proto.account_resource.is_none() {
                        new_owner_proto.account_resource =
                            Some(tron_backend_execution::protocol::account::AccountResource::default());
                    }
                    if let Some(ref mut res) = new_owner_proto.account_resource {
                        res.delegated_frozen_balance_for_energy = res
                            .delegated_frozen_balance_for_energy
                            .checked_sub(unfreeze_amount)
                            .ok_or("Delegated frozen balance underflow")?;
                    }

                    // Update receiver account when allowed.
                    let should_update_receiver = allow_tvm_constantinople == 0
                        || receiver_proto
                            .as_ref()
                            .map(|rcvr| {
                                rcvr.r#type
                                    != tron_backend_execution::protocol::AccountType::Contract as i32
                            })
                            .unwrap_or(false);

                    if should_update_receiver {
                        if let Some(rcvr) = receiver_proto {
                            let acquired = rcvr
                                .account_resource
                                .as_ref()
                                .map(|r| r.acquired_delegated_frozen_balance_for_energy)
                                .unwrap_or(0);
                            let old_weight = acquired / super::super::TRX_PRECISION as i64;
                            let mut new_rcvr = rcvr;
                            if new_rcvr.account_resource.is_none() {
                                new_rcvr.account_resource =
                                    Some(tron_backend_execution::protocol::account::AccountResource::default());
                            }

                            if allow_tvm_solidity059 == 1 && acquired < unfreeze_amount {
                                if let Some(ref mut res) = new_rcvr.account_resource {
                                    res.acquired_delegated_frozen_balance_for_energy = 0;
                                }
                                decrease = -(unfreeze_amount / super::super::TRX_PRECISION as i64);
                            } else if let Some(ref mut res) = new_rcvr.account_resource {
                                res.acquired_delegated_frozen_balance_for_energy = res
                                    .acquired_delegated_frozen_balance_for_energy
                                    .checked_sub(unfreeze_amount)
                                    .ok_or("Acquired delegated frozen balance underflow")?;
                                let new_weight = res.acquired_delegated_frozen_balance_for_energy
                                    / super::super::TRX_PRECISION as i64;
                                decrease = new_weight - old_weight;
                            }

                            updated_receiver = Some((receiver, new_rcvr));
                        }
                    } else {
                        decrease = -(unfreeze_amount / super::super::TRX_PRECISION as i64);
                    }
                }
                _ => return Err(INVALID_RESOURCE_CODE.to_string()),
            }

            new_owner_proto.balance = owner_proto
                .balance
                .checked_add(unfreeze_amount)
                .ok_or("Balance overflow")?;

            if new_delegated_resource.frozen_balance_for_bandwidth == 0
                && new_delegated_resource.frozen_balance_for_energy == 0
            {
                storage_adapter
                    .delete_delegated_resource_v1(&transaction.from, &receiver)
                    .map_err(|e| format!("Failed to delete DelegatedResource (v1): {}", e))?;

                // Update delegation account index: use optimized layout when enabled.
                // Java reference: UnfreezeBalanceActuator checks supportAllowDelegateOptimization()
                if support_delegate_optimization {
                    storage_adapter
                        .undelegate_v1_optimized(&transaction.from, &receiver)
                        .map_err(|e| {
                            format!(
                                "Failed to update DelegatedResourceAccountIndex (v1 optimized): {}",
                                e
                            )
                        })?;
                } else {
                    storage_adapter
                        .undelegate_resource_account_index_v1(&transaction.from, &receiver)
                        .map_err(|e| {
                            format!(
                                "Failed to update DelegatedResourceAccountIndex (v1): {}",
                                e
                            )
                        })?;
                }
            } else {
                storage_adapter
                    .put_delegated_resource_v1(&transaction.from, &receiver, &new_delegated_resource)
                    .map_err(|e| format!("Failed to update DelegatedResource (v1): {}", e))?;
            }
        } else {
            // Self-unfreeze
            match params.resource {
                FreezeResource::Bandwidth => {
                    if owner_proto.frozen.is_empty() {
                        return Err("no frozenBalance(BANDWIDTH)".to_string());
                    }

                    let allowed_unfreeze_count = owner_proto
                        .frozen
                        .iter()
                        .filter(|f| f.expire_time <= now_ms)
                        .count();
                    if allowed_unfreeze_count == 0 {
                        return Err("It's not time to unfreeze(BANDWIDTH).".to_string());
                    }

                    let old_total_frozen = owner_proto
                        .frozen
                        .iter()
                        .try_fold(0i64, |acc, f| acc.checked_add(f.frozen_balance))
                        .ok_or("Balance overflow")?;
                    let old_weight = old_total_frozen / super::super::TRX_PRECISION as i64;

                    let mut remaining_frozen = Vec::with_capacity(owner_proto.frozen.len());
                    for frozen in owner_proto.frozen.iter() {
                        if frozen.expire_time <= now_ms {
                            unfreeze_amount = unfreeze_amount
                                .checked_add(frozen.frozen_balance)
                                .ok_or("Balance overflow")?;
                        } else {
                            remaining_frozen.push(frozen.clone());
                        }
                    }

                    new_owner_proto.frozen = remaining_frozen;

                    let new_total_frozen = new_owner_proto
                        .frozen
                        .iter()
                        .try_fold(0i64, |acc, f| acc.checked_add(f.frozen_balance))
                        .ok_or("Balance overflow")?;
                    let new_weight = new_total_frozen / super::super::TRX_PRECISION as i64;
                    decrease = new_weight - old_weight;

                    new_owner_proto.balance = owner_proto
                        .balance
                        .checked_add(unfreeze_amount)
                        .ok_or("Balance overflow")?;
                }
                FreezeResource::Energy => {
                    let frozen_energy = owner_proto
                        .account_resource
                        .as_ref()
                        .and_then(|r| r.frozen_balance_for_energy.as_ref())
                        .cloned()
                        .unwrap_or_default();

                    if frozen_energy.frozen_balance <= 0 {
                        return Err("no frozenBalance(Energy)".to_string());
                    }
                    if frozen_energy.expire_time > now_ms {
                        return Err("It's not time to unfreeze(Energy).".to_string());
                    }

                    unfreeze_amount = frozen_energy.frozen_balance;
                    let old_weight =
                        unfreeze_amount / super::super::TRX_PRECISION as i64;
                    decrease = -old_weight;

                    if new_owner_proto.account_resource.is_some() {
                        if let Some(ref mut res) = new_owner_proto.account_resource {
                            res.frozen_balance_for_energy = None;
                        }
                    }

                    new_owner_proto.balance = owner_proto
                        .balance
                        .checked_add(unfreeze_amount)
                        .ok_or("Balance overflow")?;
                }
                FreezeResource::TronPower => {
                    if !allow_new_resource_model {
                        return Err(INVALID_RESOURCE_CODE.to_string());
                    }

                    let tron_power = owner_proto.tron_power.as_ref().cloned().unwrap_or_default();
                    if tron_power.frozen_balance <= 0 {
                        return Err("no frozenBalance(TronPower)".to_string());
                    }
                    if tron_power.expire_time > now_ms {
                        return Err("It's not time to unfreeze(TronPower).".to_string());
                    }

                    unfreeze_amount = tron_power.frozen_balance;
                    let old_weight =
                        unfreeze_amount / super::super::TRX_PRECISION as i64;
                    decrease = -old_weight;

                    new_owner_proto.tron_power = None;
                    new_owner_proto.balance = owner_proto
                        .balance
                        .checked_add(unfreeze_amount)
                        .ok_or("Balance overflow")?;
                }
                FreezeResource::Unknown => {
                    // Unknown resource codes: match Java's default case exactly.
                    // Java reference: UnfreezeBalanceActuator.validate() default case (self-freeze path).
                    if allow_new_resource_model {
                        return Err("ResourceCode error.valid ResourceCode[BANDWIDTH、Energy、TRON_POWER]".to_string());
                    } else {
                        return Err(INVALID_RESOURCE_CODE.to_string());
                    }
                }
            }
        }

        // Update global weights (java-tron: DynamicPropertiesStore.addTotal*Weight)
        let weight_delta = if allow_new_reward {
            decrease
        } else {
            -(unfreeze_amount / super::super::TRX_PRECISION as i64)
        };

        match params.resource {
            FreezeResource::Bandwidth => storage_adapter
                .add_total_net_weight(weight_delta)
                .map_err(|e| format!("Failed to update total net weight: {}", e))?,
            FreezeResource::Energy => storage_adapter
                .add_total_energy_weight(weight_delta)
                .map_err(|e| format!("Failed to update total energy weight: {}", e))?,
            FreezeResource::TronPower => storage_adapter
                .add_total_tron_power_weight(weight_delta)
                .map_err(|e| format!("Failed to update total tron power weight: {}", e))?,
            FreezeResource::Unknown => {
                // Unreachable: Unknown is rejected during validation
            }
        }

        // Vote clearing (java-tron: needToClearVote)
        let mut need_to_clear_vote = true;
        if allow_new_resource_model && new_owner_proto.old_tron_power == -1 {
            match params.resource {
                FreezeResource::Bandwidth | FreezeResource::Energy => {
                    need_to_clear_vote = false;
                }
                FreezeResource::TronPower | FreezeResource::Unknown => {}
            }
        }

        if need_to_clear_vote {
            // Ensure a VotesRecord exists (java-tron writes a VotesCapsule even when no votes).
            let existing_votes = storage_adapter
                .get_votes(&transaction.from)
                .map_err(|e| format!("Failed to load votes record: {}", e))?;

            let mut votes_record = match existing_votes {
                Some(votes) => votes,
                None => {
                    let mut old_votes = Vec::with_capacity(owner_proto.votes.len());
                    for vote in owner_proto.votes.iter() {
                        let addr_bytes = vote.vote_address.as_slice();
                        let evm_bytes = if addr_bytes.len() == 21
                            && (addr_bytes[0] == 0x41 || addr_bytes[0] == 0xa0)
                        {
                            &addr_bytes[1..]
                        } else if addr_bytes.len() == 20 {
                            addr_bytes
                        } else {
                            continue;
                        };
                        if vote.vote_count < 0 {
                            continue;
                        }
                        old_votes.push(tron_backend_execution::Vote::new(
                            Address::from_slice(evm_bytes),
                            vote.vote_count as u64,
                        ));
                    }
                    VotesRecord::new(transaction.from, old_votes, Vec::new())
                }
            };

            votes_record.clear_new_votes();
            storage_adapter
                .set_votes(transaction.from, &votes_record)
                .map_err(|e| format!("Failed to persist votes record: {}", e))?;
            new_owner_proto.votes.clear();
        }

        // Java: invalidate oldTronPower after unfreeze when the new resource model is enabled.
        if allow_new_resource_model && new_owner_proto.old_tron_power != -1 {
            new_owner_proto.old_tron_power = -1;
        }

        // Persist updated accounts and related stores.
        storage_adapter
            .put_account_proto(&transaction.from, &new_owner_proto)
            .map_err(|e| format!("Failed to persist owner account proto: {}", e))?;

        if let Some((receiver, receiver_proto)) = updated_receiver {
            storage_adapter
                .put_account_proto(&receiver, &receiver_proto)
                .map_err(|e| format!("Failed to persist receiver account proto: {}", e))?;
        }

        // Keep the Rust-side freeze ledger updated (not part of Java DB layout).
        let freeze_resource = params.resource as u8;
        let (remaining_frozen, expiration_hint) = match params.resource {
            FreezeResource::Bandwidth => {
                let self_frozen = new_owner_proto
                    .frozen
                    .iter()
                    .try_fold(0i64, |acc, f| acc.checked_add(f.frozen_balance))
                    .unwrap_or(0);
                let delegated_frozen = new_owner_proto.delegated_frozen_balance_for_bandwidth;
                let total = self_frozen.saturating_add(delegated_frozen).max(0);
                let expiration = new_owner_proto
                    .frozen
                    .iter()
                    .map(|f| f.expire_time)
                    .max()
                    .unwrap_or(0);
                (total as u64, expiration)
            }
            FreezeResource::Energy => {
                let self_frozen = new_owner_proto
                    .account_resource
                    .as_ref()
                    .and_then(|r| r.frozen_balance_for_energy.as_ref())
                    .map(|f| f.frozen_balance)
                    .unwrap_or(0);
                let delegated_frozen = new_owner_proto
                    .account_resource
                    .as_ref()
                    .map(|r| r.delegated_frozen_balance_for_energy)
                    .unwrap_or(0);
                let total = self_frozen.saturating_add(delegated_frozen).max(0);
                let expiration = new_owner_proto
                    .account_resource
                    .as_ref()
                    .and_then(|r| r.frozen_balance_for_energy.as_ref())
                    .map(|f| f.expire_time)
                    .unwrap_or(0);
                (total as u64, expiration)
            }
            FreezeResource::TronPower => {
                let frozen = new_owner_proto
                    .tron_power
                    .as_ref()
                    .map(|f| f.frozen_balance)
                    .unwrap_or(0)
                    .max(0);
                let expiration = new_owner_proto
                    .tron_power
                    .as_ref()
                    .map(|f| f.expire_time)
                    .unwrap_or(0);
                (frozen as u64, expiration)
            }
            FreezeResource::Unknown => {
                // Unreachable: Unknown is rejected during validation
                (0, 0)
            }
        };

        if remaining_frozen > 0 {
            let existing_expiration = storage_adapter
                .get_freeze_record(&transaction.from, freeze_resource)
                .map_err(|e| format!("Failed to read freeze record: {}", e))?
                .map(|r| r.expiration_timestamp)
                .unwrap_or(0);
            let record = tron_backend_execution::FreezeRecord::new(
                remaining_frozen,
                existing_expiration.max(expiration_hint),
            );
            storage_adapter
                .set_freeze_record(transaction.from, freeze_resource, &record)
                .map_err(|e| format!("Failed to persist freeze record: {}", e))?;
        } else {
            storage_adapter
                .remove_freeze_record(&transaction.from, freeze_resource)
                .map_err(|e| format!("Failed to remove freeze record: {}", e))?;
        }

        // Emit exactly one state change for CSV parity.
        let mut new_owner = owner_account.clone();
        new_owner.balance = U256::from(new_owner_proto.balance as u64);

        let state_changes = vec![TronStateChange::AccountChange {
            address: transaction.from,
            old_account: Some(owner_account),
            new_account: Some(new_owner),
        }];

        // Phase 2: Emit freeze ledger changes when enabled
        let emit_freeze_changes = self
            .get_execution_config()
            .ok()
            .map(|cfg| cfg.remote.emit_freeze_ledger_changes)
            .unwrap_or(false);

        let freeze_changes = if emit_freeze_changes {
            use tron_backend_execution::FreezeLedgerResource;
            let resource = match params.resource {
                FreezeResource::Bandwidth => FreezeLedgerResource::Bandwidth,
                FreezeResource::Energy => FreezeLedgerResource::Energy,
                FreezeResource::TronPower => FreezeLedgerResource::TronPower,
                FreezeResource::Unknown => {
                    // Unreachable: Unknown is rejected during validation
                    return Err("Unknown resource code".to_string());
                }
            };

            let (amount, expiration_ms) = match storage_adapter
                .get_freeze_record(&transaction.from, freeze_resource)
            {
                Ok(Some(record)) => (record.frozen_amount as i64, record.expiration_timestamp),
                _ => (0, 0),
            };

            let change = tron_backend_execution::FreezeLedgerChange {
                owner_address: transaction.from,
                resource,
                amount, // Absolute total after operation
                expiration_ms,
                v2_model: false, // UnfreezeBalanceContract is V1 model
            };

            info!(
                "Emitting unfreeze change: owner={}, resource={:?}, amount={}, expiration={}",
                tron_backend_common::to_tron_address(&transaction.from),
                resource,
                amount,
                expiration_ms
            );

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
            // Read current global totals from DynamicProperties (already updated in-buffer).
            let total_net_weight = storage_adapter.get_total_net_weight()
                .map_err(|e| format!("Failed to get total net weight: {}", e))?;
            let total_net_limit = storage_adapter.get_total_net_limit()
                .map_err(|e| format!("Failed to get total net limit: {}", e))?;
            let total_energy_weight = storage_adapter.get_total_energy_weight()
                .map_err(|e| format!("Failed to get total energy weight: {}", e))?;
            let total_energy_limit = storage_adapter.get_total_energy_limit()
                .map_err(|e| format!("Failed to get total energy limit: {}", e))?;

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

        // Build Transaction.Result with unfreeze_amount for receipt passthrough
        let tron_transaction_result = TransactionResultBuilder::new()
            .with_unfreeze_amount(unfreeze_amount)
            .build();

        debug!("UnfreezeBalance completed successfully: state_changes=1, energy_used=0, bandwidth_used={}, freeze_ledger_updated=true, freeze_changes={}, global_changes={}, tron_transaction_result_len={}",
               bandwidth_used, freeze_changes.len(), global_resource_changes.len(), tron_transaction_result.len());

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
            trc10_changes: vec![], // Not applicable for freeze contracts
            vote_changes: vec![], // Not applicable for freeze contracts
            withdraw_changes: vec![], // Not applicable for freeze contracts
            tron_transaction_result: Some(tron_transaction_result), // Phase 0.4: Receipt passthrough with unfreeze_amount
            contract_address: None, // Not applicable for freeze contracts
        })
    }

    /// Parse UnfreezeBalanceContract parameters from protobuf-encoded data
    ///
    /// UnfreezeBalanceContract protobuf structure:
    /// - owner_address: bytes (field 1) - we get this from transaction.from
    /// - resource: ResourceCode enum (field 10)
    /// - receiver_address: bytes (field 15) - optional, Phase 1 ignores
    pub(crate) fn parse_unfreeze_balance_params(data: &revm_primitives::Bytes) -> Result<UnfreezeParams, String> {
        // Simple protobuf parser for the specific fields we need
        let mut resource: FreezeResource = FreezeResource::Bandwidth; // Default
        let mut resource_raw: i64 = 0; // Track raw value for Java-parity error messages
        let mut receiver_address = Vec::new();
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
                    // Java defers validation of unknown values to validate() method, so we don't fail early.
                    if wire_type != 0 { return Err("Invalid wire type for resource".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    resource_raw = value as i64;
                    resource = match value {
                        0 => FreezeResource::Bandwidth,
                        1 => FreezeResource::Energy,
                        2 => FreezeResource::TronPower,
                        _ => FreezeResource::Unknown,
                    };
                    pos = pos + new_pos;
                },
                15 => {
                    // receiver_address (bytes)
                    if wire_type != 2 { return Err("Invalid wire type for receiver_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos;
                    let len = len as usize;
                    if pos + len > data.len() {
                        return Err("Invalid receiver_address length".to_string());
                    }
                    receiver_address = data[pos..pos + len].to_vec();
                    pos = pos + len;
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

        Ok(UnfreezeParams { resource, resource_raw, receiver_address })
    }

    /// Execute a FREEZE_BALANCE_V2_CONTRACT (Phase 2: with freeze ledger changes)
    /// Handles V2 freeze which uses FrozenV2 list instead of single Frozen field
    pub(crate) fn execute_freeze_balance_v2_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        info!(
            "Executing FREEZE_BALANCE_V2_CONTRACT: owner={}, data_len={}",
            tron_backend_common::to_tron_address(&transaction.from),
            transaction.data.len()
        );

        // Java parity: Check type_url in Any wrapper.
        // If contract_parameter is provided, validate it matches the expected type.
        if let Some(ref param) = transaction.metadata.contract_parameter {
            if !param.type_url.ends_with("FreezeBalanceV2Contract") {
                return Err(format!(
                    "contract type error,expected type [FreezeBalanceV2Contract],real type[{}]",
                    param.type_url
                ));
            }
        }

        // === Validation (match java-tron FreezeBalanceV2Actuator messages) ===
        if !storage_adapter
            .support_unfreeze_delay()
            .map_err(|e| format!("Failed to read UNFREEZE_DELAY_DAYS: {}", e))?
        {
            return Err(
                "Not support FreezeV2 transaction, need to be opened by the committee".to_string(),
            );
        }

        // Parse freeze V2 parameters from transaction data
        let params = Self::parse_freeze_balance_v2_params(&transaction.data)?;

        debug!(
            "Parsed freeze V2 params: owner_len={}, frozen_balance={}, resource={:?}",
            params.owner_address.len(),
            params.frozen_balance,
            params.resource
        );

        let prefix = storage_adapter.address_prefix();
        if params.owner_address.len() != 21 || params.owner_address[0] != prefix {
            return Err("Invalid address".to_string());
        }
        let readable_owner_address = hex::encode(&params.owner_address);
        let owner_address = Address::from_slice(&params.owner_address[1..]);

        // Load owner proto (we must update frozen_v2 fields for fixture parity).
        let owner_proto = storage_adapter
            .get_account_proto(&owner_address)
            .map_err(|e| format!("Failed to load owner account proto: {}", e))?
            .ok_or_else(|| format!("Account[{}] not exists", readable_owner_address))?;

        // Load owner account info view for CSV parity state change tracking.
        let owner_account = storage_adapter
            .get_account(&owner_address)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .unwrap_or_default();

        if params.frozen_balance <= 0 {
            return Err("frozenBalance must be positive".to_string());
        }

        if params.frozen_balance < super::super::TRX_PRECISION as i64 {
            return Err("frozenBalance must be greater than or equal to 1 TRX".to_string());
        }

        if params.frozen_balance > owner_proto.balance {
            return Err("frozenBalance must be less than or equal to accountBalance".to_string());
        }

        let allow_new_resource_model = storage_adapter
            .support_allow_new_resource_model()
            .map_err(|e| format!("Failed to read ALLOW_NEW_RESOURCE_MODEL: {}", e))?;
        let resource = match params.resource {
            Some(FreezeResource::Bandwidth) | Some(FreezeResource::Energy) => {
                params.resource.unwrap()
            }
            Some(FreezeResource::TronPower) => {
                if !allow_new_resource_model {
                    return Err("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]".to_string());
                }
                FreezeResource::TronPower
            }
            Some(FreezeResource::Unknown) => {
                // Unknown resource codes: match Java's default case exactly.
                if allow_new_resource_model {
                    return Err("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY、TRON_POWER]".to_string());
                } else {
                    return Err("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]".to_string());
                }
            }
            None => {
                if allow_new_resource_model {
                    return Err(
                        "ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY、TRON_POWER]"
                            .to_string(),
                    );
                }
                return Err("ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]".to_string());
            }
        };

        // Apply state changes to the Account proto.
        let mut new_owner_proto = owner_proto.clone();

        // Java: initialize oldTronPower when the new resource model is enabled and oldTronPower==0.
        if allow_new_resource_model && new_owner_proto.old_tron_power == 0 {
            let tron_power = storage_adapter
                .get_tron_power_in_sun(&owner_address, false)
                .map_err(|e| format!("Failed to compute tron power: {}", e))?;
            new_owner_proto.old_tron_power = if tron_power == 0 {
                -1
            } else {
                tron_power
                    .try_into()
                    .map_err(|_| "tron power exceeds i64::MAX".to_string())?
            };
        }

        // Weight deltas are computed on (frozenV2 + delegatedV2) totals.
        fn frozen_v2_sum(account: &tron_backend_execution::protocol::Account, r#type: i32) -> i64 {
            account
                .frozen_v2
                .iter()
                .filter(|f| f.r#type == r#type)
                .map(|f| f.amount)
                .sum()
        }

        fn frozen_v2_with_delegated(
            account: &tron_backend_execution::protocol::Account,
            resource: FreezeResource,
        ) -> i64 {
            match resource {
                FreezeResource::Bandwidth => {
                    frozen_v2_sum(account, 0) + account.delegated_frozen_v2_balance_for_bandwidth
                }
                FreezeResource::Energy => {
                    let delegated = account
                        .account_resource
                        .as_ref()
                        .map(|r| r.delegated_frozen_v2_balance_for_energy)
                        .unwrap_or(0);
                    frozen_v2_sum(account, 1) + delegated
                }
                FreezeResource::TronPower => frozen_v2_sum(account, 2),
                FreezeResource::Unknown => 0, // Unreachable: Unknown is rejected during validation
            }
        }

        let old_weight = frozen_v2_with_delegated(&owner_proto, resource)
            / super::super::TRX_PRECISION as i64;

        // Update frozen_v2 list (aggregate by resource type).
        let resource_type = resource as i32;
        let mut updated = false;
        for freeze_entry in new_owner_proto.frozen_v2.iter_mut() {
            if freeze_entry.r#type == resource_type {
                freeze_entry.amount = freeze_entry
                    .amount
                    .checked_add(params.frozen_balance)
                    .ok_or("Frozen balance overflow")?;
                updated = true;
                break;
            }
        }
        if !updated {
            new_owner_proto
                .frozen_v2
                .push(tron_backend_execution::protocol::account::FreezeV2 {
                    r#type: resource_type,
                    amount: params.frozen_balance,
                });
        }

        let new_weight = frozen_v2_with_delegated(&new_owner_proto, resource)
            / super::super::TRX_PRECISION as i64;
        let weight_delta = new_weight - old_weight;

        match resource {
            FreezeResource::Bandwidth => storage_adapter
                .add_total_net_weight(weight_delta)
                .map_err(|e| format!("Failed to update total net weight: {}", e))?,
            FreezeResource::Energy => storage_adapter
                .add_total_energy_weight(weight_delta)
                .map_err(|e| format!("Failed to update total energy weight: {}", e))?,
            FreezeResource::TronPower => storage_adapter
                .add_total_tron_power_weight(weight_delta)
                .map_err(|e| format!("Failed to update total tron power weight: {}", e))?,
            FreezeResource::Unknown => {
                // Unreachable: Unknown is rejected during validation
            }
        }

        // Update balance last (no effect on weights).
        new_owner_proto.balance = owner_proto
            .balance
            .checked_sub(params.frozen_balance)
            .ok_or("Balance underflow")?;

        // Persist updated owner proto.
        storage_adapter
            .put_account_proto(&owner_address, &new_owner_proto)
            .map_err(|e| format!("Failed to persist owner account proto: {}", e))?;

        // Keep the Rust-side freeze ledger updated (not part of Java DB layout).
        let freeze_amount = params.frozen_balance as u64;
        let default_duration_millis = 3 * 86400 * 1000; // 3 days
        let expiration_timestamp = (context.block_timestamp + default_duration_millis) as i64;

        // Add to freeze ledger (aggregates if previous freeze exists)
        storage_adapter
            .add_freeze_amount(
                owner_address,
                resource as u8,
                freeze_amount,
                expiration_timestamp,
            )
            .map_err(|e| format!("Failed to persist freeze record: {}", e))?;

        // Build state change for CSV parity.
        let mut new_owner = owner_account.clone();
        new_owner.balance = U256::from(new_owner_proto.balance as u64);

        // Emit exactly one state change for CSV parity
        let state_changes = vec![
            TronStateChange::AccountChange {
                address: owner_address,
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
                &owner_address,
                resource as u8
            ).map_err(|e| format!("Failed to read freeze record: {}", e))?;

            if let Some(record) = freeze_record {
                // Map FreezeResource to FreezeLedgerResource
                use tron_backend_execution::FreezeLedgerResource;
                let freeze_ledger_resource = match resource {
                    FreezeResource::Bandwidth => FreezeLedgerResource::Bandwidth,
                    FreezeResource::Energy => FreezeLedgerResource::Energy,
                    FreezeResource::TronPower => FreezeLedgerResource::TronPower,
                    FreezeResource::Unknown => {
                        // Unreachable: Unknown is rejected during validation
                        return Err("Unknown resource code".to_string());
                    }
                };

                let change = tron_backend_execution::FreezeLedgerChange {
                    owner_address,
                    resource: freeze_ledger_resource,
                    amount: record.frozen_amount as i64, // Absolute total after operation
                    expiration_ms: record.expiration_timestamp,  // Latest expiration
                    v2_model: true, // FreezeBalanceV2Contract is V2 model
                };

                info!("Emitting freeze V2 change: owner={}, resource={:?}, amount={}, expiration={}",
                      tron_backend_common::to_tron_address(&owner_address),
                      freeze_ledger_resource, record.frozen_amount, record.expiration_timestamp);

                vec![change]
            } else {
                // No record found - this shouldn't happen since we just added it
                warn!("Freeze record not found after add_freeze_amount for owner={}, resource={:?}",
                      tron_backend_common::to_tron_address(&owner_address), resource);
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
            // Read current global totals from DynamicProperties (already updated in-buffer).
            let total_net_weight = storage_adapter.get_total_net_weight()
                .map_err(|e| format!("Failed to get total net weight: {}", e))?;
            let total_net_limit = storage_adapter.get_total_net_limit()
                .map_err(|e| format!("Failed to get total net limit: {}", e))?;
            let total_energy_weight = storage_adapter.get_total_energy_weight()
                .map_err(|e| format!("Failed to get total energy weight: {}", e))?;
            let total_energy_limit = storage_adapter.get_total_energy_limit()
                .map_err(|e| format!("Failed to get total energy limit: {}", e))?;

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
            trc10_changes: vec![], // Not applicable for freeze contracts
            vote_changes: vec![], // Not applicable for freeze contracts
            withdraw_changes: vec![], // Not applicable for freeze contracts
            tron_transaction_result: None,
            contract_address: None,
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
        info!(
            "Executing UNFREEZE_BALANCE_V2_CONTRACT: owner={}, data_len={}",
            tron_backend_common::to_tron_address(&transaction.from),
            transaction.data.len()
        );

        // Java parity: Check type_url in Any wrapper.
        // If contract_parameter is provided, validate it matches the expected type.
        if let Some(ref param) = transaction.metadata.contract_parameter {
            if !param.type_url.ends_with("UnfreezeBalanceV2Contract") {
                return Err(format!(
                    "contract type error, expected type [UnfreezeBalanceV2Contract], real type[{}]",
                    param.type_url
                ));
            }
        }

        // Parse unfreeze V2 parameters from transaction data
        let params = Self::parse_unfreeze_balance_v2_params(&transaction.data)?;

        debug!(
            "Parsed unfreeze V2 params: unfreeze_balance={}, resource={:?}",
            params.unfreeze_balance, params.resource
        );

        // === Validation (match java-tron UnfreezeBalanceV2Actuator messages) ===
        if !storage_adapter
            .support_unfreeze_delay()
            .map_err(|e| format!("Failed to read UNFREEZE_DELAY_DAYS: {}", e))?
        {
            return Err(
                "Not support UnfreezeV2 transaction, need to be opened by the committee"
                    .to_string(),
            );
        }

        // java-tron: DecodeUtil.addressValid(ownerAddress)
        let prefix = storage_adapter.address_prefix();
        let owner_raw = transaction.metadata.from_raw.as_deref().unwrap_or(&[]);
        if owner_raw.len() != 21 || owner_raw[0] != prefix {
            return Err("Invalid address".to_string());
        }
        let readable_owner_address = hex::encode(owner_raw);

        // Load owner proto (we must update frozenV2/unfrozenV2 fields for fixture parity).
        let owner_proto = storage_adapter
            .get_account_proto(&transaction.from)
            .map_err(|e| format!("Failed to load owner account proto: {}", e))?
            .ok_or_else(|| {
                format!("Account[{}] does not exist", readable_owner_address)
            })?;

        let owner_account = storage_adapter
            .get_account(&transaction.from)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .unwrap_or_default();

        let allow_new_resource_model = storage_adapter
            .support_allow_new_resource_model()
            .map_err(|e| format!("Failed to read ALLOW_NEW_RESOURCE_MODEL: {}", e))?;

        fn frozen_v2_sum(
            account: &tron_backend_execution::protocol::Account,
            r#type: i32,
        ) -> i64 {
            account
                .frozen_v2
                .iter()
                .filter(|f| f.r#type == r#type)
                .map(|f| f.amount)
                .sum()
        }

        fn frozen_v2_with_delegated(
            account: &tron_backend_execution::protocol::Account,
            resource: FreezeResource,
        ) -> i64 {
            match resource {
                FreezeResource::Bandwidth => {
                    frozen_v2_sum(account, 0) + account.delegated_frozen_v2_balance_for_bandwidth
                }
                FreezeResource::Energy => {
                    let delegated = account
                        .account_resource
                        .as_ref()
                        .map(|r| r.delegated_frozen_v2_balance_for_energy)
                        .unwrap_or(0);
                    frozen_v2_sum(account, 1) + delegated
                }
                FreezeResource::TronPower => frozen_v2_sum(account, 2),
                FreezeResource::Unknown => 0, // Unreachable: Unknown is rejected during validation
            }
        }

        // ResourceCode validation (java-tron: switch (contract.getResource())).
        let resource = match params.resource {
            Some(FreezeResource::Unknown) => {
                // Unknown resource codes: match Java's default case exactly.
                return Err(if allow_new_resource_model {
                    "ResourceCode error.valid ResourceCode[BANDWIDTH、Energy、TRON_POWER]".to_string()
                } else {
                    "ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]".to_string()
                });
            }
            Some(r) => r,
            None => {
                return Err(if allow_new_resource_model {
                    "ResourceCode error.valid ResourceCode[BANDWIDTH、Energy、TRON_POWER]".to_string()
                } else {
                    "ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]".to_string()
                });
            }
        };

        let resource_type = resource as i32;
        let frozen_amount = owner_proto
            .frozen_v2
            .iter()
            .find(|f| f.r#type == resource_type)
            .map(|f| f.amount)
            .unwrap_or(0);

        match resource {
            FreezeResource::Bandwidth => {
                if frozen_amount <= 0 {
                    return Err("no frozenBalance(BANDWIDTH)".to_string());
                }
            }
            FreezeResource::Energy => {
                if frozen_amount <= 0 {
                    return Err("no frozenBalance(Energy)".to_string());
                }
            }
            FreezeResource::TronPower => {
                if !allow_new_resource_model {
                    return Err(
                        "ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]".to_string(),
                    );
                }
                if frozen_amount <= 0 {
                    return Err("no frozenBalance(TronPower)".to_string());
                }
            }
            FreezeResource::Unknown => {
                // Unreachable: Unknown is rejected during validation earlier
            }
        }

        // Validation: unfreeze_balance must be positive and <= frozenAmount.
        if params.unfreeze_balance <= 0 || params.unfreeze_balance > frozen_amount {
            return Err(format!(
                "Invalid unfreeze_balance, [{}] is error",
                params.unfreeze_balance
            ));
        }

        let now = storage_adapter
            .get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to read latest_block_header_timestamp: {}", e))?;

        // Validation: unfreezing times must be < 32 (java-tron: UNFREEZE_MAX_TIMES).
        let unfreezing_count = owner_proto
            .unfrozen_v2
            .iter()
            .filter(|u| u.unfreeze_expire_time > now)
            .count();
        if unfreezing_count >= 32 {
            return Err("Invalid unfreeze operation, unfreezing times is over limit".to_string());
        }

        // Validation succeeded; apply state changes.
        let unfreeze_delay_days = storage_adapter
            .get_unfreeze_delay_days()
            .map_err(|e| format!("Failed to read UNFREEZE_DELAY_DAYS: {}", e))?;
        let delay_ms = unfreeze_delay_days
            .checked_mul(86_400_000)
            .ok_or("Overflow computing unfreeze delay")?;
        let unfreeze_expire_time = now
            .checked_add(delay_ms)
            .ok_or("Overflow computing unfreeze expire time")?;

        let mut new_owner_proto = owner_proto.clone();

        // Java: initialize oldTronPower when the new resource model is enabled and oldTronPower==0.
        if allow_new_resource_model && new_owner_proto.old_tron_power == 0 {
            let tron_power = storage_adapter
                .get_tron_power_in_sun(&transaction.from, false)
                .map_err(|e| format!("Failed to compute tron power: {}", e))?;
            new_owner_proto.old_tron_power = if tron_power == 0 {
                -1
            } else {
                tron_power
                    .try_into()
                    .map_err(|_| "tron power exceeds i64::MAX".to_string())?
            };
        }

        // Sweep expired unfrozenV2 entries into balance.
        let mut withdraw_expire_amount: i64 = 0;
        let mut remaining_unfrozen: Vec<tron_backend_execution::protocol::account::UnFreezeV2> =
            Vec::with_capacity(new_owner_proto.unfrozen_v2.len());
        for entry in new_owner_proto.unfrozen_v2.iter() {
            if entry.unfreeze_expire_time <= now {
                withdraw_expire_amount = withdraw_expire_amount
                    .checked_add(entry.unfreeze_amount)
                    .ok_or("Overflow calculating withdraw_expire_amount")?;
            } else {
                remaining_unfrozen.push(entry.clone());
            }
        }

        if withdraw_expire_amount > 0 {
            new_owner_proto.balance = new_owner_proto
                .balance
                .checked_add(withdraw_expire_amount)
                .ok_or("Balance overflow")?;
        }
        new_owner_proto.unfrozen_v2 = remaining_unfrozen;

        // Update frozen_v2 list (java-tron: AccountCapsule.addFrozenBalanceForResource).
        //
        // Important fixture parity detail: java-tron keeps the FreezeV2 entry even when
        // its amount becomes 0 (it does not remove it from the list).
        let remaining_frozen = frozen_amount - params.unfreeze_balance;
        if let Some(existing) = new_owner_proto
            .frozen_v2
            .iter_mut()
            .find(|f| f.r#type == resource_type)
        {
            existing.amount = existing
                .amount
                .checked_sub(params.unfreeze_balance)
                .ok_or("Overflow subtracting frozenV2 amount")?;
        } else {
            return Err(format!(
                "no frozenBalance({})",
                match resource {
                    FreezeResource::Bandwidth => "BANDWIDTH",
                    FreezeResource::Energy => "Energy",
                    FreezeResource::TronPower => "TronPower",
                    FreezeResource::Unknown => "Unknown",
                }
            ));
        }

        // Append new pending unfreezeV2 entry.
        new_owner_proto
            .unfrozen_v2
            .push(tron_backend_execution::protocol::account::UnFreezeV2 {
                r#type: resource_type,
                unfreeze_amount: params.unfreeze_balance,
                unfreeze_expire_time,
            });

        // Update total resource weights in DynamicPropertiesStore (delta-based).
        let old_weight = frozen_v2_with_delegated(&owner_proto, resource)
            / super::super::TRX_PRECISION as i64;
        let new_weight = frozen_v2_with_delegated(&new_owner_proto, resource)
            / super::super::TRX_PRECISION as i64;
        let weight_delta = new_weight - old_weight;

        match resource {
            FreezeResource::Bandwidth => storage_adapter
                .add_total_net_weight(weight_delta)
                .map_err(|e| format!("Failed to update total net weight: {}", e))?,
            FreezeResource::Energy => storage_adapter
                .add_total_energy_weight(weight_delta)
                .map_err(|e| format!("Failed to update total energy weight: {}", e))?,
            FreezeResource::TronPower => storage_adapter
                .add_total_tron_power_weight(weight_delta)
                .map_err(|e| format!("Failed to update total tron power weight: {}", e))?,
            FreezeResource::Unknown => {
                // Unreachable: Unknown is rejected during validation
            }
        }

        // === Update votes (java-tron: UnfreezeBalanceV2Actuator#updateVote) ===
        if !new_owner_proto.votes.is_empty() {
            // java-tron: if supportAllowNewResourceModel, handle migration clearing.
            if allow_new_resource_model {
                if new_owner_proto.old_tron_power == -1 {
                    match resource {
                        FreezeResource::Bandwidth | FreezeResource::Energy => {
                            // there is no need to change votes
                        }
                        FreezeResource::TronPower | FreezeResource::Unknown => {
                            // continue to possible rescaling below
                        }
                    }
                } else {
                    // clear all votes at once when new resource model start
                    let existing_votes = storage_adapter
                        .get_votes(&transaction.from)
                        .map_err(|e| format!("Failed to load votes record: {}", e))?;
                    let mut votes_record = match existing_votes {
                        Some(v) => v,
                        None => {
                            let mut old_votes = Vec::with_capacity(new_owner_proto.votes.len());
                            for vote in new_owner_proto.votes.iter() {
                                let addr_bytes = vote.vote_address.as_slice();
                                let evm_bytes = if addr_bytes.len() == 21
                                    && (addr_bytes[0] == 0x41 || addr_bytes[0] == 0xa0)
                                {
                                    &addr_bytes[1..]
                                } else if addr_bytes.len() == 20 {
                                    addr_bytes
                                } else {
                                    continue;
                                };
                                if vote.vote_count < 0 {
                                    continue;
                                }
                                old_votes.push(tron_backend_execution::Vote::new(
                                    Address::from_slice(evm_bytes),
                                    vote.vote_count as u64,
                                ));
                            }
                            VotesRecord::new(transaction.from, old_votes, Vec::new())
                        }
                    };

                    votes_record.clear_new_votes();
                    storage_adapter
                        .set_votes(transaction.from, &votes_record)
                        .map_err(|e| format!("Failed to persist votes record: {}", e))?;
                    new_owner_proto.votes.clear();
                }
            }

            // If votes are still present after the migration logic, consider rescaling.
            if !new_owner_proto.votes.is_empty() {
                let total_vote: i64 = new_owner_proto
                    .votes
                    .iter()
                    .map(|v| v.vote_count)
                    .sum();
                if total_vote > 0 {
                    // Compute owned tron power after the unfreeze (java-tron: getTronPower/getAllTronPower).
                    fn tron_power_in_sun(account: &tron_backend_execution::protocol::Account) -> i128 {
                        let mut tp: i128 = 0;
                        for frozen in &account.frozen {
                            tp = tp.saturating_add(frozen.frozen_balance as i128);
                        }
                        if let Some(res) = account.account_resource.as_ref() {
                            if let Some(frozen_energy) = res.frozen_balance_for_energy.as_ref() {
                                tp = tp.saturating_add(frozen_energy.frozen_balance as i128);
                            }
                            tp = tp.saturating_add(res.delegated_frozen_balance_for_energy as i128);
                            tp = tp.saturating_add(res.delegated_frozen_v2_balance_for_energy as i128);
                        }
                        tp = tp.saturating_add(account.delegated_frozen_balance_for_bandwidth as i128);
                        tp = tp.saturating_add(account.delegated_frozen_v2_balance_for_bandwidth as i128);

                        const TRON_POWER_TYPE: i32 =
                            tron_backend_execution::protocol::ResourceCode::TronPower as i32;
                        for frozen_v2 in &account.frozen_v2 {
                            if frozen_v2.r#type != TRON_POWER_TYPE {
                                tp = tp.saturating_add(frozen_v2.amount as i128);
                            }
                        }
                        tp
                    }

                    fn all_tron_power_in_sun(
                        account: &tron_backend_execution::protocol::Account,
                    ) -> i128 {
                        const TRON_POWER_TYPE: i32 =
                            tron_backend_execution::protocol::ResourceCode::TronPower as i32;
                        let base = tron_power_in_sun(account);
                        let tp_frozen_balance: i128 = account
                            .tron_power
                            .as_ref()
                            .map(|f| f.frozen_balance as i128)
                            .unwrap_or(0);
                        let tp_frozen_v2_balance: i128 = account
                            .frozen_v2
                            .iter()
                            .filter(|f| f.r#type == TRON_POWER_TYPE)
                            .map(|f| f.amount as i128)
                            .sum();
                        let tp_frozen_total = tp_frozen_balance.saturating_add(tp_frozen_v2_balance);

                        match account.old_tron_power {
                            -1 => tp_frozen_total,
                            0 => base.saturating_add(tp_frozen_total),
                            v if v > 0 => (v as i128).saturating_add(tp_frozen_total),
                            _ => tp_frozen_total,
                        }
                    }

                    let owned_tron_power_sun: i128 = if allow_new_resource_model {
                        all_tron_power_in_sun(&new_owner_proto)
                    } else {
                        tron_power_in_sun(&new_owner_proto)
                    };

                    let required_tron_power_sun: i128 = (total_vote as i128)
                        .saturating_mul(super::super::TRX_PRECISION as i128);
                    if owned_tron_power_sun < required_tron_power_sun {
                        let existing_votes = storage_adapter
                            .get_votes(&transaction.from)
                            .map_err(|e| format!("Failed to load votes record: {}", e))?;
                        let mut votes_record = match existing_votes {
                            Some(v) => v,
                            None => {
                                let mut old_votes = Vec::with_capacity(new_owner_proto.votes.len());
                                for vote in new_owner_proto.votes.iter() {
                                    let addr_bytes = vote.vote_address.as_slice();
                                    let evm_bytes = if addr_bytes.len() == 21
                                        && (addr_bytes[0] == 0x41 || addr_bytes[0] == 0xa0)
                                    {
                                        &addr_bytes[1..]
                                    } else if addr_bytes.len() == 20 {
                                        addr_bytes
                                    } else {
                                        continue;
                                    };
                                    if vote.vote_count < 0 {
                                        continue;
                                    }
                                    old_votes.push(tron_backend_execution::Vote::new(
                                        Address::from_slice(evm_bytes),
                                        vote.vote_count as u64,
                                    ));
                                }
                                VotesRecord::new(transaction.from, old_votes, Vec::new())
                            }
                        };

                        let mut scaled_votes_proto = Vec::new();
                        let mut scaled_votes_domain = Vec::new();
                        let owned_tp_f64 = owned_tron_power_sun as f64;
                        let total_vote_f64 = total_vote as f64;
                        let trx_precision_f64 = super::super::TRX_PRECISION as f64;

                        for vote in new_owner_proto.votes.iter() {
                            let vote_count = vote.vote_count;
                            if vote_count <= 0 {
                                continue;
                            }
                            let new_vote_count = ((vote_count as f64) / total_vote_f64
                                * owned_tp_f64
                                / trx_precision_f64) as i64;
                            if new_vote_count > 0 {
                                scaled_votes_proto.push(tron_backend_execution::protocol::Vote {
                                    vote_address: vote.vote_address.clone(),
                                    vote_count: new_vote_count,
                                });

                                let addr_bytes = vote.vote_address.as_slice();
                                let evm_bytes = if addr_bytes.len() == 21
                                    && (addr_bytes[0] == 0x41 || addr_bytes[0] == 0xa0)
                                {
                                    &addr_bytes[1..]
                                } else if addr_bytes.len() == 20 {
                                    addr_bytes
                                } else {
                                    continue;
                                };
                                scaled_votes_domain.push(tron_backend_execution::Vote::new(
                                    Address::from_slice(evm_bytes),
                                    new_vote_count as u64,
                                ));
                            }
                        }

                        votes_record.clear_new_votes();
                        votes_record.new_votes = scaled_votes_domain;
                        storage_adapter
                            .set_votes(transaction.from, &votes_record)
                            .map_err(|e| format!("Failed to persist votes record: {}", e))?;

                        new_owner_proto.votes.clear();
                        new_owner_proto.votes.extend(scaled_votes_proto);
                    }
                }
            }
        }

        // Java: invalidate oldTronPower under the new resource model after updating weights/votes.
        if allow_new_resource_model && new_owner_proto.old_tron_power != -1 {
            new_owner_proto.old_tron_power = -1;
        }

        // Persist updated owner proto.
        storage_adapter
            .put_account_proto(&transaction.from, &new_owner_proto)
            .map_err(|e| format!("Failed to persist owner account proto: {}", e))?;

        // Keep the Rust-side freeze ledger updated (not part of Java DB layout).
        let freeze_resource = resource as u8;
        if remaining_frozen > 0 {
            let existing_expiration = storage_adapter
                .get_freeze_record(&transaction.from, freeze_resource)
                .map_err(|e| format!("Failed to read freeze record: {}", e))?
                .map(|r| r.expiration_timestamp)
                .unwrap_or(0);
            let record =
                tron_backend_execution::FreezeRecord::new(remaining_frozen as u64, existing_expiration);
            storage_adapter
                .set_freeze_record(transaction.from, freeze_resource, &record)
                .map_err(|e| format!("Failed to persist freeze record: {}", e))?;
        } else {
            storage_adapter
                .remove_freeze_record(&transaction.from, freeze_resource)
                .map_err(|e| format!("Failed to remove freeze record: {}", e))?;
        }

        // Emit exactly one state change for CSV parity.
        let mut new_owner = owner_account.clone();
        new_owner.balance = U256::from(new_owner_proto.balance as u64);
        let state_changes = vec![TronStateChange::AccountChange {
            address: transaction.from,
            old_account: Some(owner_account),
            new_account: Some(new_owner),
        }];

        // Phase 2: Emit freeze ledger changes when enabled
        let emit_freeze_changes = self.get_execution_config()
            .ok()
            .map(|cfg| cfg.remote.emit_freeze_ledger_changes)
            .unwrap_or(false);

        let freeze_changes = if emit_freeze_changes {
            // Read back the updated freeze record to get absolute amount
            let updated_record = storage_adapter.get_freeze_record(
                &transaction.from,
                freeze_resource
            ).map_err(|e| format!("Failed to read updated freeze record: {}", e))?;

            use tron_backend_execution::FreezeLedgerResource;
            let resource = match resource {
                FreezeResource::Bandwidth => FreezeLedgerResource::Bandwidth,
                FreezeResource::Energy => FreezeLedgerResource::Energy,
                FreezeResource::TronPower => FreezeLedgerResource::TronPower,
                FreezeResource::Unknown => {
                    // Unreachable: Unknown is rejected during validation
                    return Err("Unknown resource code".to_string());
                }
            };

            let change = if remaining_frozen > 0 {
                let record = updated_record.ok_or("Freeze record missing after update")?;
                tron_backend_execution::FreezeLedgerChange {
                    owner_address: transaction.from,
                    resource,
                    amount: record.frozen_amount as i64,
                    expiration_ms: record.expiration_timestamp,
                    v2_model: true,
                }
            } else {
                tron_backend_execution::FreezeLedgerChange {
                    owner_address: transaction.from,
                    resource,
                    amount: 0,
                    expiration_ms: 0,
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
            // Read current global totals from DynamicProperties (already updated in-buffer).
            let total_net_weight = storage_adapter.get_total_net_weight()
                .map_err(|e| format!("Failed to get total net weight: {}", e))?;
            let total_net_limit = storage_adapter.get_total_net_limit()
                .map_err(|e| format!("Failed to get total net limit: {}", e))?;
            let total_energy_weight = storage_adapter.get_total_energy_weight()
                .map_err(|e| format!("Failed to get total energy weight: {}", e))?;
            let total_energy_limit = storage_adapter.get_total_energy_limit()
                .map_err(|e| format!("Failed to get total energy limit: {}", e))?;

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

        // UnfreezeBalanceV2 receipt: only emit withdraw_expire_amount when sweeping expired entries.
        let mut receipt_builder = TransactionResultBuilder::new();
        if withdraw_expire_amount > 0 {
            receipt_builder = receipt_builder.with_withdraw_expire_amount(withdraw_expire_amount);
        }
        let tron_transaction_result = receipt_builder.build();

        debug!("UnfreezeBalanceV2 completed successfully: state_changes=1, energy_used=0, bandwidth_used={}, freeze_ledger_updated=true, freeze_changes={}, global_changes={}, tron_transaction_result_len={}",
               bandwidth_used, freeze_changes.len(), global_resource_changes.len(), tron_transaction_result.len());

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
            trc10_changes: vec![], // Not applicable for freeze contracts
            vote_changes: vec![], // Not applicable for freeze contracts
            withdraw_changes: vec![], // Not applicable for freeze contracts
            tron_transaction_result: Some(tron_transaction_result), // Receipt passthrough (withdraw_expire_amount when present)
            contract_address: None, // Not applicable for freeze contracts
        })
    }

    /// Parse FreezeBalanceV2Contract parameters from protobuf-encoded data
    ///
    /// FreezeBalanceV2Contract protobuf structure:
    /// - owner_address: bytes (field 1) - we get this from transaction.from
    /// - frozen_balance: int64 (field 2)
    /// - resource: ResourceCode enum (field 3)
    pub(crate) fn parse_freeze_balance_v2_params(data: &revm_primitives::Bytes) -> Result<FreezeV2Params, String> {
        // Proto3 semantics: missing scalar fields read as 0.
        let mut owner_address: Vec<u8> = Vec::new();
        let mut frozen_balance: i64 = 0;
        let mut resource: Option<FreezeResource> = Some(FreezeResource::Bandwidth); // Default
        let mut pos = 0;

        while pos < data.len() {
            // Read tag
            let (tag, new_pos) = read_varint(&data[pos..])?;
            pos = pos + new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            match field_number {
                1 => {
                    // owner_address (bytes)
                    if wire_type != 2 { return Err("Invalid wire type for owner_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos = pos + new_pos;
                    let len = len as usize;
                    if pos + len > data.len() {
                        return Err("Invalid owner_address length".to_string());
                    }
                    owner_address = data[pos..pos + len].to_vec();
                    pos = pos + len;
                },
                2 => {
                    // frozen_balance (int64)
                    if wire_type != 0 { return Err("Invalid wire type for frozen_balance".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    frozen_balance = value as i64;
                    pos = pos + new_pos;
                },
                3 => {
                    // resource (enum ResourceCode)
                    if wire_type != 0 { return Err("Invalid wire type for resource".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    resource = match value {
                        0 => Some(FreezeResource::Bandwidth),
                        1 => Some(FreezeResource::Energy),
                        2 => Some(FreezeResource::TronPower),
                        _ => None,
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

        Ok(FreezeV2Params {
            owner_address,
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
        let mut resource: Option<FreezeResource> = Some(FreezeResource::Bandwidth); // Default
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
                        0 => Some(FreezeResource::Bandwidth),
                        1 => Some(FreezeResource::Energy),
                        2 => Some(FreezeResource::TronPower),
                        _ => None,
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

        // Proto3 semantics: missing scalar fields read as 0.
        let unfreeze_balance = unfreeze_balance.unwrap_or(0);

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
    /// - receiver_address: bytes (field 15) - optional (delegate freeze when supportDR is enabled)
    pub(crate) fn parse_freeze_balance_params(data: &revm_primitives::Bytes) -> Result<FreezeParams, String> {
        if data.is_empty() {
            return Err("FreezeBalance params cannot be empty".to_string());
        }

        // Simple protobuf parser for the specific fields we need
        // Protobuf wire format: tag (field_number << 3 | wire_type)
        // int64 uses wire_type 0 (varint)
        // bytes uses wire_type 2 (length-delimited)

        // Proto3 semantics: missing scalar fields read as 0.
        let mut frozen_balance: i64 = 0;
        let mut frozen_duration: i64 = 0;
        let mut resource: FreezeResource = FreezeResource::Bandwidth; // Default
        let mut resource_raw: i64 = 0; // Track raw value for Java-parity error messages
        let mut receiver_address: Vec<u8> = Vec::new();

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
                    frozen_balance = value as i64;
                    pos = pos + new_pos;
                },
                3 => {
                    // frozen_duration (int64)
                    if wire_type != 0 { return Err("Invalid wire type for frozen_duration".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    frozen_duration = value as i64;
                    pos = pos + new_pos;
                },
                10 => {
                    // resource (enum ResourceCode)
                    // Java defers validation of unknown values to validate() method, so we don't fail early.
                    if wire_type != 0 { return Err("Invalid wire type for resource".to_string()); }
                    let (value, new_pos) = read_varint(&data[pos..])?;
                    resource_raw = value as i64;
                    resource = match value {
                        0 => FreezeResource::Bandwidth,
                        1 => FreezeResource::Energy,
                        2 => FreezeResource::TronPower,
                        _ => FreezeResource::Unknown,
                    };
                    pos = pos + new_pos;
                },
                15 => {
                    // receiver_address (bytes)
                    if wire_type != 2 { return Err("Invalid wire type for receiver_address".to_string()); }
                    let (len, new_pos) = read_varint(&data[pos..])?;
                    pos += new_pos;
                    let len_usize = len as usize;
                    if pos + len_usize > data.len() {
                        return Err("receiver_address out of bounds".to_string());
                    }
                    receiver_address = data[pos..pos + len_usize].to_vec();
                    pos += len_usize;
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

        Ok(FreezeParams {
            frozen_balance,
            resource,
            resource_raw,
            frozen_duration,
            receiver_address,
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
