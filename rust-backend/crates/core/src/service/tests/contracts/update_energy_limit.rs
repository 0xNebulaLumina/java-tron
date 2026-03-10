//! UpdateEnergyLimitContract tests (type 45).
//!
//! Tests for Java parity validation against UpdateEnergyLimitContractActuator.java:
//! - Fork gate: checkForEnergyLimit (block_num >= configurable threshold)
//! - Any.is(UpdateEnergyLimitContract.class) type_url check
//! - owner_address parsed from contract bytes
//! - owner_address validation (length == 21, correct prefix)
//! - account existence
//! - origin_energy_limit > 0
//! - contract existence (empty contract_address → "Contract does not exist")
//! - ownership check (origin_address must match)

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, new_test_context, seed_dynamic_properties};
use revm_primitives::{Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TxMetadata};
use tron_backend_storage::StorageEngine;

fn new_test_service_with_update_energy_limit_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            update_energy_limit_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Build UpdateEnergyLimitContract protobuf bytes.
/// UpdateEnergyLimitContract:
///   bytes owner_address = 1;
///   bytes contract_address = 2;
///   int64 origin_energy_limit = 3;
fn build_update_energy_limit_data(
    owner_address: &[u8],
    contract_address: &[u8],
    origin_energy_limit: i64,
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
    // field 3: origin_energy_limit (varint)
    if origin_energy_limit != 0 {
        encode_varint(&mut data, (3 << 3) | 0);
        encode_varint(&mut data, origin_energy_limit as u64);
    }
    data
}

/// Seed LATEST_BLOCK_HEADER_NUMBER in dynamic properties.
fn seed_block_number(storage_engine: &StorageEngine, block_num: i64) {
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_NUMBER",
            &block_num.to_be_bytes(),
        )
        .unwrap();
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
                tron_backend_execution::TronContractType::UpdateEnergyLimitContract,
            ),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.UpdateEnergyLimitContract".to_string(),
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

#[test]
fn test_parse_empty_contract_address_falls_through() {
    // Empty contract_address should NOT error in parsing; it should be empty vec
    // and fail later at "Contract does not exist".
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let data = build_update_energy_limit_data(&owner, &[], 100);

    let service = new_test_service_with_update_energy_limit_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_block_number(&storage_engine, 100);
    seed_account(&storage_engine, &owner, 100_000_000);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);
    adapter.set_block_num_for_energy_limit(0);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Contract does not exist");
}

#[test]
fn test_fork_gate_blocks_when_threshold_high() {
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_energy_limit_data(&owner, &contract_addr, 100);

    let service = new_test_service_with_update_energy_limit_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_block_number(&storage_engine, 10); // block 10

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);
    adapter.set_block_num_for_energy_limit(100); // threshold 100 > block 10

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "contract type error, unexpected type [UpdateEnergyLimitContract]"
    );
}

#[test]
fn test_invalid_owner_address_empty() {
    // Empty owner_address → "Invalid address"
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    let data = build_update_energy_limit_data(&[], &contract_addr, 100);

    let service = new_test_service_with_update_energy_limit_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_block_number(&storage_engine, 100);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);
    adapter.set_block_num_for_energy_limit(0);

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
                tron_backend_execution::TronContractType::UpdateEnergyLimitContract,
            ),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.UpdateEnergyLimitContract".to_string(),
                value: data,
            }),
            from_raw: Some(vec![]), // empty from_raw
            ..Default::default()
        },
    };

    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid address");
}

#[test]
fn test_origin_energy_limit_must_be_positive() {
    let owner = make_from_raw(&Address::from([0xabu8; 20]));
    let contract_addr = make_from_raw(&Address::from([0x11u8; 20]));
    // Use 0 energy limit (field not emitted for 0 in protobuf, so default=0)
    let data = build_update_energy_limit_data(&owner, &contract_addr, 0);

    let service = new_test_service_with_update_energy_limit_enabled();
    let tmp = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(tmp.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_block_number(&storage_engine, 100);
    seed_account(&storage_engine, &owner, 100_000_000);

    let (mut adapter, _) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);
    adapter.set_block_num_for_energy_limit(0);

    let owner_addr = Address::from_slice(&owner[1..]);
    let tx = make_transaction(owner_addr, owner.clone(), data);
    let ctx = new_test_context();
    let result = service.execute_non_vm_contract(&mut adapter, &tx, &ctx);

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "origin energy limit must be > 0");
}
