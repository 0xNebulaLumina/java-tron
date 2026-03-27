//! WitnessCreateContract tests — permission parity with Java's setDefaultWitnessPermission.

use super::super::super::*;
use super::common::{encode_witness_create_contract, make_from_raw, seed_dynamic_properties};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::protocol::permission::PermissionType;
use tron_backend_execution::{
    EngineBackedEvmStateStore, TronExecutionContext, TronContractParameter, TronTransaction, TxMetadata,
};

fn create_service(witness_create_enabled: bool) -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            witness_create_enabled,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

fn create_context() -> TronExecutionContext {
    TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    }
}

fn create_witness_create_tx(owner_address: Address, url: &str) -> TronTransaction {
    let owner_tron = make_from_raw(&owner_address);
    let contract_proto = encode_witness_create_contract(&owner_tron, url.as_bytes());
    TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(url.as_bytes().to_vec()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter {
                type_url: "protocol.WitnessCreateContract".to_string(),
                value: contract_proto,
            }),
            ..Default::default()
        },
    }
}

/// Test Case A: ALLOW_MULTI_SIGN=1 → witness permissions are set
#[test]
fn test_witness_create_sets_default_permissions_when_multi_sign_enabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    // Seed ALLOW_MULTI_SIGN = 1
    storage_engine
        .put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ALLOW_BLACKHOLE_OPTIMIZATION",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    // Seed ACCOUNT_UPGRADE_COST (default 9999000000 SUN = 9999 TRX)
    storage_engine
        .put(
            "properties",
            b"ACCOUNT_UPGRADE_COST",
            &9999000000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = create_service(true);

    let owner_address = Address::from([1u8; 20]);
    let owner_from_raw = make_from_raw(&owner_address);
    let owner_account = AccountInfo {
        balance: U256::from(10_000_000_000u64), // 10000 TRX, more than upgrade cost
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    let transaction = create_witness_create_tx(owner_address, "https://witness.example.com");
    let context = create_context();

    let result =
        service.execute_witness_create_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "WitnessCreate should succeed: {:?}",
        result.err()
    );
    let execution_result = result.unwrap();
    assert!(execution_result.success);

    // Verify account proto has permissions set
    let account_proto = storage_adapter
        .get_account_proto(&owner_address)
        .unwrap()
        .unwrap();
    assert!(
        account_proto.is_witness,
        "Account should be marked as witness"
    );

    // Verify witness_permission (id=1, type=Witness)
    let wp = account_proto
        .witness_permission
        .expect("witness_permission should be set");
    assert_eq!(wp.r#type, PermissionType::Witness as i32);
    assert_eq!(wp.id, 1);
    assert_eq!(wp.permission_name, "witness");
    assert_eq!(wp.threshold, 1);
    assert_eq!(wp.parent_id, 0);
    assert!(
        wp.operations.is_empty(),
        "witness permission operations should be empty"
    );
    assert_eq!(wp.keys.len(), 1);
    assert_eq!(wp.keys[0].address, owner_from_raw);
    assert_eq!(wp.keys[0].weight, 1);

    // Verify owner_permission (id=0, type=Owner) — set because account had none
    let op = account_proto
        .owner_permission
        .expect("owner_permission should be set");
    assert_eq!(op.r#type, PermissionType::Owner as i32);
    assert_eq!(op.id, 0);
    assert_eq!(op.permission_name, "owner");
    assert_eq!(op.threshold, 1);
    assert_eq!(op.parent_id, 0);
    assert_eq!(op.keys.len(), 1);
    assert_eq!(op.keys[0].address, owner_from_raw);
    assert_eq!(op.keys[0].weight, 1);

    // Verify active_permission (id=2, type=Active) — set because account had none
    assert_eq!(
        account_proto.active_permission.len(),
        1,
        "Should have exactly 1 active permission"
    );
    let ap = &account_proto.active_permission[0];
    assert_eq!(ap.r#type, PermissionType::Active as i32);
    assert_eq!(ap.id, 2);
    assert_eq!(ap.permission_name, "active");
    assert_eq!(ap.threshold, 1);
    assert_eq!(ap.parent_id, 0);
    assert_eq!(ap.keys.len(), 1);
    assert_eq!(ap.keys[0].address, owner_from_raw);
    assert_eq!(ap.keys[0].weight, 1);
    // ACTIVE_DEFAULT_OPERATIONS should be 32 bytes
    assert_eq!(
        ap.operations.len(),
        32,
        "Active operations should be 32 bytes"
    );
}

/// Test Case B: ALLOW_MULTI_SIGN=0 → no permissions set
#[test]
fn test_witness_create_no_permissions_when_multi_sign_disabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    // Seed ALLOW_MULTI_SIGN = 0 (disabled)
    storage_engine
        .put("properties", b"ALLOW_MULTI_SIGN", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ALLOW_BLACKHOLE_OPTIMIZATION",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ACCOUNT_UPGRADE_COST",
            &9999000000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = create_service(true);

    let owner_address = Address::from([2u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(10_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    let transaction = create_witness_create_tx(owner_address, "https://witness-no-multisign.com");
    let context = create_context();

    let result =
        service.execute_witness_create_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "WitnessCreate should succeed: {:?}",
        result.err()
    );

    let account_proto = storage_adapter
        .get_account_proto(&owner_address)
        .unwrap()
        .unwrap();
    assert!(
        account_proto.is_witness,
        "Account should be marked as witness"
    );

    // No permissions should be set when ALLOW_MULTI_SIGN=0
    assert!(
        account_proto.witness_permission.is_none(),
        "witness_permission should NOT be set"
    );
    assert!(
        account_proto.owner_permission.is_none(),
        "owner_permission should NOT be set"
    );
    assert!(
        account_proto.active_permission.is_empty(),
        "active_permission should be empty"
    );
}

/// Test that existing owner_permission is preserved (not overwritten) when multi-sign is enabled
#[test]
fn test_witness_create_preserves_existing_owner_permission() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    storage_engine
        .put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ALLOW_BLACKHOLE_OPTIMIZATION",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ACCOUNT_UPGRADE_COST",
            &9999000000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = create_service(true);

    let owner_address = Address::from([3u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(10_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    // Pre-set an owner_permission with custom threshold=2 on the account proto
    use tron_backend_execution::protocol::{Key, Permission};
    let custom_owner_perm = Permission {
        r#type: PermissionType::Owner as i32,
        id: 0,
        permission_name: "owner".to_string(),
        threshold: 2, // Custom threshold
        parent_id: 0,
        operations: vec![],
        keys: vec![Key {
            address: make_from_raw(&owner_address),
            weight: 1,
        }],
    };
    let mut account_proto = storage_adapter
        .get_account_proto(&owner_address)
        .unwrap()
        .unwrap_or_default();
    account_proto.owner_permission = Some(custom_owner_perm);
    storage_adapter
        .put_account_proto(&owner_address, &account_proto)
        .unwrap();

    let transaction = create_witness_create_tx(owner_address, "https://custom-perm.com");
    let context = create_context();

    let result =
        service.execute_witness_create_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "WitnessCreate should succeed: {:?}",
        result.err()
    );

    let account_proto = storage_adapter
        .get_account_proto(&owner_address)
        .unwrap()
        .unwrap();

    // Owner permission should be preserved (threshold=2 not overwritten)
    let op = account_proto
        .owner_permission
        .expect("owner_permission should exist");
    assert_eq!(
        op.threshold, 2,
        "Existing owner_permission should be preserved"
    );

    // But witness_permission should be newly set
    assert!(
        account_proto.witness_permission.is_some(),
        "witness_permission should be set"
    );

    // And active_permission should also be set (was empty before)
    assert_eq!(
        account_proto.active_permission.len(),
        1,
        "active_permission should be added"
    );
}

/// Test that existing active_permission is preserved (not overwritten) when multi-sign is enabled
#[test]
fn test_witness_create_preserves_existing_active_permission() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    storage_engine
        .put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ALLOW_BLACKHOLE_OPTIMIZATION",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"ACCOUNT_UPGRADE_COST",
            &9999000000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = create_service(true);

    let owner_address = Address::from([4u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(10_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    // Pre-set an active_permission on the account proto
    use tron_backend_execution::protocol::{Key, Permission};
    let custom_active_perm = Permission {
        r#type: PermissionType::Active as i32,
        id: 2,
        permission_name: "custom_active".to_string(),
        threshold: 3, // Custom threshold
        parent_id: 0,
        operations: vec![0xFF; 32],
        keys: vec![Key {
            address: make_from_raw(&owner_address),
            weight: 1,
        }],
    };
    let mut account_proto = storage_adapter
        .get_account_proto(&owner_address)
        .unwrap()
        .unwrap_or_default();
    account_proto.active_permission.push(custom_active_perm);
    storage_adapter
        .put_account_proto(&owner_address, &account_proto)
        .unwrap();

    let transaction = create_witness_create_tx(owner_address, "https://custom-active.com");
    let context = create_context();

    let result =
        service.execute_witness_create_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "WitnessCreate should succeed: {:?}",
        result.err()
    );

    let account_proto = storage_adapter
        .get_account_proto(&owner_address)
        .unwrap()
        .unwrap();

    // Active permission should be preserved (not replaced)
    assert_eq!(
        account_proto.active_permission.len(),
        1,
        "Should still have 1 active permission"
    );
    assert_eq!(
        account_proto.active_permission[0].permission_name, "custom_active",
        "Existing active_permission should be preserved"
    );
    assert_eq!(
        account_proto.active_permission[0].threshold, 3,
        "Custom threshold should be preserved"
    );

    // But witness_permission should be newly set
    assert!(
        account_proto.witness_permission.is_some(),
        "witness_permission should be set"
    );
}

/// Regression: when transaction.data contains a DIFFERENT URL than contract_parameter.value,
/// the handler must use contract_parameter.value (the protobuf source of truth), NOT tx.data.
#[test]
fn test_witness_create_uses_contract_parameter_not_tx_data() {
    use tron_backend_storage::StorageEngine;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x33u8; 20]);
    let owner_tron = make_from_raw(&owner_address);
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

    let service = create_service(true);
    let context = create_context();

    // contract_parameter.value encodes "proto-url.com" as the URL
    let proto_url = b"proto-url.com";
    let contract_proto = encode_witness_create_contract(&owner_tron, proto_url);

    // transaction.data contains a DIFFERENT URL — handler must ignore this
    let tx = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(b"WRONG-tx-data-url.com".to_vec()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter {
                type_url: "protocol.WitnessCreateContract".to_string(),
                value: contract_proto,
            }),
            ..Default::default()
        },
    };

    let result = service.execute_witness_create_contract(&mut storage_adapter, &tx, &context);
    assert!(result.is_ok(), "WitnessCreate should succeed: {:?}", result.err());

    // Verify the witness was stored with the proto URL, not the tx.data URL
    let witness = storage_adapter.get_witness(&owner_address).unwrap();
    assert!(witness.is_some(), "Witness should exist");
    let witness = witness.unwrap();
    assert_eq!(
        witness.url, "proto-url.com",
        "Witness URL should come from contract_parameter.value, not transaction.data"
    );
}

/// Regression: malformed contract_parameter.value should fail before account checks.
#[test]
fn test_witness_create_rejects_malformed_contract_parameter() {
    use tron_backend_storage::StorageEngine;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x34u8; 20]);
    // No account seeded — malformed protobuf should fail before account check

    let service = create_service(true);
    let context = create_context();

    // Malformed protobuf: claims 200-byte field but only has 2 bytes
    let malformed = vec![0x12, 0xC8, 0x01, 0x41, 0x42];

    let tx = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(b"valid-url.com".to_vec()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::WitnessCreateContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter {
                type_url: "protocol.WitnessCreateContract".to_string(),
                value: malformed,
            }),
            ..Default::default()
        },
    };

    let result = service.execute_witness_create_contract(&mut storage_adapter, &tx, &context);
    assert!(result.is_err(), "Malformed protobuf should fail");
    let err = result.err().unwrap();
    assert!(
        err.contains("parsing") || err.contains("truncated"),
        "Error should indicate protobuf decode failure, got: {}",
        err,
    );
}
