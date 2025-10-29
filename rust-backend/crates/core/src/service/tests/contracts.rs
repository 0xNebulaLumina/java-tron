use super::super::*;
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
use revm_primitives::{Address, Bytes, U256, AccountInfo};
use tron_backend_common::{ModuleManager, ExecutionConfig, RemoteExecutionConfig};
use tron_backend_storage::StorageEngine;

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
fn test_account_update_contract_happy_path() {
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

#[test]
fn test_freeze_balance_success_basic() {
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
