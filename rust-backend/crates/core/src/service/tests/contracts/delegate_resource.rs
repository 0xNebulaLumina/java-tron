//! DelegateResourceContract tests for "available FreezeV2" validation parity.
//!
//! Tests validate that Rust matches Java's `DelegateResourceActuator.validate()` behavior,
//! which constrains delegation by "available FreezeV2 after usage" rather than raw frozen balance.
//!
//! See: planning/review_again/DELEGATE_RESOURCE_CONTRACT.planning.md
//! See: planning/review_again/DELEGATE_RESOURCE_CONTRACT.todo.md

use super::super::super::*;
use super::common::{encode_varint, make_from_raw};
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
use tron_backend_execution::protocol::{Account, account::{AccountResource, FreezeV2}};
use revm_primitives::{Address, Bytes, U256, AccountInfo};
use tron_backend_common::{ModuleManager, ExecutionConfig, RemoteExecutionConfig};
use tron_backend_storage::StorageEngine;

/// Helper to seed all required dynamic properties for delegate resource tests.
fn seed_delegate_resource_properties(storage_engine: &StorageEngine) {
    let props_db = "properties";

    // Enable delegate resource
    storage_engine.put(props_db, b"ALLOW_DELEGATE_RESOURCE", &1i64.to_be_bytes()).unwrap();

    // Enable unfreeze delay (required for delegate resource)
    storage_engine.put(props_db, b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes()).unwrap();

    // Set global bandwidth totals (realistic values)
    // total_net_weight = 50B TRX = 50_000_000_000_000_000 SUN
    // total_net_limit = 43_200_000_000 (typical daily limit)
    storage_engine.put(props_db, b"TOTAL_NET_WEIGHT", &50_000_000_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_LIMIT", &43_200_000_000i64.to_be_bytes()).unwrap();

    // Set global energy totals (realistic values)
    // total_energy_weight = 50B TRX = 50_000_000_000_000_000 SUN
    // total_energy_current_limit = 90_000_000_000 (typical)
    storage_engine.put(props_db, b"TOTAL_ENERGY_WEIGHT", &50_000_000_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_CURRENT_LIMIT", &90_000_000_000i64.to_be_bytes()).unwrap();

    // Other required properties
    storage_engine.put(props_db, b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"ALLOW_BLACKHOLE_OPTIMIZATION", &1i64.to_be_bytes()).unwrap();
}

/// Helper to set the latest block header timestamp for slot calculation.
fn set_latest_block_timestamp(storage_engine: &StorageEngine, timestamp_ms: i64) {
    let props_db = "properties";
    // Java parity: stored under lowercase key "latest_block_header_timestamp".
    storage_engine.put(props_db, b"latest_block_header_timestamp", &timestamp_ms.to_be_bytes()).unwrap();
}

/// Build DelegateResourceContract protobuf data.
///
/// Protobuf format:
/// - Field 1: owner_address (bytes, length-delimited) - Java parity: must match DelegateResourceContract.owner_address
/// - Field 2: resource (varint) - 0=BANDWIDTH, 1=ENERGY
/// - Field 3: balance (varint) - amount to delegate in SUN
/// - Field 4: receiver_address (bytes, length-delimited)
/// - Field 5: lock (varint/bool) - optional
/// - Field 6: lock_period (varint) - optional
fn build_delegate_resource_proto(
    owner_address: &[u8],
    resource: i32,
    balance: i64,
    receiver_address: &[u8],
    lock: bool,
    lock_period: i64,
) -> Vec<u8> {
    let mut data = Vec::new();

    // Field 1: owner_address (length-delimited) - Java parity fix
    data.push((1 << 3) | 2);
    encode_varint(&mut data, owner_address.len() as u64);
    data.extend_from_slice(owner_address);

    // Field 2: resource (varint)
    data.push((2 << 3) | 0);
    encode_varint(&mut data, resource as u64);

    // Field 3: balance (varint)
    data.push((3 << 3) | 0);
    encode_varint(&mut data, balance as u64);

    // Field 4: receiver_address (length-delimited)
    data.push((4 << 3) | 2);
    encode_varint(&mut data, receiver_address.len() as u64);
    data.extend_from_slice(receiver_address);

    // Field 5: lock (varint/bool) - only if lock=true
    if lock {
        data.push((5 << 3) | 0);
        encode_varint(&mut data, 1);

        // Field 6: lock_period (varint) - only if lock=true and period specified
        if lock_period > 0 {
            data.push((6 << 3) | 0);
            encode_varint(&mut data, lock_period as u64);
        }
    }

    data
}

/// Create an Account proto with FreezeV2 balance and optional usage.
fn create_owner_account_with_freeze_v2(
    balance: i64,
    frozen_v2_amount: i64,
    resource: i32, // 0=BANDWIDTH, 1=ENERGY
    net_usage: i64,
    latest_consume_time: i64,
    net_window_size: i64,
    energy_usage: i64,
    latest_consume_time_for_energy: i64,
    energy_window_size: i64,
) -> Account {
    let frozen_v2 = vec![FreezeV2 {
        r#type: resource,
        amount: frozen_v2_amount,
    }];

    Account {
        balance,
        frozen_v2,
        net_usage,
        latest_consume_time,
        net_window_size,
        net_window_optimized: true, // Use optimized window
        account_resource: Some(AccountResource {
            energy_usage,
            latest_consume_time_for_energy,
            energy_window_size,
            energy_window_optimized: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Create a simple receiver account.
fn create_receiver_account(balance: i64) -> Account {
    Account {
        balance,
        r#type: 0, // Normal account, not contract
        ..Default::default()
    }
}

/// Create test service with delegate resource enabled.
fn new_delegate_resource_service() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            delegate_resource_enabled: true,
            undelegate_resource_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

// ============================================================================
// Test: BANDWIDTH delegation fails when raw frozen_v2 >= balance but
//       available (frozen_v2 - v2Usage) < balance
// ============================================================================

#[test]
fn test_delegate_resource_bandwidth_fails_when_usage_exceeds_available() {
    // Setup:
    // - Owner has 10 TRX frozen for BANDWIDTH
    // - Owner has significant net_usage that consumes most of the frozen balance
    // - Attempting to delegate 10 TRX should FAIL because available < 10 TRX

    let owner_address = Address::from([1u8; 20]);
    let receiver_address = Address::from([2u8; 20]);

    let frozen_v2_amount = 10_000_000i64; // 10 TRX frozen for bandwidth
    let delegate_balance = 10_000_000i64; // Try to delegate all 10 TRX

    // Set high net_usage (relative to frozen amount)
    // With realistic global totals, net_usage gets scaled to SUN usage.
    // Formula: netUsage = accountNetUsage * TRX_PRECISION * (totalNetWeight / totalNetLimit)
    // With our totals: scale_factor = 50_000_000_000_000_000 / 43_200_000_000 = ~1.157M
    // So net_usage of 10 => scaled ~11.57M SUN usage, which would exceed 10 TRX frozen.
    //
    // For this test, use a net_usage value that represents significant bandwidth consumption.
    // If net_usage=10, scaled = 10 * 1_000_000 * (50_000_000_000_000_000 / 43_200_000_000)
    //                       = 10 * 1_000_000 * 1_157_407
    //                       = 11_574_070_000_000 (way too high)
    // So even a small raw net_usage can result in huge scaled usage with these global ratios.
    //
    // For testing, let's use more balanced global totals:
    // total_net_weight = 100_000_000_000 (100B SUN = 100k TRX worth)
    // total_net_limit = 100_000_000_000 (same, so ratio = 1)
    // Then: netUsage = accountNetUsage * 1_000_000 * 1 = accountNetUsage * 1M
    // If we want v2_net_usage of 5M SUN to test partial availability,
    // we need raw net_usage of 5 (since 5 * 1M = 5M SUN)

    let net_usage = 8; // Will scale to 8M SUN usage (8 TRX)
    let current_slot = 1000000i64; // Current slot
    let latest_consume_time = current_slot; // Just consumed, no decay yet

    // Create storage and seed properties with balanced totals for clearer testing
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let props_db = "properties";
    storage_engine.put(props_db, b"ALLOW_DELEGATE_RESOURCE", &1i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes()).unwrap();

    // Use 1:1 ratio for easy calculation: netUsage * 1M = scaled usage
    storage_engine.put(props_db, b"TOTAL_NET_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_CURRENT_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();

    // Set timestamp for slot calculation (slot = timestamp_ms / 3000)
    let timestamp_ms = current_slot * 3000;
    storage_engine.put(props_db, b"latest_block_header_timestamp", &timestamp_ms.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Create owner account with frozen V2 and high net_usage
    let owner_account = create_owner_account_with_freeze_v2(
        100_000_000, // 100 TRX balance (irrelevant for this test)
        frozen_v2_amount, // 10 TRX frozen for bandwidth
        0, // BANDWIDTH
        net_usage, // 8 bandwidth units -> 8M SUN scaled usage -> 8 TRX used
        latest_consume_time, // No decay (just consumed)
        28800, // Default window size
        0, 0, 28800, // No energy usage
    );
    storage_adapter.put_account_proto(&owner_address, &owner_account).unwrap();

    // Create EVM account for owner
    let owner_evm = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_evm).unwrap();

    // Create receiver account
    let receiver_account = create_receiver_account(50_000_000);
    storage_adapter.put_account_proto(&receiver_address, &receiver_account).unwrap();
    let receiver_evm = AccountInfo {
        balance: U256::from(50_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(receiver_address, receiver_evm).unwrap();

    // Build transaction
    let owner_raw = make_from_raw(&owner_address);
    let receiver_raw = make_from_raw(&receiver_address);
    let proto_data = build_delegate_resource_proto(
        &owner_raw, // Java parity: owner_address from contract, not from_raw
        0, // BANDWIDTH
        delegate_balance, // 10 TRX
        &receiver_raw,
        false, // no lock
        0,
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(proto_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::DelegateResourceContract),
            asset_id: None,
            from_raw: Some(owner_raw.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: timestamp_ms as u64,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let service = new_delegate_resource_service();

    // Execute - should fail with "available FreezeBandwidthV2 balance" error
    let result = service.execute_delegate_resource_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err(), "Should fail when available balance < delegate balance. Got: {:?}", result);
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("available FreezeBandwidthV2 balance"),
        "Expected 'available FreezeBandwidthV2 balance' error, got: {}", err_msg
    );
}

#[test]
fn test_delegate_resource_bandwidth_succeeds_when_usage_allows_delegation() {
    // Setup:
    // - Owner has 10 TRX frozen for BANDWIDTH
    // - Owner has NO net_usage (usage=0)
    // - Attempting to delegate 5 TRX should SUCCEED

    let owner_address = Address::from([3u8; 20]);
    let receiver_address = Address::from([4u8; 20]);

    let frozen_v2_amount = 10_000_000i64; // 10 TRX frozen for bandwidth
    let delegate_balance = 5_000_000i64; // Delegate 5 TRX (should fit)

    let net_usage = 0; // No usage - all frozen balance is available
    // Available = 10 TRX - 0 = 10 TRX, so delegating 5 TRX should succeed

    let current_slot = 1000000i64;
    let latest_consume_time = 0i64; // No consumption

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let props_db = "properties";
    storage_engine.put(props_db, b"ALLOW_DELEGATE_RESOURCE", &1i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_CURRENT_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();

    let timestamp_ms = current_slot * 3000;
    storage_engine.put(props_db, b"latest_block_header_timestamp", &timestamp_ms.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_account = create_owner_account_with_freeze_v2(
        100_000_000,
        frozen_v2_amount,
        0, // BANDWIDTH
        net_usage,
        latest_consume_time,
        28800,
        0, 0, 28800,
    );
    storage_adapter.put_account_proto(&owner_address, &owner_account).unwrap();

    let owner_evm = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_evm).unwrap();

    let receiver_account = create_receiver_account(50_000_000);
    storage_adapter.put_account_proto(&receiver_address, &receiver_account).unwrap();
    let receiver_evm = AccountInfo {
        balance: U256::from(50_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(receiver_address, receiver_evm).unwrap();

    let owner_raw = make_from_raw(&owner_address);
    let receiver_raw = make_from_raw(&receiver_address);
    let proto_data = build_delegate_resource_proto(
        &owner_raw,
        0,
        delegate_balance,
        &receiver_raw,
        false,
        0,
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(proto_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::DelegateResourceContract),
            asset_id: None,
            from_raw: Some(owner_raw.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: timestamp_ms as u64,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let service = new_delegate_resource_service();

    // Execute - should succeed
    let result = service.execute_delegate_resource_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_ok(), "Should succeed when available >= delegate balance. Error: {:?}", result.err());
    let exec_result = result.unwrap();
    assert!(exec_result.success);
    assert_eq!(exec_result.state_changes.len(), 2); // owner + receiver changes
}

// ============================================================================
// Test: ENERGY delegation fails when raw frozen_v2 >= balance but
//       available (frozen_v2 - v2Usage) < balance
// ============================================================================

#[test]
fn test_delegate_resource_energy_fails_when_usage_exceeds_available() {
    // Setup:
    // - Owner has 10 TRX frozen for ENERGY
    // - Owner has significant energy_usage that consumes most of the frozen balance
    // - Attempting to delegate 10 TRX should FAIL

    let owner_address = Address::from([5u8; 20]);
    let receiver_address = Address::from([6u8; 20]);

    let frozen_v2_amount = 10_000_000i64; // 10 TRX frozen for energy
    let delegate_balance = 10_000_000i64; // Try to delegate all 10 TRX
    let energy_usage = 8; // 8M SUN scaled usage = 8 TRX used

    let current_slot = 1000000i64;
    let latest_consume_time_for_energy = current_slot;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let props_db = "properties";
    storage_engine.put(props_db, b"ALLOW_DELEGATE_RESOURCE", &1i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_CURRENT_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();

    let timestamp_ms = current_slot * 3000;
    storage_engine.put(props_db, b"latest_block_header_timestamp", &timestamp_ms.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_account = create_owner_account_with_freeze_v2(
        100_000_000,
        frozen_v2_amount,
        1, // ENERGY
        0, 0, 28800, // No net usage
        energy_usage,
        latest_consume_time_for_energy,
        28800,
    );
    storage_adapter.put_account_proto(&owner_address, &owner_account).unwrap();

    let owner_evm = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_evm).unwrap();

    let receiver_account = create_receiver_account(50_000_000);
    storage_adapter.put_account_proto(&receiver_address, &receiver_account).unwrap();
    let receiver_evm = AccountInfo {
        balance: U256::from(50_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(receiver_address, receiver_evm).unwrap();

    let owner_raw = make_from_raw(&owner_address);
    let receiver_raw = make_from_raw(&receiver_address);
    let proto_data = build_delegate_resource_proto(
        &owner_raw,
        1, // ENERGY
        delegate_balance,
        &receiver_raw,
        false,
        0,
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(proto_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::DelegateResourceContract),
            asset_id: None,
            from_raw: Some(owner_raw.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: timestamp_ms as u64,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let service = new_delegate_resource_service();

    // Execute - should fail with "available FreezeEnergyV2 balance" error
    let result = service.execute_delegate_resource_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err(), "Should fail when available energy < delegate balance. Got: {:?}", result);
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("available FreezeEnergyV2 balance"),
        "Expected 'available FreezeEnergyV2 balance' error, got: {}", err_msg
    );
}

#[test]
fn test_delegate_resource_energy_succeeds_when_usage_allows_delegation() {
    // Setup:
    // - Owner has 10 TRX frozen for ENERGY
    // - Owner has NO energy_usage (usage=0)
    // - Attempting to delegate 5 TRX should SUCCEED

    let owner_address = Address::from([7u8; 20]);
    let receiver_address = Address::from([8u8; 20]);

    let frozen_v2_amount = 10_000_000i64;
    let delegate_balance = 5_000_000i64;
    let energy_usage = 0; // No usage - all frozen balance is available

    let current_slot = 1000000i64;
    let latest_consume_time_for_energy = 0i64; // No consumption

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let props_db = "properties";
    storage_engine.put(props_db, b"ALLOW_DELEGATE_RESOURCE", &1i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_CURRENT_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();

    let timestamp_ms = current_slot * 3000;
    storage_engine.put(props_db, b"latest_block_header_timestamp", &timestamp_ms.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_account = create_owner_account_with_freeze_v2(
        100_000_000,
        frozen_v2_amount,
        1, // ENERGY
        0, 0, 28800, // No net usage
        energy_usage,
        latest_consume_time_for_energy,
        28800,
    );
    storage_adapter.put_account_proto(&owner_address, &owner_account).unwrap();

    let owner_evm = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_evm).unwrap();

    let receiver_account = create_receiver_account(50_000_000);
    storage_adapter.put_account_proto(&receiver_address, &receiver_account).unwrap();
    let receiver_evm = AccountInfo {
        balance: U256::from(50_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(receiver_address, receiver_evm).unwrap();

    let owner_raw = make_from_raw(&owner_address);
    let receiver_raw = make_from_raw(&receiver_address);
    let proto_data = build_delegate_resource_proto(
        &owner_raw,
        1, // ENERGY
        delegate_balance,
        &receiver_raw,
        false,
        0,
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(proto_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::DelegateResourceContract),
            asset_id: None,
            from_raw: Some(owner_raw.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: timestamp_ms as u64,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let service = new_delegate_resource_service();

    let result = service.execute_delegate_resource_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_ok(), "Should succeed when available energy >= delegate balance. Error: {:?}", result.err());
    let exec_result = result.unwrap();
    assert!(exec_result.success);
    assert_eq!(exec_result.state_changes.len(), 2);
}

// ============================================================================
// Test: Lock=true vs Lock=false - availability check should be independent
// ============================================================================

#[test]
fn test_delegate_resource_with_lock_fails_same_as_without_lock() {
    // The availability check should be the same regardless of lock setting.
    // This test ensures that lock=true with insufficient available balance still fails.

    let owner_address = Address::from([9u8; 20]);
    let receiver_address = Address::from([10u8; 20]);

    let frozen_v2_amount = 10_000_000i64;
    let delegate_balance = 10_000_000i64;
    let net_usage = 8; // 8 TRX used, only 2 TRX available

    let current_slot = 1000000i64;
    let latest_consume_time = current_slot;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let props_db = "properties";
    storage_engine.put(props_db, b"ALLOW_DELEGATE_RESOURCE", &1i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_CURRENT_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();

    let timestamp_ms = current_slot * 3000;
    storage_engine.put(props_db, b"latest_block_header_timestamp", &timestamp_ms.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_account = create_owner_account_with_freeze_v2(
        100_000_000,
        frozen_v2_amount,
        0, // BANDWIDTH
        net_usage,
        latest_consume_time,
        28800,
        0, 0, 28800,
    );
    storage_adapter.put_account_proto(&owner_address, &owner_account).unwrap();

    let owner_evm = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_evm).unwrap();

    let receiver_account = create_receiver_account(50_000_000);
    storage_adapter.put_account_proto(&receiver_address, &receiver_account).unwrap();
    let receiver_evm = AccountInfo {
        balance: U256::from(50_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(receiver_address, receiver_evm).unwrap();

    let owner_raw = make_from_raw(&owner_address);
    let receiver_raw = make_from_raw(&receiver_address);

    // Test with lock=true
    let proto_data = build_delegate_resource_proto(
        &owner_raw,
        0, // BANDWIDTH
        delegate_balance,
        &receiver_raw,
        true, // LOCK enabled
        1000, // lock period in blocks
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(proto_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::DelegateResourceContract),
            asset_id: None,
            from_raw: Some(owner_raw.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: timestamp_ms as u64,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let service = new_delegate_resource_service();

    // Execute - should fail with same error as without lock
    let result = service.execute_delegate_resource_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err(), "Should fail with lock=true when available balance < delegate balance");
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("available FreezeBandwidthV2 balance"),
        "Expected 'available FreezeBandwidthV2 balance' error, got: {}", err_msg
    );
}

#[test]
fn test_delegate_resource_with_lock_succeeds_when_available() {
    // Lock=true should succeed when available balance is sufficient

    let owner_address = Address::from([11u8; 20]);
    let receiver_address = Address::from([12u8; 20]);

    let frozen_v2_amount = 10_000_000i64;
    let delegate_balance = 5_000_000i64;
    let net_usage = 0; // No usage, all 10 TRX available

    let current_slot = 1000000i64;
    let latest_consume_time = 0i64; // No consumption

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let props_db = "properties";
    storage_engine.put(props_db, b"ALLOW_DELEGATE_RESOURCE", &1i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_CURRENT_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();

    let timestamp_ms = current_slot * 3000;
    storage_engine.put(props_db, b"latest_block_header_timestamp", &timestamp_ms.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_account = create_owner_account_with_freeze_v2(
        100_000_000,
        frozen_v2_amount,
        0, // BANDWIDTH
        net_usage,
        latest_consume_time,
        28800,
        0, 0, 28800,
    );
    storage_adapter.put_account_proto(&owner_address, &owner_account).unwrap();

    let owner_evm = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_evm).unwrap();

    let receiver_account = create_receiver_account(50_000_000);
    storage_adapter.put_account_proto(&receiver_address, &receiver_account).unwrap();
    let receiver_evm = AccountInfo {
        balance: U256::from(50_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(receiver_address, receiver_evm).unwrap();

    let owner_raw = make_from_raw(&owner_address);
    let receiver_raw = make_from_raw(&receiver_address);
    let proto_data = build_delegate_resource_proto(
        &owner_raw,
        0, // BANDWIDTH
        delegate_balance,
        &receiver_raw,
        true, // LOCK enabled
        1000, // lock period
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(proto_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::DelegateResourceContract),
            asset_id: None,
            from_raw: Some(owner_raw.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: timestamp_ms as u64,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let service = new_delegate_resource_service();

    let result = service.execute_delegate_resource_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_ok(), "Should succeed with lock=true when available >= delegate balance. Error: {:?}", result.err());
    let exec_result = result.unwrap();
    assert!(exec_result.success);
}

// ============================================================================
// Test: Usage decay affects available balance
// ============================================================================

#[test]
fn test_delegate_resource_usage_decay_increases_available() {
    // If usage occurred in the past, it should decay, making more balance available.
    //
    // Setup:
    // - Owner has 10 TRX frozen for BANDWIDTH
    // - Owner consumed some bandwidth long ago (completely decayed: latest_consume_time = 0)
    // - After full decay, no usage remains (0 TRX used, 10 TRX available)
    // - Delegating 5 TRX should SUCCEED (because usage fully decayed)

    let owner_address = Address::from([13u8; 20]);
    let receiver_address = Address::from([14u8; 20]);

    let frozen_v2_amount = 10_000_000i64; // 10 TRX
    let delegate_balance = 5_000_000i64; // 5 TRX - should succeed after decay

    // Use same setup as working tests but with non-zero net_usage and old timestamp
    let net_usage = 8; // Would be 8 TRX if no decay
    let latest_consume_time = 0i64; // Very old - beyond any window, so fully decayed
    let current_slot = 1000000i64;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let props_db = "properties";
    storage_engine.put(props_db, b"ALLOW_DELEGATE_RESOURCE", &1i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_CURRENT_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();

    let timestamp_ms = current_slot * 3000;
    storage_engine.put(props_db, b"latest_block_header_timestamp", &timestamp_ms.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Use the same helper as other working tests for consistent setup
    let owner_account = create_owner_account_with_freeze_v2(
        100_000_000,
        frozen_v2_amount,
        0, // BANDWIDTH
        net_usage, // 8 - but will fully decay since latest_consume_time = 0
        latest_consume_time, // 0 - very old, beyond any window
        28800, // window size (will be normalized if optimized)
        0, 0, 28800,
    );
    storage_adapter.put_account_proto(&owner_address, &owner_account).unwrap();

    let owner_evm = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_evm).unwrap();

    let receiver_account = create_receiver_account(50_000_000);
    storage_adapter.put_account_proto(&receiver_address, &receiver_account).unwrap();
    let receiver_evm = AccountInfo {
        balance: U256::from(50_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(receiver_address, receiver_evm).unwrap();

    let owner_raw = make_from_raw(&owner_address);
    let receiver_raw = make_from_raw(&receiver_address);
    let proto_data = build_delegate_resource_proto(
        &owner_raw,
        0,
        delegate_balance,
        &receiver_raw,
        false,
        0,
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(proto_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::DelegateResourceContract),
            asset_id: None,
            from_raw: Some(owner_raw.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: timestamp_ms as u64,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let service = new_delegate_resource_service();

    let result = service.execute_delegate_resource_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_ok(), "Should succeed because usage fully decayed. Error: {:?}", result.err());
    let exec_result = result.unwrap();
    assert!(exec_result.success);
}

#[test]
fn test_delegate_resource_expired_usage_fully_resets() {
    // If usage is older than the window, it should fully reset to zero.
    //
    // Setup:
    // - Owner has 10 TRX frozen for BANDWIDTH
    // - Owner consumed 10 TRX worth of bandwidth way beyond 24h window
    // - After full decay, usage = 0, so all 10 TRX is available
    // - Delegating 10 TRX should SUCCEED

    let owner_address = Address::from([15u8; 20]);
    let receiver_address = Address::from([16u8; 20]);

    let frozen_v2_amount = 10_000_000i64;
    let delegate_balance = 10_000_000i64;

    let net_usage = 10; // Would be 10 TRX if no decay
    let window_size = 28800i64; // 24 hours in slots
    let current_slot = 1000000i64;
    let latest_consume_time = 0i64; // Very old, beyond any window
    // Usage should fully reset to 0, making all 10 TRX available

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let props_db = "properties";
    storage_engine.put(props_db, b"ALLOW_DELEGATE_RESOURCE", &1i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_ENERGY_CURRENT_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();

    let timestamp_ms = current_slot * 3000;
    storage_engine.put(props_db, b"latest_block_header_timestamp", &timestamp_ms.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Use the same helper as other working tests for consistent setup
    let owner_account = create_owner_account_with_freeze_v2(
        100_000_000,
        frozen_v2_amount,
        0, // BANDWIDTH
        net_usage, // 10 - but will fully decay since latest_consume_time = 0
        latest_consume_time, // 0 - very old, beyond any window
        28800,
        0, 0, 28800,
    );
    storage_adapter.put_account_proto(&owner_address, &owner_account).unwrap();

    let owner_evm = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_evm).unwrap();

    let receiver_account = create_receiver_account(50_000_000);
    storage_adapter.put_account_proto(&receiver_address, &receiver_account).unwrap();
    let receiver_evm = AccountInfo {
        balance: U256::from(50_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(receiver_address, receiver_evm).unwrap();

    let owner_raw = make_from_raw(&owner_address);
    let receiver_raw = make_from_raw(&receiver_address);
    let proto_data = build_delegate_resource_proto(
        &owner_raw,
        0,
        delegate_balance,
        &receiver_raw,
        false,
        0,
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(proto_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::DelegateResourceContract),
            asset_id: None,
            from_raw: Some(owner_raw.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: timestamp_ms as u64,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let service = new_delegate_resource_service();

    let result = service.execute_delegate_resource_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_ok(), "Should succeed because expired usage fully resets. Error: {:?}", result.err());
    let exec_result = result.unwrap();
    assert!(exec_result.success);
}

// ============================================================================
// Test: Minimum delegate amount (1 TRX)
// ============================================================================

#[test]
fn test_delegate_resource_fails_below_minimum() {
    // Delegate balance must be >= 1 TRX (1_000_000 SUN)

    let owner_address = Address::from([17u8; 20]);
    let receiver_address = Address::from([18u8; 20]);

    let frozen_v2_amount = 10_000_000i64;
    let delegate_balance = 500_000i64; // 0.5 TRX - below minimum

    let current_slot = 1000000i64;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let props_db = "properties";
    storage_engine.put(props_db, b"ALLOW_DELEGATE_RESOURCE", &1i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();

    let timestamp_ms = current_slot * 3000;
    storage_engine.put(props_db, b"latest_block_header_timestamp", &timestamp_ms.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_account = create_owner_account_with_freeze_v2(
        100_000_000,
        frozen_v2_amount,
        0, // BANDWIDTH
        0, 0, 28800, // No usage
        0, 0, 28800,
    );
    storage_adapter.put_account_proto(&owner_address, &owner_account).unwrap();

    let owner_evm = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_evm).unwrap();

    let receiver_account = create_receiver_account(50_000_000);
    storage_adapter.put_account_proto(&receiver_address, &receiver_account).unwrap();
    let receiver_evm = AccountInfo {
        balance: U256::from(50_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(receiver_address, receiver_evm).unwrap();

    let owner_raw = make_from_raw(&owner_address);
    let receiver_raw = make_from_raw(&receiver_address);
    let proto_data = build_delegate_resource_proto(
        &owner_raw,
        0,
        delegate_balance,
        &receiver_raw,
        false,
        0,
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(proto_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::DelegateResourceContract),
            asset_id: None,
            from_raw: Some(owner_raw.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: timestamp_ms as u64,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let service = new_delegate_resource_service();

    let result = service.execute_delegate_resource_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err());
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("greater than or equal to 1 TRX"),
        "Expected minimum amount error, got: {}", err_msg
    );
}

// ============================================================================
// Test: Cannot delegate to self
// ============================================================================

#[test]
fn test_delegate_resource_fails_self_delegation() {
    let owner_address = Address::from([19u8; 20]);

    let frozen_v2_amount = 10_000_000i64;
    let delegate_balance = 5_000_000i64;
    let current_slot = 1000000i64;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let props_db = "properties";
    storage_engine.put(props_db, b"ALLOW_DELEGATE_RESOURCE", &1i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_WEIGHT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"TOTAL_NET_LIMIT", &100_000_000_000i64.to_be_bytes()).unwrap();
    storage_engine.put(props_db, b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();

    let timestamp_ms = current_slot * 3000;
    storage_engine.put(props_db, b"latest_block_header_timestamp", &timestamp_ms.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_account = create_owner_account_with_freeze_v2(
        100_000_000,
        frozen_v2_amount,
        0, // BANDWIDTH
        0, 0, 28800,
        0, 0, 28800,
    );
    storage_adapter.put_account_proto(&owner_address, &owner_account).unwrap();

    let owner_evm = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_evm).unwrap();

    // Try to delegate to self
    let owner_raw = make_from_raw(&owner_address);
    let receiver_raw = make_from_raw(&owner_address); // Same as owner for self-delegation test
    let proto_data = build_delegate_resource_proto(
        &owner_raw,
        0,
        delegate_balance,
        &receiver_raw,
        false,
        0,
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(proto_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::DelegateResourceContract),
            asset_id: None,
            from_raw: Some(owner_raw.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: timestamp_ms as u64,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let service = new_delegate_resource_service();

    let result = service.execute_delegate_resource_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err());
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("must not be the same as ownerAddress"),
        "Expected self-delegation error, got: {}", err_msg
    );
}
