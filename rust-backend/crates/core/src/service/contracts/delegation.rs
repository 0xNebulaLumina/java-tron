// Delegation contract execution handlers
// This module contains handlers for DelegateResourceContract and UndelegateResourceContract
// Part of Phase 2 delegation parity implementation

use tracing::{debug, info, warn, error};
use revm_primitives::Address;

use tron_backend_execution::{
    TronTransaction, TronExecutionContext, TronExecutionResult, TronStateChange,
    DelegationChange, DelegationOp, EvmStateStore,
};

use crate::service::BackendService;
use crate::service::contracts::proto::read_varint;

/// Resource type constants (matching Java ResourceCode enum)
const RESOURCE_BANDWIDTH: u8 = 0;
const RESOURCE_ENERGY: u8 = 1;

/// Parsed DelegateResourceContract parameters
#[derive(Debug, Clone)]
pub struct DelegateResourceParams {
    /// Receiver address (who receives the delegated resources)
    pub receiver_address: Address,
    /// Resource type: 0=BANDWIDTH, 1=ENERGY
    pub resource: u8,
    /// Amount to delegate in SUN
    pub balance: i64,
    /// Lock period flag (V2 model)
    pub lock: bool,
    /// Lock period in milliseconds (if lock=true)
    pub lock_period: i64,
}

/// Parsed UnDelegateResourceContract parameters
#[derive(Debug, Clone)]
pub struct UnDelegateResourceParams {
    /// Receiver address (who had the delegated resources)
    pub receiver_address: Address,
    /// Resource type: 0=BANDWIDTH, 1=ENERGY
    pub resource: u8,
    /// Amount to undelegate in SUN
    pub balance: i64,
}

impl BackendService {
    /// Execute a DELEGATE_RESOURCE_CONTRACT
    /// Delegates frozen resources (bandwidth/energy) from owner to receiver
    ///
    /// Protobuf structure:
    /// message DelegateResourceContract {
    ///   bytes owner_address = 1;
    ///   ResourceCode resource = 2;
    ///   int64 balance = 3;
    ///   bytes receiver_address = 4;
    ///   bool lock = 5;
    ///   int64 lock_period = 6;
    /// }
    pub(crate) fn execute_delegate_resource_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        let execution_config = self.get_execution_config()?;
        let emit_delegation_changes = execution_config.remote.emit_delegation_changes;

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("DelegateResource owner={}", owner_tron);

        // 1. Parse DelegateResourceContract from transaction data
        let params = Self::parse_delegate_resource_contract(&transaction.data)?;

        let receiver_tron = tron_backend_common::to_tron_address(&params.receiver_address);
        info!(
            "DelegateResource: owner={}, receiver={}, resource={}, balance={} SUN, lock={}, lock_period={}ms",
            owner_tron, receiver_tron, params.resource, params.balance, params.lock, params.lock_period
        );

        // 2. Validate parameters
        if params.balance <= 0 {
            warn!("DelegateResource: balance must be positive, got {}", params.balance);
            return Err("delegated balance must be more than 0".to_string());
        }

        if params.resource != RESOURCE_BANDWIDTH && params.resource != RESOURCE_ENERGY {
            warn!("DelegateResource: invalid resource type {}", params.resource);
            return Err(format!("Invalid resource type: {}", params.resource));
        }

        // 3. Validate owner has sufficient frozen balance
        // Get freeze record for owner
        let freeze_record = storage_adapter.get_freeze_record(&owner, params.resource)
            .map_err(|e| format!("Failed to get freeze record: {}", e))?;

        let frozen_amount = freeze_record.map(|r| r.frozen_amount).unwrap_or(0);
        if (frozen_amount as i64) < params.balance {
            warn!("DelegateResource: insufficient frozen balance. have={}, need={}", frozen_amount, params.balance);
            return Err(format!(
                "frozen balance({}) < delegating balance({})",
                frozen_amount, params.balance
            ));
        }

        // 4. Validate receiver account exists
        let receiver_account = storage_adapter.get_account(&params.receiver_address)
            .map_err(|e| format!("Failed to get receiver account: {}", e))?;
        if receiver_account.is_none() {
            warn!("DelegateResource: receiver account {} not exist", receiver_tron);
            return Err(format!("Account {} not exist", receiver_tron));
        }

        // 5. Get owner account (for state change)
        let owner_account = storage_adapter.get_account(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or("Owner account not found")?;

        // 6. Calculate expiration time
        let expire_time_ms = if params.lock && params.lock_period > 0 {
            // Use block timestamp + lock_period
            (context.block_timestamp as i64) + params.lock_period
        } else {
            0 // No lock
        };

        // 7. Update delegation record
        let mut delegation_record = storage_adapter.get_delegation(&owner, &params.receiver_address, params.lock)
            .map_err(|e| format!("Failed to get delegation record: {}", e))?
            .unwrap_or_default();

        // Update record based on resource type
        match params.resource {
            RESOURCE_BANDWIDTH => {
                delegation_record.frozen_balance_for_bandwidth = delegation_record
                    .frozen_balance_for_bandwidth
                    .checked_add(params.balance)
                    .ok_or("Delegation bandwidth overflow")?;
                if params.lock {
                    delegation_record.expire_time_for_bandwidth = expire_time_ms;
                }
            }
            RESOURCE_ENERGY => {
                delegation_record.frozen_balance_for_energy = delegation_record
                    .frozen_balance_for_energy
                    .checked_add(params.balance)
                    .ok_or("Delegation energy overflow")?;
                if params.lock {
                    delegation_record.expire_time_for_energy = expire_time_ms;
                }
            }
            _ => unreachable!("Resource type already validated"),
        }

        // 8. Persist delegation record
        storage_adapter.set_delegation(&owner, &params.receiver_address, params.lock, &delegation_record)
            .map_err(|e| format!("Failed to set delegation record: {}", e))?;

        info!(
            "DelegateResource: persisted delegation record for owner={}, receiver={}, lock={}",
            owner_tron, receiver_tron, params.lock
        );

        // 9. Build state changes
        // For CSV parity: emit account change for owner (delegated totals change)
        let mut state_changes = Vec::new();
        state_changes.push(TronStateChange::AccountChange {
            address: owner,
            old_account: Some(owner_account.clone()),
            new_account: Some(owner_account), // Account balance unchanged, delegated fields change
        });

        // 10. Build delegation changes if enabled
        let mut delegation_changes = Vec::new();
        if emit_delegation_changes {
            delegation_changes.push(DelegationChange::new(
                owner,
                params.receiver_address,
                params.resource,
                params.balance,
                expire_time_ms,
                true, // V2 model
                DelegationOp::Add,
            ));

            info!(
                "DelegateResource: emitting DelegationChange(ADD) for resource={}, amount={}, expire={}",
                params.resource, params.balance, expire_time_ms
            );
        }

        // 11. Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        info!(
            "DelegateResource completed: owner={}, receiver={}, resource={}, amount={}, state_changes={}, delegation_changes={}",
            owner_tron, receiver_tron, params.resource, params.balance,
            state_changes.len(), delegation_changes.len()
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0, // System contracts use 0 energy
            bandwidth_used,
            state_changes,
            logs: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            delegation_changes,
        })
    }

    /// Execute an UNDELEGATE_RESOURCE_CONTRACT
    /// Cancels/reduces delegated resources from owner to receiver
    ///
    /// Protobuf structure:
    /// message UnDelegateResourceContract {
    ///   bytes owner_address = 1;
    ///   ResourceCode resource = 2;
    ///   int64 balance = 3;
    ///   bytes receiver_address = 4;
    /// }
    pub(crate) fn execute_undelegate_resource_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        let execution_config = self.get_execution_config()?;
        let emit_delegation_changes = execution_config.remote.emit_delegation_changes;

        let owner = transaction.from;
        let owner_tron = tron_backend_common::to_tron_address(&owner);

        info!("UnDelegateResource owner={}", owner_tron);

        // 1. Parse UnDelegateResourceContract from transaction data
        let params = Self::parse_undelegate_resource_contract(&transaction.data)?;

        let receiver_tron = tron_backend_common::to_tron_address(&params.receiver_address);
        info!(
            "UnDelegateResource: owner={}, receiver={}, resource={}, balance={} SUN",
            owner_tron, receiver_tron, params.resource, params.balance
        );

        // 2. Validate parameters
        if params.balance <= 0 {
            warn!("UnDelegateResource: balance must be positive, got {}", params.balance);
            return Err("delegated balance must be more than 0".to_string());
        }

        if params.resource != RESOURCE_BANDWIDTH && params.resource != RESOURCE_ENERGY {
            warn!("UnDelegateResource: invalid resource type {}", params.resource);
            return Err(format!("Invalid resource type: {}", params.resource));
        }

        // 3. Get owner account
        let owner_account = storage_adapter.get_account(&owner)
            .map_err(|e| format!("Failed to get owner account: {}", e))?
            .ok_or("Owner account not found")?;

        // 4. Check unlocked delegation record first
        let unlock_record = storage_adapter.get_delegation(&owner, &params.receiver_address, false)
            .map_err(|e| format!("Failed to get unlock delegation record: {}", e))?;

        let unlocked_amount = match params.resource {
            RESOURCE_BANDWIDTH => unlock_record.as_ref().map(|r| r.frozen_balance_for_bandwidth).unwrap_or(0),
            RESOURCE_ENERGY => unlock_record.as_ref().map(|r| r.frozen_balance_for_energy).unwrap_or(0),
            _ => 0,
        };

        // 5. Check if sufficient unlocked delegation exists
        if unlocked_amount < params.balance {
            // Check if we can undelegate from locked (only if not locked)
            // For simplicity in Phase 1, we require sufficient unlocked balance
            warn!(
                "UnDelegateResource: insufficient unlocked delegation. have={}, need={}",
                unlocked_amount, params.balance
            );
            return Err(format!(
                "Not enough unlocked balance for undelegate. available={}, requested={}",
                unlocked_amount, params.balance
            ));
        }

        // 6. Update delegation record
        let mut delegation_record = unlock_record.unwrap_or_default();

        match params.resource {
            RESOURCE_BANDWIDTH => {
                delegation_record.frozen_balance_for_bandwidth = delegation_record
                    .frozen_balance_for_bandwidth
                    .checked_sub(params.balance)
                    .ok_or("Delegation bandwidth underflow")?;
            }
            RESOURCE_ENERGY => {
                delegation_record.frozen_balance_for_energy = delegation_record
                    .frozen_balance_for_energy
                    .checked_sub(params.balance)
                    .ok_or("Delegation energy underflow")?;
            }
            _ => unreachable!("Resource type already validated"),
        }

        // 7. Persist or remove delegation record
        if delegation_record.is_empty() {
            storage_adapter.remove_delegation(&owner, &params.receiver_address, false)
                .map_err(|e| format!("Failed to remove delegation record: {}", e))?;
            info!("UnDelegateResource: removed empty delegation record");
        } else {
            storage_adapter.set_delegation(&owner, &params.receiver_address, false, &delegation_record)
                .map_err(|e| format!("Failed to update delegation record: {}", e))?;
            info!("UnDelegateResource: updated delegation record");
        }

        // 8. Build state changes
        let mut state_changes = Vec::new();
        state_changes.push(TronStateChange::AccountChange {
            address: owner,
            old_account: Some(owner_account.clone()),
            new_account: Some(owner_account),
        });

        // 9. Build delegation changes if enabled
        let mut delegation_changes = Vec::new();
        if emit_delegation_changes {
            delegation_changes.push(DelegationChange::new(
                owner,
                params.receiver_address,
                params.resource,
                params.balance,
                0, // No expiration for undelegate
                true, // V2 model
                DelegationOp::Remove,
            ));

            info!(
                "UnDelegateResource: emitting DelegationChange(REMOVE) for resource={}, amount={}",
                params.resource, params.balance
            );
        }

        // 10. Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        info!(
            "UnDelegateResource completed: owner={}, receiver={}, resource={}, amount={}, state_changes={}, delegation_changes={}",
            owner_tron, receiver_tron, params.resource, params.balance,
            state_changes.len(), delegation_changes.len()
        );

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used,
            state_changes,
            logs: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            delegation_changes,
        })
    }

    /// Parse DelegateResourceContract from protobuf bytes
    fn parse_delegate_resource_contract(data: &[u8]) -> Result<DelegateResourceParams, String> {
        let mut receiver_address: Option<Address> = None;
        let mut resource: u8 = 0;
        let mut balance: i64 = 0;
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
                (1, 2) => { // owner_address - skip, use transaction.from
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read owner_address length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 0) => { // resource (enum as varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read resource: {}", e))?;
                    pos += bytes_read;
                    resource = value as u8;
                }
                (3, 0) => { // balance (int64 as varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read balance: {}", e))?;
                    pos += bytes_read;
                    balance = value as i64;
                }
                (4, 2) => { // receiver_address (bytes)
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read receiver_address length: {}", e))?;
                    pos += bytes_read;

                    if pos + length as usize > data.len() {
                        return Err("Invalid receiver_address length".to_string());
                    }

                    let addr_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    // Convert 21-byte Tron address to 20-byte EVM address
                    let evm_addr = if addr_bytes.len() == 21 && addr_bytes[0] == 0x41 {
                        &addr_bytes[1..]
                    } else if addr_bytes.len() == 20 {
                        addr_bytes
                    } else {
                        return Err(format!("Invalid receiver_address length: {}", addr_bytes.len()));
                    };

                    let mut addr = [0u8; 20];
                    addr.copy_from_slice(evm_addr);
                    receiver_address = Some(Address::from(addr));
                }
                (5, 0) => { // lock (bool as varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read lock: {}", e))?;
                    pos += bytes_read;
                    lock = value != 0;
                }
                (6, 0) => { // lock_period (int64 as varint)
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read lock_period: {}", e))?;
                    pos += bytes_read;
                    lock_period = value as i64;
                }
                _ => {
                    // Skip unknown field
                    let bytes_skipped = Self::skip_protobuf_field_delegation(&data[pos..], wire_type)?;
                    pos += bytes_skipped;
                }
            }
        }

        let receiver = receiver_address.ok_or("Missing receiver_address")?;

        Ok(DelegateResourceParams {
            receiver_address: receiver,
            resource,
            balance,
            lock,
            lock_period,
        })
    }

    /// Parse UnDelegateResourceContract from protobuf bytes
    fn parse_undelegate_resource_contract(data: &[u8]) -> Result<UnDelegateResourceParams, String> {
        let mut receiver_address: Option<Address> = None;
        let mut resource: u8 = 0;
        let mut balance: i64 = 0;

        let mut pos = 0;

        while pos < data.len() {
            let (field_header, bytes_read) = read_varint(&data[pos..])
                .map_err(|e| format!("Failed to read field header: {}", e))?;
            pos += bytes_read;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => { // owner_address - skip
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read owner_address length: {}", e))?;
                    pos += bytes_read + length as usize;
                }
                (2, 0) => { // resource
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read resource: {}", e))?;
                    pos += bytes_read;
                    resource = value as u8;
                }
                (3, 0) => { // balance
                    let (value, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read balance: {}", e))?;
                    pos += bytes_read;
                    balance = value as i64;
                }
                (4, 2) => { // receiver_address
                    let (length, bytes_read) = read_varint(&data[pos..])
                        .map_err(|e| format!("Failed to read receiver_address length: {}", e))?;
                    pos += bytes_read;

                    if pos + length as usize > data.len() {
                        return Err("Invalid receiver_address length".to_string());
                    }

                    let addr_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    let evm_addr = if addr_bytes.len() == 21 && addr_bytes[0] == 0x41 {
                        &addr_bytes[1..]
                    } else if addr_bytes.len() == 20 {
                        addr_bytes
                    } else {
                        return Err(format!("Invalid receiver_address length: {}", addr_bytes.len()));
                    };

                    let mut addr = [0u8; 20];
                    addr.copy_from_slice(evm_addr);
                    receiver_address = Some(Address::from(addr));
                }
                _ => {
                    let bytes_skipped = Self::skip_protobuf_field_delegation(&data[pos..], wire_type)?;
                    pos += bytes_skipped;
                }
            }
        }

        let receiver = receiver_address.ok_or("Missing receiver_address")?;

        Ok(UnDelegateResourceParams {
            receiver_address: receiver,
            resource,
            balance,
        })
    }

    /// Skip a protobuf field based on wire type (delegation-specific helper to avoid name collision)
    fn skip_protobuf_field_delegation(data: &[u8], wire_type: u64) -> Result<usize, String> {
        match wire_type {
            0 => { // Varint
                let (_, bytes_read) = read_varint(data)
                    .map_err(|e| format!("Failed to skip varint: {}", e))?;
                Ok(bytes_read)
            }
            1 => { // 64-bit
                Ok(8)
            }
            2 => { // Length-delimited
                let (length, bytes_read) = read_varint(data)
                    .map_err(|e| format!("Failed to read length: {}", e))?;
                Ok(bytes_read + length as usize)
            }
            5 => { // 32-bit
                Ok(4)
            }
            _ => Err(format!("Unknown wire type: {}", wire_type))
        }
    }
}
