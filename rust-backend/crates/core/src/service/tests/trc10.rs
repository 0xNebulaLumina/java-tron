use super::super::*;
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata, Trc10Op};
use revm_primitives::{Address, Bytes, U256, AccountInfo};
use tron_backend_common::{ModuleManager, ExecutionConfig, RemoteExecutionConfig};
use tron_backend_storage::StorageEngine;

// Helper function to encode varint for protobuf
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
fn test_trc10_asset_issue_happy_path() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
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

    // Create test account (owner must exist)
    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner_address, owner_account.clone()).is_ok());

    // Create AssetIssueContract payload
    let payload = vec![
        // Field 1 (owner_address): tag=0x0a (field 1, wire type 2), length=21, data=21 bytes
        0x0a, 0x15,
        0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        // Field 2 (name): tag=0x12, length=9, "TestToken"
        0x12, 0x09,
        0x54, 0x65, 0x73, 0x74, 0x54, 0x6f, 0x6b, 0x65, 0x6e,
        // Field 3 (abbr): tag=0x1a, length=2, "TT"
        0x1a, 0x02,
        0x54, 0x54,
        // Field 4 (total_supply): tag=0x20 (field 4, wire type 0), value=10000000
        0x20, 0x80, 0xad, 0xe2, 0x04,
        // Field 6 (trx_num): tag=0x30, value=1
        0x30, 0x01,
        // Field 7 (precision): tag=0x38, value=6
        0x38, 0x06,
        // Field 8 (num): tag=0x40, value=1
        0x40, 0x01,
        // Field 9 (start_time): tag=0x48, value=1234567890000 + 1000 (in future)
        0x48, 0xe8, 0xe5, 0xd4, 0xf5, 0xc7, 0x47,
        // Field 10 (end_time): tag=0x50, value=1234567890000 + 86400000 (1 day later)
        0x50, 0xd0, 0xa4, 0xe3, 0x82, 0xcd, 0x47,
    ];

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(payload),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
    };

    // Execute the contract
    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &context);

    // Assert success
    assert!(result.is_ok(), "Asset issue handler should succeed: {:?}", result.err());
    let execution_result = result.unwrap();

    assert!(execution_result.success, "Execution should be successful");
    assert_eq!(execution_result.energy_used, 0, "Energy used should be 0");
    assert_eq!(execution_result.state_changes.len(), 1, "Should have exactly 1 state change");
    assert!(execution_result.logs.is_empty(), "Should have no logs");
    assert!(execution_result.error.is_none(), "Should have no error");

    // Verify TRC-10 change
    assert_eq!(execution_result.trc10_changes.len(), 1, "Should have 1 TRC-10 change");
    let trc10_change = &execution_result.trc10_changes[0];
    assert_eq!(trc10_change.op, Trc10Op::Issue);
    assert_eq!(trc10_change.name, b"TestToken");
    assert_eq!(trc10_change.abbr, b"TT");
    assert_eq!(trc10_change.total_supply, 10000000);
    assert_eq!(trc10_change.precision, 6);
    assert_eq!(trc10_change.trx_num, 1);
    assert_eq!(trc10_change.num, 1);
}

#[test]
fn test_trc10_asset_issue_validation_failure() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
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

    // Create test account
    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner_address, owner_account).is_ok());

    // Test with invalid payload (empty)
    let payload = vec![];
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(payload),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &context);

    // Should fail
    assert!(result.is_err(), "Asset issue with empty payload should fail");
}

#[test]
fn test_trc10_asset_issue_zero_total_supply() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
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

    // Create test account
    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner_address, owner_account).is_ok());

    // Create payload with total_supply=0
    let payload = vec![
        // Field 1 (owner_address)
        0x0a, 0x15,
        0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        // Field 2 (name)
        0x12, 0x09,
        0x54, 0x65, 0x73, 0x74, 0x54, 0x6f, 0x6b, 0x65, 0x6e,
        // Field 4 (total_supply): 0
        0x20, 0x00,
        // Field 6 (trx_num): 1
        0x30, 0x01,
        // Field 8 (num): 1
        0x40, 0x01,
        // Field 9 (start_time): future
        0x48, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01,
        // Field 10 (end_time): even more future
        0x50, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02,
    ];

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(payload),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
    };

    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &context);

    // Should fail with validation error
    assert!(result.is_err(), "Asset issue with zero total supply should fail");
    let error_msg = result.unwrap_err();
    assert!(error_msg.contains("must be positive") || error_msg.contains("greater than 0"),
        "Error should mention total supply validation, got: {}", error_msg);
}

#[test]
fn test_trc10_participate_happy_path() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
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

    // Create test accounts
    let owner_address = Address::from([1u8; 20]);
    let to_address_evm = Address::from([2u8; 20]);

    let owner_account = AccountInfo {
        balance: U256::from(10000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    let to_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner_address, owner_account).is_ok());
    assert!(storage_adapter.set_account(to_address_evm, to_account).is_ok());

    // Create to_address with 0x41 prefix (21 bytes)
    let mut to_address_tron = vec![0x41];
    to_address_tron.extend_from_slice(&[2u8; 20]);

    // Create ParticipateAssetIssueContract payload
    let payload = vec![
        // Field 1 (owner_address): tag=0x0a, length=21
        0x0a, 0x15,
        0x41, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
        0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
        // Field 2 (to_address): tag=0x12, length=21
        0x12, 0x15,
        0x41, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,
        0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,
        // Field 3 (asset_name): tag=0x1a, length=9, "TestToken"
        0x1a, 0x09,
        0x54, 0x65, 0x73, 0x74, 0x54, 0x6f, 0x6b, 0x65, 0x6e,
        // Field 4 (amount): tag=0x20, value=1000000
        0x20, 0xc0, 0x84, 0x3d,
    ];

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(payload),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
    };

    // Execute the contract
    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &context);

    // Assert success
    assert!(result.is_ok(), "Participate handler should succeed: {:?}", result.err());
    let execution_result = result.unwrap();

    assert!(execution_result.success, "Execution should be successful");
    assert_eq!(execution_result.energy_used, 0, "Energy used should be 0");
    assert_eq!(execution_result.state_changes.len(), 2, "Should have 2 state changes (owner + to)");

    // Verify TRC-10 change
    assert_eq!(execution_result.trc10_changes.len(), 1, "Should have 1 TRC-10 change");
    let trc10_change = &execution_result.trc10_changes[0];
    assert_eq!(trc10_change.op, Trc10Op::Participate);
    assert_eq!(trc10_change.asset_id, b"TestToken");
    assert_eq!(trc10_change.amount, 1000000);
}

#[test]
fn test_trc10_participate_validation_failure() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
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

    // Create test account
    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner_address, owner_account).is_ok());

    // Test with invalid payload (empty)
    let payload = vec![];
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(payload),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &context);

    // Should fail
    assert!(result.is_err(), "Participate with empty payload should fail");
}

#[test]
fn test_trc10_gating_asset_issue_disabled() {
    // Create mock storage and service with TRC-10 disabled
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            trc10_enabled: false, // DISABLED
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
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

    let payload = vec![
        0x0a, 0x15,
        0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
    ];

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(payload),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
        },
    };

    let context = TronExecutionContext {
        block_number: 1000,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
    };

    // Execute via execute_non_vm_contract (which checks feature flags)
    let result = service.execute_non_vm_contract(&mut storage_adapter, &transaction, &context);

    // Should fail because TRC-10 is disabled
    assert!(result.is_err(), "Asset issue should fail when TRC-10 disabled");
    let error_msg = result.unwrap_err();
    assert!(error_msg.contains("disabled") || error_msg.contains("falling back"),
        "Error should mention TRC-10 being disabled, got: {}", error_msg);
}

#[test]
fn test_trc10_op_enum_values() {
    // Test that Trc10Op enum values match expected values for proto compatibility
    assert_eq!(Trc10Op::Issue as i32, 0);
    assert_eq!(Trc10Op::Participate as i32, 1);
    assert_eq!(Trc10Op::Transfer as i32, 2);
}
