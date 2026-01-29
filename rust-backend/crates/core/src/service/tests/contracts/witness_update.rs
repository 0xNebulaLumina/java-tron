//! WitnessUpdateContract tests.

use super::super::super::*;
use super::common::{seed_dynamic_properties, make_from_raw};
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
use revm_primitives::{Address, Bytes, U256, AccountInfo};
use tron_backend_common::{ModuleManager, ExecutionConfig, RemoteExecutionConfig};

#[test]
fn test_witness_update_contract_happy_path() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
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
            from_raw: Some(make_from_raw(&owner_address)),
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
    // WitnessUpdateContract does not emit state changes (matches embedded CSV semantics)
    assert_eq!(execution_result.state_changes.len(), 0, "Should have no state changes");
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
    seed_dynamic_properties(&storage_engine);
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

    // For URL validation tests (1 and 2), we need account+witness to exist so execution reaches URL check
    // Use a different address for URL validation tests
    let url_test_address = Address::from([99u8; 20]);
    let url_test_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(url_test_address, url_test_account).is_ok());
    let url_test_witness = tron_backend_execution::WitnessInfo::new(url_test_address, "existing-url".to_string(), 0);
    assert!(storage_adapter.put_witness(&url_test_witness).is_ok());

    // Test 1: Empty URL should fail
    let empty_url_tx = TronTransaction {
        from: url_test_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(vec![]), // Empty URL
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&url_test_address)),
            ..Default::default()
        },
    };

    let result = service.execute_witness_update_contract(&mut storage_adapter, &empty_url_tx, &context);
    assert!(result.is_err(), "Empty URL should fail");
    assert!(result.unwrap_err().contains("Invalid url"), "Error should mention 'Invalid url'");

    // Test 2: URL too long (>256 bytes) should fail
    let long_url_bytes: Vec<u8> = vec![b'x'; 257];
    let long_url_tx = TronTransaction {
        from: url_test_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(long_url_bytes),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&url_test_address)),
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
            from_raw: Some(make_from_raw(&owner_address)),
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
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let result = service.execute_witness_update_contract(&mut storage_adapter, &missing_witness_tx, &context);
    assert!(result.is_err(), "Missing witness should fail");
    assert!(result.unwrap_err().contains("Witness does not exist"), "Error should mention 'Witness does not exist'");

    // Test 5: Invalid UTF-8 is accepted lossily (matches Java's ByteString#toStringUtf8 behavior)
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
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let result = service.execute_witness_update_contract(&mut storage_adapter, &invalid_utf8_tx, &context);
    // Invalid UTF-8 is converted lossily with replacement characters, not rejected
    assert!(result.is_ok(), "Invalid UTF-8 should be accepted with lossy conversion");
    let execution_result = result.unwrap();
    assert!(execution_result.success, "Execution should succeed");
}

#[test]
fn test_witness_update_tracks_aext_when_enabled() {
    // Create mock storage and service with AEXT tracking enabled
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
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
            from_raw: Some(make_from_raw(&owner_address)),
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
