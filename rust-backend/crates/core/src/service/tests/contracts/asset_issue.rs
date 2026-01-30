//! AssetIssueContract tests (TRC-10 Asset Issuance).

use super::super::super::*;
use super::common::{encode_varint, new_test_context, seed_dynamic_properties};
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
use revm_primitives::{Address, Bytes, U256, AccountInfo};
use tron_backend_common::{ModuleManager, ExecutionConfig, RemoteExecutionConfig};
use tron_backend_storage::StorageEngine;

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

#[test]
fn test_asset_issue_contract_trc10_change_emission() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // Enable same-token-name mode so precision passes through (not forced to 0)
    storage_engine.put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes()).unwrap();
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
            // token_id is now self-contained (populated by Rust) for executor-only parity
            assert!(asset_issued.token_id.is_some(), "Token ID should be populated");
            assert_eq!(asset_issued.token_id.as_ref().unwrap(), "1000001", "Token ID should be 1000001 (first allocation)");
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
    encode_varint(&mut contract_data, 1_000_000);

    // Field 10: end_time
    contract_data.push(80u8);
    encode_varint(&mut contract_data, 2_000_000);

    // Field 21: url
    contract_data.push(170u8);
    contract_data.push(1u8);
    contract_data.push(10u8);
    contract_data.extend_from_slice(b"http://url");

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

#[test]
fn test_asset_issue_validate_fail_wrong_address_prefix() {
    // Test that mainnet DB (0x41 prefix) rejects testnet addresses (0xa0 prefix)
    use prost::Message;
    use tron_backend_execution::protocol::AssetIssueContractData;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    // Create owner_address with testnet prefix (0xa0) instead of mainnet (0x41)
    let mut owner_address = vec![0xa0u8]; // Wrong prefix for mainnet DB
    owner_address.extend_from_slice(&[1u8; 20]);

    let contract = AssetIssueContractData {
        owner_address,
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

    // Should fail with "Invalid ownerAddress" because prefix doesn't match DB prefix
    let result = service.execute_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress");
}

#[test]
fn test_asset_issue_token_id_populated_in_trc10_change() {
    // Verify token_id is now populated in Trc10Change::AssetIssued (not None)
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    let owner_address = Address::from([0xaau8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(2_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let contract_data = build_asset_issue_contract_data(
        owner_address,
        b"TestToken",
        1000,
        1,
        1,
        1000000,
        2000000,
        b"https://test.example",
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
    assert!(result.is_ok(), "Asset issue should succeed: {:?}", result.err());
    let execution_result = result.unwrap();

    // Verify token_id is populated (not None) - this is the key change for Task 3
    assert_eq!(execution_result.trc10_changes.len(), 1);
    match &execution_result.trc10_changes[0] {
        tron_backend_execution::Trc10Change::AssetIssued(asset_issued) => {
            assert!(asset_issued.token_id.is_some(), "token_id should be Some (self-contained)");
            // First token allocation should be 1000001 (TOKEN_ID_NUM defaults to 1000000, incremented by 1)
            assert_eq!(asset_issued.token_id.as_ref().unwrap(), "1000001");
        }
        _ => panic!("Expected AssetIssued change"),
    }
}

#[test]
fn test_asset_issue_token_id_num_persisted_alongside_token_id() {
    // Guard against future refactors: Rust must persist TOKEN_ID_NUM even though it also emits token_id.
    // Java only increments TOKEN_ID_NUM when token_id is empty (RuntimeSpiImpl.java:700), so if Rust
    // stops persisting TOKEN_ID_NUM, Java's fallback path would re-use stale IDs causing collisions.
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    // Verify initial TOKEN_ID_NUM (default is 1000000)
    let initial_token_id_num = storage_adapter.get_token_id_num().unwrap();
    assert_eq!(initial_token_id_num, 1_000_000, "Initial TOKEN_ID_NUM should be 1000000");

    // Issue first asset
    let owner1 = Address::from([0x11u8; 20]);
    storage_adapter.set_account(owner1, AccountInfo {
        balance: U256::from(2_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    }).unwrap();

    let contract_data1 = build_asset_issue_contract_data(
        owner1, b"Token1", 1000, 1, 1, 1000000, 2000000, b"https://t1.example",
    );
    let tx1 = TronTransaction {
        from: owner1,
        to: None,
        value: U256::ZERO,
        data: contract_data1,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result1 = service.execute_asset_issue_contract(&mut storage_adapter, &tx1, &new_test_context()).unwrap();

    // Verify token_id in change matches persisted TOKEN_ID_NUM
    let token_id1 = match &result1.trc10_changes[0] {
        tron_backend_execution::Trc10Change::AssetIssued(issued) => issued.token_id.clone().unwrap(),
        _ => panic!("Expected AssetIssued"),
    };
    assert_eq!(token_id1, "1000001");

    // KEY ASSERTION: TOKEN_ID_NUM must be persisted to storage
    let persisted_token_id_num = storage_adapter.get_token_id_num().unwrap();
    assert_eq!(persisted_token_id_num, 1_000_001, "TOKEN_ID_NUM must be persisted after asset issue");
    assert_eq!(token_id1, persisted_token_id_num.to_string(), "Emitted token_id must match persisted TOKEN_ID_NUM");

    // Issue second asset (different owner) to verify incrementing works
    let owner2 = Address::from([0x22u8; 20]);
    storage_adapter.set_account(owner2, AccountInfo {
        balance: U256::from(2_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    }).unwrap();

    let contract_data2 = build_asset_issue_contract_data(
        owner2, b"Token2", 2000, 1, 1, 1000000, 2000000, b"https://t2.example",
    );
    let tx2 = TronTransaction {
        from: owner2,
        to: None,
        value: U256::ZERO,
        data: contract_data2,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AssetIssueContract),
            asset_id: None,
            ..Default::default()
        },
    };

    let result2 = service.execute_asset_issue_contract(&mut storage_adapter, &tx2, &new_test_context()).unwrap();

    let token_id2 = match &result2.trc10_changes[0] {
        tron_backend_execution::Trc10Change::AssetIssued(issued) => issued.token_id.clone().unwrap(),
        _ => panic!("Expected AssetIssued"),
    };
    assert_eq!(token_id2, "1000002", "Second token should get incremented ID");

    let final_token_id_num = storage_adapter.get_token_id_num().unwrap();
    assert_eq!(final_token_id_num, 1_000_002, "TOKEN_ID_NUM must be incremented after second asset issue");
}
