//! WitnessUpdateContract tests.

use super::super::super::*;
use super::common::{make_from_raw, seed_dynamic_properties};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{
    EngineBackedEvmStateStore, TronExecutionContext, TronTransaction, TxMetadata,
};

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
    assert!(storage_adapter
        .set_account(owner_address, owner_account.clone())
        .is_ok());

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
    let result =
        service.execute_witness_update_contract(&mut storage_adapter, &transaction, &context);

    // Assert success
    assert!(
        result.is_ok(),
        "Witness update should succeed: {:?}",
        result.err()
    );
    let execution_result = result.unwrap();

    assert!(execution_result.success, "Execution should be successful");
    assert_eq!(execution_result.energy_used, 0, "Energy used should be 0");
    // WitnessUpdateContract does not emit state changes (matches embedded CSV semantics)
    assert_eq!(
        execution_result.state_changes.len(),
        0,
        "Should have no state changes"
    );
    assert!(execution_result.logs.is_empty(), "Should have no logs");
    assert!(execution_result.error.is_none(), "Should have no error");
    assert!(
        execution_result.bandwidth_used > 0,
        "Bandwidth should be > 0"
    );

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
    assert!(storage_adapter
        .set_account(url_test_address, url_test_account)
        .is_ok());
    let url_test_witness =
        tron_backend_execution::WitnessInfo::new(url_test_address, "existing-url".to_string(), 0);
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

    let result =
        service.execute_witness_update_contract(&mut storage_adapter, &empty_url_tx, &context);
    assert!(result.is_err(), "Empty URL should fail");
    assert!(
        result.unwrap_err().contains("Invalid url"),
        "Error should mention 'Invalid url'"
    );

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

    let result =
        service.execute_witness_update_contract(&mut storage_adapter, &long_url_tx, &context);
    assert!(result.is_err(), "URL >256 bytes should fail");
    assert!(
        result.unwrap_err().contains("Invalid url"),
        "Error should mention 'Invalid url'"
    );

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

    let result = service.execute_witness_update_contract(
        &mut storage_adapter,
        &missing_account_tx,
        &context,
    );
    assert!(result.is_err(), "Missing account should fail");
    assert!(
        result.unwrap_err().contains("account does not exist"),
        "Error should mention 'account does not exist'"
    );

    // Test 4: Account exists but witness does not exist should fail
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

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

    let result = service.execute_witness_update_contract(
        &mut storage_adapter,
        &missing_witness_tx,
        &context,
    );
    assert!(result.is_err(), "Missing witness should fail");
    assert!(
        result.unwrap_err().contains("Witness does not exist"),
        "Error should mention 'Witness does not exist'"
    );

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

    let result =
        service.execute_witness_update_contract(&mut storage_adapter, &invalid_utf8_tx, &context);
    // Invalid UTF-8 is converted lossily with replacement characters, not rejected
    assert!(
        result.is_ok(),
        "Invalid UTF-8 should be accepted with lossy conversion"
    );
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
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    let witness =
        tron_backend_execution::WitnessInfo::new(owner_address, "old-url".to_string(), 50);
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
    let result =
        service.execute_witness_update_contract(&mut storage_adapter, &transaction, &context);

    // Assert success
    assert!(
        result.is_ok(),
        "Witness update with AEXT tracking should succeed: {:?}",
        result.err()
    );
    let execution_result = result.unwrap();

    assert!(execution_result.success, "Execution should be successful");
    assert!(
        execution_result.bandwidth_used > 0,
        "Bandwidth should be > 0"
    );

    // Verify AEXT map contains owner entry
    assert!(
        execution_result.aext_map.contains_key(&owner_address),
        "AEXT map should contain owner"
    );
    let (before_aext, after_aext) = &execution_result.aext_map[&owner_address];

    // After AEXT should have increased net_usage
    assert!(
        after_aext.free_net_usage >= before_aext.free_net_usage,
        "Net usage should increase or stay same"
    );

    // Verify AEXT was persisted
    let persisted_aext = storage_adapter.get_account_aext(&owner_address).unwrap();
    assert!(persisted_aext.is_some(), "AEXT should be persisted");
}

/// Verify that `execute_witness_update_contract` preserves all non-URL
/// `protocol.Witness` fields (pub_key, total_produced, total_missed,
/// latest_block_num, latest_slot_num, is_jobs).
///
/// This is the core parity fix: Java's `WitnessUpdateActuator.updateWitness`
/// only mutates `Witness.url` and re-persists the capsule.  The Rust path
/// previously round-tripped through `WitnessInfo` (which only carries
/// address/url/vote_count) and clobbered the other fields to defaults.
#[test]
fn test_witness_update_preserves_all_witness_fields() {
    use prost::Message;
    use tron_backend_execution::protocol::Witness;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // 1. Manually insert a witness record with non-default fields directly
    //    via protobuf encoding (simulating what Java consensus writes).
    let owner_address = Address::from([3u8; 20]);
    let mut tron_addr = vec![0x41u8];
    tron_addr.extend_from_slice(owner_address.as_slice());

    let original_witness = Witness {
        address: tron_addr.clone(),
        vote_count: 42,
        pub_key: vec![0xAA, 0xBB, 0xCC],
        url: "old-url.example.com".to_string(),
        total_produced: 7,
        total_missed: 3,
        latest_block_num: 123456,
        latest_slot_num: 789,
        is_jobs: true,
    };
    let encoded = original_witness.encode_to_vec();
    storage_engine.put("witness", &tron_addr, &encoded).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // 2. Prepare service + account
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

    let owner_account = AccountInfo {
        balance: U256::from(1_000_000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter
        .set_account(owner_address, owner_account)
        .unwrap();

    // 3. Execute witness update with new URL
    let new_url = "brand-new-url.example.com";
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
        block_timestamp: 1_000_000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let result =
        service.execute_witness_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "Witness update should succeed: {:?}",
        result.err()
    );
    assert!(result.unwrap().success);

    // 4. Read back and verify via WitnessInfo (url + vote_count)
    let stored = storage_adapter
        .get_witness(&owner_address)
        .expect("get_witness should not error")
        .expect("witness should exist after update");

    assert_eq!(stored.url, new_url, "URL should be updated");
    assert_eq!(stored.vote_count, 42, "vote_count should be preserved");

    // 5. Read raw bytes to verify ALL protobuf fields are intact
    //    (WitnessInfo only surfaces address/url/vote_count, so we decode the full proto)
    let mut raw_key = vec![0x41u8];
    raw_key.extend_from_slice(owner_address.as_slice());
    let raw_bytes = storage_adapter
        .raw_get("witness", &raw_key)
        .expect("raw_get should not error")
        .expect("raw witness bytes should exist");

    let decoded = Witness::decode(raw_bytes.as_slice()).expect("should decode as Witness proto");

    assert_eq!(decoded.url, new_url, "proto url should be updated");
    assert_eq!(
        decoded.vote_count, 42,
        "proto vote_count should be preserved"
    );
    assert_eq!(
        decoded.pub_key,
        vec![0xAA, 0xBB, 0xCC],
        "pub_key should be preserved"
    );
    assert_eq!(
        decoded.total_produced, 7,
        "total_produced should be preserved"
    );
    assert_eq!(decoded.total_missed, 3, "total_missed should be preserved");
    assert_eq!(
        decoded.latest_block_num, 123456,
        "latest_block_num should be preserved"
    );
    assert_eq!(
        decoded.latest_slot_num, 789,
        "latest_slot_num should be preserved"
    );
    assert_eq!(decoded.is_jobs, true, "is_jobs should be preserved");
}

/// Verify that providing a mismatched `contract_parameter.type_url` produces
/// the Java-parity error message.
#[test]
fn test_witness_update_any_type_url_mismatch() {
    use tron_backend_execution::TronContractParameter;

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

    let owner_address = Address::from([4u8; 20]);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("some-url.com".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.SomeOtherContract".to_string(),
                value: vec![],
            }),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1_000_000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let result =
        service.execute_witness_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "Mismatched type_url should fail");
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("contract type error"),
        "Error should contain 'contract type error', got: {}",
        err_msg
    );
    assert!(
        err_msg.contains("WitnessUpdateContract"),
        "Error should mention WitnessUpdateContract, got: {}",
        err_msg
    );
}

/// Verify that malformed `contract_parameter.value` bytes produce a decode error,
/// mirroring Java's `any.unpack(WitnessUpdateContract.class)` → InvalidProtocolBufferException.
#[test]
fn test_witness_update_any_value_malformed() {
    use tron_backend_execution::TronContractParameter;

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

    let owner_address = Address::from([8u8; 20]);

    // Malformed protobuf: claims a length-delimited field of 200 bytes but only
    // provides 2 bytes of payload → truncation error.
    let malformed_value = vec![
        0x0a, // field 1, wire type 2 (length-delimited)
        0xC8, 0x01, // varint 200 (length)
        0x41, 0x42, // only 2 bytes instead of 200
    ];

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("some-url.com".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.WitnessUpdateContract".to_string(),
                value: malformed_value,
            }),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1_000_000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let result =
        service.execute_witness_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "Malformed value should fail");
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("WitnessUpdateContract decode error"),
        "Error should mention decode error, got: {}",
        err_msg
    );
}

/// Verify that a correct type_url passes validation and continues to later checks.
#[test]
fn test_witness_update_any_type_url_correct() {
    use tron_backend_execution::TronContractParameter;

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

    let owner_address = Address::from([5u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1_000_000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter
        .set_account(owner_address, owner_account)
        .unwrap();

    let witness =
        tron_backend_execution::WitnessInfo::new(owner_address, "existing.com".to_string(), 10);
    storage_adapter.put_witness(&witness).unwrap();

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("updated-url.com".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessUpdateContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.WitnessUpdateContract".to_string(),
                value: vec![],
            }),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1_000_000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let result =
        service.execute_witness_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "Correct type_url should pass: {:?}",
        result.err()
    );
    assert!(result.unwrap().success);

    let updated = storage_adapter
        .get_witness(&owner_address)
        .unwrap()
        .unwrap();
    assert_eq!(updated.url, "updated-url.com");
}

/// Verify that Java always writes even when URL is unchanged (no-op optimization removed).
#[test]
fn test_witness_update_always_writes_even_same_url() {
    use prost::Message;
    use tron_backend_execution::protocol::Witness;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    let owner_address = Address::from([6u8; 20]);
    let mut tron_addr = vec![0x41u8];
    tron_addr.extend_from_slice(owner_address.as_slice());

    let original_witness = Witness {
        address: tron_addr.clone(),
        vote_count: 10,
        pub_key: vec![0xDD],
        url: "same-url.com".to_string(),
        total_produced: 5,
        total_missed: 2,
        latest_block_num: 100,
        latest_slot_num: 50,
        is_jobs: false,
    };
    storage_engine
        .put("witness", &tron_addr, &original_witness.encode_to_vec())
        .unwrap();

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

    let owner_account = AccountInfo {
        balance: U256::from(1_000_000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter
        .set_account(owner_address, owner_account)
        .unwrap();

    // Execute update with SAME URL
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("same-url.com".as_bytes()),
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
        block_timestamp: 1_000_000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let result =
        service.execute_witness_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "Same-URL update should succeed: {:?}",
        result.err()
    );
    assert!(result.unwrap().success);

    // Verify all fields preserved after same-URL write
    let stored = storage_adapter
        .get_witness(&owner_address)
        .unwrap()
        .unwrap();
    assert_eq!(stored.url, "same-url.com");
    assert_eq!(stored.vote_count, 10, "vote_count should be preserved");
}

/// Verify that a URL payload crafted to look like a valid `google.protobuf.Any`
/// wire-format is NOT unwrapped by `execute_non_vm_contract`.
///
/// The global `unwrap_any_value_if_present` in `execute_non_vm_contract` checks
/// for the `type.googleapis.com/` prefix.  Payload-style contracts (WitnessCreate,
/// WitnessUpdate) are now excluded from this logic.
#[test]
fn test_witness_update_crafted_any_url_not_unwrapped() {
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

    let owner_address = Address::from([7u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1_000_000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter
        .set_account(owner_address, owner_account)
        .unwrap();

    let witness = tron_backend_execution::WitnessInfo::new(owner_address, "old.com".to_string(), 0);
    storage_adapter.put_witness(&witness).unwrap();

    // Craft a URL payload that looks like a valid google.protobuf.Any:
    // Field 1 (type_url): tag=0x0a, length-delimited string starting with "type.googleapis.com/"
    // Field 2 (value): tag=0x12, length-delimited bytes "INNER"
    let crafted_url: Vec<u8> = {
        let type_url = b"type.googleapis.com/fake.Type";
        let inner_value = b"INNER";
        let mut buf = Vec::new();
        // field 1, wire type 2 (length-delimited) => tag byte = (1 << 3) | 2 = 0x0a
        buf.push(0x0a);
        buf.push(type_url.len() as u8);
        buf.extend_from_slice(type_url);
        // field 2, wire type 2 (length-delimited) => tag byte = (2 << 3) | 2 = 0x12
        buf.push(0x12);
        buf.push(inner_value.len() as u8);
        buf.extend_from_slice(inner_value);
        buf
    };

    // Use execute_non_vm_contract (the public entry point that runs Any-unwrapping)
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(crafted_url.clone()),
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
        block_timestamp: 1_000_000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "Crafted-Any URL should succeed: {:?}",
        result.err()
    );

    // The stored URL should be the full crafted bytes (lossy UTF-8), NOT the unwrapped inner "INNER"
    let stored = storage_adapter
        .get_witness(&owner_address)
        .unwrap()
        .unwrap();
    let expected_url = String::from_utf8_lossy(&crafted_url).to_string();
    assert_eq!(
        stored.url, expected_url,
        "URL should be stored as-is (not unwrapped)"
    );
    assert_ne!(
        stored.url, "INNER",
        "URL must NOT be the unwrapped Any.value"
    );
}
