//! UpdateAssetContract tests (TRC-10 Update Asset Metadata).
//!
//! Tests for Java parity validation against UpdateAssetActuator.java:
//! - Any.is(UpdateAssetContract.class) type_url check
//! - owner_address parsed from contract bytes (parity with any.unpack())
//! - owner_address validation (length == 21, correct prefix)
//! - account existence
//! - asset issued name/id + store existence (checked BEFORE url/desc/limits)
//! - URL validation (non-empty, <= 256 bytes)
//! - description validation (<= 200 bytes)
//! - limit bounds (0 <= limit < ONE_DAY_NET_LIMIT)
//! - dual-store update preserves per-store fields independently
//! - happy path: updates only the four fields on each store entry

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, new_test_context, seed_dynamic_properties};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::protocol::AssetIssueContractData;
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TxMetadata};
use tron_backend_storage::StorageEngine;

fn new_test_service_with_update_asset_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            trc10_enabled: true,
            update_asset_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Build UpdateAssetContract protobuf bytes.
/// UpdateAssetContract:
///   bytes  owner_address    = 1;
///   bytes  description      = 2;
///   bytes  url              = 3;
///   int64  new_limit        = 4;
///   int64  new_public_limit = 5;
fn build_update_asset_contract_data(
    owner_address: &[u8],
    description: &[u8],
    url: &[u8],
    new_limit: i64,
    new_public_limit: i64,
) -> Vec<u8> {
    let mut data = Vec::new();
    // field 1: owner_address (bytes)
    if !owner_address.is_empty() {
        encode_varint(&mut data, (1 << 3) | 2);
        encode_varint(&mut data, owner_address.len() as u64);
        data.extend_from_slice(owner_address);
    }
    // field 2: description (bytes)
    if !description.is_empty() {
        encode_varint(&mut data, (2 << 3) | 2);
        encode_varint(&mut data, description.len() as u64);
        data.extend_from_slice(description);
    }
    // field 3: url (bytes)
    if !url.is_empty() {
        encode_varint(&mut data, (3 << 3) | 2);
        encode_varint(&mut data, url.len() as u64);
        data.extend_from_slice(url);
    }
    // field 4: new_limit (varint)
    if new_limit != 0 {
        encode_varint(&mut data, 4 << 3);
        encode_varint(&mut data, new_limit as u64);
    }
    // field 5: new_public_limit (varint)
    if new_public_limit != 0 {
        encode_varint(&mut data, 5 << 3);
        encode_varint(&mut data, new_public_limit as u64);
    }
    data
}

/// Seed an account with asset issued name and ID.
fn seed_account_with_asset_issued(
    storage_adapter: &mut EngineBackedEvmStateStore,
    owner: &Address,
    balance: i64,
    asset_issued_name: &[u8],
    asset_issued_id: &[u8],
) {
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

    let mut account_proto = storage_adapter.get_account_proto(owner).unwrap().unwrap();
    account_proto.asset_issued_name = asset_issued_name.to_vec();
    account_proto.asset_issued_id = asset_issued_id.to_vec();
    storage_adapter
        .put_account_proto(owner, &account_proto)
        .unwrap();
}

/// Seed an asset issue entry in the specified store via the adapter.
fn seed_asset_issue(
    storage_adapter: &mut EngineBackedEvmStateStore,
    key: &[u8],
    asset: &AssetIssueContractData,
    v2_store: bool,
) {
    storage_adapter
        .put_asset_issue(key, asset, v2_store)
        .unwrap();
}

/// Build a standard TxMetadata with contract_parameter for UpdateAssetContract.
fn make_metadata_with_contract(
    owner_tron: &[u8],
    description: &[u8],
    url: &[u8],
    new_limit: i64,
    new_public_limit: i64,
) -> TxMetadata {
    let contract_bytes =
        build_update_asset_contract_data(owner_tron, description, url, new_limit, new_public_limit);
    TxMetadata {
        contract_type: Some(tron_backend_execution::TronContractType::UpdateAssetContract),
        from_raw: Some(owner_tron.to_vec()),
        contract_parameter: Some(tron_backend_execution::TronContractParameter {
            type_url: "type.googleapis.com/protocol.UpdateAssetContract".to_string(),
            value: contract_bytes,
        }),
        ..Default::default()
    }
}

/// Create a default AssetIssueContractData for testing.
fn default_asset_issue(owner_tron: &[u8], name: &[u8], id: &str) -> AssetIssueContractData {
    AssetIssueContractData {
        owner_address: owner_tron.to_vec(),
        name: name.to_vec(),
        abbr: b"TST".to_vec(),
        total_supply: 1_000_000,
        trx_num: 1,
        num: 1,
        start_time: 1000,
        end_time: 2000,
        description: b"original description".to_vec(),
        url: b"https://original.url".to_vec(),
        free_asset_net_limit: 100,
        public_free_asset_net_limit: 200,
        public_free_asset_net_usage: 50,
        public_latest_free_net_time: 999,
        id: id.to_string(),
        ..Default::default()
    }
}

/// Helper: set up storage adapter with common dynamic properties for V2 mode.
fn setup_v2_adapter() -> (EngineBackedEvmStateStore, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    let adapter = EngineBackedEvmStateStore::new(storage_engine);
    (adapter, temp_dir)
}

/// Helper: set up storage adapter with common dynamic properties for legacy mode.
fn setup_legacy_adapter() -> (EngineBackedEvmStateStore, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &0i64.to_be_bytes())
        .unwrap();
    let adapter = EngineBackedEvmStateStore::new(storage_engine);
    (adapter, temp_dir)
}

/// Helper: seed account + V2 asset for common test setup.
fn seed_full_v2_setup(
    storage_adapter: &mut EngineBackedEvmStateStore,
    owner: &Address,
    owner_tron: &[u8],
) {
    let asset = default_asset_issue(owner_tron, b"TestToken", "100001");
    seed_asset_issue(storage_adapter, b"100001", &asset, true);
    seed_account_with_asset_issued(storage_adapter, owner, 1_000_000, b"TestToken", b"100001");
}

// =============================================================================
// 1. Any type_url validation
// =============================================================================

#[test]
fn test_update_asset_wrong_type_url() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);
    let contract_bytes =
        build_update_asset_contract_data(&owner_tron, b"desc", b"https://url.com", 100, 200);

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UpdateAssetContract),
            from_raw: Some(owner_tron),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.WrongContract".to_string(),
                value: contract_bytes,
            }),
            ..Default::default()
        },
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "contract type error, expected type [UpdateAssetContract],real type[class com.google.protobuf.Any]"
    );
}

// =============================================================================
// 2. Owner address validation (Java parity: DecodeUtil.addressValid)
// =============================================================================

#[test]
fn test_update_asset_invalid_owner_address_20_bytes() {
    // Java requires exactly 21 bytes; 20-byte address should fail
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    // Use 20-byte address (no 0x41 prefix) as owner_address in contract bytes
    let owner_20_bytes = owner.as_slice().to_vec();
    let contract_bytes =
        build_update_asset_contract_data(&owner_20_bytes, b"desc", b"https://url.com", 100, 200);

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UpdateAssetContract),
            from_raw: Some(owner_20_bytes.clone()),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.UpdateAssetContract".to_string(),
                value: contract_bytes,
            }),
            ..Default::default()
        },
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid ownerAddress");
}

#[test]
fn test_update_asset_invalid_owner_address_wrong_prefix() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    // 21-byte address with wrong prefix
    let mut owner_bad_prefix = vec![0x42u8];
    owner_bad_prefix.extend_from_slice(owner.as_slice());
    let contract_bytes =
        build_update_asset_contract_data(&owner_bad_prefix, b"desc", b"https://url.com", 100, 200);

    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::UpdateAssetContract),
            from_raw: Some(owner_bad_prefix.clone()),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.UpdateAssetContract".to_string(),
                value: contract_bytes,
            }),
            ..Default::default()
        },
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid ownerAddress");
}

// =============================================================================
// 3. Account does not exist
// =============================================================================

#[test]
fn test_update_asset_account_not_exist() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    let metadata = make_metadata_with_contract(&owner_tron, b"desc", b"https://url.com", 100, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Account does not exist");
}

// =============================================================================
// 4. Account has not issued any asset
// =============================================================================

#[test]
fn test_update_asset_no_asset_issued_v2_mode() {
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    // Create account WITHOUT asset_issued_id
    seed_account_with_asset_issued(&mut storage_adapter, &owner, 1_000_000, b"", b"");

    let metadata = make_metadata_with_contract(&owner_tron, b"desc", b"https://url.com", 100, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Account has not issued any asset");
}

#[test]
fn test_update_asset_no_asset_issued_legacy_mode() {
    let (mut storage_adapter, _temp_dir) = setup_legacy_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    // Create account WITHOUT asset_issued_name
    seed_account_with_asset_issued(&mut storage_adapter, &owner, 1_000_000, b"", b"");

    let metadata = make_metadata_with_contract(&owner_tron, b"desc", b"https://url.com", 100, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Account has not issued any asset");
}

// =============================================================================
// 5. Asset store existence (checked BEFORE url/desc/limits — validation order)
// =============================================================================

#[test]
fn test_update_asset_store_not_exist_v2_mode() {
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    // Create account with asset_issued_id but do NOT seed asset in V2 store
    seed_account_with_asset_issued(&mut storage_adapter, &owner, 1_000_000, b"", b"100001");

    let metadata = make_metadata_with_contract(&owner_tron, b"desc", b"https://url.com", 100, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "Asset is not existed in AssetIssueV2Store"
    );
}

#[test]
fn test_update_asset_store_not_exist_legacy_mode() {
    let (mut storage_adapter, _temp_dir) = setup_legacy_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    // Create account with asset_issued_name but do NOT seed asset in legacy store
    seed_account_with_asset_issued(
        &mut storage_adapter,
        &owner,
        1_000_000,
        b"TestToken",
        b"100001",
    );

    let metadata = make_metadata_with_contract(&owner_tron, b"desc", b"https://url.com", 100, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "Asset is not existed in AssetIssueStore"
    );
}

/// Regression: combined-bad inputs (missing asset + invalid URL).
/// Java checks asset store existence BEFORE URL → should get asset error, not URL error.
#[test]
fn test_update_asset_error_precedence_missing_asset_and_invalid_url() {
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    // Account with asset_issued_id set, but NO asset in V2 store
    seed_account_with_asset_issued(&mut storage_adapter, &owner, 1_000_000, b"", b"100001");

    // Empty URL (invalid) — but asset store check should fire first
    let metadata = make_metadata_with_contract(&owner_tron, b"desc", b"", 100, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    // Java returns asset error first, not URL error
    assert_eq!(
        result.unwrap_err(),
        "Asset is not existed in AssetIssueV2Store"
    );
}

// =============================================================================
// 6. URL validation
// =============================================================================

#[test]
fn test_update_asset_invalid_url_empty() {
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_full_v2_setup(&mut storage_adapter, &owner, &owner_tron);

    // Empty URL
    let metadata = make_metadata_with_contract(&owner_tron, b"desc", b"", 100, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid url");
}

#[test]
fn test_update_asset_invalid_url_too_long() {
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_full_v2_setup(&mut storage_adapter, &owner, &owner_tron);

    // URL > 256 bytes
    let long_url = vec![b'a'; 257];
    let metadata = make_metadata_with_contract(&owner_tron, b"desc", &long_url, 100, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid url");
}

// =============================================================================
// 7. Description validation
// =============================================================================

#[test]
fn test_update_asset_invalid_description_too_long() {
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_full_v2_setup(&mut storage_adapter, &owner, &owner_tron);

    // Description > 200 bytes
    let long_desc = vec![b'a'; 201];
    let metadata =
        make_metadata_with_contract(&owner_tron, &long_desc, b"https://url.com", 100, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid description");
}

// =============================================================================
// 8. Limit validation
// =============================================================================

#[test]
fn test_update_asset_invalid_free_asset_net_limit_negative() {
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_full_v2_setup(&mut storage_adapter, &owner, &owner_tron);

    // new_limit = -1 (negative)
    let metadata = make_metadata_with_contract(&owner_tron, b"desc", b"https://url.com", -1, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid FreeAssetNetLimit");
}

#[test]
fn test_update_asset_invalid_free_asset_net_limit_too_large() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    // Set ONE_DAY_NET_LIMIT to 57_600_000_000 (Java default)
    storage_engine
        .put(
            "properties",
            b"ONE_DAY_NET_LIMIT",
            &57_600_000_000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_full_v2_setup(&mut storage_adapter, &owner, &owner_tron);

    // new_limit >= ONE_DAY_NET_LIMIT
    let metadata = make_metadata_with_contract(
        &owner_tron,
        b"desc",
        b"https://url.com",
        57_600_000_000,
        200,
    );
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid FreeAssetNetLimit");
}

#[test]
fn test_update_asset_invalid_public_free_asset_net_limit() {
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_full_v2_setup(&mut storage_adapter, &owner, &owner_tron);

    // new_public_limit = -1
    let metadata = make_metadata_with_contract(&owner_tron, b"desc", b"https://url.com", 100, -1);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid PublicFreeAssetNetLimit");
}

// =============================================================================
// 9. ONE_DAY_NET_LIMIT default parity
// =============================================================================

#[test]
fn test_update_asset_one_day_net_limit_default_matches_java() {
    // When ONE_DAY_NET_LIMIT is absent, default should be 57_600_000_000 (Java).
    // A limit value of 8_640_000_000 should be accepted (below Java default).
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    // Do NOT seed ONE_DAY_NET_LIMIT — rely on default
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_full_v2_setup(&mut storage_adapter, &owner, &owner_tron);

    // 8_640_000_000 < 57_600_000_000 → should pass (would fail with old default)
    let metadata =
        make_metadata_with_contract(&owner_tron, b"desc", b"https://url.com", 8_640_000_000, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());
}

// =============================================================================
// 10. Happy path: V2 mode
// =============================================================================

#[test]
fn test_update_asset_happy_path_v2_mode() {
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    let mut asset = default_asset_issue(&owner_tron, b"TestToken", "100001");
    asset.public_free_asset_net_usage = 42;
    asset.public_latest_free_net_time = 999;
    seed_asset_issue(&mut storage_adapter, b"100001", &asset, true);
    seed_account_with_asset_issued(
        &mut storage_adapter,
        &owner,
        1_000_000,
        b"TestToken",
        b"100001",
    );

    let metadata = make_metadata_with_contract(
        &owner_tron,
        b"new description",
        b"https://new.url",
        500,
        600,
    );
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());

    let exec_result = result.unwrap();
    assert!(exec_result.success);
    assert_eq!(exec_result.state_changes.len(), 0);

    // Verify the updated asset in V2 store
    let updated_asset = storage_adapter
        .get_asset_issue(b"100001", 1)
        .unwrap()
        .expect("V2 asset should exist");
    assert_eq!(updated_asset.free_asset_net_limit, 500);
    assert_eq!(updated_asset.public_free_asset_net_limit, 600);
    assert_eq!(updated_asset.url, b"https://new.url");
    assert_eq!(updated_asset.description, b"new description");
    // Preserved fields should be unchanged
    assert_eq!(updated_asset.public_free_asset_net_usage, 42);
    assert_eq!(updated_asset.public_latest_free_net_time, 999);
    assert_eq!(updated_asset.total_supply, 1_000_000);
}

// =============================================================================
// 11. Happy path: legacy mode (dual-store update preserves per-store fields)
// =============================================================================

#[test]
fn test_update_asset_happy_path_legacy_mode_preserves_per_store_fields() {
    let (mut storage_adapter, _temp_dir) = setup_legacy_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    // Create legacy and V2 entries with DIFFERENT values for non-updated fields
    let mut legacy_asset = default_asset_issue(&owner_tron, b"TestToken", "100001");
    legacy_asset.public_free_asset_net_usage = 111;
    legacy_asset.public_latest_free_net_time = 222;

    let mut v2_asset = default_asset_issue(&owner_tron, b"TestToken", "100001");
    v2_asset.public_free_asset_net_usage = 333;
    v2_asset.public_latest_free_net_time = 444;

    seed_asset_issue(&mut storage_adapter, b"TestToken", &legacy_asset, false);
    seed_asset_issue(&mut storage_adapter, b"100001", &v2_asset, true);
    seed_account_with_asset_issued(
        &mut storage_adapter,
        &owner,
        1_000_000,
        b"TestToken",
        b"100001",
    );

    let metadata = make_metadata_with_contract(
        &owner_tron,
        b"updated desc",
        b"https://updated.url",
        700,
        800,
    );
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());
    assert_eq!(result.as_ref().unwrap().state_changes.len(), 0);

    // Verify legacy store: updated fields + preserved per-store fields
    let updated_legacy = storage_adapter
        .get_asset_issue(b"TestToken", 0)
        .unwrap()
        .expect("Legacy asset should exist");
    assert_eq!(updated_legacy.free_asset_net_limit, 700);
    assert_eq!(updated_legacy.public_free_asset_net_limit, 800);
    assert_eq!(updated_legacy.url, b"https://updated.url");
    assert_eq!(updated_legacy.description, b"updated desc");
    // Per-store fields preserved from legacy entry (NOT copied from V2)
    assert_eq!(updated_legacy.public_free_asset_net_usage, 111);
    assert_eq!(updated_legacy.public_latest_free_net_time, 222);

    // Verify V2 store: updated fields + preserved per-store fields
    let updated_v2 = storage_adapter
        .get_asset_issue(b"100001", 1)
        .unwrap()
        .expect("V2 asset should exist");
    assert_eq!(updated_v2.free_asset_net_limit, 700);
    assert_eq!(updated_v2.public_free_asset_net_limit, 800);
    assert_eq!(updated_v2.url, b"https://updated.url");
    assert_eq!(updated_v2.description, b"updated desc");
    // Per-store fields preserved from V2 entry (NOT copied from legacy)
    assert_eq!(updated_v2.public_free_asset_net_usage, 333);
    assert_eq!(updated_v2.public_latest_free_net_time, 444);
}

// =============================================================================
// 12. Empty description is allowed (Java: validAssetDescription allows empty)
// =============================================================================

#[test]
fn test_update_asset_empty_description_is_valid() {
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_full_v2_setup(&mut storage_adapter, &owner, &owner_tron);

    // Empty description (allowed by Java's validAssetDescription)
    let metadata = make_metadata_with_contract(&owner_tron, b"", b"https://url.com", 100, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(
        result.is_ok(),
        "Empty description should be valid, got: {:?}",
        result.err()
    );
}

// =============================================================================
// 13. URL exactly at limit (256 bytes) is valid
// =============================================================================

#[test]
fn test_update_asset_url_exactly_256_bytes_is_valid() {
    let (mut storage_adapter, _temp_dir) = setup_v2_adapter();
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_full_v2_setup(&mut storage_adapter, &owner, &owner_tron);

    // URL exactly 256 bytes (boundary)
    let url_256 = vec![b'a'; 256];
    let metadata = make_metadata_with_contract(&owner_tron, b"desc", &url_256, 100, 200);
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(
        result.is_ok(),
        "URL of exactly 256 bytes should be valid, got: {:?}",
        result.err()
    );
}

// =============================================================================
// 14. Limit boundary: new_limit = 0 is valid, new_limit = ONE_DAY_NET_LIMIT - 1 is valid
// =============================================================================

#[test]
fn test_update_asset_limit_boundary_zero_and_max_minus_one() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    let one_day_limit: i64 = 57_600_000_000;
    storage_engine
        .put(
            "properties",
            b"ONE_DAY_NET_LIMIT",
            &one_day_limit.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_update_asset_enabled();

    let owner = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_full_v2_setup(&mut storage_adapter, &owner, &owner_tron);

    // new_limit = 0, new_public_limit = ONE_DAY_NET_LIMIT - 1 → both valid
    let metadata = make_metadata_with_contract(
        &owner_tron,
        b"desc",
        b"https://url.com",
        0,
        one_day_limit - 1,
    );
    let transaction = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata,
    };

    let result = service.execute_update_asset_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(
        result.is_ok(),
        "Boundary limits should be valid, got: {:?}",
        result.err()
    );
}
