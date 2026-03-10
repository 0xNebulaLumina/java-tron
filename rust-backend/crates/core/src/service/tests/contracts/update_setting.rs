//! UpdateSettingContract tests (type 33).
//!
//! Tests for Java parity validation against UpdateSettingContractActuator.java:
//! - Any.is(UpdateSettingContract.class) type_url check
//! - owner_address parsed from contract bytes
//! - owner_address validation (length == 21, correct prefix)
//! - account existence
//! - consume_user_resource_percent in [0, 100]
//! - contract existence (empty contract_address → "Contract does not exist")
//! - ownership check (origin_address must match)
//! - happy-path: updates consume_user_resource_percent in ContractStore

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, new_test_context, seed_dynamic_properties};
use prost::Message;
use revm_primitives::{Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TxMetadata};
use tron_backend_storage::StorageEngine;

fn new_test_service_with_update_setting_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            update_setting_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Build UpdateSettingContract protobuf bytes.
/// UpdateSettingContract:
///   bytes owner_address = 1;
///   bytes contract_address = 2;
///   int64 consume_user_resource_percent = 3;
fn build_update_setting_data(
    owner_address: &[u8],
    contract_address: &[u8],
    consume_user_resource_percent: i64,
) -> Vec<u8> {
    let mut data = Vec::new();
    // field 1: owner_address (bytes)
    if !owner_address.is_empty() {
        encode_varint(&mut data, (1 << 3) | 2);
        encode_varint(&mut data, owner_address.len() as u64);
        data.extend_from_slice(owner_address);
    }
    // field 2: contract_address (bytes)
    if !contract_address.is_empty() {
        encode_varint(&mut data, (2 << 3) | 2);
        encode_varint(&mut data, contract_address.len() as u64);
        data.extend_from_slice(contract_address);
    }
    // field 3: consume_user_resource_percent (varint)
    if consume_user_resource_percent != 0 {
        encode_varint(&mut data, (3 << 3) | 0);
        encode_varint(&mut data, consume_user_resource_percent as u64);
    }
    data
}

fn make_transaction(
    owner: Address,
    owner_tron: Vec<u8>,
    contract_bytes: Vec<u8>,
) -> TronTransaction {
    TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_bytes.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UpdateSettingContract,
            ),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.UpdateSettingContract".to_string(),
                value: contract_bytes,
            }),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
    }
}

/// Seed a minimal account.
fn seed_account(storage_engine: &StorageEngine, address_21: &[u8], balance: u64) {
    let mut buf = Vec::new();
    // field 3: address (bytes)
    encode_varint(&mut buf, (3 << 3) | 2);
    encode_varint(&mut buf, address_21.len() as u64);
    buf.extend_from_slice(address_21);
    // field 4: balance (varint)
    encode_varint(&mut buf, (4 << 3) | 0);
    encode_varint(&mut buf, balance);
    storage_engine.put("account", address_21, &buf).unwrap();
}

/// Seed a SmartContract in the contract store using prost encoding.
fn seed_smart_contract(
    storage_engine: &StorageEngine,
    origin_address: &[u8],
    contract_address: &[u8],
    consume_user_resource_percent: i64,
) {
    let contract = tron_backend_execution::protocol::SmartContract {
        origin_address: origin_address.to_vec(),
        contract_address: contract_address.to_vec(),
        consume_user_resource_percent,
        origin_energy_limit: 0,
        ..Default::default()
    };
    let mut buf = Vec::new();
    contract.encode(&mut buf).unwrap();
    storage_engine
        .put("contract", contract_address, &buf)
        .unwrap();
}

// ---------------------------------------------------------------------------
// Validation tests
// ---------------------------------------------------------------------------

#[test]
fn test_type_url_mismatch() {
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_setting_data(&owner, &contract_addr, 50);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::from_slice(&owner[1..]);
    // Use wrong type_url
    let tx = TronTransaction {
        from: owner_addr,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(data.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UpdateSettingContract,
            ),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.TransferContract".to_string(),
                value: data,
            }),
            from_raw: Some(owner.clone()),
            ..Default::default()
        },
    };

    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "contract type error, expected type [UpdateSettingContract], real type[class com.google.protobuf.Any]"
    );
}

#[test]
fn test_invalid_owner_address_empty() {
    // Empty owner_address → "Invalid address"
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_setting_data(&[], &contract_addr, 50);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let tx = TronTransaction {
        from: Address::ZERO,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(data.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UpdateSettingContract,
            ),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.UpdateSettingContract".to_string(),
                value: data,
            }),
            from_raw: Some(vec![]),
            ..Default::default()
        },
    };

    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid address");
}

#[test]
fn test_invalid_owner_address_wrong_length() {
    // Owner with wrong length (10 bytes instead of 21) → "Invalid address"
    let short_owner = vec![0x41, 0xab, 0xcd, 0xef, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_setting_data(&short_owner, &contract_addr, 50);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::ZERO;
    let tx = make_transaction(owner_addr, short_owner, data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid address");
}

#[test]
fn test_owner_account_not_exist() {
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_setting_data(&owner, &contract_addr, 50);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // Do NOT seed the owner account

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        format!("Account[{}] does not exist", hex::encode(&owner))
    );
}

#[test]
fn test_percent_over_100() {
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_setting_data(&owner, &contract_addr, 101);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_account(&storage_engine, &owner, 100_000_000);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "percent not in [0, 100]");
}

#[test]
fn test_negative_percent() {
    // Protobuf encodes negative i64 as a large varint (zigzag-equivalent).
    // The Rust parser reads raw varint and casts to i64, so -1 encodes as
    // 0xFFFFFFFF_FFFFFFFF which when cast to i64 is -1 → "percent not in [0, 100]".
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_setting_data(&owner, &contract_addr, -1);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_account(&storage_engine, &owner, 100_000_000);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "percent not in [0, 100]");
}

#[test]
fn test_contract_not_exist() {
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_setting_data(&owner, &contract_addr, 50);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_account(&storage_engine, &owner, 100_000_000);
    // Do NOT seed the smart contract

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Contract does not exist");
}

#[test]
fn test_empty_contract_address_falls_through() {
    // Empty contract_address → "Contract does not exist"
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let data = build_update_setting_data(&owner, &[], 50);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_account(&storage_engine, &owner, 100_000_000);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Contract does not exist");
}

#[test]
fn test_not_owner_of_contract() {
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let real_origin = make_from_raw(&Address::from([0xcd; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_setting_data(&owner, &contract_addr, 50);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_account(&storage_engine, &owner, 100_000_000);
    // Seed contract with a different origin_address
    seed_smart_contract(&storage_engine, &real_origin, &contract_addr, 25);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        format!(
            "Account[{}] is not the owner of the contract",
            hex::encode(&owner)
        )
    );
}

// ---------------------------------------------------------------------------
// Happy-path tests
// ---------------------------------------------------------------------------

#[test]
fn test_happy_path_update_percent() {
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_setting_data(&owner, &contract_addr, 75);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_account(&storage_engine, &owner, 100_000_000);
    seed_smart_contract(&storage_engine, &owner, &contract_addr, 25);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());

    let exec_result = result.unwrap();
    assert!(exec_result.success);
    assert_eq!(exec_result.energy_used, 0);
    assert!(exec_result.state_changes.is_empty());

    // Commit the write buffer to storage so we can verify the update
    adapter.commit_buffer().expect("commit should succeed");

    // Verify the contract was updated in storage
    let updated = adapter
        .get_smart_contract(&contract_addr)
        .expect("should read contract")
        .expect("contract should exist");
    assert_eq!(updated.consume_user_resource_percent, 75);
}

#[test]
fn test_happy_path_update_to_zero() {
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    // percent=0 means the field won't be encoded by build_update_setting_data (proto default)
    let data = build_update_setting_data(&owner, &contract_addr, 0);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_account(&storage_engine, &owner, 100_000_000);
    seed_smart_contract(&storage_engine, &owner, &contract_addr, 50);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());

    adapter.commit_buffer().expect("commit should succeed");

    let updated = adapter
        .get_smart_contract(&contract_addr)
        .expect("should read contract")
        .expect("contract should exist");
    assert_eq!(updated.consume_user_resource_percent, 0);
}

#[test]
fn test_happy_path_update_to_100() {
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_setting_data(&owner, &contract_addr, 100);

    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_account(&storage_engine, &owner, 100_000_000);
    seed_smart_contract(&storage_engine, &owner, &contract_addr, 0);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());

    adapter.commit_buffer().expect("commit should succeed");

    let updated = adapter
        .get_smart_contract(&contract_addr)
        .expect("should read contract")
        .expect("contract should exist");
    assert_eq!(updated.consume_user_resource_percent, 100);
}

// ---------------------------------------------------------------------------
// Feature gate test
// ---------------------------------------------------------------------------

#[test]
fn test_disabled_config_falls_back() {
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_setting_data(&owner, &contract_addr, 50);

    // Create service with update_setting_enabled = false (default)
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            update_setting_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    let service = BackendService::new(module_manager);

    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("UPDATE_SETTING_CONTRACT execution is disabled"));
}

// ---------------------------------------------------------------------------
// Parser edge-case tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_empty_data() {
    // Empty protobuf bytes → defaults: owner=[], contract=[], percent=0
    let service = new_test_service_with_update_setting_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

    // Use empty data bytes
    let tx = TronTransaction {
        from: Address::ZERO,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::UpdateSettingContract,
            ),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.UpdateSettingContract".to_string(),
                value: vec![],
            }),
            from_raw: Some(vec![]),
            ..Default::default()
        },
    };

    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);
    // Empty owner → "Invalid address"
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid address");
}
