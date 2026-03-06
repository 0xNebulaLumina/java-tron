//! UnfreezeAssetContract tests (TRC-10 Unfreeze Asset Supply).
//!
//! Tests for Java parity validation against UnfreezeAssetActuator.java:
//! - Any.is(UnfreezeAssetContract.class) type_url check
//! - owner_address validation (length == 21, correct prefix)
//! - account existence
//! - frozen supply non-empty
//! - asset issued name/id non-empty
//! - time gate (expired entries exist)
//! - happy path: unfreezes expired entries and credits TRC-10 balance

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, new_test_context, seed_dynamic_properties};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TxMetadata};
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
fn build_unfreeze_asset_contract_data(owner_address: &[u8]) -> Bytes {
    let mut data = Vec::new();
    if !owner_address.is_empty() {
        encode_varint(&mut data, (1 << 3) | 2);
        encode_varint(&mut data, owner_address.len() as u64);
        data.extend_from_slice(owner_address);
    }
    Bytes::from(data)
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

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: build_unfreeze_asset_contract_data(&owner_tron),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
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

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: build_unfreeze_asset_contract_data(&owner_tron),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.UnfreezeAssetContract".to_string(),
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
// 2. Address validation
// =============================================================================

#[test]
fn test_unfreeze_asset_invalid_address_empty() {
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
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: None, // Empty from_raw → invalid
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
fn test_unfreeze_asset_invalid_address_wrong_length() {
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
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(vec![0x41, 0x01, 0x02]), // Only 3 bytes, not 21
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
fn test_unfreeze_asset_invalid_address_wrong_prefix() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let mut wrong_prefix = vec![0x00u8]; // Wrong prefix (should be 0x41)
    wrong_prefix.extend_from_slice(owner.as_slice());

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(wrong_prefix),
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
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
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

    // Seed account with NO frozen supply
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
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
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
    // Set ALLOW_SAME_TOKEN_NAME = 0 (legacy)
    storage_engine
        .put(
            "properties",
            b" ALLOW_SAME_TOKEN_NAME",
            &0i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    // Seed with frozen supply but NO asset_issued_name
    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(500, 1000)], // Has frozen supply
        b"",               // Empty asset_issued_name
        b"1000001",
    );

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
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
    // Set ALLOW_SAME_TOKEN_NAME = 1 (new mode)
    storage_engine
        .put(
            "properties",
            b" ALLOW_SAME_TOKEN_NAME",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_unfreeze_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    // Seed with frozen supply but NO asset_issued_id
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
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
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
    // Set ALLOW_SAME_TOKEN_NAME = 1
    storage_engine
        .put(
            "properties",
            b" ALLOW_SAME_TOKEN_NAME",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    // Set current timestamp to 500 (all entries expire at 1000)
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

    // All frozen entries have expire_time=1000 which is > now=500
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
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
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
    // Set ALLOW_SAME_TOKEN_NAME = 1
    storage_engine
        .put(
            "properties",
            b" ALLOW_SAME_TOKEN_NAME",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    // Set current timestamp to 1500 (entry at 1000 is expired, entry at 2000 is not)
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

    // Two frozen entries: (500 tokens, expire=1000) and (300 tokens, expire=2000)
    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(500, 1000), (300, 2000)],
        b"TEST",
        b"1000001",
    );

    // Seed initial TRC-10 balance of 100 in asset_v2
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    account_proto
        .asset_v2
        .insert("1000001".to_string(), 100);
    storage_adapter
        .put_account_proto(&owner, &account_proto)
        .unwrap();

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
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

    // Verify account state: only the non-expired entry should remain
    let updated_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(updated_account.frozen_supply.len(), 1);
    assert_eq!(updated_account.frozen_supply[0].frozen_balance, 300);
    assert_eq!(updated_account.frozen_supply[0].expire_time, 2000);

    // Verify TRC-10 balance: was 100, unfroze 500 → should be 600
    assert_eq!(
        *updated_account.asset_v2.get("1000001").unwrap(),
        600
    );
}

#[test]
fn test_unfreeze_asset_happy_path_all_expired() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put(
            "properties",
            b" ALLOW_SAME_TOKEN_NAME",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    // Set current timestamp to 5000 (all entries are expired)
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

    // All entries expired
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
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    let updated_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    // All entries should be removed
    assert!(updated_account.frozen_supply.is_empty());
    // TRC-10 balance: 0 + 500 + 300 + 200 = 1000
    assert_eq!(
        *updated_account.asset_v2.get("1000001").unwrap(),
        1000
    );
}

#[test]
fn test_unfreeze_asset_happy_path_allow_same_token_name_0() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // Set ALLOW_SAME_TOKEN_NAME = 0 (legacy mode)
    storage_engine
        .put(
            "properties",
            b" ALLOW_SAME_TOKEN_NAME",
            &0i64.to_be_bytes(),
        )
        .unwrap();
    // Set current timestamp to 1500
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

    // Frozen entry that is expired
    seed_account_with_frozen_supply(
        &mut storage_adapter,
        &owner,
        1_000_000,
        vec![(500, 1000)],
        b"TEST", // asset_issued_name
        b"",     // no asset_issued_id (legacy mode uses name)
    );

    // Seed initial TRC-10 balance
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    account_proto.asset.insert("TEST".to_string(), 100);
    account_proto
        .asset_v2
        .insert("1000001".to_string(), 100);
    storage_adapter
        .put_account_proto(&owner, &account_proto)
        .unwrap();

    // Create asset issue record (needed for legacy mode name→tokenId mapping)
    let mut asset_issue =
        tron_backend_execution::protocol::AssetIssueContractData::default();
    asset_issue.id = "1000001".to_string();
    asset_issue.owner_address = owner_tron.clone();
    storage_adapter
        .put_asset_issue(b"TEST", &asset_issue, false)
        .unwrap();

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
    };

    let result = service.execute_unfreeze_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    let updated_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert!(updated_account.frozen_supply.is_empty());
    // Both asset and asset_v2 should be updated
    assert_eq!(*updated_account.asset.get("TEST").unwrap(), 600);
    assert_eq!(
        *updated_account.asset_v2.get("1000001").unwrap(),
        600
    );
}

#[test]
fn test_unfreeze_asset_no_asset_issue_lookup_when_allow_same_token_name_1() {
    // When ALLOW_SAME_TOKEN_NAME == 1, Java does NOT look up AssetIssueStore.
    // Rust should succeed even without an AssetIssue record in the store.
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put(
            "properties",
            b" ALLOW_SAME_TOKEN_NAME",
            &1i64.to_be_bytes(),
        )
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
    // NOTE: Intentionally NOT creating an AssetIssue record.
    // In ALLOW_SAME_TOKEN_NAME==1 mode, Java doesn't need AssetIssueStore.

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
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
    assert_eq!(
        *updated_account.asset_v2.get("1000001").unwrap(),
        500
    );
}

// =============================================================================
// 8. Validation ordering test
// =============================================================================

#[test]
fn test_unfreeze_asset_type_error_before_address_error() {
    // Type check should happen before address validation
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
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: None, // Would trigger "Invalid address" if reached
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
    // Should get type error, NOT address error (type check comes first)
    assert!(
        result.as_ref().err().unwrap().contains("contract type error"),
        "Expected type error to come before address error, but got: {}",
        result.err().unwrap()
    );
}

#[test]
fn test_unfreeze_asset_expire_time_equals_now() {
    // Java: frozen.getExpireTime() <= now — so exactly equal means expired
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put(
            "properties",
            b" ALLOW_SAME_TOKEN_NAME",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    // Set timestamp exactly equal to expire_time
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
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UnfreezeAssetContract,
            ),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
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
    assert_eq!(
        *updated_account.asset_v2.get("1000001").unwrap(),
        500
    );
}
