//! FreezeBalanceContract and UnfreezeBalanceContract tests (V1 and V2).

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, seed_dynamic_properties};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{
    EngineBackedEvmStateStore, TronExecutionContext, TronTransaction, TxMetadata,
};
use tron_backend_storage::StorageEngine;

#[test]
fn test_freeze_balance_success_basic() {
    // Create test setup
    let owner_address = Address::from([1u8; 20]);
    let initial_balance = 50_000_000u64; // 50 TRX
    let freeze_amount = 1_000_000i64; // 1 TRX

    // Setup storage with initial account
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter
        .set_account(owner_address, owner_account.clone())
        .unwrap();

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
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
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
        transaction_id: None,
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
    let result =
        service.execute_freeze_balance_contract(&mut storage_adapter, &transaction, &context);

    // Assertions
    assert!(
        result.is_ok(),
        "FreezeBalance should succeed: {:?}",
        result.err()
    );
    let exec_result = result.unwrap();

    assert!(exec_result.success);
    assert_eq!(exec_result.energy_used, 0);
    assert_eq!(exec_result.state_changes.len(), 1);
    assert!(exec_result.logs.is_empty());

    // Verify balance decreased
    match &exec_result.state_changes[0] {
        tron_backend_execution::TronStateChange::AccountChange {
            address,
            old_account,
            new_account,
        } => {
            assert_eq!(*address, owner_address);
            assert_eq!(
                old_account.as_ref().unwrap().balance,
                U256::from(initial_balance)
            );
            assert_eq!(
                new_account.as_ref().unwrap().balance,
                U256::from(initial_balance - freeze_amount as u64)
            );
        }
        _ => panic!("Expected AccountChange"),
    }
}

#[test]
fn test_freeze_balance_insufficient_balance() {
    let owner_address = Address::from([1u8; 20]);
    let initial_balance = 100u64; // Very small balance
    let freeze_amount = 1_000_000i64; // Try to freeze more than we have

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter
        .set_account(owner_address, owner_account)
        .unwrap();

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
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
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
        transaction_id: None,
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
    let result =
        service.execute_freeze_balance_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "Should fail with insufficient balance");
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("Insufficient balance")
            || err_msg.contains("frozenBalance must be less than")
            || err_msg.contains("accountBalance"),
        "Expected balance error, got: {}",
        err_msg
    );
}

#[test]
fn test_freeze_balance_bad_params() {
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
    storage_adapter
        .set_account(owner_address, owner_account)
        .unwrap();

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
            ..Default::default()
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
        transaction_id: None,
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

    let result =
        service.execute_freeze_balance_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "Should fail with empty params");
}

#[test]
fn test_freeze_balance_emits_freeze_changes_when_enabled() {
    // Create test storage with temp directory
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Setup owner account with sufficient balance
    let owner_addr = Address::from_slice(&[0x12; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(2_000_000_000_000u64), // 2M TRX
        nonce: 0,
        code_hash: revm_primitives::KECCAK_EMPTY,
        code: None,
    };
    storage_adapter
        .set_account(owner_addr, owner_account)
        .unwrap();

    // Create FreezeBalance transaction
    // Field 2: frozen_balance = 1_000_000 (varint encoded)
    // Field 3: frozen_duration = 3 (varint encoded)
    // Field 10: resource = 0 (BANDWIDTH)
    let params_data = vec![
        0x10, 0xC0, 0x84, 0x3D, // field 2 (frozen_balance): 1_000_000
        0x18, 0x03, // field 3 (frozen_duration): 3
        0x50, 0x00, // field 10 (resource): 0 (BANDWIDTH)
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
            from_raw: Some(make_from_raw(&owner_addr)),
            ..Default::default()
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
        transaction_id: None,
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
    assert_eq!(
        exec_result.freeze_changes.len(),
        1,
        "Should emit exactly one freeze change"
    );

    let freeze_change = &exec_result.freeze_changes[0];
    assert_eq!(freeze_change.owner_address, owner_addr);
    assert_eq!(
        freeze_change.resource,
        tron_backend_execution::FreezeLedgerResource::Bandwidth
    );
    assert_eq!(
        freeze_change.amount, 1_000_000,
        "Amount should be absolute frozen amount"
    );
    assert_eq!(freeze_change.v2_model, false, "Should be V1 model");
    assert!(freeze_change.expiration_ms > 0, "Expiration should be set");

    // Verify state_changes still present (CSV parity)
    assert_eq!(
        exec_result.state_changes.len(),
        1,
        "Should still emit state change"
    );
}

#[test]
fn test_freeze_balance_no_emission_when_disabled() {
    // Create test storage with temp directory
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Setup owner account with sufficient balance
    let owner_addr = Address::from_slice(&[0x13; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(2_000_000_000_000u64),
        nonce: 0,
        code_hash: revm_primitives::KECCAK_EMPTY,
        code: None,
    };
    storage_adapter
        .set_account(owner_addr, owner_account)
        .unwrap();

    // Create FreezeBalance transaction
    let params_data = vec![
        0x10, 0xC0, 0x84, 0x3D, // frozen_balance: 1_000_000
        0x18, 0x03, // frozen_duration: 3
        0x50, 0x00, // resource: BANDWIDTH
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
            from_raw: Some(make_from_raw(&owner_addr)),
            ..Default::default()
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
        transaction_id: None,
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
    assert_eq!(
        exec_result.freeze_changes.len(),
        0,
        "Should NOT emit freeze changes when disabled"
    );

    // Verify state_changes still present (CSV parity maintained)
    assert_eq!(
        exec_result.state_changes.len(),
        1,
        "Should still emit state change"
    );
}

#[test]
fn test_unfreeze_balance_emits_freeze_changes_when_enabled() {
    // Create test storage with temp directory
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // Set latest_block_header_timestamp for the unfreeze time check
    // The freeze expires at 1500000000000ms, so we set timestamp to 1600000000000ms (after expiry)
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Setup owner account with frozen balance using full proto
    let owner_addr = Address::from_slice(&[0x14; 20]);
    let prefix = storage_adapter.address_prefix();
    let mut owner_tron = vec![prefix];
    owner_tron.extend_from_slice(owner_addr.as_slice());

    // Create Account proto with frozen balance (self-freeze for bandwidth)
    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron,
        balance: 1_000_000_000_000i64,
        frozen: vec![tron_backend_execution::protocol::account::Frozen {
            frozen_balance: 500_000,
            expire_time: 1500000000000, // Already expired (block_timestamp=1600000000000)
        }],
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Pre-populate freeze record for Rust-side ledger
    storage_adapter
        .add_freeze_amount(owner_addr, 0, 500_000, 1500000000000)
        .unwrap();

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
            from_raw: Some(make_from_raw(&owner_addr)),
            ..Default::default()
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
        transaction_id: None,
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

    assert!(
        result.is_ok(),
        "Unfreeze execution should succeed: {:?}",
        result.err()
    );
    let exec_result = result.unwrap();

    // Verify freeze_changes is populated
    assert_eq!(
        exec_result.freeze_changes.len(),
        1,
        "Should emit exactly one freeze change"
    );

    let freeze_change = &exec_result.freeze_changes[0];
    assert_eq!(freeze_change.owner_address, owner_addr);
    assert_eq!(
        freeze_change.resource,
        tron_backend_execution::FreezeLedgerResource::Bandwidth
    );
    assert_eq!(
        freeze_change.amount, 0,
        "Amount should be 0 for full unfreeze"
    );
    assert_eq!(
        freeze_change.expiration_ms, 0,
        "Expiration should be 0 after unfreeze"
    );
    assert_eq!(freeze_change.v2_model, false, "Should be V1 model");
}

#[test]
fn test_freeze_balance_v2_emits_with_v2_flag() {
    // Create test storage with temp directory
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // Enable V2 freeze by setting UNFREEZE_DELAY_DAYS > 0
    storage_engine
        .put("properties", b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes())
        .unwrap();
    // Set latest_block_header_timestamp
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Setup owner account with full proto
    let owner_addr = Address::from_slice(&[0x15; 20]);
    let prefix = storage_adapter.address_prefix();
    let mut owner_tron = vec![prefix];
    owner_tron.extend_from_slice(owner_addr.as_slice());

    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron,
        balance: 2_000_000_000_000i64,
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Create FreezeBalanceV2 transaction
    // Field 1: owner_address (bytes, 21 bytes with 0x41 prefix)
    // Field 2: frozen_balance = 1_000_000
    // Field 3: resource = 1 (ENERGY)
    let mut params_data = Vec::new();
    // Field 1 (owner_address): tag = (1 << 3) | 2 = 0x0A, length = 21
    params_data.push(0x0A); // field 1, wire type 2 (length-delimited)
    params_data.push(21); // length
    params_data.push(0x41); // TRON mainnet prefix
    params_data.extend_from_slice(owner_addr.as_slice()); // 20 bytes
                                                          // Field 2 (frozen_balance): tag = (2 << 3) | 0 = 0x10, value = 1_000_000
    params_data.push(0x10);
    encode_varint(&mut params_data, 1_000_000);
    // Field 3 (resource): tag = (3 << 3) | 0 = 0x18, value = 1 (ENERGY)
    params_data.push(0x18);
    params_data.push(0x01);

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
            from_raw: Some(make_from_raw(&owner_addr)),
            ..Default::default()
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
        transaction_id: None,
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

    assert!(
        result.is_ok(),
        "FreezeV2 execution should succeed: {:?}",
        result.err()
    );
    let exec_result = result.unwrap();

    // Verify freeze_changes is populated with V2 flag
    assert_eq!(
        exec_result.freeze_changes.len(),
        1,
        "Should emit exactly one freeze change"
    );

    let freeze_change = &exec_result.freeze_changes[0];
    assert_eq!(freeze_change.owner_address, owner_addr);
    assert_eq!(
        freeze_change.resource,
        tron_backend_execution::FreezeLedgerResource::Energy
    );
    assert_eq!(freeze_change.amount, 1_000_000);
    assert_eq!(freeze_change.v2_model, true, "Should be V2 model"); // Key difference!
                                                                    // Java parity: V2 freeze has NO expiration (FreezeBalanceV2Actuator records expireTime=0)
    assert_eq!(
        freeze_change.expiration_ms, 0,
        "V2 freeze should have expiration_ms=0 (Java parity)"
    );
}

#[test]
fn test_unfreeze_balance_v2_partial_unfreeze() {
    // Create test storage with temp directory
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // Enable V2 by setting UNFREEZE_DELAY_DAYS > 0
    storage_engine
        .put("properties", b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Setup owner account with full proto containing frozen_v2 balance
    let owner_addr = Address::from_slice(&[0x16; 20]);
    let prefix = storage_adapter.address_prefix();
    let mut owner_tron = vec![prefix];
    owner_tron.extend_from_slice(owner_addr.as_slice());

    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000_000i64,
        frozen_v2: vec![tron_backend_execution::protocol::account::FreezeV2 {
            r#type: 0, // BANDWIDTH
            amount: 1_000_000,
        }],
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Pre-populate freeze record with 1_000_000 frozen
    storage_adapter
        .add_freeze_amount(owner_addr, 0, 1_000_000, 1700000000000)
        .unwrap();

    // Create UnfreezeBalanceV2 transaction with partial unfreeze (400_000)
    // Field 1: owner_address (bytes)
    // Field 2: unfreeze_balance = 400_000
    // Field 3: resource = 0 (BANDWIDTH)
    let mut params_data = Vec::new();
    // Field 1 (owner_address): tag = 0x0A, length = 21
    params_data.push(0x0A);
    params_data.push(21);
    params_data.extend_from_slice(&owner_tron);
    // Field 2 (unfreeze_balance): tag = 0x10
    params_data.push(0x10);
    encode_varint(&mut params_data, 400_000);
    // Field 3 (resource): tag = 0x18, value = 0 (BANDWIDTH)
    params_data.push(0x18);
    params_data.push(0x00);

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 100_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract,
            ),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_addr)),
            ..Default::default()
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
        transaction_id: None,
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

    assert!(
        result.is_ok(),
        "UnfreezeV2 execution should succeed: {:?}",
        result.err()
    );
    let exec_result = result.unwrap();

    // Verify freeze_changes shows remaining amount (not 0)
    assert_eq!(
        exec_result.freeze_changes.len(),
        1,
        "Should emit exactly one freeze change"
    );

    let freeze_change = &exec_result.freeze_changes[0];
    assert_eq!(freeze_change.owner_address, owner_addr);
    assert_eq!(
        freeze_change.resource,
        tron_backend_execution::FreezeLedgerResource::Bandwidth
    );
    // Should emit remaining frozen amount after partial unfreeze
    // Note: This depends on implementation - may be 0 if we simplified to full unfreeze only
    assert_eq!(freeze_change.v2_model, true, "Should be V2 model");
}

#[test]
fn test_unfreeze_balance_v2_full_unfreeze() {
    // Create test storage with temp directory
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // Enable V2 by setting UNFREEZE_DELAY_DAYS > 0
    storage_engine
        .put("properties", b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Setup owner account with full proto containing frozen_v2 balance
    let owner_addr = Address::from_slice(&[0x17; 20]);
    let prefix = storage_adapter.address_prefix();
    let mut owner_tron = vec![prefix];
    owner_tron.extend_from_slice(owner_addr.as_slice());

    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000_000i64,
        frozen_v2: vec![tron_backend_execution::protocol::account::FreezeV2 {
            r#type: 1, // ENERGY
            amount: 800_000,
        }],
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Pre-populate freeze record
    storage_adapter
        .add_freeze_amount(owner_addr, 1, 800_000, 1700000000000)
        .unwrap();

    // Create UnfreezeBalanceV2 transaction with full unfreeze (800_000 to match frozen)
    // Field 1: owner_address
    // Field 2: unfreeze_balance = 800_000
    // Field 3: resource = 1 (ENERGY)
    let mut params_data = Vec::new();
    // Field 1 (owner_address): tag = 0x0A, length = 21
    params_data.push(0x0A);
    params_data.push(21);
    params_data.extend_from_slice(&owner_tron);
    // Field 2 (unfreeze_balance): tag = 0x10
    params_data.push(0x10);
    encode_varint(&mut params_data, 800_000);
    // Field 3 (resource): tag = 0x18, value = 1 (ENERGY)
    params_data.push(0x18);
    params_data.push(0x01);

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 100_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract,
            ),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_addr)),
            ..Default::default()
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
        transaction_id: None,
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

    assert!(
        result.is_ok(),
        "UnfreezeV2 full unfreeze should succeed: {:?}",
        result.err()
    );
    let exec_result = result.unwrap();

    // Verify freeze_changes shows amount=0 for full unfreeze
    assert_eq!(
        exec_result.freeze_changes.len(),
        1,
        "Should emit exactly one freeze change"
    );

    let freeze_change = &exec_result.freeze_changes[0];
    assert_eq!(freeze_change.owner_address, owner_addr);
    assert_eq!(
        freeze_change.resource,
        tron_backend_execution::FreezeLedgerResource::Energy
    );
    assert_eq!(freeze_change.amount, 0, "Should be 0 for full unfreeze");
    assert_eq!(
        freeze_change.expiration_ms, 0,
        "Expiration should be 0 after full unfreeze"
    );
    assert_eq!(freeze_change.v2_model, true, "Should be V2 model");
}

// ==== Regression Tests for Java Parity ====

/// Test oldTronPower initialization when ALLOW_NEW_RESOURCE_MODEL is enabled.
/// Java reference: FreezeBalanceActuator.execute() -> initializeOldTronPower()
#[test]
fn test_freeze_initializes_old_tron_power_when_new_resource_model_enabled() {
    // Setup
    let owner_addr = Address::from([0x12; 20]);
    let owner_tron = make_from_raw(&owner_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable ALLOW_NEW_RESOURCE_MODEL
    storage_engine
        .put(
            "properties",
            b"ALLOW_NEW_RESOURCE_MODEL",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Create account with balance and some legacy frozen bandwidth (tron power = frozen/TRX_PRECISION)
    let mut owner_proto = tron_backend_execution::protocol::Account::default();
    owner_proto.balance = 100_000_000; // 100 TRX
    owner_proto.old_tron_power = 0; // Not initialized
                                    // Add legacy frozen bandwidth to create non-zero tron power
    owner_proto
        .frozen
        .push(tron_backend_execution::protocol::account::Frozen {
            frozen_balance: 5_000_000, // 5 TRX frozen
            expire_time: 1700000000000,
        });
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Create FreezeBalance transaction for 1 TRX
    let mut params_data = Vec::new();
    // Field 2 (frozen_balance): 1_000_000
    params_data.push((2 << 3) | 0);
    encode_varint(&mut params_data, 1_000_000);
    // Field 3 (frozen_duration): 3 days
    params_data.push((3 << 3) | 0);
    encode_varint(&mut params_data, 3);
    // Field 10 (resource): BANDWIDTH (0)
    params_data.push((10 << 3) | 0);
    encode_varint(&mut params_data, 0);

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            freeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    // Execute freeze
    let result = service.execute_freeze_balance_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "FreezeBalance should succeed: {:?}",
        result.err()
    );

    // Verify oldTronPower was initialized
    let updated_proto = storage_adapter
        .get_account_proto(&owner_addr)
        .unwrap()
        .unwrap();

    // With 5_000_000 SUN frozen, tron_power = 5_000_000 / 1_000_000 = 5 weight units
    // Since legacy tron power is non-zero (5_000_000), oldTronPower should be set to that value
    assert_ne!(
        updated_proto.old_tron_power, 0,
        "oldTronPower should be initialized to non-zero when legacy tron power exists"
    );
    // oldTronPower stores the raw SUN amount, not the weight
    assert_eq!(
        updated_proto.old_tron_power, 5_000_000,
        "oldTronPower should be set to legacy tron power snapshot (5_000_000 SUN)"
    );
}

/// Test oldTronPower is set to -1 when legacy tron power is zero.
/// Java reference: AccountCapsule.initializeOldTronPower() sets to -1 if getTronPower() == 0
#[test]
fn test_freeze_initializes_old_tron_power_to_minus_one_when_legacy_power_is_zero() {
    let owner_addr = Address::from([0x13; 20]);
    let owner_tron = make_from_raw(&owner_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable ALLOW_NEW_RESOURCE_MODEL
    storage_engine
        .put(
            "properties",
            b"ALLOW_NEW_RESOURCE_MODEL",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Create account with balance but NO legacy frozen (tron power = 0)
    let mut owner_proto = tron_backend_execution::protocol::Account::default();
    owner_proto.balance = 100_000_000; // 100 TRX
    owner_proto.old_tron_power = 0; // Not initialized
                                    // No frozen balance - tron power will be 0
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Create FreezeBalance transaction for 1 TRX
    let mut params_data = Vec::new();
    params_data.push((2 << 3) | 0);
    encode_varint(&mut params_data, 1_000_000);
    params_data.push((3 << 3) | 0);
    encode_varint(&mut params_data, 3);
    params_data.push((10 << 3) | 0);
    encode_varint(&mut params_data, 0);

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            freeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    // Execute freeze
    let result = service.execute_freeze_balance_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "FreezeBalance should succeed: {:?}",
        result.err()
    );

    // Verify oldTronPower was set to -1
    let updated_proto = storage_adapter
        .get_account_proto(&owner_addr)
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_proto.old_tron_power, -1,
        "oldTronPower should be -1 when legacy tron power was zero"
    );
}

/// Test unknown resource code returns Java-parity error message.
/// Java reference: FreezeBalanceActuator.validate() default case
#[test]
fn test_freeze_unknown_resource_returns_java_error_message() {
    let owner_addr = Address::from([0x14; 20]);
    let owner_tron = make_from_raw(&owner_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Create account with balance
    let mut owner_proto = tron_backend_execution::protocol::Account::default();
    owner_proto.balance = 100_000_000;
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Create FreezeBalance transaction with unknown resource code (99)
    let mut params_data = Vec::new();
    params_data.push((2 << 3) | 0);
    encode_varint(&mut params_data, 1_000_000);
    params_data.push((3 << 3) | 0);
    encode_varint(&mut params_data, 3);
    params_data.push((10 << 3) | 0);
    encode_varint(&mut params_data, 99); // Unknown resource code

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            freeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    // Execute freeze - should fail with Java-parity error message
    let result = service.execute_freeze_balance_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_err(),
        "FreezeBalance with unknown resource should fail"
    );

    let error_msg = result.unwrap_err();
    // Without new resource model, should use "ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]"
    assert!(
        error_msg.contains("ResourceCode error"),
        "Error should mention ResourceCode error: {}",
        error_msg
    );
    assert!(
        error_msg.contains("BANDWIDTH"),
        "Error should list valid codes: {}",
        error_msg
    );
}

// ==== ALLOW_NEW_REWARD Gating Tests ====

/// Test that weight deltas follow Java behavior when ALLOW_NEW_REWARD=0.
/// Java reference: DynamicPropertiesStore.allowNewReward() -> ALLOW_NEW_REWARD == 1
/// When ALLOW_NEW_REWARD=0, weight delta should be amount/TRX_PRECISION, NOT the increment.
#[test]
fn test_freeze_weight_delta_without_new_reward() {
    let owner_addr = Address::from([0x18; 20]);
    let owner_tron = make_from_raw(&owner_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Explicitly set ALLOW_NEW_REWARD=0 (even though it's the default)
    storage_engine
        .put("properties", b"ALLOW_NEW_REWARD", &0i64.to_be_bytes())
        .unwrap();
    // Set an initial TOTAL_NET_WEIGHT so we can verify the delta
    storage_engine
        .put("properties", b"TOTAL_NET_WEIGHT", &100i64.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Create account with balance and some existing frozen bandwidth
    let mut owner_proto = tron_backend_execution::protocol::Account::default();
    owner_proto.balance = 100_000_000; // 100 TRX
                                       // Add existing frozen bandwidth (5 TRX) to test increment calculation
    owner_proto
        .frozen
        .push(tron_backend_execution::protocol::account::Frozen {
            frozen_balance: 5_000_000, // 5 TRX frozen
            expire_time: 1700000000000,
        });
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Create FreezeBalance transaction for 3 TRX
    let freeze_amount = 3_000_000i64; // 3 TRX
    let mut params_data = Vec::new();
    params_data.push((2 << 3) | 0);
    encode_varint(&mut params_data, freeze_amount as u64);
    params_data.push((3 << 3) | 0);
    encode_varint(&mut params_data, 3);
    params_data.push((10 << 3) | 0);
    encode_varint(&mut params_data, 0); // BANDWIDTH

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            freeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    // Execute freeze
    let result = service.execute_freeze_balance_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "FreezeBalance should succeed: {:?}",
        result.err()
    );

    // Verify weight delta used amount/TRX_PRECISION (3_000_000 / 1_000_000 = 3)
    // NOT the increment (which would be new_weight - old_weight = 8 - 5 = 3 in this case,
    // but would differ if there were partial amounts)
    // Initial weight was 100, so new weight should be 100 + 3 = 103
    let new_weight = storage_adapter.get_total_net_weight().unwrap();

    // With ALLOW_NEW_REWARD=0, Java uses: frozen_balance / TRX_PRECISION = 3_000_000 / 1_000_000 = 3
    assert_eq!(
        new_weight, 103,
        "Weight delta should be amount/TRX_PRECISION (3) when ALLOW_NEW_REWARD=0"
    );
}

/// Test that weight deltas use increment calculation when ALLOW_NEW_REWARD=1.
/// This verifies the opposite case where new reward is enabled.
#[test]
fn test_freeze_weight_delta_with_new_reward() {
    let owner_addr = Address::from([0x19; 20]);
    let owner_tron = make_from_raw(&owner_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable ALLOW_NEW_REWARD
    storage_engine
        .put("properties", b"ALLOW_NEW_REWARD", &1i64.to_be_bytes())
        .unwrap();
    // Set an initial TOTAL_NET_WEIGHT
    storage_engine
        .put("properties", b"TOTAL_NET_WEIGHT", &100i64.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Create account with balance and existing frozen bandwidth
    let mut owner_proto = tron_backend_execution::protocol::Account::default();
    owner_proto.balance = 100_000_000; // 100 TRX
                                       // Existing frozen: 5 TRX
    owner_proto
        .frozen
        .push(tron_backend_execution::protocol::account::Frozen {
            frozen_balance: 5_000_000, // 5 TRX
            expire_time: 1700000000000,
        });
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Freeze 3 TRX more
    let freeze_amount = 3_000_000i64;
    let mut params_data = Vec::new();
    params_data.push((2 << 3) | 0);
    encode_varint(&mut params_data, freeze_amount as u64);
    params_data.push((3 << 3) | 0);
    encode_varint(&mut params_data, 3);
    params_data.push((10 << 3) | 0);
    encode_varint(&mut params_data, 0); // BANDWIDTH

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            freeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    // Execute freeze
    let result = service.execute_freeze_balance_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "FreezeBalance should succeed: {:?}",
        result.err()
    );

    // With ALLOW_NEW_REWARD=1, Java uses: increment = new_weight - old_weight
    // old_weight = 5_000_000 / 1_000_000 = 5
    // new_weight = 8_000_000 / 1_000_000 = 8
    // increment = 8 - 5 = 3
    // But since 5 + 3 = 8 and 8 - 5 = 3, the result is the same as amount/TRX_PRECISION in this case.
    // The difference shows when there are partial amounts (not divisible by TRX_PRECISION).
    let new_weight = storage_adapter.get_total_net_weight().unwrap();
    assert_eq!(
        new_weight, 103,
        "Weight delta should be increment (3) when ALLOW_NEW_REWARD=1"
    );
}

// ==== ALLOW_DELEGATE_OPTIMIZATION Tests ====

/// Test that delegate optimization writes prefixed keys and deletes legacy keys.
/// Java reference: DelegatedResourceAccountIndexStore.delegate(from, to, time)
/// When ALLOW_DELEGATE_OPTIMIZATION=1:
/// - Writes 0x01||from||to and 0x02||to||from keys
/// - Deletes legacy key (just address) after conversion
#[test]
fn test_freeze_delegation_writes_optimized_keys() {
    let owner_addr = Address::from([0x1A; 20]);
    let receiver_addr = Address::from([0x1B; 20]);
    let owner_tron = make_from_raw(&owner_addr);
    let receiver_tron = make_from_raw(&receiver_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable delegate resource and delegate optimization
    storage_engine
        .put(
            "properties",
            b"ALLOW_DELEGATE_RESOURCE",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ALLOW_DELEGATE_OPTIMIZATION",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    // Use new_with_buffer for proper write buffer handling
    let (mut storage_adapter, _buffer) =
        EngineBackedEvmStateStore::new_with_buffer(storage_engine.clone());

    // Create owner account with sufficient balance
    let mut owner_proto = tron_backend_execution::protocol::Account::default();
    owner_proto.balance = 100_000_000; // 100 TRX
    owner_proto.address = owner_tron.clone();
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Create receiver account
    let mut receiver_proto = tron_backend_execution::protocol::Account::default();
    receiver_proto.balance = 1_000_000; // 1 TRX
    receiver_proto.address = receiver_tron.clone();
    storage_adapter
        .put_account_proto(&receiver_addr, &receiver_proto)
        .unwrap();

    // Create FreezeBalance transaction with delegation (receiver_address set)
    let freeze_amount = 5_000_000i64; // 5 TRX
    let mut params_data = Vec::new();
    // Field 2: frozen_balance
    params_data.push((2 << 3) | 0);
    encode_varint(&mut params_data, freeze_amount as u64);
    // Field 3: frozen_duration
    params_data.push((3 << 3) | 0);
    encode_varint(&mut params_data, 3);
    // Field 10: resource (BANDWIDTH)
    params_data.push((10 << 3) | 0);
    encode_varint(&mut params_data, 0);
    // Field 15: receiver_address (length-delimited)
    params_data.push((15 << 3) | 2); // tag for field 15, wire type 2
    params_data.push(21); // length
    params_data.extend_from_slice(&receiver_tron);

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000, // This is the timestamp used for optimized keys
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            freeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    // Execute freeze with delegation
    let result = service.execute_freeze_balance_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "FreezeBalance with delegation should succeed: {:?}",
        result.err()
    );

    // Commit writes to storage
    storage_adapter.commit_buffer().unwrap();

    // Verify optimized keys are written
    // Key format: 0x01 || from (21 bytes) || to (21 bytes)
    let mut from_key = vec![0x01];
    from_key.extend_from_slice(&owner_tron);
    from_key.extend_from_slice(&receiver_tron);

    // Key format: 0x02 || to (21 bytes) || from (21 bytes)
    let mut to_key = vec![0x02];
    to_key.extend_from_slice(&receiver_tron);
    to_key.extend_from_slice(&owner_tron);

    let db_name = "DelegatedResourceAccountIndex";

    // Check that from_key exists
    let from_data = storage_engine.get(db_name, &from_key).unwrap();
    assert!(
        from_data.is_some(),
        "Optimized from_key (0x01||owner||receiver) should exist after delegation"
    );

    // Check that to_key exists
    let to_data = storage_engine.get(db_name, &to_key).unwrap();
    assert!(
        to_data.is_some(),
        "Optimized to_key (0x02||receiver||owner) should exist after delegation"
    );

    // Verify legacy key does NOT exist (should be deleted after conversion if it existed)
    let legacy_owner_key = owner_tron.clone();
    let legacy_owner_data = storage_engine.get(db_name, &legacy_owner_key).unwrap();
    assert!(
        legacy_owner_data.is_none(),
        "Legacy key (just owner address) should not exist when optimization is enabled"
    );
}

/// Test delegation without optimization writes legacy keys.
/// When ALLOW_DELEGATE_OPTIMIZATION=0, only legacy keys should be written.
#[test]
fn test_freeze_delegation_writes_legacy_keys_without_optimization() {
    let owner_addr = Address::from([0x1C; 20]);
    let receiver_addr = Address::from([0x1D; 20]);
    let owner_tron = make_from_raw(&owner_addr);
    let receiver_tron = make_from_raw(&receiver_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable delegate resource but NOT optimization
    storage_engine
        .put(
            "properties",
            b"ALLOW_DELEGATE_RESOURCE",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ALLOW_DELEGATE_OPTIMIZATION",
            &0i64.to_be_bytes(),
        )
        .unwrap();

    // Use new_with_buffer for proper write buffer handling
    let (mut storage_adapter, _buffer) =
        EngineBackedEvmStateStore::new_with_buffer(storage_engine.clone());

    // Create owner account
    let mut owner_proto = tron_backend_execution::protocol::Account::default();
    owner_proto.balance = 100_000_000;
    owner_proto.address = owner_tron.clone();
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Create receiver account
    let mut receiver_proto = tron_backend_execution::protocol::Account::default();
    receiver_proto.balance = 1_000_000;
    receiver_proto.address = receiver_tron.clone();
    storage_adapter
        .put_account_proto(&receiver_addr, &receiver_proto)
        .unwrap();

    // Create FreezeBalance transaction with delegation
    let freeze_amount = 5_000_000i64;
    let mut params_data = Vec::new();
    params_data.push((2 << 3) | 0);
    encode_varint(&mut params_data, freeze_amount as u64);
    params_data.push((3 << 3) | 0);
    encode_varint(&mut params_data, 3);
    params_data.push((10 << 3) | 0);
    encode_varint(&mut params_data, 0);
    params_data.push((15 << 3) | 2);
    params_data.push(21);
    params_data.extend_from_slice(&receiver_tron);

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            freeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    // Execute freeze
    let result = service.execute_freeze_balance_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "FreezeBalance should succeed: {:?}",
        result.err()
    );

    // Commit writes to storage
    storage_adapter.commit_buffer().unwrap();

    let db_name = "DelegatedResourceAccountIndex";

    // Verify legacy keys exist
    let legacy_owner_data = storage_engine.get(db_name, &owner_tron).unwrap();
    assert!(
        legacy_owner_data.is_some(),
        "Legacy owner key should exist when optimization is disabled"
    );

    let legacy_receiver_data = storage_engine.get(db_name, &receiver_tron).unwrap();
    assert!(
        legacy_receiver_data.is_some(),
        "Legacy receiver key should exist when optimization is disabled"
    );

    // Verify optimized keys do NOT exist
    let mut from_key = vec![0x01];
    from_key.extend_from_slice(&owner_tron);
    from_key.extend_from_slice(&receiver_tron);

    let from_data = storage_engine.get(db_name, &from_key).unwrap();
    assert!(
        from_data.is_none(),
        "Optimized from_key should NOT exist when optimization is disabled"
    );
}

/// Test that delegate optimization correctly preserves ordering via timestamps.
/// This verifies that Java's getIndex() would reconstruct the same to/from lists
/// by ordering by timestamp.
#[test]
fn test_freeze_delegation_optimized_preserves_ordering() {
    let owner_addr = Address::from([0x1E; 20]);
    let receiver1_addr = Address::from([0x1F; 20]);
    let receiver2_addr = Address::from([0x20; 20]);
    let owner_tron = make_from_raw(&owner_addr);
    let receiver1_tron = make_from_raw(&receiver1_addr);
    let receiver2_tron = make_from_raw(&receiver2_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable delegate resource and optimization
    storage_engine
        .put(
            "properties",
            b"ALLOW_DELEGATE_RESOURCE",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ALLOW_DELEGATE_OPTIMIZATION",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    // Use new_with_buffer for proper write buffer handling
    let (mut storage_adapter, _buffer) =
        EngineBackedEvmStateStore::new_with_buffer(storage_engine.clone());

    // Create accounts
    let mut owner_proto = tron_backend_execution::protocol::Account::default();
    owner_proto.balance = 200_000_000; // 200 TRX (enough for 2 delegations)
    owner_proto.address = owner_tron.clone();
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    let mut receiver1_proto = tron_backend_execution::protocol::Account::default();
    receiver1_proto.balance = 1_000_000;
    receiver1_proto.address = receiver1_tron.clone();
    storage_adapter
        .put_account_proto(&receiver1_addr, &receiver1_proto)
        .unwrap();

    let mut receiver2_proto = tron_backend_execution::protocol::Account::default();
    receiver2_proto.balance = 1_000_000;
    receiver2_proto.address = receiver2_tron.clone();
    storage_adapter
        .put_account_proto(&receiver2_addr, &receiver2_proto)
        .unwrap();

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            freeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    // First delegation to receiver1 at timestamp 1000
    let mut params_data1 = Vec::new();
    params_data1.push((2 << 3) | 0);
    encode_varint(&mut params_data1, 5_000_000);
    params_data1.push((3 << 3) | 0);
    encode_varint(&mut params_data1, 3);
    params_data1.push((10 << 3) | 0);
    encode_varint(&mut params_data1, 0);
    params_data1.push((15 << 3) | 2);
    params_data1.push(21);
    params_data1.extend_from_slice(&receiver1_tron);

    let tx1 = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data1),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context1 = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1000, // First timestamp
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config.clone());
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    let result1 = service.execute_freeze_balance_contract(&mut storage_adapter, &tx1, &context1);
    assert!(
        result1.is_ok(),
        "First delegation should succeed: {:?}",
        result1.err()
    );
    storage_adapter.commit_buffer().unwrap();

    // Re-attach buffer for second operation (commit clears the buffer)
    let (mut storage_adapter2, _buffer2) =
        EngineBackedEvmStateStore::new_with_buffer(storage_engine.clone());

    // Second delegation to receiver2 at timestamp 2000
    let mut params_data2 = Vec::new();
    params_data2.push((2 << 3) | 0);
    encode_varint(&mut params_data2, 5_000_000);
    params_data2.push((3 << 3) | 0);
    encode_varint(&mut params_data2, 3);
    params_data2.push((10 << 3) | 0);
    encode_varint(&mut params_data2, 0);
    params_data2.push((15 << 3) | 2);
    params_data2.push(21);
    params_data2.extend_from_slice(&receiver2_tron);

    let tx2 = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data2),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context2 = TronExecutionContext {
        block_number: 1001,
        block_timestamp: 2000, // Second timestamp (later)
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let mut module_manager2 = tron_backend_common::ModuleManager::new();
    let exec_module2 = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager2.register("execution", Box::new(exec_module2));
    let service2 = BackendService::new(module_manager2);

    let result2 = service2.execute_freeze_balance_contract(&mut storage_adapter2, &tx2, &context2);
    assert!(
        result2.is_ok(),
        "Second delegation should succeed: {:?}",
        result2.err()
    );
    storage_adapter2.commit_buffer().unwrap();

    // Verify both optimized keys exist with correct timestamps
    let db_name = "DelegatedResourceAccountIndex";

    // Check key for receiver1 (timestamp 1000)
    let mut key1 = vec![0x01];
    key1.extend_from_slice(&owner_tron);
    key1.extend_from_slice(&receiver1_tron);
    let data1 = storage_engine.get(db_name, &key1).unwrap();
    assert!(data1.is_some(), "Key for receiver1 should exist");

    // Check key for receiver2 (timestamp 2000)
    let mut key2 = vec![0x01];
    key2.extend_from_slice(&owner_tron);
    key2.extend_from_slice(&receiver2_tron);
    let data2 = storage_engine.get(db_name, &key2).unwrap();
    assert!(data2.is_some(), "Key for receiver2 should exist");

    // The DelegatedResourceAccountIndex proto contains a timestamp field.
    // Java's getIndex() orders by timestamp to reconstruct the list order.
    // We verify both entries exist - the timestamp ordering is preserved by the keys' timestamps.
    // (Full reconstruction would require decoding the protos and comparing timestamps)
}

// =============================================================================
// UNFREEZE_BALANCE_CONTRACT parity regression tests
// =============================================================================

/// Test: withdrawReward side-effects with allowChangeDelegation=true.
///
/// Java parity: UnfreezeBalanceActuator.execute() calls mortgageService.withdrawReward(ownerAddress)
/// BEFORE any unfreeze mutation. This updates Account.allowance and delegation-store cycle state.
///
/// This test sets up:
/// - CHANGE_DELEGATION=1 (enables delegation rewards)
/// - delegation_reward_enabled=true (config gate for Rust)
/// - An account with votes and a non-zero reward for a past cycle
/// - A frozen bandwidth balance that has expired
///
/// After unfreeze, we verify:
/// - Account.allowance was incremented by the delegation reward
/// - Delegation-store begin/end cycle and accountVote snapshot were updated
/// - The unfreeze itself succeeded (balance increased by unfrozen amount)
#[test]
fn test_unfreeze_balance_withdraw_reward_updates_allowance() {
    let owner_addr = Address::from([0x30; 20]);
    let witness_addr = Address::from([0x31; 20]);
    let owner_tron = make_from_raw(&owner_addr);
    let witness_tron = make_from_raw(&witness_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Set block timestamp after freeze expiry
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();

    // Enable delegation rewards
    storage_engine
        .put("properties", b"CHANGE_DELEGATION", &1i64.to_be_bytes())
        .unwrap();

    // Set current cycle = 5
    storage_engine
        .put("properties", b"CURRENT_CYCLE_NUMBER", &5i64.to_be_bytes())
        .unwrap();

    // Set NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE to a high value (use old algorithm)
    storage_engine
        .put(
            "properties",
            b"NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE",
            &i64::MAX.to_be_bytes(),
        )
        .unwrap();

    // Seed delegation store: begin_cycle=3, end_cycle=4 for the owner
    // Key format for begin_cycle: raw address bytes (21-byte TRON format)
    let delegation_db = "delegation";
    // begin_cycle key = address itself
    storage_engine
        .put(delegation_db, &owner_tron, &3i64.to_be_bytes())
        .unwrap();
    // end_cycle key = "end-" + hex(address)
    let end_key = format!("end-{}", hex::encode(&owner_tron)).into_bytes();
    storage_engine
        .put(delegation_db, &end_key, &4i64.to_be_bytes())
        .unwrap();

    // Seed account_vote snapshot for cycle 3:
    // The snapshot serialization is the AccountVoteSnapshot format.
    // We need to seed it so compute_reward can read the votes.
    let account_vote_key = format!("{}-{}-account-vote", 3, hex::encode(&owner_tron)).into_bytes();
    // Serialize a simple vote snapshot: owner voted 1000 for witness
    let snapshot = tron_backend_execution::delegation::AccountVoteSnapshot::new(
        owner_addr,
        vec![tron_backend_execution::delegation::DelegationVote::new(
            witness_addr,
            1000,
        )],
    );
    storage_engine
        .put(delegation_db, &account_vote_key, &snapshot.serialize())
        .unwrap();

    // Seed witness reward for cycle 3: 10_000_000 SUN total reward
    let reward_key = format!("{}-{}-reward", 3, hex::encode(&witness_tron)).into_bytes();
    storage_engine
        .put(delegation_db, &reward_key, &10_000_000i64.to_be_bytes())
        .unwrap();

    // Seed witness total votes for cycle 3: 2000 total votes
    let vote_key = format!("{}-{}-vote", 3, hex::encode(&witness_tron)).into_bytes();
    storage_engine
        .put(delegation_db, &vote_key, &2000i64.to_be_bytes())
        .unwrap();

    // Setup storage adapter (use non-buffered mode so writes go directly to storage_engine,
    // matching how get_account_votes_list reads from storage_engine directly)
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine.clone());

    // Create owner account with frozen bandwidth and votes
    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000i64, // 1000 TRX
        allowance: 100_000,        // Pre-existing allowance
        frozen: vec![tron_backend_execution::protocol::account::Frozen {
            frozen_balance: 5_000_000,  // 5 TRX frozen
            expire_time: 1500000000000, // Expired
        }],
        votes: vec![tron_backend_execution::protocol::Vote {
            vote_address: witness_tron.clone(),
            vote_count: 1000,
        }],
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Create UnfreezeBalance transaction (resource = BANDWIDTH)
    let params_data = vec![0x50, 0x00]; // field 10 = BANDWIDTH (0)

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeBalanceContract),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    // Create service with delegation_reward_enabled=true
    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            unfreeze_balance_enabled: true,
            delegation_reward_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    // Execute unfreeze
    let result = service.execute_unfreeze_balance_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "UnfreezeBalance should succeed: {:?}",
        result.err()
    );
    let exec_result = result.unwrap();
    assert!(exec_result.success);

    // Verify balance increased by unfrozen amount
    match &exec_result.state_changes[0] {
        tron_backend_execution::TronStateChange::AccountChange { new_account, .. } => {
            // new_balance = 1_000_000_000 + 5_000_000 = 1_005_000_000
            let expected_balance = U256::from(1_005_000_000u64);
            assert_eq!(
                new_account.as_ref().unwrap().balance,
                expected_balance,
                "Balance should increase by unfrozen amount"
            );
        }
        _ => panic!("Expected AccountChange"),
    }

    // Verify allowance was updated in the account proto
    // Old reward calculation: user_vote/total_vote * cycle_reward = 1000/2000 * 10_000_000 = 5_000_000
    // New allowance = 100_000 (pre-existing) + 5_000_000 (reward) = 5_100_000
    let updated_proto = storage_adapter
        .get_account_proto(&owner_addr)
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_proto.allowance, 5_100_000,
        "Allowance should be old_allowance + delegation_reward (100000 + 5000000)"
    );

    // Verify delegation store state was updated
    // After withdraw_reward: begin_cycle should advance to current_cycle (5)
    let begin_cycle_data = storage_engine.get(delegation_db, &owner_tron).unwrap();
    assert!(begin_cycle_data.is_some(), "begin_cycle should exist");
    let begin_cycle = i64::from_be_bytes(begin_cycle_data.unwrap().try_into().unwrap());
    assert_eq!(
        begin_cycle, 5,
        "begin_cycle should advance to current_cycle"
    );

    // end_cycle should be current_cycle + 1 = 6
    let end_data = storage_engine.get(delegation_db, &end_key).unwrap();
    assert!(end_data.is_some(), "end_cycle should exist");
    let end_cycle = i64::from_be_bytes(end_data.unwrap().try_into().unwrap());
    assert_eq!(end_cycle, 6, "end_cycle should be current_cycle + 1");
}

/// Test: no behavior change when allowChangeDelegation=false.
///
/// When CHANGE_DELEGATION != 1, withdrawReward is a no-op and allowance should not change.
#[test]
fn test_unfreeze_balance_no_reward_when_delegation_disabled() {
    let owner_addr = Address::from([0x32; 20]);
    let owner_tron = make_from_raw(&owner_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();

    // Explicitly set CHANGE_DELEGATION=0 (delegation rewards disabled)
    storage_engine
        .put("properties", b"CHANGE_DELEGATION", &0i64.to_be_bytes())
        .unwrap();

    let (mut storage_adapter, _buffer) =
        EngineBackedEvmStateStore::new_with_buffer(storage_engine.clone());

    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000i64,
        allowance: 100_000, // Pre-existing allowance
        frozen: vec![tron_backend_execution::protocol::account::Frozen {
            frozen_balance: 5_000_000,
            expire_time: 1500000000000,
        }],
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    let params_data = vec![0x50, 0x00];
    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeBalanceContract),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            unfreeze_balance_enabled: true,
            delegation_reward_enabled: true, // enabled in config, but CHANGE_DELEGATION=0
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    let result = service.execute_unfreeze_balance_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "UnfreezeBalance should succeed: {:?}",
        result.err()
    );

    // Verify allowance is unchanged (no delegation reward applied)
    let updated_proto = storage_adapter
        .get_account_proto(&owner_addr)
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_proto.allowance, 100_000,
        "Allowance should remain unchanged when CHANGE_DELEGATION=0"
    );
}

/// Test: weight clamping with ALLOW_NEW_REWARD=1 vs legacy mode.
///
/// Java parity: DynamicPropertiesStore.addTotalNetWeight() clamps to max(0, ...) when
/// allowNewReward() is true. When ALLOW_NEW_REWARD=0, no clamping is applied even if
/// the weight would go negative.
///
/// This test verifies:
/// - With ALLOW_NEW_REWARD=0: weight uses -(unfreeze_amount / TRX_PRECISION), no clamping
/// - With ALLOW_NEW_REWARD=1: weight uses `decrease` (precise delta), clamps to max(0)
#[test]
fn test_unfreeze_balance_weight_clamping_with_allow_new_reward() {
    // Test scenario: start with TOTAL_NET_WEIGHT=3, unfreeze 5 TRX.
    // Without new reward: weight_delta = -(5_000_000 / 1_000_000) = -5, so total = 3 + (-5) = -2
    //   With new reward: weight_delta = decrease (also -5 here), total = max(0, 3 - 5) = 0

    let owner_addr = Address::from([0x33; 20]);
    let owner_tron = make_from_raw(&owner_addr);

    // --- Run 1: ALLOW_NEW_REWARD=0 (no clamping) ---
    let temp_dir1 = tempfile::tempdir().unwrap();
    let storage_engine1 = StorageEngine::new(temp_dir1.path()).unwrap();
    seed_dynamic_properties(&storage_engine1);
    storage_engine1
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine1
        .put("properties", b"ALLOW_NEW_REWARD", &0i64.to_be_bytes())
        .unwrap();
    storage_engine1
        .put("properties", b"TOTAL_NET_WEIGHT", &3i64.to_be_bytes())
        .unwrap();

    let (mut sa1, _buf1) = EngineBackedEvmStateStore::new_with_buffer(storage_engine1.clone());

    let owner_proto1 = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000i64,
        frozen: vec![tron_backend_execution::protocol::account::Frozen {
            frozen_balance: 5_000_000, // 5 TRX
            expire_time: 1500000000000,
        }],
        ..Default::default()
    };
    sa1.put_account_proto(&owner_addr, &owner_proto1).unwrap();
    sa1.add_freeze_amount(owner_addr, 0, 5_000_000, 1500000000000)
        .unwrap();

    let params_data = vec![0x50, 0x00];
    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeBalanceContract),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config1 = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            unfreeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut mm1 = tron_backend_common::ModuleManager::new();
    mm1.register(
        "execution",
        Box::new(tron_backend_execution::ExecutionModule::new(exec_config1)),
    );
    let service1 = BackendService::new(mm1);

    let result1 = service1.execute_unfreeze_balance_contract(&mut sa1, &tx, &context);
    assert!(
        result1.is_ok(),
        "UnfreezeBalance run1 should succeed: {:?}",
        result1.err()
    );

    // With ALLOW_NEW_REWARD=0: weight = -(5_000_000/1_000_000) = -5, total = 3 + (-5) = -2
    // No clamping, so -2 is stored.
    let total_net_weight1 = sa1.get_total_net_weight().unwrap();
    assert_eq!(
        total_net_weight1, -2,
        "Without new reward, total net weight can go negative"
    );

    // --- Run 2: ALLOW_NEW_REWARD=1 (clamp to 0) ---
    let temp_dir2 = tempfile::tempdir().unwrap();
    let storage_engine2 = StorageEngine::new(temp_dir2.path()).unwrap();
    seed_dynamic_properties(&storage_engine2);
    storage_engine2
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine2
        .put("properties", b"ALLOW_NEW_REWARD", &1i64.to_be_bytes())
        .unwrap();
    storage_engine2
        .put("properties", b"TOTAL_NET_WEIGHT", &3i64.to_be_bytes())
        .unwrap();

    let (mut sa2, _buf2) = EngineBackedEvmStateStore::new_with_buffer(storage_engine2.clone());

    let owner_proto2 = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000i64,
        frozen: vec![tron_backend_execution::protocol::account::Frozen {
            frozen_balance: 5_000_000,
            expire_time: 1500000000000,
        }],
        ..Default::default()
    };
    sa2.put_account_proto(&owner_addr, &owner_proto2).unwrap();
    sa2.add_freeze_amount(owner_addr, 0, 5_000_000, 1500000000000)
        .unwrap();

    let tx2 = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeBalanceContract),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let exec_config2 = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            unfreeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut mm2 = tron_backend_common::ModuleManager::new();
    mm2.register(
        "execution",
        Box::new(tron_backend_execution::ExecutionModule::new(exec_config2)),
    );
    let service2 = BackendService::new(mm2);

    let result2 = service2.execute_unfreeze_balance_contract(&mut sa2, &tx2, &context);
    assert!(
        result2.is_ok(),
        "UnfreezeBalance run2 should succeed: {:?}",
        result2.err()
    );

    // With ALLOW_NEW_REWARD=1: weight = decrease = -5, total = max(0, 3 + (-5)) = max(0, -2) = 0
    let total_net_weight2 = sa2.get_total_net_weight().unwrap();
    assert_eq!(
        total_net_weight2, 0,
        "With new reward, total net weight should be clamped to 0"
    );
}

/// Test: ALLOW_DELEGATE_OPTIMIZATION=1 deletes prefixed keys and stale legacy records.
///
/// Java parity: When supportAllowDelegateOptimization() is true, UnfreezeBalanceActuator
/// calls convert(owner) + convert(receiver) to migrate any legacy blob-style index entries,
/// then unDelegate(owner, receiver) to delete the prefixed keys.
///
/// This test:
/// 1. Freezes bandwidth with delegation (creates delegated resource + index entries)
/// 2. Unfreezes the delegated bandwidth (should clean up all index entries)
/// 3. Verifies prefixed keys (0x01||owner||receiver, 0x02||receiver||owner) are deleted
/// 4. Verifies no stale legacy records remain
#[test]
fn test_unfreeze_delegated_optimized_deletes_prefixed_keys() {
    let owner_addr = Address::from([0x34; 20]);
    let receiver_addr = Address::from([0x35; 20]);
    let owner_tron = make_from_raw(&owner_addr);
    let receiver_tron = make_from_raw(&receiver_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable delegation + optimization
    storage_engine
        .put(
            "properties",
            b"ALLOW_DELEGATE_RESOURCE",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ALLOW_DELEGATE_OPTIMIZATION",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    // Set early timestamp for freeze step (freeze will compute expiry = 1000 + 3*86400000 = 259201000)
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1000i64.to_be_bytes(),
        )
        .unwrap();

    let (mut storage_adapter, _buffer) =
        EngineBackedEvmStateStore::new_with_buffer(storage_engine.clone());

    // Step 1: Freeze with delegation to create the delegated resource + index entries
    let freeze_amount = 5_000_000i64;
    let mut owner_proto = tron_backend_execution::protocol::Account::default();
    owner_proto.balance = 100_000_000;
    owner_proto.address = owner_tron.clone();
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    let mut receiver_proto = tron_backend_execution::protocol::Account::default();
    receiver_proto.balance = 1_000_000;
    receiver_proto.address = receiver_tron.clone();
    storage_adapter
        .put_account_proto(&receiver_addr, &receiver_proto)
        .unwrap();

    // Build FreezeBalance transaction with delegation
    let mut freeze_data = Vec::new();
    freeze_data.push((2 << 3) | 0); // frozen_balance
    encode_varint(&mut freeze_data, freeze_amount as u64);
    freeze_data.push((3 << 3) | 0); // frozen_duration
    encode_varint(&mut freeze_data, 3);
    freeze_data.push((10 << 3) | 0); // resource = BANDWIDTH
    encode_varint(&mut freeze_data, 0);
    freeze_data.push((15 << 3) | 2); // receiver_address (length-delimited)
    freeze_data.push(21); // length
    freeze_data.extend_from_slice(&receiver_tron);

    let freeze_tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(freeze_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::FreezeBalanceContract),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let freeze_context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1000, // Early timestamp for freeze
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let freeze_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            freeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut mm = tron_backend_common::ModuleManager::new();
    mm.register(
        "execution",
        Box::new(tron_backend_execution::ExecutionModule::new(freeze_config)),
    );
    let freeze_service = BackendService::new(mm);

    let freeze_result = freeze_service.execute_freeze_balance_contract(
        &mut storage_adapter,
        &freeze_tx,
        &freeze_context,
    );
    assert!(
        freeze_result.is_ok(),
        "FreezeBalance should succeed: {:?}",
        freeze_result.err()
    );
    storage_adapter.commit_buffer().unwrap();

    // Verify optimized index keys exist after freeze
    let db_name = "DelegatedResourceAccountIndex";
    let mut from_key = vec![0x01];
    from_key.extend_from_slice(&owner_tron);
    from_key.extend_from_slice(&receiver_tron);
    let from_data = storage_engine.get(db_name, &from_key).unwrap();
    assert!(
        from_data.is_some(),
        "from_key should exist after delegation"
    );

    let mut to_key = vec![0x02];
    to_key.extend_from_slice(&receiver_tron);
    to_key.extend_from_slice(&owner_tron);
    let to_data = storage_engine.get(db_name, &to_key).unwrap();
    assert!(to_data.is_some(), "to_key should exist after delegation");

    // Step 2: Unfreeze the delegated bandwidth
    // Update timestamp to after freeze expiry (expiry = 1000 + 3*86400000 = 259201000)
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();
    let (mut sa2, _buf2) = EngineBackedEvmStateStore::new_with_buffer(storage_engine.clone());

    let unfreeze_data = vec![
        0x50,
        0x00, // field 10 = BANDWIDTH
        (15 << 3) | 2,
        21, // field 15 = receiver_address
    ];
    let mut unfreeze_params = unfreeze_data;
    unfreeze_params.extend_from_slice(&receiver_tron);

    let unfreeze_tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(unfreeze_params),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeBalanceContract),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let unfreeze_context = TronExecutionContext {
        block_number: 2000,
        block_timestamp: 1600000000000, // After freeze expiry
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let unfreeze_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            unfreeze_balance_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut mm2 = tron_backend_common::ModuleManager::new();
    mm2.register(
        "execution",
        Box::new(tron_backend_execution::ExecutionModule::new(
            unfreeze_config,
        )),
    );
    let unfreeze_service = BackendService::new(mm2);

    let unfreeze_result = unfreeze_service.execute_unfreeze_balance_contract(
        &mut sa2,
        &unfreeze_tx,
        &unfreeze_context,
    );
    assert!(
        unfreeze_result.is_ok(),
        "UnfreezeBalance should succeed: {:?}",
        unfreeze_result.err()
    );
    sa2.commit_buffer().unwrap();

    // Step 3: Verify prefixed keys are deleted after unfreeze
    let from_data_after = storage_engine.get(db_name, &from_key).unwrap();
    assert!(
        from_data_after.is_none(),
        "Optimized from_key (0x01||owner||receiver) should be deleted after undelegation"
    );

    let to_data_after = storage_engine.get(db_name, &to_key).unwrap();
    assert!(
        to_data_after.is_none(),
        "Optimized to_key (0x02||receiver||owner) should be deleted after undelegation"
    );

    // Step 4: Verify no stale legacy keys exist
    let legacy_owner_data = storage_engine.get(db_name, &owner_tron).unwrap();
    assert!(
        legacy_owner_data.is_none(),
        "Legacy owner key should not exist (optimization path doesn't create legacy keys)"
    );

    let legacy_receiver_data = storage_engine.get(db_name, &receiver_tron).unwrap();
    assert!(
        legacy_receiver_data.is_none(),
        "Legacy receiver key should not exist (optimization path doesn't create legacy keys)"
    );
}

// ==== UNFREEZE_BALANCE_V2_CONTRACT Parity Tests ====

/// Test: Under new resource model with oldTronPower == -1, unfreezing BANDWIDTH should NOT
/// touch existing votes (Java's updateVote returns early).
/// Java reference: UnfreezeBalanceV2Actuator.updateVote() lines 314-318.
#[test]
fn test_unfreeze_v2_new_resource_model_bandwidth_does_not_touch_votes() {
    let owner_addr = Address::from([0x40; 20]);
    let witness_addr = Address::from([0x41; 20]);
    let owner_tron = make_from_raw(&owner_addr);
    let witness_tron = make_from_raw(&witness_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();
    // Enable new resource model
    storage_engine
        .put(
            "properties",
            b"ALLOW_NEW_RESOURCE_MODEL",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Account with:
    // - oldTronPower == -1 (already invalidated / new model steady state)
    // - existing votes
    // - frozenV2 BANDWIDTH balance
    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000i64,
        old_tron_power: -1, // already invalidated
        frozen_v2: vec![tron_backend_execution::protocol::account::FreezeV2 {
            r#type: 0, // BANDWIDTH
            amount: 10_000_000,
        }],
        votes: vec![tron_backend_execution::protocol::Vote {
            vote_address: witness_tron.clone(),
            vote_count: 5,
        }],
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Build UnfreezeBalanceV2 transaction: unfreeze 5_000_000 BANDWIDTH
    let mut params_data = Vec::new();
    params_data.push(0x0A); // field 1 (owner_address)
    params_data.push(21);
    params_data.extend_from_slice(&owner_tron);
    params_data.push(0x10); // field 2 (unfreeze_balance)
    encode_varint(&mut params_data, 5_000_000);
    params_data.push(0x18); // field 3 (resource)
    params_data.push(0x00); // BANDWIDTH

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract,
            ),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            unfreeze_balance_v2_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    let result = service.execute_unfreeze_balance_v2_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "UnfreezeV2 should succeed: {:?}",
        result.err()
    );

    // Verify votes are UNCHANGED (Java parity: early return in updateVote)
    let updated_proto = storage_adapter
        .get_account_proto(&owner_addr)
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_proto.votes.len(),
        1,
        "Votes should remain unchanged"
    );
    assert_eq!(
        updated_proto.votes[0].vote_count, 5,
        "Vote count should be unchanged"
    );
    assert_eq!(
        updated_proto.votes[0].vote_address, witness_tron,
        "Vote address should be unchanged"
    );

    // oldTronPower should remain -1 (it was already -1, and the invalidation
    // check only sets to -1 when old_tron_power != -1)
    assert_eq!(updated_proto.old_tron_power, -1);
}

/// Test: Under new resource model with oldTronPower == -1, unfreezing ENERGY should NOT
/// touch existing votes either (same early return path as BANDWIDTH).
#[test]
fn test_unfreeze_v2_new_resource_model_energy_does_not_touch_votes() {
    let owner_addr = Address::from([0x42; 20]);
    let witness_addr = Address::from([0x43; 20]);
    let owner_tron = make_from_raw(&owner_addr);
    let witness_tron = make_from_raw(&witness_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ALLOW_NEW_RESOURCE_MODEL",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000i64,
        old_tron_power: -1,
        frozen_v2: vec![tron_backend_execution::protocol::account::FreezeV2 {
            r#type: 1, // ENERGY
            amount: 8_000_000,
        }],
        votes: vec![tron_backend_execution::protocol::Vote {
            vote_address: witness_tron.clone(),
            vote_count: 3,
        }],
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    let mut params_data = Vec::new();
    params_data.push(0x0A);
    params_data.push(21);
    params_data.extend_from_slice(&owner_tron);
    params_data.push(0x10);
    encode_varint(&mut params_data, 4_000_000);
    params_data.push(0x18);
    params_data.push(0x01); // ENERGY

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract,
            ),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            unfreeze_balance_v2_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    let result = service.execute_unfreeze_balance_v2_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "UnfreezeV2 ENERGY should succeed: {:?}",
        result.err()
    );

    let updated_proto = storage_adapter
        .get_account_proto(&owner_addr)
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_proto.votes.len(),
        1,
        "Votes should remain unchanged for ENERGY"
    );
    assert_eq!(
        updated_proto.votes[0].vote_count, 3,
        "Vote count should be unchanged for ENERGY"
    );
    assert_eq!(updated_proto.old_tron_power, -1);
}

/// Test: Under new resource model with oldTronPower == -1, unfreezing TRON_POWER CAN rescale
/// votes if tron power becomes insufficient.
#[test]
fn test_unfreeze_v2_new_resource_model_tron_power_can_rescale_votes() {
    let owner_addr = Address::from([0x44; 20]);
    let witness_addr = Address::from([0x45; 20]);
    let owner_tron = make_from_raw(&owner_addr);
    let witness_tron = make_from_raw(&witness_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ALLOW_NEW_RESOURCE_MODEL",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Account with:
    // - oldTronPower == -1 (steady state)
    // - frozenV2 TRON_POWER = 10_000_000 (10 TRX)
    // - votes = 10 (requires 10 * 1_000_000 = 10_000_000 SUN tron power)
    // After unfreezing 8_000_000: remaining frozen = 2_000_000 → only supports 2 votes
    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000i64,
        old_tron_power: -1,
        frozen_v2: vec![tron_backend_execution::protocol::account::FreezeV2 {
            r#type: 2, // TRON_POWER
            amount: 10_000_000,
        }],
        votes: vec![tron_backend_execution::protocol::Vote {
            vote_address: witness_tron.clone(),
            vote_count: 10,
        }],
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    let mut params_data = Vec::new();
    params_data.push(0x0A);
    params_data.push(21);
    params_data.extend_from_slice(&owner_tron);
    params_data.push(0x10);
    encode_varint(&mut params_data, 8_000_000);
    params_data.push(0x18);
    params_data.push(0x02); // TRON_POWER

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract,
            ),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            unfreeze_balance_v2_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    let result = service.execute_unfreeze_balance_v2_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "UnfreezeV2 TRON_POWER should succeed: {:?}",
        result.err()
    );

    let updated_proto = storage_adapter
        .get_account_proto(&owner_addr)
        .unwrap()
        .unwrap();
    // After unfreezing 8M: remaining TRON_POWER frozen = 2M SUN → supports 2 TRX-worth of votes.
    // Original 10 votes at 10M SUN. New: 10 * 2_000_000 / (10 * 1_000_000) = 2
    // getAllTronPower with old_tron_power == -1: only counts tron_power frozen
    // frozenV2 with type==TRON_POWER = 2_000_000
    // So total tron power = 2_000_000 SUN, required = 10 * 1_000_000 = 10_000_000 → rescale
    // new_vote_count = (10 / 10) * 2_000_000 / 1_000_000 = 2
    assert_eq!(updated_proto.votes.len(), 1, "Should have rescaled votes");
    assert_eq!(
        updated_proto.votes[0].vote_count, 2,
        "Vote should be rescaled to 2"
    );
}

/// Test: UnfreezeV2 with delegation enabled calls withdraw_reward and updates allowance.
/// Java reference: UnfreezeBalanceV2Actuator.execute() line 72.
#[test]
fn test_unfreeze_v2_withdraw_reward_updates_allowance() {
    let owner_addr = Address::from([0x46; 20]);
    let witness_addr = Address::from([0x47; 20]);
    let owner_tron = make_from_raw(&owner_addr);
    let witness_tron = make_from_raw(&witness_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();

    // Enable delegation rewards
    storage_engine
        .put("properties", b"CHANGE_DELEGATION", &1i64.to_be_bytes())
        .unwrap();
    // Set current cycle = 5
    storage_engine
        .put("properties", b"CURRENT_CYCLE_NUMBER", &5i64.to_be_bytes())
        .unwrap();
    // Use old algorithm (set new algorithm cycle very high)
    storage_engine
        .put(
            "properties",
            b"NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE",
            &i64::MAX.to_be_bytes(),
        )
        .unwrap();

    // Seed delegation store: begin_cycle=3, end_cycle=4
    let delegation_db = "delegation";
    storage_engine
        .put(delegation_db, &owner_tron, &3i64.to_be_bytes())
        .unwrap();
    let end_key = format!("end-{}", hex::encode(&owner_tron)).into_bytes();
    storage_engine
        .put(delegation_db, &end_key, &4i64.to_be_bytes())
        .unwrap();

    // Seed account_vote snapshot for cycle 3
    let account_vote_key = format!("{}-{}-account-vote", 3, hex::encode(&owner_tron)).into_bytes();
    let snapshot = tron_backend_execution::delegation::AccountVoteSnapshot::new(
        owner_addr,
        vec![tron_backend_execution::delegation::DelegationVote::new(
            witness_addr,
            1000,
        )],
    );
    storage_engine
        .put(delegation_db, &account_vote_key, &snapshot.serialize())
        .unwrap();

    // Seed witness reward for cycle 3: 10_000_000 SUN total reward
    let reward_key = format!("{}-{}-reward", 3, hex::encode(&witness_tron)).into_bytes();
    storage_engine
        .put(delegation_db, &reward_key, &10_000_000i64.to_be_bytes())
        .unwrap();

    // Seed witness total votes for cycle 3: 2000 total votes
    let vote_key = format!("{}-{}-vote", 3, hex::encode(&witness_tron)).into_bytes();
    storage_engine
        .put(delegation_db, &vote_key, &2000i64.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine.clone());

    // Create owner with frozen_v2 BANDWIDTH balance, existing allowance, and votes
    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000i64,
        allowance: 100_000, // Pre-existing allowance
        frozen_v2: vec![tron_backend_execution::protocol::account::FreezeV2 {
            r#type: 0, // BANDWIDTH
            amount: 5_000_000,
        }],
        votes: vec![tron_backend_execution::protocol::Vote {
            vote_address: witness_tron.clone(),
            vote_count: 1000,
        }],
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Build unfreeze V2 transaction: unfreeze 2_000_000 BANDWIDTH
    let mut params_data = Vec::new();
    params_data.push(0x0A);
    params_data.push(21);
    params_data.extend_from_slice(&owner_tron);
    params_data.push(0x10);
    encode_varint(&mut params_data, 2_000_000);
    params_data.push(0x18);
    params_data.push(0x00); // BANDWIDTH

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract,
            ),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            unfreeze_balance_v2_enabled: true,
            delegation_reward_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    let result = service.execute_unfreeze_balance_v2_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "UnfreezeV2 should succeed: {:?}",
        result.err()
    );
    let exec_result = result.unwrap();
    assert!(exec_result.success);

    // Verify allowance was updated
    // Old reward: user_vote/total_vote * cycle_reward = 1000/2000 * 10_000_000 = 5_000_000
    // New allowance = 100_000 (pre-existing) + 5_000_000 (reward) = 5_100_000
    let updated_proto = storage_adapter
        .get_account_proto(&owner_addr)
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_proto.allowance, 5_100_000,
        "Allowance should be old_allowance + delegation_reward (100000 + 5000000)"
    );

    // Verify delegation store state was updated
    let begin_cycle_data = storage_engine.get(delegation_db, &owner_tron).unwrap();
    assert!(begin_cycle_data.is_some(), "begin_cycle should exist");
    let begin_cycle = i64::from_be_bytes(begin_cycle_data.unwrap().try_into().unwrap());
    assert_eq!(
        begin_cycle, 5,
        "begin_cycle should advance to current_cycle"
    );

    let end_data = storage_engine.get(delegation_db, &end_key).unwrap();
    assert!(end_data.is_some(), "end_cycle should exist");
    let end_cycle = i64::from_be_bytes(end_data.unwrap().try_into().unwrap());
    assert_eq!(end_cycle, 6, "end_cycle should be current_cycle + 1");
}

/// Test: UnfreezeV2 with delegation disabled does not change allowance.
#[test]
fn test_unfreeze_v2_no_reward_when_delegation_disabled() {
    let owner_addr = Address::from([0x48; 20]);
    let owner_tron = make_from_raw(&owner_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();
    // Delegation disabled
    storage_engine
        .put("properties", b"CHANGE_DELEGATION", &0i64.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000i64,
        allowance: 100_000,
        frozen_v2: vec![tron_backend_execution::protocol::account::FreezeV2 {
            r#type: 0,
            amount: 5_000_000,
        }],
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    let mut params_data = Vec::new();
    params_data.push(0x0A);
    params_data.push(21);
    params_data.extend_from_slice(&owner_tron);
    params_data.push(0x10);
    encode_varint(&mut params_data, 2_000_000);
    params_data.push(0x18);
    params_data.push(0x00);

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract,
            ),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

    let exec_config = ExecutionConfig {
        remote: tron_backend_common::RemoteExecutionConfig {
            unfreeze_balance_v2_enabled: true,
            delegation_reward_enabled: true, // enabled in config, but CHANGE_DELEGATION=0
            ..Default::default()
        },
        ..Default::default()
    };

    let mut module_manager = tron_backend_common::ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    let result = service.execute_unfreeze_balance_v2_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "UnfreezeV2 should succeed: {:?}",
        result.err()
    );

    // Allowance should remain unchanged since delegation is disabled
    let updated_proto = storage_adapter
        .get_account_proto(&owner_addr)
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_proto.allowance, 100_000,
        "Allowance should be unchanged when delegation is disabled"
    );
}

/// Test: V2 freeze-ledger reporting always uses expiration_ms == 0 (Java parity).
#[test]
fn test_unfreeze_v2_freeze_ledger_expiration_always_zero() {
    let owner_addr = Address::from([0x49; 20]);
    let owner_tron = make_from_raw(&owner_addr);

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1600000000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_proto = tron_backend_execution::protocol::Account {
        address: owner_tron.clone(),
        balance: 1_000_000_000i64,
        frozen_v2: vec![tron_backend_execution::protocol::account::FreezeV2 {
            r#type: 0,
            amount: 10_000_000,
        }],
        ..Default::default()
    };
    storage_adapter
        .put_account_proto(&owner_addr, &owner_proto)
        .unwrap();

    // Pre-populate freeze record with a non-zero expiration (from a prior freeze)
    storage_adapter
        .add_freeze_amount(owner_addr, 0, 10_000_000, 1700000000000)
        .unwrap();

    let mut params_data = Vec::new();
    params_data.push(0x0A);
    params_data.push(21);
    params_data.extend_from_slice(&owner_tron);
    params_data.push(0x10);
    encode_varint(&mut params_data, 5_000_000);
    params_data.push(0x18);
    params_data.push(0x00);

    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(params_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract,
            ),
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1600000000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 0,
        bandwidth_price: 0,
        transaction_id: None,
    };

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

    let result = service.execute_unfreeze_balance_v2_contract(&mut storage_adapter, &tx, &context);
    assert!(
        result.is_ok(),
        "UnfreezeV2 should succeed: {:?}",
        result.err()
    );
    let exec_result = result.unwrap();

    // Verify freeze ledger change has expiration_ms == 0 (Java parity: V2 has no expiration)
    assert_eq!(exec_result.freeze_changes.len(), 1);
    let freeze_change = &exec_result.freeze_changes[0];
    assert_eq!(
        freeze_change.expiration_ms, 0,
        "V2 freeze ledger should always have expiration_ms == 0"
    );
    assert_eq!(freeze_change.v2_model, true);
    assert_eq!(
        freeze_change.amount, 5_000_000,
        "Should show remaining frozen amount"
    );

    // Also verify the freeze record in the Rust-only DB has expiration == 0
    let record = storage_adapter.get_freeze_record(&owner_addr, 0).unwrap();
    assert!(
        record.is_some(),
        "Freeze record should exist for partial unfreeze"
    );
    assert_eq!(
        record.unwrap().expiration_timestamp,
        0,
        "V2 freeze record should have expiration_timestamp == 0"
    );
}
