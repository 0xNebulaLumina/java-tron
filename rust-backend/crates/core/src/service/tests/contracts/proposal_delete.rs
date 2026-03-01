//! ProposalDeleteContract tests.
//!
//! These tests verify Java parity for PROPOSAL_DELETE_CONTRACT execution.
//! Key parity points tested:
//! 1. Proto3 defaulting: missing proposal_id defaults to 0 (not a parse error)
//! 2. Surgical state patching preserves raw proposal bytes (parameter map order)
//! 3. Correct error strings for validation failures

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, seed_dynamic_properties};
use revm_primitives::{Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{
    protocol::Proposal, EngineBackedEvmStateStore, TronContractParameter, TronContractType,
    TronExecutionContext, TronTransaction, TxMetadata,
};
use std::collections::BTreeMap;

/// Helper to build a ProposalDeleteContract protobuf
fn build_proposal_delete_contract(
    owner_address: &[u8],
    proposal_id: i64,
) -> Vec<u8> {
    let mut buf = Vec::new();

    // Field 1: owner_address (bytes, wire type 2)
    encode_varint(&mut buf, (1 << 3) | 2);
    encode_varint(&mut buf, owner_address.len() as u64);
    buf.extend_from_slice(owner_address);

    // Field 2: proposal_id (int64, wire type 0)
    if proposal_id != 0 {
        encode_varint(&mut buf, (2 << 3) | 0);
        encode_varint(&mut buf, proposal_id as u64);
    }

    buf
}

/// Build a ProposalDeleteContract WITHOUT proposal_id field (proto3 default = 0)
fn build_proposal_delete_contract_no_proposal_id(owner_address: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();

    // Field 1: owner_address (bytes, wire type 2)
    encode_varint(&mut buf, (1 << 3) | 2);
    encode_varint(&mut buf, owner_address.len() as u64);
    buf.extend_from_slice(owner_address);

    // Deliberately omit field 2 (proposal_id)
    buf
}

/// Create a TronContractParameter for ProposalDeleteContract
fn make_contract_parameter(contract_data: Vec<u8>) -> TronContractParameter {
    TronContractParameter {
        type_url: "type.googleapis.com/protocol.ProposalDeleteContract".to_string(),
        value: contract_data,
    }
}

/// Create a test service with proposal_delete enabled
fn new_test_service() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            proposal_delete_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Create a test context with specified block timestamp
fn new_test_context(block_timestamp: u64) -> TronExecutionContext {
    TronExecutionContext {
        block_number: 1,
        block_timestamp,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    }
}

/// Test that missing proposal_id field defaults to 0 (proto3 parity).
///
/// In proto3, absent int64 fields decode as 0. Java's protobuf decoding returns
/// getProposalId() == 0, so validation proceeds and fails with "Proposal[0] not exists".
/// Rust must match this behavior instead of returning "Missing proposal_id".
#[test]
fn test_proposal_delete_missing_proposal_id_defaults_to_zero() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Seed LATEST_PROPOSAL_NUM to 0 (so proposal_id=0 fails the > check: 0 > 0 is false,
    // then the get_proposal check fires)
    storage_engine
        .put("properties", b"LATEST_PROPOSAL_NUM", &0i64.to_be_bytes())
        .unwrap();

    // Seed latest_block_header_timestamp
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service();

    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);

    // Create owner account
    let owner_account = revm_primitives::AccountInfo {
        balance: U256::from(1_000_000_000_000i64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    // Build contract WITHOUT proposal_id field (omitted = proto3 default 0)
    let contract_data = build_proposal_delete_contract_no_proposal_id(&owner_tron);
    let contract_param = make_contract_parameter(contract_data);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ProposalDeleteContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context(1_000_000);
    let result =
        service.execute_proposal_delete_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err(), "Should fail when proposal_id=0 doesn't exist");
    let err = result.unwrap_err();
    assert_eq!(
        err, "Proposal[0] not exists",
        "Error should match Java's proto3 defaulting behavior: {}",
        err
    );
}

/// Test that parse_proposal_delete_contract returns 0 for explicitly set proposal_id=0.
///
/// When proposal_id is explicitly set to 0 in the protobuf, proto3 omits the field
/// on the wire (since 0 is the default). The parser should still return 0.
#[test]
fn test_proposal_delete_explicit_proposal_id_zero() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b"LATEST_PROPOSAL_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service();

    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);

    let owner_account = revm_primitives::AccountInfo {
        balance: U256::from(1_000_000_000_000i64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    // Build contract with explicit proposal_id=0 (build_proposal_delete_contract
    // skips field 2 when proposal_id==0, simulating proto3's wire format)
    let contract_data = build_proposal_delete_contract(&owner_tron, 0);
    let contract_param = make_contract_parameter(contract_data);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ProposalDeleteContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context(1_000_000);
    let result =
        service.execute_proposal_delete_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err(), "Should fail when proposal 0 doesn't exist");
    let err = result.unwrap_err();
    assert_eq!(
        err, "Proposal[0] not exists",
        "Error should be 'Proposal[0] not exists': {}",
        err
    );
}

/// Test that surgical state patching preserves original parameter map order.
///
/// When a proposal has parameters in non-sorted order (e.g., keys [16, 0, 1]),
/// the ProposalDelete operation should preserve that order in the persisted bytes
/// because it uses surgical patching (only modifying the state field) rather than
/// re-encoding through BTreeMap which would sort by key.
#[test]
fn test_proposal_delete_preserves_parameter_order() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b"LATEST_PROPOSAL_NUM", &5i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);

    // Manually encode a proposal with parameters in non-sorted order: [16, 0, 1]
    // This simulates what Java would produce when parameters are inserted in this order.
    let pre_delete_bytes = build_proposal_raw_nonsorted_params(
        1, // proposal_id
        &owner_tron,
        &[(16, 1), (0, 1000000), (1, 3)], // Non-sorted parameter order
        2_000_000, // expiration_time (future)
        500_000,   // create_time
        0,         // state = PENDING
    );

    // Store the proposal with non-sorted parameter order directly in storage engine
    let proposal_key = 1i64.to_be_bytes().to_vec();
    storage_engine
        .put("proposal", &proposal_key, &pre_delete_bytes)
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service();

    // Create owner account
    let owner_account = revm_primitives::AccountInfo {
        balance: U256::from(1_000_000_000_000i64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    // Now execute ProposalDelete
    let contract_data = build_proposal_delete_contract(&owner_tron, 1);
    let contract_param = make_contract_parameter(contract_data);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ProposalDeleteContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context(1_000_000);
    let result =
        service.execute_proposal_delete_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_ok(), "ProposalDelete should succeed: {:?}", result.err());

    // Read back the raw proposal bytes via get_proposal_with_raw
    let (_, post_delete_bytes) = storage_adapter
        .get_proposal_with_raw(1)
        .unwrap()
        .expect("Proposal should exist after delete");

    // Build expected bytes: same as pre-delete but with state=3 (CANCELED) appended
    let expected_bytes = build_proposal_raw_nonsorted_params(
        1,
        &owner_tron,
        &[(16, 1), (0, 1000000), (1, 3)], // Same non-sorted order preserved
        2_000_000,
        500_000,
        3, // state = CANCELED
    );

    assert_eq!(
        hex::encode(&post_delete_bytes),
        hex::encode(&expected_bytes),
        "Post-delete bytes should preserve original parameter order.\n\
         Expected: {}\n\
         Actual:   {}",
        hex::encode(&expected_bytes),
        hex::encode(&post_delete_bytes),
    );
}

/// Test that surgical_update_proposal_state correctly patches state field.
#[test]
fn test_surgical_update_proposal_state_replaces_existing() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    let storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Build a raw proposal with state=2 (APPROVED)
    let owner = vec![0x41u8; 21];
    let raw = build_proposal_raw_nonsorted_params(
        1,
        &owner,
        &[(16, 1), (0, 1000000)],
        2_000_000,
        500_000,
        2, // state = APPROVED
    );

    // Patch to CANCELED (3)
    let patched = storage_adapter
        .surgical_update_proposal_state(&raw, 3)
        .unwrap();

    let expected = build_proposal_raw_nonsorted_params(
        1,
        &owner,
        &[(16, 1), (0, 1000000)],
        2_000_000,
        500_000,
        3, // state = CANCELED
    );

    assert_eq!(
        hex::encode(&patched),
        hex::encode(&expected),
        "Surgical state patch should replace existing state field"
    );
}

/// Test that surgical_update_proposal_state inserts state when absent.
#[test]
fn test_surgical_update_proposal_state_inserts_when_absent() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    let storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Build a raw proposal with state=0 (PENDING, which means field 7 is absent)
    let owner = vec![0x41u8; 21];
    let raw = build_proposal_raw_nonsorted_params(
        1,
        &owner,
        &[(16, 1)],
        2_000_000,
        500_000,
        0, // state = PENDING (field 7 omitted in proto3)
    );

    // Patch to CANCELED (3) - should insert field 7
    let patched = storage_adapter
        .surgical_update_proposal_state(&raw, 3)
        .unwrap();

    let expected = build_proposal_raw_nonsorted_params(
        1,
        &owner,
        &[(16, 1)],
        2_000_000,
        500_000,
        3, // state = CANCELED (field 7 now present)
    );

    assert_eq!(
        hex::encode(&patched),
        hex::encode(&expected),
        "Surgical state patch should insert state field when absent"
    );
}

// =====================================================================
// Helper: manually encode a Proposal protobuf with specified field order
// =====================================================================

/// Build raw Proposal protobuf bytes with explicit parameter ordering.
/// This bypasses BTreeMap to control the exact map entry order.
fn build_proposal_raw_nonsorted_params(
    proposal_id: i64,
    proposer_address: &[u8],
    parameters: &[(i64, i64)], // (key, value) pairs in desired order
    expiration_time: i64,
    create_time: i64,
    state: i32,
) -> Vec<u8> {
    let mut out = Vec::new();

    // Field 1: proposal_id (int64, varint)
    if proposal_id != 0 {
        encode_varint(&mut out, (1 << 3) | 0);
        encode_varint(&mut out, proposal_id as u64);
    }

    // Field 2: proposer_address (bytes)
    if !proposer_address.is_empty() {
        encode_varint(&mut out, (2 << 3) | 2);
        encode_varint(&mut out, proposer_address.len() as u64);
        out.extend_from_slice(proposer_address);
    }

    // Field 3: parameters (map<int64,int64>) - entries in the specified order
    for &(key, value) in parameters {
        let mut entry_buf = Vec::new();
        // Map entry field 1: key (int64, varint) - ALWAYS encoded (even if 0)
        encode_varint(&mut entry_buf, (1 << 3) | 0);
        encode_varint(&mut entry_buf, key as u64);
        // Map entry field 2: value (int64, varint)
        encode_varint(&mut entry_buf, (2 << 3) | 0);
        encode_varint(&mut entry_buf, value as u64);

        encode_varint(&mut out, (3 << 3) | 2);
        encode_varint(&mut out, entry_buf.len() as u64);
        out.extend_from_slice(&entry_buf);
    }

    // Field 4: expiration_time (int64, varint)
    if expiration_time != 0 {
        encode_varint(&mut out, (4 << 3) | 0);
        encode_varint(&mut out, expiration_time as u64);
    }

    // Field 5: create_time (int64, varint)
    if create_time != 0 {
        encode_varint(&mut out, (5 << 3) | 0);
        encode_varint(&mut out, create_time as u64);
    }

    // Field 6: approvals (repeated bytes) - none in these tests

    // Field 7: state (enum, varint) - proto3 omits default 0
    if state != 0 {
        encode_varint(&mut out, (7 << 3) | 0);
        encode_varint(&mut out, state as u64);
    }

    out
}
