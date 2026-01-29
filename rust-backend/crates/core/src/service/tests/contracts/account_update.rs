//! AccountUpdateContract tests.

use super::super::super::*;
use super::common::seed_dynamic_properties;
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
use revm_primitives::{Address, Bytes, U256, AccountInfo};
use tron_backend_common::{ModuleManager, ExecutionConfig};

#[test]
fn test_account_update_contract_happy_path() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
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
    seed_dynamic_properties(&storage_engine);
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
    seed_dynamic_properties(&storage_engine);
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
