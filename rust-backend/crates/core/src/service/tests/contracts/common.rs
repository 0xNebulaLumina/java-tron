//! Common helpers for contract tests.

use super::super::super::*;
use revm_primitives::{Address, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{TronExecutionContext, TxMetadata};
use tron_backend_storage::StorageEngine;

/// Helper function for tests to encode varint
pub fn encode_varint(buf: &mut Vec<u8>, mut value: u64) {
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

/// Seed required dynamic properties for tests.
/// Many system contracts check for ALLOW_MULTI_SIGN.
pub fn seed_dynamic_properties(storage_engine: &StorageEngine) {
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
}

/// Create a TRON-format from_raw (21 bytes: 0x41 prefix + 20-byte address)
pub fn make_from_raw(addr: &Address) -> Vec<u8> {
    let mut raw = vec![0x41u8];
    raw.extend_from_slice(addr.as_slice());
    raw
}

/// Helper to create a test service with system contracts enabled
pub fn new_test_service_with_system_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Encode a WitnessCreateContract protobuf: { bytes owner_address = 1; bytes url = 2; }
pub fn encode_witness_create_contract(owner_tron: &[u8], url: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    if !owner_tron.is_empty() {
        buf.push(0x0a); // field 1, wire type 2
        encode_varint(&mut buf, owner_tron.len() as u64);
        buf.extend_from_slice(owner_tron);
    }
    if !url.is_empty() {
        buf.push(0x12); // field 2, wire type 2
        encode_varint(&mut buf, url.len() as u64);
        buf.extend_from_slice(url);
    }
    buf
}

/// Encode a WitnessUpdateContract protobuf: { bytes owner_address = 1; bytes update_url = 12; }
pub fn encode_witness_update_contract(owner_tron: &[u8], update_url: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    if !owner_tron.is_empty() {
        buf.push(0x0a); // field 1, wire type 2
        encode_varint(&mut buf, owner_tron.len() as u64);
        buf.extend_from_slice(owner_tron);
    }
    if !update_url.is_empty() {
        // field 12, wire type 2 → tag = (12 << 3) | 2 = 98 = 0x62
        buf.push(0x62);
        encode_varint(&mut buf, update_url.len() as u64);
        buf.extend_from_slice(update_url);
    }
    buf
}

/// Create a default test context
pub fn new_test_context() -> TronExecutionContext {
    TronExecutionContext {
        block_number: 1,
        block_timestamp: 1,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    }
}
