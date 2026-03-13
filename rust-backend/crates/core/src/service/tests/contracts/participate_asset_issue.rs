//! ParticipateAssetIssueContract tests (TRC-10 Token Sale Participation).
//!
//! Tests for Java parity validation:
//! - owner_address validation (length == 21, correct prefix)
//! - to_address validation (length == 21, correct prefix)
//! - empty asset_name error message ("No asset named null")
//! - amount validation
//! - self-participation rejection

use super::super::super::*;
use super::common::{encode_varint, new_test_context, seed_dynamic_properties, make_from_raw};
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata};
use revm_primitives::{Address, Bytes, U256, AccountInfo};
use tron_backend_common::{ModuleManager, ExecutionConfig, RemoteExecutionConfig};
use tron_backend_storage::StorageEngine;

fn new_test_service_with_participate_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            trc10_enabled: true,
            participate_asset_issue_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Build ParticipateAssetIssueContract protobuf bytes
fn build_participate_contract_data(
    owner_address: &[u8],
    to_address: &[u8],
    asset_name: &[u8],
    amount: i64,
) -> Bytes {
    let mut data = Vec::new();

    // Field 1: owner_address (bytes)
    if !owner_address.is_empty() {
        encode_varint(&mut data, (1 << 3) | 2);
        encode_varint(&mut data, owner_address.len() as u64);
        data.extend_from_slice(owner_address);
    }

    // Field 2: to_address (bytes)
    if !to_address.is_empty() {
        encode_varint(&mut data, (2 << 3) | 2);
        encode_varint(&mut data, to_address.len() as u64);
        data.extend_from_slice(to_address);
    }

    // Field 3: asset_name (bytes)
    encode_varint(&mut data, (3 << 3) | 2);
    encode_varint(&mut data, asset_name.len() as u64);
    data.extend_from_slice(asset_name);

    // Field 4: amount (int64)
    encode_varint(&mut data, (4 << 3) | 0);
    encode_varint(&mut data, amount as u64);

    Bytes::from(data)
}

/// Create a valid 21-byte TRON address with 0x41 prefix
fn make_tron_address(addr: &Address) -> Vec<u8> {
    let mut tron_addr = vec![0x41u8];
    tron_addr.extend_from_slice(addr.as_slice());
    tron_addr
}

// =============================================================================
// owner_address validation tests
// =============================================================================

#[test]
fn test_participate_validate_fail_owner_address_empty() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);

    // Build contract with EMPTY owner_address
    let contract_data = build_participate_contract_data(
        &[],  // Empty owner_address
        &make_tron_address(&issuer),
        b"TEST",
        100,
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail with empty owner_address");
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress", "Error message should match Java parity");
}

#[test]
fn test_participate_validate_fail_owner_address_too_short() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);

    // Build contract with 20-byte owner_address (missing prefix)
    let contract_data = build_participate_contract_data(
        owner.as_slice(),  // Only 20 bytes, missing 0x41 prefix
        &make_tron_address(&issuer),
        b"TEST",
        100,
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail with 20-byte owner_address");
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress", "Error message should match Java parity");
}

#[test]
fn test_participate_validate_fail_owner_address_wrong_prefix() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);

    // Build owner_address with testnet prefix (0xa0) instead of mainnet (0x41)
    let mut wrong_prefix_owner = vec![0xa0u8];
    wrong_prefix_owner.extend_from_slice(owner.as_slice());

    let contract_data = build_participate_contract_data(
        &wrong_prefix_owner,  // Wrong prefix
        &make_tron_address(&issuer),
        b"TEST",
        100,
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail with wrong prefix owner_address");
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress", "Error message should match Java parity");
}

// =============================================================================
// to_address validation tests
// =============================================================================

#[test]
fn test_participate_validate_fail_to_address_empty() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);

    // Build contract with EMPTY to_address
    let contract_data = build_participate_contract_data(
        &make_tron_address(&owner),
        &[],  // Empty to_address
        b"TEST",
        100,
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail with empty to_address");
    assert_eq!(result.err().unwrap(), "Invalid toAddress", "Error message should match Java parity");
}

#[test]
fn test_participate_validate_fail_to_address_too_short() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);

    // Build contract with 20-byte to_address (missing prefix)
    let contract_data = build_participate_contract_data(
        &make_tron_address(&owner),
        issuer.as_slice(),  // Only 20 bytes, missing 0x41 prefix
        b"TEST",
        100,
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail with 20-byte to_address");
    assert_eq!(result.err().unwrap(), "Invalid toAddress", "Error message should match Java parity");
}

#[test]
fn test_participate_validate_fail_to_address_wrong_prefix() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);

    // Build to_address with testnet prefix (0xa0) instead of mainnet (0x41)
    let mut wrong_prefix_to = vec![0xa0u8];
    wrong_prefix_to.extend_from_slice(issuer.as_slice());

    let contract_data = build_participate_contract_data(
        &make_tron_address(&owner),
        &wrong_prefix_to,  // Wrong prefix
        b"TEST",
        100,
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail with wrong prefix to_address");
    assert_eq!(result.err().unwrap(), "Invalid toAddress", "Error message should match Java parity");
}

// =============================================================================
// asset_name error message parity test
// =============================================================================

#[test]
fn test_participate_validate_fail_empty_asset_name_message_parity() {
    // Java's ByteArray.toStr([]) returns "null", not ""
    // So error message should be "No asset named null"
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);

    // Set up owner account with sufficient balance
    storage_adapter.set_account(owner, AccountInfo {
        balance: U256::from(1_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    }).unwrap();

    // Build contract with EMPTY asset_name
    let contract_data = build_participate_contract_data(
        &make_tron_address(&owner),
        &make_tron_address(&issuer),
        &[],  // Empty asset_name
        100,
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail with empty asset_name");
    // Java parity: ByteArray.toStr([]) == "null"
    assert_eq!(result.err().unwrap(), "No asset named null",
        "Error message should use 'null' for empty asset_name (Java parity)");
}

// =============================================================================
// Additional validation tests
// =============================================================================

#[test]
fn test_participate_validate_fail_amount_zero() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);

    let contract_data = build_participate_contract_data(
        &make_tron_address(&owner),
        &make_tron_address(&issuer),
        b"TEST",
        0,  // Zero amount
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail with zero amount");
    assert_eq!(result.err().unwrap(), "Amount must greater than 0!");
}

#[test]
fn test_participate_validate_fail_self_participation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);

    // Build contract where owner == to_address (self-participation)
    let contract_data = build_participate_contract_data(
        &make_tron_address(&owner),
        &make_tron_address(&owner),  // Same as owner
        b"TEST",
        100,
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(owner),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail with self-participation");
    assert_eq!(result.err().unwrap(), "Cannot participate asset Issue yourself !");
}

#[test]
fn test_participate_validate_fail_owner_account_not_exist() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);
    // Note: owner account is NOT created

    let contract_data = build_participate_contract_data(
        &make_tron_address(&owner),
        &make_tron_address(&issuer),
        b"TEST",
        100,
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail when owner account does not exist");
    assert_eq!(result.err().unwrap(), "Account does not exist!");
}

#[test]
fn test_participate_validate_fail_insufficient_balance() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);

    // Set up owner account with insufficient balance
    storage_adapter.set_account(owner, AccountInfo {
        balance: U256::from(50u64),  // Only 50, but trying to spend 100
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    }).unwrap();

    let contract_data = build_participate_contract_data(
        &make_tron_address(&owner),
        &make_tron_address(&issuer),
        b"TEST",
        100,  // More than balance
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail with insufficient balance");
    assert_eq!(result.err().unwrap(), "No enough balance !");
}

#[test]
fn test_participate_validate_fail_asset_not_exist() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);

    // Set up owner account with sufficient balance
    storage_adapter.set_account(owner, AccountInfo {
        balance: U256::from(1_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    }).unwrap();

    // Note: asset "NONEXISTENT" is NOT created

    let contract_data = build_participate_contract_data(
        &make_tron_address(&owner),
        &make_tron_address(&issuer),
        b"NONEXISTENT",
        100,
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_err(), "Should fail when asset does not exist");
    assert_eq!(result.err().unwrap(), "No asset named NONEXISTENT");
}

// =============================================================================
// Happy path test
// =============================================================================

#[test]
fn test_participate_happy_path() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // Set timestamp to be within asset's time window
    storage_engine.put("properties", b"latest_block_header_timestamp", &1_500_000i64.to_be_bytes()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_participate_enabled();

    let owner = Address::from([1u8; 20]);
    let issuer = Address::from([2u8; 20]);

    // Set up owner account with sufficient balance
    storage_adapter.set_account(owner, AccountInfo {
        balance: U256::from(1_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    }).unwrap();

    // Set up issuer account
    storage_adapter.set_account(issuer, AccountInfo {
        balance: U256::from(0u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    }).unwrap();

    // Give issuer tokens
    let mut issuer_proto = storage_adapter.get_account_proto(&issuer).unwrap().unwrap();
    issuer_proto.asset.insert("TEST".to_string(), 10_000);
    issuer_proto.asset_v2.insert("1000001".to_string(), 10_000);
    storage_adapter.put_account_proto(&issuer, &issuer_proto).unwrap();

    // Create asset issue record
    let mut asset_issue = tron_backend_execution::protocol::AssetIssueContractData::default();
    asset_issue.id = "1000001".to_string();
    asset_issue.owner_address = make_tron_address(&issuer);
    asset_issue.trx_num = 1;
    asset_issue.num = 10;  // 10 tokens per TRX
    asset_issue.start_time = 1_000_000;
    asset_issue.end_time = 2_000_000;
    storage_adapter.put_asset_issue(b"TEST", &asset_issue, false).unwrap();

    let contract_data = build_participate_contract_data(
        &make_tron_address(&owner),
        &make_tron_address(&issuer),
        b"TEST",
        100,  // Spend 100 TRX
    );

    let transaction = TronTransaction {
        from: owner,
        to: Some(issuer),
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::ParticipateAssetIssueContract),
            asset_id: None,
            from_raw: Some(make_tron_address(&owner)),
            ..Default::default()
        },
    };

    let result = service.execute_participate_asset_issue_contract(&mut storage_adapter, &transaction, &new_test_context());
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    let exec_result = result.unwrap();
    assert!(exec_result.success);
    assert!(exec_result.error.is_none());

    // Verify TRC-10 change emission
    assert_eq!(exec_result.trc10_changes.len(), 1);
    match &exec_result.trc10_changes[0] {
        tron_backend_execution::Trc10Change::AssetTransferred(transferred) => {
            assert_eq!(transferred.owner_address, issuer, "Token sender should be issuer");
            assert_eq!(transferred.to_address, owner, "Token receiver should be participant");
            assert_eq!(transferred.amount, 1000, "Should receive 10 tokens per TRX * 100 TRX = 1000 tokens");
        }
        _ => panic!("Expected AssetTransferred change"),
    }

    // Verify balance changes in state_changes
    assert_eq!(exec_result.state_changes.len(), 2);
}
