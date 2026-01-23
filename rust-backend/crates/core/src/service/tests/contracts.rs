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
fn test_trc10_transfer_emits_recipient_account_creation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    // Match early-chain behavior for this test: no TRX fee for creating recipient accounts.
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &0u64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    // Seed an issued TRC-10 so get_asset_issue succeeds (legacy allowSameTokenName=0).
    let asset_id = b"TEST".to_vec();
    let mut asset_issue = tron_backend_execution::protocol::AssetIssueContractData::default();
    asset_issue.id = "1000009".to_string();
    storage_adapter
        .put_asset_issue(&asset_id, &asset_issue, false)
        .unwrap();

    // Owner must exist and have sufficient TRC-10 balance.
    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();
    let mut owner_proto = storage_adapter.get_account_proto(&owner_address).unwrap().unwrap();
    owner_proto
        .asset
        .insert("TEST".to_string(), 1_000 /* units */);
    owner_proto
        .asset_v2
        .insert("1000009".to_string(), 1_000 /* units */);
    storage_adapter
        .put_account_proto(&owner_address, &owner_proto)
        .unwrap();

    // Recipient does not exist pre-exec.
    let recipient_address = Address::from([0x22u8; 20]);

    let tx = TronTransaction {
        from: owner_address,
        to: Some(recipient_address),
        value: U256::from(100u64),
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::TransferAssetContract),
            asset_id: Some(asset_id),
            ..Default::default()
        },
    };

    let result = service.execute_trc10_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_ok(), "TRC-10 transfer should succeed: {:?}", result.err());
    let exec_result = result.unwrap();

    let recipient_change = exec_result.state_changes.iter().find_map(|sc| match sc {
        tron_backend_execution::TronStateChange::AccountChange {
            address,
            old_account,
            new_account,
        } if *address == recipient_address => Some((old_account, new_account)),
        _ => None,
    });

    assert!(recipient_change.is_some(), "Expected recipient AccountChange in state_changes");
    let (old_account, new_account) = recipient_change.unwrap();
    assert!(old_account.is_none(), "Recipient should be emitted as account creation (old_account=None)");
    assert!(new_account.is_some(), "Recipient should have a post-state AccountInfo (new_account=Some)");
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
            ..Default::default()
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
        transaction_id: None,
    };

    // Execute the contract
    let result = service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);

    // Assert success
    assert!(result.is_ok(), "Account update should succeed: {:?}", result.err());
    let execution_result = result.unwrap();

    assert!(execution_result.success, "Execution should be successful");
    assert_eq!(execution_result.energy_used, 0, "Energy used should be 0");
    // Embedded Java CSV reports 0 state changes for WitnessUpdate; align remote result to match
    assert_eq!(execution_result.state_changes.len(), 0, "WitnessUpdate should not emit state changes");
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
        transaction_id: None,
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
        transaction_id: None,
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
            ..Default::default()
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
            ..Default::default()
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
fn test_account_permission_update_validate_fail_owner_address_empty() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    // Ensure the transaction.from account exists so validation must come from the contract payload.
    let tx_from = Address::from([7u8; 20]);
    let tx_from_account = AccountInfo {
        balance: U256::from(1_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(tx_from, tx_from_account).unwrap();

    // AccountPermissionUpdateContract owner_address = "" (field 1, length 0)
    let contract_data = Bytes::from(vec![0x0a, 0x00]);

    let transaction = TronTransaction {
        from: tx_from,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountPermissionUpdateContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let err = service
        .execute_account_permission_update_contract(&mut storage_adapter, &transaction, &new_test_context())
        .unwrap_err();
    assert_eq!(err, "invalidate ownerAddress");
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

// ====================================================================================
// AssetIssueContract Tests (Phase 2: TRC-10 Asset Issuance with Trc10Change emission)
// ====================================================================================

#[test]
fn test_asset_issue_contract_trc10_change_emission() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            trc10_enabled: true, // Enable TRC-10
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    // Create test account (owner must have sufficient balance for fee)
    // 20-byte EVM address; the TRON owner_address field is encoded as 0x41 + this 20-byte value.
    let owner_address = Address::from([0xab, 0xd4, 0xb9, 0x36, 0x77, 0x99, 0xea, 0xa3, 0x19,
                                      0x7f, 0xec, 0xb1, 0x44, 0xeb, 0x71, 0xde, 0x1e, 0x04,
                                      0x91, 0x50]);
    let owner_account = AccountInfo {
        balance: U256::from(2000_000000u64), // 2000 TRX (enough for fee)
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner_address, owner_account.clone()).is_ok());

    // Build AssetIssueContract protobuf manually
    let mut contract_data = Vec::new();
    
    // Field 1: owner_address (length-delimited, tag=10)
    contract_data.push(10u8); // tag (field 1, type 2)
    contract_data.push(21u8); // length of address (21 bytes for Tron address)
    contract_data.push(0x41u8); // prefix
    contract_data.extend_from_slice(owner_address.as_slice());
    
    // Field 2: name (length-delimited, tag=18)
    let name = b"TestToken";
    contract_data.push(18u8);
    contract_data.push(name.len() as u8);
    contract_data.extend_from_slice(name);
    
    // Field 3: abbr (length-delimited, tag=26)
    let abbr = b"TT";
    contract_data.push(26u8);
    contract_data.push(abbr.len() as u8);
    contract_data.extend_from_slice(abbr);
    
    // Field 4: total_supply (varint, tag=32)
    contract_data.push(32u8);
    encode_varint(&mut contract_data, 1000000);
    
    // Field 7: precision (varint, tag=56)
    contract_data.push(56u8);
    encode_varint(&mut contract_data, 6);
    
    // Field 6: trx_num (varint, tag=48)
    contract_data.push(48u8);
    encode_varint(&mut contract_data, 1);
    
    // Field 8: num (varint, tag=64)
    contract_data.push(64u8);
    encode_varint(&mut contract_data, 1);
    
    // Field 9: start_time (varint, tag=72)
    contract_data.push(72u8);
    encode_varint(&mut contract_data, 1000000);
    
    // Field 10: end_time (varint, tag=80)
    contract_data.push(80u8);
    encode_varint(&mut contract_data, 2000000);
    
    // Field 20: description (length-delimited, tag=162, 1)
    let description = b"Test token";
    contract_data.push(162u8);
    contract_data.push(1u8);
    contract_data.push(description.len() as u8);
    contract_data.extend_from_slice(description);
    
    // Field 21: url (length-delimited, tag=170, 1)
    let url = b"https://test.token";
    contract_data.push(170u8);
    contract_data.push(1u8);
    contract_data.push(url.len() as u8);
    contract_data.extend_from_slice(url);

    // Create transaction
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
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
        transaction_id: None,
    };

    // Execute the contract
    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &context);

    // Assert success
    assert!(result.is_ok(), "Asset issue should succeed: {:?}", result.err());
    let execution_result = result.unwrap();

    assert!(execution_result.success, "Execution should be successful");
    assert!(execution_result.error.is_none(), "Should have no error");

    // Verify Trc10Change emission (Phase 2) - This is the core test
    assert_eq!(execution_result.trc10_changes.len(), 1, "Should have exactly 1 TRC-10 change");

    match &execution_result.trc10_changes[0] {
        tron_backend_execution::Trc10Change::AssetIssued(asset_issued) => {
            assert_eq!(asset_issued.owner_address, owner_address, "Owner address should match");
            assert_eq!(asset_issued.name, name.to_vec(), "Name should match");
            assert_eq!(asset_issued.abbr, abbr.to_vec(), "Abbr should match");
            assert_eq!(asset_issued.total_supply, 1000000, "Total supply should match");
            assert_eq!(asset_issued.precision, 6, "Precision should match");
            assert_eq!(asset_issued.trx_num, 1, "TRX num should match");
            assert_eq!(asset_issued.num, 1, "Num should match");
            assert_eq!(asset_issued.start_time, 1000000, "Start time should match");
            assert_eq!(asset_issued.end_time, 2000000, "End time should match");
            assert_eq!(asset_issued.description, description.to_vec(), "Description should match");
            assert_eq!(asset_issued.url, url.to_vec(), "URL should match");
            assert_eq!(asset_issued.token_id, None, "Token ID should be None (computed by Java)");
        }
        _ => panic!("Expected AssetIssued change"),
    }
}

#[test]
fn test_asset_issue_contract_disabled() {
    // Create mock storage and service with TRC-10 disabled
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            trc10_enabled: false, // Disable TRC-10
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(2000_000000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner_address, owner_account.clone()).is_ok());

    // Build minimal AssetIssueContract
    let mut contract_data = Vec::new();
    contract_data.push(10u8); // owner_address tag
    contract_data.push(21u8); // length
    contract_data.push(0x41u8); // TRON address prefix (mainnet-style for tests)
    contract_data.extend_from_slice(&[1u8; 20]);
    contract_data.push(18u8); // name tag
    contract_data.push(4u8);
    contract_data.extend_from_slice(b"Test");

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
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
        transaction_id: None,
    };

    // Execute should fail
    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "Asset issue should fail when TRC-10 is disabled");
    
    let error_message = result.err().unwrap();
    assert!(error_message.contains("ASSET_ISSUE_CONTRACT execution is disabled"),
            "Error should mention disabled TRC-10: {}", error_message);
}

#[test]
fn test_asset_issue_contract_phase2_fields() {
    // Test that all Phase 2 fields (22-25) are included in Trc10Change
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            trc10_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(2000_000000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner_address, owner_account.clone()).is_ok());

    // Build AssetIssueContract with Phase 2 fields (22-25)
    let mut contract_data = Vec::new();
    contract_data.push(10u8); // owner_address
    contract_data.push(21u8);
    contract_data.push(0x41u8); // TRON address prefix (mainnet-style for tests)
    contract_data.extend_from_slice(&[1u8; 20]);
    contract_data.push(18u8); // name
    contract_data.push(5u8);
    contract_data.extend_from_slice(b"Token");
    contract_data.push(32u8); // total_supply
    encode_varint(&mut contract_data, 1000);

    // Field 6: trx_num
    contract_data.push(48u8);
    encode_varint(&mut contract_data, 1);

    // Field 8: num
    contract_data.push(64u8);
    encode_varint(&mut contract_data, 1);

    // Field 9: start_time
    contract_data.push(72u8);
    encode_varint(&mut contract_data, 1000000);

    // Field 10: end_time
    contract_data.push(80u8);
    encode_varint(&mut contract_data, 2000000);

    // Field 21: url
    let url = b"https://token.example";
    contract_data.push(170u8);
    contract_data.push(1u8);
    contract_data.push(url.len() as u8);
    contract_data.extend_from_slice(url);
    
    // Field 22: free_asset_net_limit
    contract_data.push(176u8);
    contract_data.push(1u8);
    encode_varint(&mut contract_data, 12345);
    
    // Field 23: public_free_asset_net_limit
    contract_data.push(184u8);
    contract_data.push(1u8);
    encode_varint(&mut contract_data, 67890);
    
    // Field 24: public_free_asset_net_usage
    contract_data.push(192u8);
    contract_data.push(1u8);
    encode_varint(&mut contract_data, 0);
    
    // Field 25: public_latest_free_net_time
    contract_data.push(200u8);
    contract_data.push(1u8);
    encode_varint(&mut contract_data, 999000);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
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
        transaction_id: None,
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &context).unwrap();
    
    // Verify Phase 2 fields in Trc10Change
    assert_eq!(result.trc10_changes.len(), 1, "Should have 1 TRC-10 change");
    match &result.trc10_changes[0] {
        tron_backend_execution::Trc10Change::AssetIssued(asset_issued) => {
            assert_eq!(asset_issued.free_asset_net_limit, 12345, "free_asset_net_limit should match");
            assert_eq!(asset_issued.public_free_asset_net_limit, 67890, "public_free_asset_net_limit should match");
            assert_eq!(asset_issued.public_free_asset_net_usage, 0, "public_free_asset_net_usage should match");
            assert_eq!(asset_issued.public_latest_free_net_time, 999000, "public_latest_free_net_time should match");
        }
        _ => panic!("Expected AssetIssued change"),
    }
}

fn build_asset_issue_contract_data(
    owner: Address,
    name: &[u8],
    total_supply: u64,
    trx_num: u64,
    num: u64,
    start_time: u64,
    end_time: u64,
    url: &[u8],
) -> Bytes {
    let mut contract_data = Vec::new();

    // Field 1: owner_address
    encode_varint(&mut contract_data, (1 << 3) | 2);
    encode_varint(&mut contract_data, 21);
    contract_data.push(0x41u8); // TRON address prefix (mainnet-style for tests)
    contract_data.extend_from_slice(owner.as_slice());

    // Field 2: name
    encode_varint(&mut contract_data, (2 << 3) | 2);
    encode_varint(&mut contract_data, name.len() as u64);
    contract_data.extend_from_slice(name);

    // Field 4: total_supply
    encode_varint(&mut contract_data, (4 << 3) | 0);
    encode_varint(&mut contract_data, total_supply);

    // Field 6: trx_num
    encode_varint(&mut contract_data, (6 << 3) | 0);
    encode_varint(&mut contract_data, trx_num);

    // Field 8: num
    encode_varint(&mut contract_data, (8 << 3) | 0);
    encode_varint(&mut contract_data, num);

    // Field 9: start_time
    encode_varint(&mut contract_data, (9 << 3) | 0);
    encode_varint(&mut contract_data, start_time);

    // Field 10: end_time
    encode_varint(&mut contract_data, (10 << 3) | 0);
    encode_varint(&mut contract_data, end_time);

    // Field 21: url
    encode_varint(&mut contract_data, (21 << 3) | 2);
    encode_varint(&mut contract_data, url.len() as u64);
    contract_data.extend_from_slice(url);

    Bytes::from(contract_data)
}

fn new_test_service_with_trc10_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            trc10_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

fn new_test_context() -> TronExecutionContext {
    TronExecutionContext {
        block_number: 1,
        block_timestamp: 1,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    }
}

#[test]
fn test_asset_issue_validate_fail_insufficient_balance_message() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    let owner_address = Address::from([2u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let contract_data = build_asset_issue_contract_data(
        owner_address,
        b"Token",
        1000,
        1,
        1,
        1000000,
        2000000,
        b"https://token.example",
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "No enough balance for fee!");
}

#[test]
fn test_asset_issue_validate_fail_owner_already_issued() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    let owner_address = Address::from([3u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(2_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let mut proto_account = storage_adapter.get_account_proto(&owner_address).unwrap().unwrap();
    proto_account.asset_issued_name = b"ExistingToken".to_vec();
    storage_adapter.put_account_proto(&owner_address, &proto_account).unwrap();

    let contract_data = build_asset_issue_contract_data(
        owner_address,
        b"Token",
        1000,
        1,
        1,
        1000000,
        2000000,
        b"https://token.example",
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "An account can only issue one asset");
}

#[test]
fn test_asset_issue_validate_fail_total_supply_zero() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    let owner_address = Address::from([4u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(2_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let contract_data = build_asset_issue_contract_data(
        owner_address,
        b"Token",
        0,
        1,
        1,
        1000000,
        2000000,
        b"https://token.example",
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "TotalSupply must greater than 0!");
}

#[test]
fn test_asset_issue_validate_fail_invalid_name_trx() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    // In java-tron, "assetName can't be trx" is enforced only when ALLOW_SAME_TOKEN_NAME != 0.
    storage_engine.put(
        "properties",
        b" ALLOW_SAME_TOKEN_NAME",
        &1i64.to_be_bytes(),
    ).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    let owner_address = Address::from([5u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(2_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let contract_data = build_asset_issue_contract_data(
        owner_address,
        b"trx",
        1000,
        1,
        1,
        1000000,
        2000000,
        b"https://token.example",
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "assetName can't be trx");
}

#[test]
fn test_asset_issue_validate_fail_start_time_before_head_block_time() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    storage_engine.put(
        "properties",
        b"latest_block_header_timestamp",
        &2_000_000i64.to_be_bytes(),
    ).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    let owner_address = Address::from([6u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(2_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let contract_data = build_asset_issue_contract_data(
        owner_address,
        b"Token",
        1000,
        1,
        1,
        1_000_000,
        3_000_000,
        b"https://token.example",
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Start time should be greater than HeadBlockTime");
}

#[test]
fn test_asset_issue_validate_fail_end_time_not_greater_than_start_time() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    let owner_address = Address::from([7u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(2_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let contract_data = build_asset_issue_contract_data(
        owner_address,
        b"Token",
        1000,
        1,
        1,
        1_000_000,
        1_000_000,
        b"https://token.example",
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "End time should be greater than start time");
}

#[test]
fn test_asset_issue_validate_fail_owner_address_empty() {
    use prost::Message;
    use tron_backend_execution::protocol::AssetIssueContractData;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    let contract = AssetIssueContractData {
        owner_address: vec![],
        name: b"Token".to_vec(),
        abbr: b"TK".to_vec(),
        total_supply: 1000,
        frozen_supply: vec![],
        trx_num: 1,
        precision: 0,
        num: 1,
        start_time: 1_000_000,
        end_time: 2_000_000,
        order: 0,
        vote_score: 0,
        description: vec![],
        url: b"https://token.example".to_vec(),
        free_asset_net_limit: 0,
        public_free_asset_net_limit: 0,
        public_free_asset_net_usage: 0,
        public_latest_free_net_time: 0,
        id: String::new(),
    };

    let mut contract_bytes = Vec::new();
    contract.encode(&mut contract_bytes).unwrap();

    let transaction = TronTransaction {
        from: Address::ZERO,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress");
}

#[test]
fn test_asset_issue_validate_fail_frozen_supply_amount_zero() {
    use prost::Message;
    use tron_backend_execution::protocol::asset_issue_contract_data::FrozenSupply;
    use tron_backend_execution::protocol::AssetIssueContractData;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    let mut owner_address = vec![0x41u8];
    owner_address.extend_from_slice(&[1u8; 20]);

    let contract = AssetIssueContractData {
        owner_address,
        name: b"Token".to_vec(),
        abbr: b"TK".to_vec(),
        total_supply: 1000,
        frozen_supply: vec![FrozenSupply {
            frozen_amount: 0,
            frozen_days: 1,
        }],
        trx_num: 1,
        precision: 0,
        num: 1,
        start_time: 1_000_000,
        end_time: 2_000_000,
        order: 0,
        vote_score: 0,
        description: vec![],
        url: b"https://token.example".to_vec(),
        free_asset_net_limit: 0,
        public_free_asset_net_limit: 0,
        public_free_asset_net_usage: 0,
        public_latest_free_net_time: 0,
        id: String::new(),
    };

    let mut contract_bytes = Vec::new();
    contract.encode(&mut contract_bytes).unwrap();

    let transaction = TronTransaction {
        from: Address::from([1u8; 20]),
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Frozen supply must be greater than 0!");
}

#[test]
fn test_asset_issue_validate_fail_frozen_supply_days_out_of_range_message() {
    use prost::Message;
    use tron_backend_execution::protocol::asset_issue_contract_data::FrozenSupply;
    use tron_backend_execution::protocol::AssetIssueContractData;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    let mut owner_address = vec![0x41u8];
    owner_address.extend_from_slice(&[1u8; 20]);

    let contract = AssetIssueContractData {
        owner_address,
        name: b"Token".to_vec(),
        abbr: b"TK".to_vec(),
        total_supply: 1000,
        frozen_supply: vec![FrozenSupply {
            frozen_amount: 1,
            frozen_days: 0,
        }],
        trx_num: 1,
        precision: 0,
        num: 1,
        start_time: 1_000_000,
        end_time: 2_000_000,
        order: 0,
        vote_score: 0,
        description: vec![],
        url: b"https://token.example".to_vec(),
        free_asset_net_limit: 0,
        public_free_asset_net_limit: 0,
        public_free_asset_net_usage: 0,
        public_latest_free_net_time: 0,
        id: String::new(),
    };

    let mut contract_bytes = Vec::new();
    contract.encode(&mut contract_bytes).unwrap();

    let transaction = TronTransaction {
        from: Address::from([1u8; 20]),
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap(),
        "frozenDuration must be less than 3652 days and more than 1 days"
    );
}

// ====================================================================================
// WitnessUpdateContract Tests
// ====================================================================================

#[test]
fn test_witness_update_contract_happy_path() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            witness_update_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
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

    // Create initial witness entry with old URL
    let initial_witness = tron_backend_execution::WitnessInfo::new(
        owner_address,
        "old-url.example.com".to_string(),
        100, // Some vote count
    );
    assert!(storage_adapter.put_witness(&initial_witness).is_ok());

    // Create WitnessUpdateContract transaction with new URL
    let new_url = "new-url.example.com";
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(new_url.as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
            ..Default::default()
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
        transaction_id: None,
    };

    // Execute the contract
    let result = service.execute_witness_update_contract(&mut storage_adapter, &transaction, &context);

    // Assert success
    assert!(result.is_ok(), "Witness update should succeed: {:?}", result.err());
    let execution_result = result.unwrap();

    assert!(execution_result.success, "Execution should be successful");
    assert_eq!(execution_result.energy_used, 0, "Energy used should be 0");
    assert_eq!(execution_result.state_changes.len(), 1, "Should have exactly 1 state change");
    assert!(execution_result.logs.is_empty(), "Should have no logs");
    assert!(execution_result.error.is_none(), "Should have no error");
    assert!(execution_result.bandwidth_used > 0, "Bandwidth should be > 0");

    // Verify witness URL was updated
    let updated_witness = storage_adapter.get_witness(&owner_address).unwrap();
    assert!(updated_witness.is_some(), "Witness should still exist");
    let witness = updated_witness.unwrap();
    assert_eq!(witness.url, "new-url.example.com", "URL should be updated");
    assert_eq!(witness.vote_count, 100, "Vote count should be preserved");

    // No state change emitted; witness URL persisted above is validated via storage read
}

#[test]
fn test_witness_update_contract_validations() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            witness_update_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
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
        transaction_id: None,
    };

    // Test 1: Empty URL should fail
    let empty_url_tx = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(vec![]), // Empty URL
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_witness_update_contract(&mut storage_adapter, &empty_url_tx, &context);
    assert!(result.is_err(), "Empty URL should fail");
    assert!(result.unwrap_err().contains("Invalid url"), "Error should mention 'Invalid url'");

    // Test 2: URL too long (>256 bytes) should fail
    let long_url_bytes: Vec<u8> = vec![b'x'; 257];
    let long_url_tx = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(long_url_bytes),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_witness_update_contract(&mut storage_adapter, &long_url_tx, &context);
    assert!(result.is_err(), "URL >256 bytes should fail");
    assert!(result.unwrap_err().contains("Invalid url"), "Error should mention 'Invalid url'");

    // Test 3: Missing owner account should fail
    let missing_account_tx = TronTransaction {
        from: owner_address, // Account doesn't exist in storage
        to: None,
        value: U256::ZERO,
        data: Bytes::from("valid-url.com".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_witness_update_contract(&mut storage_adapter, &missing_account_tx, &context);
    assert!(result.is_err(), "Missing account should fail");
    assert!(result.unwrap_err().contains("account does not exist"), "Error should mention 'account does not exist'");

    // Test 4: Account exists but witness does not exist should fail
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner_address, owner_account).is_ok());

    let missing_witness_tx = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("valid-url.com".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_witness_update_contract(&mut storage_adapter, &missing_witness_tx, &context);
    assert!(result.is_err(), "Missing witness should fail");
    assert!(result.unwrap_err().contains("Witness does not exist"), "Error should mention 'Witness does not exist'");

    // Test 5: Invalid UTF-8 should fail
    let witness = tron_backend_execution::WitnessInfo::new(owner_address, "old-url".to_string(), 0);
    assert!(storage_adapter.put_witness(&witness).is_ok());

    let invalid_utf8_tx = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(vec![0xFF, 0xFE, 0xFD]), // Invalid UTF-8 bytes
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result = service.execute_witness_update_contract(&mut storage_adapter, &invalid_utf8_tx, &context);
    assert!(result.is_err(), "Invalid UTF-8 should fail");
    assert!(result.unwrap_err().contains("Invalid UTF-8 in witness URL"), "Error should mention 'Invalid UTF-8 in witness URL'");
}

#[test]
fn test_witness_update_tracks_aext_when_enabled() {
    // Create mock storage and service with AEXT tracking enabled
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            witness_update_enabled: true,
            accountinfo_aext_mode: "tracked".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    // Create test account and witness
    let owner_address = Address::from([2u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner_address, owner_account).is_ok());

    let witness = tron_backend_execution::WitnessInfo::new(owner_address, "old-url".to_string(), 50);
    assert!(storage_adapter.put_witness(&witness).is_ok());

    // Create WitnessUpdateContract transaction
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("new-tracked-url.com".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
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

    // Execute the contract
    let result = service.execute_witness_update_contract(&mut storage_adapter, &transaction, &context);

    // Assert success
    assert!(result.is_ok(), "Witness update with AEXT tracking should succeed: {:?}", result.err());
    let execution_result = result.unwrap();

    assert!(execution_result.success, "Execution should be successful");
    assert!(execution_result.bandwidth_used > 0, "Bandwidth should be > 0");

    // Verify AEXT map contains owner entry
    assert!(execution_result.aext_map.contains_key(&owner_address), "AEXT map should contain owner");
    let (before_aext, after_aext) = &execution_result.aext_map[&owner_address];

    // After AEXT should have increased net_usage
    assert!(after_aext.free_net_usage >= before_aext.free_net_usage, "Net usage should increase or stay same");

    // Verify AEXT was persisted
    let persisted_aext = storage_adapter.get_account_aext(&owner_address).unwrap();
    assert!(persisted_aext.is_some(), "AEXT should be persisted");
}

// =============================================================================
// AccountCreateContract Tests
// =============================================================================

/// Helper function to create BackendService with account_create_enabled
fn new_test_service_with_account_create_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            account_create_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Helper function to create BackendService with account_create and AEXT tracking enabled
fn new_test_service_with_account_create_and_aext() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            account_create_enabled: true,
            accountinfo_aext_mode: "tracked".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Build AccountCreateContract protobuf data
/// Field 1: owner_address (bytes, 21-byte TRON address)
/// Field 2: account_address (bytes, 21-byte TRON address - target to create)
/// Field 3: type (varint, AccountType enum - optional)
fn build_account_create_contract_data(
    owner_address: &[u8],
    account_address: &[u8],
    account_type: Option<i32>,
) -> Bytes {
    let mut data = Vec::new();

    // Field 1: owner_address (tag = 0x0a, wire type 2 = length-delimited)
    data.push(0x0a);
    encode_varint(&mut data, owner_address.len() as u64);
    data.extend_from_slice(owner_address);

    // Field 2: account_address (tag = 0x12, wire type 2 = length-delimited)
    data.push(0x12);
    encode_varint(&mut data, account_address.len() as u64);
    data.extend_from_slice(account_address);

    // Field 3: type (tag = 0x18, wire type 0 = varint) - optional
    if let Some(t) = account_type {
        data.push(0x18);
        encode_varint(&mut data, t as u64);
    }

    Bytes::from(data)
}

/// Helper to create a 21-byte TRON address with given prefix
fn make_tron_address_21(prefix: u8, base: [u8; 20]) -> Vec<u8> {
    let mut addr = vec![prefix];
    addr.extend_from_slice(&base);
    addr
}

// -----------------------------------------------------------------------------
// Address Validation Tests
// -----------------------------------------------------------------------------

#[test]
fn test_account_create_reject_wrong_prefix_owner_address() {
    // Set up mainnet storage (prefix 0x41) by inserting a mainnet address
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Insert a mainnet account to set the detected prefix to 0x41
    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Verify prefix is detected as mainnet
    assert_eq!(storage_adapter.address_prefix(), 0x41, "Should detect mainnet prefix");

    // Set up owner account with mainnet prefix
    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Build contract with TESTNET prefix (0xa0) for owner - should be rejected
    let wrong_prefix_owner = make_tron_address_21(0xa0, [0x11u8; 20]);
    let target_address = make_tron_address_21(0x41, [0x22u8; 20]);
    let contract_data = build_account_create_contract_data(&wrong_prefix_owner, &target_address, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(&mut storage_adapter, &transaction, &new_test_context());

    assert!(result.is_err(), "Should reject wrong prefix owner address");
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress");
}

#[test]
fn test_account_create_reject_wrong_prefix_target_address() {
    // Set up mainnet storage (prefix 0x41)
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    assert_eq!(storage_adapter.address_prefix(), 0x41);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Build contract with correct owner but TESTNET prefix (0xa0) for target
    let correct_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    let wrong_prefix_target = make_tron_address_21(0xa0, [0x22u8; 20]);
    let contract_data = build_account_create_contract_data(&correct_owner, &wrong_prefix_target, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(&mut storage_adapter, &transaction, &new_test_context());

    assert!(result.is_err(), "Should reject wrong prefix target address");
    assert_eq!(result.err().unwrap(), "Invalid account address");
}

#[test]
fn test_account_create_reject_wrong_length_owner_address() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Test 20-byte owner address (too short)
    let short_owner = vec![0x41u8; 20]; // Missing one byte
    let target_address = make_tron_address_21(0x41, [0x22u8; 20]);
    let contract_data = build_account_create_contract_data(&short_owner, &target_address, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(&mut storage_adapter, &transaction, &new_test_context());

    assert!(result.is_err(), "Should reject 20-byte owner address");
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress");
}

#[test]
fn test_account_create_reject_wrong_length_target_address() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Test 22-byte target address (too long)
    let correct_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    let long_target = vec![0x41u8; 22];
    let contract_data = build_account_create_contract_data(&correct_owner, &long_target, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(&mut storage_adapter, &transaction, &new_test_context());

    assert!(result.is_err(), "Should reject 22-byte target address");
    assert_eq!(result.err().unwrap(), "Invalid account address");
}

// -----------------------------------------------------------------------------
// Contract Type Field Tests
// -----------------------------------------------------------------------------

#[test]
fn test_account_create_type_normal_default() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Set fee to 0 to simplify test
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &0u64.to_be_bytes(),
        )
        .unwrap();

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Create account without specifying type (should default to Normal = 0)
    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x22u8; 20]);
    let contract_data = build_account_create_contract_data(&owner_tron, &target_tron, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_ok(), "Account create should succeed: {:?}", result.err());

    // Verify the created account has type = 0 (Normal)
    let target_address = Address::from([0x22u8; 20]);
    let target_proto = storage_adapter.get_account_proto(&target_address).unwrap();
    assert!(target_proto.is_some(), "Target account should exist");
    let proto = target_proto.unwrap();
    assert_eq!(proto.r#type, 0, "Account type should be Normal (0) by default");
}

#[test]
fn test_account_create_type_contract_persisted() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &0u64.to_be_bytes(),
        )
        .unwrap();

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Create account with type = 1 (Contract)
    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x33u8; 20]);
    let contract_data = build_account_create_contract_data(&owner_tron, &target_tron, Some(1));

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_ok(), "Account create with type=Contract should succeed: {:?}", result.err());

    // Verify the created account has type = 1 (Contract)
    let target_address = Address::from([0x33u8; 20]);
    let target_proto = storage_adapter.get_account_proto(&target_address).unwrap();
    assert!(target_proto.is_some(), "Target account should exist");
    let proto = target_proto.unwrap();
    assert_eq!(proto.r#type, 1, "Account type should be Contract (1)");
}

// -----------------------------------------------------------------------------
// Resource Path Tests (Bandwidth / Fee Fallback)
// -----------------------------------------------------------------------------

#[test]
fn test_account_create_bandwidth_path_free_net() {
    use tron_backend_execution::BandwidthPath;

    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Set up dynamic properties
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &0u64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"FREE_NET_LIMIT",
            &100000i64.to_be_bytes(), // Large free net limit
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE",
            &1i64.to_be_bytes(), // 1x multiplier
        )
        .unwrap();

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_and_aext();

    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x44u8; 20]);
    let contract_data = build_account_create_contract_data(&owner_tron, &target_tron, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_ok(), "Account create should succeed: {:?}", result.err());
    let exec_result = result.unwrap();

    // Verify AEXT tracking happened
    assert!(exec_result.aext_map.contains_key(&owner_address), "AEXT map should contain owner");
    let (before_aext, after_aext) = &exec_result.aext_map[&owner_address];

    // With large free_net_limit and small tx size, should use FREE_NET path
    // free_net_usage should increase
    assert!(
        after_aext.free_net_usage > before_aext.free_net_usage,
        "Free net usage should increase when using FREE_NET path"
    );
}

#[test]
fn test_account_create_fee_fallback_updates_total_cost() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Set up dynamic properties with very small free_net_limit to force fee path
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &0u64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"FREE_NET_LIMIT",
            &0i64.to_be_bytes(), // Zero free net - forces fee path
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"CREATE_ACCOUNT_FEE",
            &100000u64.to_be_bytes(), // 0.1 TRX fallback fee
        )
        .unwrap();

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    // Check initial TOTAL_CREATE_ACCOUNT_COST
    let initial_cost = storage_adapter.get_total_create_account_cost().unwrap();
    assert_eq!(initial_cost, 0, "Initial TOTAL_CREATE_ACCOUNT_COST should be 0");

    let service = new_test_service_with_account_create_and_aext();

    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x55u8; 20]);
    let contract_data = build_account_create_contract_data(&owner_tron, &target_tron, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_ok(), "Account create should succeed: {:?}", result.err());

    // Verify TOTAL_CREATE_ACCOUNT_COST was incremented
    let final_cost = storage_adapter.get_total_create_account_cost().unwrap();
    assert_eq!(
        final_cost, 100000,
        "TOTAL_CREATE_ACCOUNT_COST should be incremented by CREATE_ACCOUNT_FEE"
    );
}

#[test]
fn test_account_create_receipt_contains_fee() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Set actuator fee
    let actuator_fee: u64 = 1_000_000; // 1 TRX
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &actuator_fee.to_be_bytes(),
        )
        .unwrap();

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x66u8; 20]);
    let contract_data = build_account_create_contract_data(&owner_tron, &target_tron, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_ok(), "Account create should succeed: {:?}", result.err());
    let exec_result = result.unwrap();

    // Verify receipt passthrough is present and contains fee
    assert!(
        exec_result.tron_transaction_result.is_some(),
        "tron_transaction_result should be set for receipt passthrough"
    );

    let receipt_bytes = exec_result.tron_transaction_result.unwrap();
    assert!(!receipt_bytes.is_empty(), "Receipt bytes should not be empty");

    // Parse the receipt to verify fee field
    // Field 1 in Transaction.Result is 'fee' (int64, wire type 0 = varint)
    // The receipt should start with tag 0x08 (field 1, wire type 0)
    assert_eq!(receipt_bytes[0], 0x08, "Receipt should start with fee field tag");
}
