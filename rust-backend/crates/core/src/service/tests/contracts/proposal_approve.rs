//! ProposalApproveContract tests.
//!
//! These tests verify Java parity for PROPOSAL_APPROVE_CONTRACT execution.
//! Key parity point tested: duplicate approval removal removes only FIRST occurrence.

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, seed_dynamic_properties};
use revm_primitives::{Address, Bytes, U256};
use std::collections::BTreeMap;
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{
    protocol::Proposal, EngineBackedEvmStateStore, TronContractType, TronExecutionContext,
    TronTransaction, TxMetadata,
};

/// Helper to build a ProposalApproveContract protobuf
fn build_proposal_approve_contract(
    owner_address: &[u8],
    proposal_id: i64,
    is_add_approval: bool,
) -> Vec<u8> {
    let mut buf = Vec::new();

    // Field 1: owner_address (bytes, wire type 2)
    encode_varint(&mut buf, (1 << 3) | 2);
    encode_varint(&mut buf, owner_address.len() as u64);
    buf.extend_from_slice(owner_address);

    // Field 2: proposal_id (int64, wire type 0)
    encode_varint(&mut buf, (2 << 3) | 0);
    encode_varint(&mut buf, proposal_id as u64);

    // Field 3: is_add_approval (bool, wire type 0)
    encode_varint(&mut buf, (3 << 3) | 0);
    encode_varint(&mut buf, if is_add_approval { 1 } else { 0 });

    buf
}

/// Create a test service with proposal_approve enabled
fn new_test_service() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            proposal_approve_enabled: true,
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

/// Test that removing an approval removes only the FIRST occurrence (Java parity).
///
/// Java's ProposalCapsule.removeApproval():
///   List<ByteString> approvals = Lists.newArrayList();
///   approvals.addAll(getApprovals());
///   approvals.remove(address);  // ArrayList.remove(Object) removes first occurrence only
///   ...
///
/// This test verifies that if a corrupted/non-canonical DB contains duplicate approvals,
/// removing the approval removes only one occurrence, not all.
#[test]
fn test_proposal_approve_remove_first_occurrence_only() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Seed LATEST_PROPOSAL_NUM
    storage_engine
        .put("properties", b"LATEST_PROPOSAL_NUM", &10i64.to_be_bytes())
        .unwrap();

    // Seed latest_block_header_timestamp (must be BEFORE proposal expiration)
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service();

    // Create test addresses
    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address); // 0x41 prefix + 20 bytes

    // Another approval address (also needs to be a witness)
    let other_address = Address::from([2u8; 20]);
    let other_tron = make_from_raw(&other_address);

    // Create owner account
    let owner_account = revm_primitives::AccountInfo {
        balance: U256::from(1_000_000_000_000i64), // 1M TRX
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    // Create owner as witness
    let owner_witness = tron_backend_execution::WitnessInfo::new(
        owner_address,
        "http://witness-1.example.com".to_string(),
        100,
    );
    storage_adapter.put_witness(&owner_witness).unwrap();

    // Create a proposal with DUPLICATE approvals [owner_tron, owner_tron, other_tron]
    // This is a corrupted/edge-case scenario but Java handles it by removing only first match.
    let proposal = Proposal {
        proposal_id: 1,
        proposer_address: other_tron.clone(),
        parameters: BTreeMap::new(),
        expiration_time: 2_000_000, // Future expiration
        create_time: 500_000,
        approvals: vec![owner_tron.clone(), owner_tron.clone(), other_tron.clone()],
        state: 0, // PENDING
    };

    // Store the proposal
    storage_adapter.put_proposal(&proposal).unwrap();

    // Build remove-approval transaction
    let contract_data = build_proposal_approve_contract(&owner_tron, 1, false /* remove */);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ProposalApproveContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = new_test_context(1_000_000); // Within proposal validity window

    // Execute the contract
    let result =
        service.execute_proposal_approve_contract(&mut storage_adapter, &transaction, &context);

    // Assert success
    assert!(
        result.is_ok(),
        "Remove approval should succeed: {:?}",
        result.err()
    );
    let execution_result = result.unwrap();
    assert!(
        execution_result.success,
        "Execution should be successful: {:?}",
        execution_result.error
    );

    // Verify the proposal now has [owner_tron, other_tron] (first occurrence removed, not all)
    let updated_proposal = storage_adapter.get_proposal(1).unwrap().unwrap();
    assert_eq!(
        updated_proposal.approvals.len(),
        2,
        "Should have 2 approvals after removing first occurrence (Java parity)"
    );
    assert_eq!(
        updated_proposal.approvals[0], owner_tron,
        "First approval should still be owner_tron (second occurrence)"
    );
    assert_eq!(
        updated_proposal.approvals[1], other_tron,
        "Second approval should be other_tron"
    );
}

/// Test happy path: adding an approval
#[test]
fn test_proposal_approve_add_approval_happy_path() {
    // Create mock storage and service
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Seed required properties
    storage_engine
        .put("properties", b"LATEST_PROPOSAL_NUM", &10i64.to_be_bytes())
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

    // Create test addresses
    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);
    let proposer_address = Address::from([2u8; 20]);
    let proposer_tron = make_from_raw(&proposer_address);

    // Create owner account and witness
    let owner_account = revm_primitives::AccountInfo {
        balance: U256::from(1_000_000_000_000i64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let owner_witness = tron_backend_execution::WitnessInfo::new(
        owner_address,
        "http://witness-1.example.com".to_string(),
        100,
    );
    storage_adapter.put_witness(&owner_witness).unwrap();

    // Create a proposal with no approvals
    let proposal = Proposal {
        proposal_id: 1,
        proposer_address: proposer_tron.clone(),
        parameters: BTreeMap::new(),
        expiration_time: 2_000_000,
        create_time: 500_000,
        approvals: vec![],
        state: 0, // PENDING
    };
    storage_adapter.put_proposal(&proposal).unwrap();

    // Build add-approval transaction
    let contract_data = build_proposal_approve_contract(&owner_tron, 1, true /* add */);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ProposalApproveContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = new_test_context(1_000_000);

    // Execute
    let result =
        service.execute_proposal_approve_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_ok(), "Add approval should succeed: {:?}", result.err());
    let execution_result = result.unwrap();
    assert!(execution_result.success, "Execution should be successful");

    // Verify approval was added
    let updated_proposal = storage_adapter.get_proposal(1).unwrap().unwrap();
    assert_eq!(updated_proposal.approvals.len(), 1, "Should have 1 approval");
    assert_eq!(updated_proposal.approvals[0], owner_tron, "Approval should be owner");
}

/// Test validation: cannot remove approval that doesn't exist
#[test]
fn test_proposal_approve_remove_not_approved_fails() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Seed properties
    storage_engine
        .put("properties", b"LATEST_PROPOSAL_NUM", &10i64.to_be_bytes())
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
    let proposer_address = Address::from([2u8; 20]);
    let proposer_tron = make_from_raw(&proposer_address);

    // Create owner account and witness
    let owner_account = revm_primitives::AccountInfo {
        balance: U256::from(1_000_000_000_000i64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();
    let owner_witness = tron_backend_execution::WitnessInfo::new(
        owner_address,
        "http://witness.example.com".to_string(),
        100,
    );
    storage_adapter.put_witness(&owner_witness).unwrap();

    // Create proposal with NO approvals from owner
    let proposal = Proposal {
        proposal_id: 1,
        proposer_address: proposer_tron.clone(),
        parameters: BTreeMap::new(),
        expiration_time: 2_000_000,
        create_time: 500_000,
        approvals: vec![], // Owner has NOT approved
        state: 0,
    };
    storage_adapter.put_proposal(&proposal).unwrap();

    // Try to remove approval
    let contract_data = build_proposal_approve_contract(&owner_tron, 1, false);
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ProposalApproveContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = new_test_context(1_000_000);
    let result =
        service.execute_proposal_approve_contract(&mut storage_adapter, &transaction, &context);

    // Should fail with Java-matching error message
    assert!(result.is_err(), "Should fail when witness hasn't approved");
    let err = result.unwrap_err();
    assert!(
        err.contains("has not approved proposal"),
        "Error should mention not approved: {}",
        err
    );
}

/// Test validation: cannot add duplicate approval
#[test]
fn test_proposal_approve_repeat_approval_fails() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b"LATEST_PROPOSAL_NUM", &10i64.to_be_bytes())
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
    let proposer_address = Address::from([2u8; 20]);
    let proposer_tron = make_from_raw(&proposer_address);

    let owner_account = revm_primitives::AccountInfo {
        balance: U256::from(1_000_000_000_000i64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();
    let owner_witness = tron_backend_execution::WitnessInfo::new(
        owner_address,
        "http://witness.example.com".to_string(),
        100,
    );
    storage_adapter.put_witness(&owner_witness).unwrap();

    // Create proposal that ALREADY has owner's approval
    let proposal = Proposal {
        proposal_id: 1,
        proposer_address: proposer_tron.clone(),
        parameters: BTreeMap::new(),
        expiration_time: 2_000_000,
        create_time: 500_000,
        approvals: vec![owner_tron.clone()], // Owner already approved
        state: 0,
    };
    storage_adapter.put_proposal(&proposal).unwrap();

    // Try to add approval again
    let contract_data = build_proposal_approve_contract(&owner_tron, 1, true);
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(contract_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ProposalApproveContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            ..Default::default()
        },
    };

    let context = new_test_context(1_000_000);
    let result =
        service.execute_proposal_approve_contract(&mut storage_adapter, &transaction, &context);

    // Should fail with Java-matching error message
    assert!(result.is_err(), "Should fail on repeat approval");
    let err = result.unwrap_err();
    assert!(
        err.contains("has approved proposal") && err.contains("before"),
        "Error should mention already approved: {}",
        err
    );
}
