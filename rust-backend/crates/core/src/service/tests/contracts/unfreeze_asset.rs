//! UnfreezeAssetContract tests (TRC-10 Unfreeze Asset Supply).
//!
//! Tests for Java parity validation against UnfreezeAssetActuator.java:
//! - Any.is(UnfreezeAssetContract.class) type_url check
//! - owner_address parsed from contract bytes (parity with any.unpack())
//! - owner_address validation (length == 21, correct prefix)
//! - account existence
//! - frozen supply non-empty
//! - asset issued name/id non-empty
//! - time gate (expired entries exist)
//! - wrapping_add for frozen balance summation (parity with Java's unchecked +=)
//! - importAsset behavior (ALLOW_ASSET_OPTIMIZATION)
//! - happy path: unfreezes expired entries and credits TRC-10 balance

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, new_test_context, seed_dynamic_properties};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{EngineBackedEvmStateStore, TronContractParameter, TronTransaction, TxMetadata};
use tron_backend_storage::StorageEngine;

fn new_test_service_with_unfreeze_asset_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            trc10_enabled: true,
            unfreeze_asset_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Build a minimal UnfreezeAssetContract protobuf bytes.
/// UnfreezeAssetContract: bytes owner_address = 1;
fn build_unfreeze_asset_contract_data(owner_address: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    if !owner_address.is_empty() {
        encode_varint(&mut data, (1 << 3) | 2);
        encode_varint(&mut data, owner_address.len() as u64);
        data.extend_from_slice(owner_address);
    }
    data
}

/// Seed an account proto with frozen_supply entries and asset issued info.
fn seed_account_with_frozen_supply(
    storage_adapter: &mut EngineBackedEvmStateStore,
    owner: &Address,
    balance: i64,
    frozen_entries: Vec<(i64, i64)>, // (frozen_balance, expire_time)
    asset_issued_name: &[u8],
    asset_issued_id: &[u8],
) {
    use tron_backend_execution::protocol::account::Frozen;

    // Set AccountInfo
    storage_adapter
        .set_account(
            *owner,
            AccountInfo {
                balance: U256::from(balance as u64),
                nonce: 0,
                code_hash: revm_primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    // Update account proto with frozen_supply and asset issued info
    let mut account_proto = storage_adapter.get_account_proto(owner).unwrap().unwrap();
    account_proto.frozen_supply = frozen_entries
        .into_iter()
        .map(|(frozen_balance, expire_time)| Frozen {
            frozen_balance,
            expire_time,
        })
        .collect();
    account_proto.asset_issued_name = asset_issued_name.to_vec();
    account_proto.asset_issued_id = asset_issued_id.to_vec();
    storage_adapter
        .put_account_proto(owner, &account_proto)
        .unwrap();
}

/// Build a standard TxMetadata with contract_parameter containing the contract bytes.
/// This mirrors Java's behavior where the contract bytes come from contract_parameter.value.
fn make_metadata_with_contract(owner_tron: &[u8]) -> TxMetadata {
    let contract_bytes = build_unfreeze_asset_contract_data(owner_tron);
    TxMetadata {
        contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
        from_raw: Some(owner_tron.to_vec()),
        contract_parameter: Some(tron_backend_execution::TronContractParameter {
            type_url: "type.googleapis.com/protocol.UnfreezeAssetContract".to_string(),
            value: contract_bytes,
        }),
        ..Default::default()
    }
}

// =============================================================================
// 1. Any type_url validation (contract-type check)
// =============================================================================

#[test]
fn test_unfreeze_asset_wrong_type_url() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);
    let contract_bytes = build_unfreeze_asset_contract_data(&owner_tron);

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: Some(owner_tron),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.WrongContract".to_string(),
                value: contract_bytes,
            }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap(),
        "contract type error, expected type [UnfreezeAssetContract], real type[class com.google.protobuf.Any]"
    );
}

#[test]
fn test_unfreeze_asset_correct_type_url_with_prefix() {
    // Validates that type_url with googleapis prefix passes the type check
    // (should then fail at a later validation step, not at type check)
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);
    let contract_bytes = build_unfreeze_asset_contract_data(&owner_tron);

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: Some(owner_tron),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.UnfreezeAssetContract".to_string(),
                value: contract_bytes,
            }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    // Should NOT fail with type error; should fail later (account not exist)
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        !err.contains("contract type error"),
        "Should not fail on type check, but got: {}",
        err
    );
}

// =============================================================================
// 2. Address validation — owner parsed from contract bytes
// =============================================================================

#[test]
fn test_unfreeze_asset_invalid_address_empty_from_proto() {
    // When the contract bytes contain no owner_address, and from_raw is also empty
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);

    // Empty contract bytes → parse_unfreeze_asset_owner_address returns empty vec
    // from_raw is also None → falls back to &[]
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(), // Empty contract data
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: None,
            contract_parameter: Some(TronContractParameter { type_url: "protocol.UnfreezeAssetContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid address");
}

#[test]
fn test_unfreeze_asset_invalid_address_wrong_length_in_proto() {
    // Contract bytes contain a 3-byte address (too short)
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let short_addr = vec![0x41, 0x01, 0x02]; // Only 3 bytes, not 21
    let contract_bytes = build_unfreeze_asset_contract_data(&short_addr);

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: Some(make_from_raw(&owner)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.UnfreezeAssetContract".to_string(), value: contract_bytes.clone() }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid address");
}

#[test]
fn test_unfreeze_asset_invalid_address_wrong_prefix_in_proto() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let mut wrong_prefix = vec![0x00u8]; // Wrong prefix (should be 0x41)
    wrong_prefix.extend_from_slice(owner.as_slice());
    let contract_bytes = build_unfreeze_asset_contract_data(&wrong_prefix);

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: Some(make_from_raw(&owner)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.UnfreezeAssetContract".to_string(), value: contract_bytes.clone() }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid address");
}

#[test]
fn test_unfreeze_asset_owner_from_proto_preferred_over_from_raw() {
    // When contract_parameter.value has a valid owner_address, it should be used
    // even if from_raw points to a different address
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let real_owner = Address::from([1u8; 20]);
    let different_addr = Address::from([2u8; 20]);
    let real_owner_tron = make_from_raw(&real_owner);
    let contract_bytes = build_unfreeze_asset_contract_data(&real_owner_tron);

    // Seed the real_owner account (the one in the proto)
    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &real_owner,
        1_000_000,
        vec![(500, 1000)],
        b"TEST",
        b"1000001",
    );

    let transaction = TronTransaction {
        from: different_addr, // Different from contract bytes
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: Some(make_from_raw(&different_addr)), // Points to different address
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.UnfreezeAssetContract".to_string(),
                value: contract_bytes,
            }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    // Should use the address from the proto (real_owner), not from_raw (different_addr).
    // Since real_owner exists but timestamp is 0, it should reach the time gate check.
    assert!(result.is_err());
    let err = result.err().unwrap();
    // Should NOT be "Account does not exist" for real_owner (it was seeded)
    // Should be "It's not time to unfreeze asset supply" (timestamp=0 < expire_time=1000)
    assert_eq!(err, "It's not time to unfreeze asset supply");
}

// =============================================================================
// 3. Account existence
// =============================================================================

#[test]
fn test_unfreeze_asset_account_not_exist() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);
    let readable = hex::encode(&owner_tron);

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(build_unfreeze_asset_contract_data(&owner_tron)),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.UnfreezeAssetContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap(),
        format!("Account[{}] does not exist", readable)
    );
}

// =============================================================================
// 4. Frozen supply non-empty
// =============================================================================

#[test]
fn test_unfreeze_asset_no_frozen_supply() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![], // No frozen supply
        b"TEST",
        b"1000001",
    );

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(build_unfreeze_asset_contract_data(&owner_tron)),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.UnfreezeAssetContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "no frozen supply balance");
}

// =============================================================================
// 5. Asset issued name/id validation
// =============================================================================

#[test]
fn test_unfreeze_asset_no_asset_issued_name_legacy_mode() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &0i64.to_be_bytes())
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(500, 1000)],
        b"", // Empty asset_issued_name
        b"1000001",
    );

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(build_unfreeze_asset_contract_data(&owner_tron)),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.UnfreezeAssetContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap(),
        "this account has not issued any asset"
    );
}

#[test]
fn test_unfreeze_asset_no_asset_issued_id_new_mode() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(500, 1000)],
        b"TEST",
        b"", // Empty asset_issued_id
    );

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(build_unfreeze_asset_contract_data(&owner_tron)),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.UnfreezeAssetContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap(),
        "this account has not issued any asset"
    );
}

// =============================================================================
// 6. Time gate
// =============================================================================

#[test]
fn test_unfreeze_asset_not_time_yet() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &500i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    // All frozen entries have expire_time > now=500
    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(500, 1000), (300, 2000)],
        b"TEST",
        b"1000001",
    );

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(build_unfreeze_asset_contract_data(&owner_tron)),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.UnfreezeAssetContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap(),
        "It's not time to unfreeze asset supply"
    );
}

// =============================================================================
// 7. Happy path tests
// =============================================================================

#[test]
fn test_unfreeze_asset_happy_path_allow_same_token_name_1() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1500i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    // Two frozen entries: (500, expire=1000) and (300, expire=2000)
    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(500, 1000), (300, 2000)],
        b"TEST",
        b"1000001",
    );

    // Seed initial TRC-10 balance
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    account_proto.asset_v2.insert("1000001".to_string(), 100);
    storage_adapter
        .put_account_proto(&owner, &account_proto)
        .unwrap();

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(build_unfreeze_asset_contract_data(&owner_tron)),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: make_metadata_with_contract(&owner_tron),
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    let exec_result = result.unwrap();
    assert!(exec_result.success);
    assert!(exec_result.error.is_none());
    assert_eq!(exec_result.state_changes.len(), 1);

    let updated_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(updated_account.frozen_supply.len(), 1);
    assert_eq!(updated_account.frozen_supply[0].frozen_balance, 300);
    assert_eq!(updated_account.frozen_supply[0].expire_time, 2000);
    // TRC-10 balance: was 100, unfroze 500 → 600
    assert_eq!(*updated_account.asset_v2.get("1000001").unwrap(), 600);
}

#[test]
fn test_unfreeze_asset_happy_path_all_expired() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &5000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(500, 1000), (300, 2000), (200, 3000)],
        b"TEST",
        b"1000001",
    );

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(build_unfreeze_asset_contract_data(&owner_tron)),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: make_metadata_with_contract(&owner_tron),
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    let updated_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert!(updated_account.frozen_supply.is_empty());
    // 0 + 500 + 300 + 200 = 1000
    assert_eq!(*updated_account.asset_v2.get("1000001").unwrap(), 1000);
}

#[test]
fn test_unfreeze_asset_happy_path_allow_same_token_name_0() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1500i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(500, 1000)],
        b"TEST",
        b"",
    );

    // Seed initial TRC-10 balance
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    account_proto.asset.insert("TEST".to_string(), 100);
    account_proto.asset_v2.insert("1000001".to_string(), 100);
    storage_adapter
        .put_account_proto(&owner, &account_proto)
        .unwrap();

    // Create asset issue record (needed in legacy mode for name→tokenId mapping)
    let mut asset_issue = tron_backend_execution::protocol::AssetIssueContractData::default();
    asset_issue.id = "1000001".to_string();
    asset_issue.owner_address = owner_tron.clone();
    storage_adapter
        .put_asset_issue(b"TEST", &asset_issue, false)
        .unwrap();

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(build_unfreeze_asset_contract_data(&owner_tron)),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: make_metadata_with_contract(&owner_tron),
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    let updated_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert!(updated_account.frozen_supply.is_empty());
    assert_eq!(*updated_account.asset.get("TEST").unwrap(), 600);
    assert_eq!(*updated_account.asset_v2.get("1000001").unwrap(), 600);
}

#[test]
fn test_unfreeze_asset_no_asset_issue_lookup_when_allow_same_token_name_1() {
    // When ALLOW_SAME_TOKEN_NAME == 1, Java does NOT look up AssetIssueStore.
    // Rust should succeed even without an AssetIssue record in the store.
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &5000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(500, 1000)],
        b"TEST",
        b"1000001",
    );
    // NOTE: No AssetIssue record created — Java doesn't need it when ALLOW_SAME_TOKEN_NAME==1

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(build_unfreeze_asset_contract_data(&owner_tron)),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: make_metadata_with_contract(&owner_tron),
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(
        result.is_ok(),
        "Should succeed without AssetIssue record when ALLOW_SAME_TOKEN_NAME==1: {:?}",
        result.err()
    );

    let updated_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert!(updated_account.frozen_supply.is_empty());
    assert_eq!(*updated_account.asset_v2.get("1000001").unwrap(), 500);
}

// =============================================================================
// 8. Validation ordering and edge case tests
// =============================================================================

#[test]
fn test_unfreeze_asset_type_error_before_address_error() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UnfreezeAssetContract),
            from_raw: None,
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.WrongContract".to_string(),
                value: vec![],
            }),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert!(
        result
            .as_ref()
            .err()
            .unwrap()
            .contains("contract type error"),
        "Expected type error to come before address error, but got: {}",
        result.err().unwrap()
    );
}

#[test]
fn test_unfreeze_asset_expire_time_equals_now() {
    // Java: frozen.getExpireTime() <= now — exactly equal means expired
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &1000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(500, 1000)], // expire_time == now
        b"TEST",
        b"1000001",
    );

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(build_unfreeze_asset_contract_data(&owner_tron)),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: make_metadata_with_contract(&owner_tron),
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(
        result.is_ok(),
        "expire_time == now should be treated as expired: {:?}",
        result.err()
    );

    let updated_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert!(updated_account.frozen_supply.is_empty());
    assert_eq!(*updated_account.asset_v2.get("1000001").unwrap(), 500);
}

// =============================================================================
// 9. Wrapping addition parity test
// =============================================================================

#[test]
fn test_unfreeze_asset_wrapping_add_for_summation() {
    // Java: unfreezeAsset += frozenBalance is unchecked (wrapping).
    // Rust must use wrapping_add for the summation.
    // The overflow is caught later by add_asset_amount_v2 (checked_add → "long overflow").
    // This test verifies that even with values close to i64::MAX, the summation wraps
    // and the final add_asset_amount_v2 catches the overflow.
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"latest_block_header_timestamp",
            &5000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    // Two frozen entries that sum to > i64::MAX  (wraps to a negative)
    // i64::MAX = 9223372036854775807
    let big = i64::MAX / 2 + 1; // 4611686018427387904
    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(big, 1000), (big, 2000)],
        b"TEST",
        b"1000001",
    );

    // Seed initial TRC-10 balance of 0
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    account_proto.asset_v2.insert("1000001".to_string(), 0);
    storage_adapter
        .put_account_proto(&owner, &account_proto)
        .unwrap();

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(build_unfreeze_asset_contract_data(&owner_tron)),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: make_metadata_with_contract(&owner_tron),
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    // The wrapping sum is negative → add_asset_amount_v2 will get a negative amount.
    // Java's addExact(0, negative_wrapped_value) would produce a negative result (no overflow
    // since both are valid i64). The exact behavior depends on Java's disableJavaLangMath flag.
    // For parity, the key point is that the summation wraps (does NOT error with "Overflow").
    // The result may succeed (if the add is valid) or fail with "long overflow" depending on
    // the specific wrapped value. Either way, it must NOT fail with our old
    // "Overflow calculating unfreeze amount" message.
    if let Err(ref err) = result {
        assert!(
            !err.contains("Overflow calculating unfreeze amount"),
            "Should use wrapping_add, not checked_add, for summation. Got: {}",
            err
        );
    }
}

// =============================================================================
// 10. Proto parsing unit test
// =============================================================================

#[test]
fn test_parse_unfreeze_asset_owner_address() {
    let owner = Address::from([0xABu8; 20]);
    let owner_tron = make_from_raw(&owner);
    let contract_bytes = build_unfreeze_asset_contract_data(&owner_tron);

    let parsed = BackendService::parse_unfreeze_asset_owner_address(&contract_bytes);
    assert!(parsed.is_ok());
    assert_eq!(parsed.unwrap(), owner_tron);
}

#[test]
fn test_parse_unfreeze_asset_owner_address_empty() {
    let parsed = BackendService::parse_unfreeze_asset_owner_address(&[]);
    assert!(parsed.is_ok());
    assert!(parsed.unwrap().is_empty());
}
