//! VoteWitnessContract tests.
//!
//! Tests for Java parity of `execute_vote_witness_contract`, including:
//! - withdrawReward (delegation reward → allowance)
//! - oldTronPower initialization under new resource model
//! - Vote bookkeeping (VotesStore old_votes seeding, Account.votes replacement)
//! - Validation error messages

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, new_test_context, seed_dynamic_properties};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{
    EngineBackedEvmStateStore, TronContractType, TronExecutionContext, TronTransaction, TxMetadata,
    WitnessInfo,
};
use tron_backend_storage::StorageEngine;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a BackendService with vote_witness_enabled.
fn new_vote_witness_service() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            vote_witness_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Encode a VoteWitnessContract protobuf from owner_address and votes list.
/// Proto layout:
///   field 1 (bytes): owner_address
///   field 2 (embedded message, repeated): Vote { field 1: vote_address, field 2: vote_count }
fn encode_vote_witness_contract(owner_address_raw: &[u8], votes: &[(&[u8], u64)]) -> Vec<u8> {
    let mut buf = Vec::new();

    // Field 1: owner_address (tag = (1 << 3) | 2 = 0x0A)
    buf.push(0x0A);
    encode_varint(&mut buf, owner_address_raw.len() as u64);
    buf.extend_from_slice(owner_address_raw);

    // Field 2 (repeated): Vote messages
    for (vote_addr, vote_count) in votes {
        // Encode inner Vote message into a temp buffer
        let mut vote_buf = Vec::new();
        // vote_address: field 1, wire type 2 (tag 0x0A)
        vote_buf.push(0x0A);
        encode_varint(&mut vote_buf, vote_addr.len() as u64);
        vote_buf.extend_from_slice(*vote_addr);
        // vote_count: field 2, wire type 0 (tag 0x10)
        vote_buf.push(0x10);
        encode_varint(&mut vote_buf, *vote_count);

        // Outer field 2 tag + length
        buf.push(0x12);
        encode_varint(&mut buf, vote_buf.len() as u64);
        buf.extend_from_slice(&vote_buf);
    }

    buf
}

/// Make a TronTransaction for VoteWitnessContract.
fn make_vote_tx(owner: Address, owner_tron: Vec<u8>, data: Vec<u8>) -> TronTransaction {
    TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::VoteWitnessContract),
            from_raw: Some(owner_tron),
            ..Default::default()
        },
    }
}

/// Seed an owner account with balance and legacy frozen bandwidth (gives tron power).
fn seed_owner_with_tron_power(
    storage_adapter: &mut EngineBackedEvmStateStore,
    owner: &Address,
    balance_sun: i64,
    frozen_bandwidth_sun: i64,
) {
    // Seed AccountInfo (for EVM layer)
    let account_info = AccountInfo {
        balance: U256::from(balance_sun as u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(*owner, account_info).unwrap();

    // Seed Account proto (for tron power computation)
    let mut proto = tron_backend_execution::protocol::Account::default();
    proto.balance = balance_sun;
    if frozen_bandwidth_sun > 0 {
        proto
            .frozen
            .push(tron_backend_execution::protocol::account::Frozen {
                frozen_balance: frozen_bandwidth_sun,
                expire_time: i64::MAX, // Far future
            });
    }
    storage_adapter.put_account_proto(owner, &proto).unwrap();
}

/// Seed a witness entry for a given address.
fn seed_witness(storage_adapter: &mut EngineBackedEvmStateStore, witness_addr: &Address) {
    let account_info = AccountInfo {
        balance: U256::from(1_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter
        .set_account(*witness_addr, account_info)
        .unwrap();

    let witness = WitnessInfo::new(*witness_addr, "http://witness.example".to_string(), 0);
    storage_adapter.put_witness(&witness).unwrap();
}

// ---------------------------------------------------------------------------
// Tests: withdrawReward integration
// ---------------------------------------------------------------------------

/// When CHANGE_DELEGATION == 0 (disabled), withdrawReward returns 0 and
/// account.allowance is unchanged. This is the default conformance fixture path.
#[test]
fn test_vote_witness_no_reward_when_delegation_disabled() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // CHANGE_DELEGATION defaults to 0 (not set = disabled)
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x01; 20]);
    let witness = Address::from([0x02; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness_tron = make_from_raw(&witness);

    seed_owner_with_tron_power(&mut storage_adapter, &owner, 10_000_000, 5_000_000);
    seed_witness(&mut storage_adapter, &witness);

    let data = encode_vote_witness_contract(&owner_tron, &[(&witness_tron, 1)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());
    let exec_result = result.unwrap();
    assert!(exec_result.success);

    // Allowance should remain 0 (no delegation reward)
    let account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(
        account.allowance, 0,
        "Allowance should remain 0 when delegation disabled"
    );
}

/// When CHANGE_DELEGATION == 1 and rewards exist, withdrawReward should
/// add the reward to account.allowance.
#[test]
fn test_vote_witness_withdraw_reward_with_delegation_enabled() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // Enable delegation
    storage_engine
        .put("properties", b"CHANGE_DELEGATION", &1i64.to_be_bytes())
        .unwrap();
    // Set current cycle to 5
    storage_engine
        .put("properties", b"CURRENT_CYCLE_NUMBER", &5i64.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x10; 20]);
    let witness = Address::from([0x20; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness_tron = make_from_raw(&witness);

    seed_owner_with_tron_power(&mut storage_adapter, &owner, 10_000_000, 5_000_000);
    seed_witness(&mut storage_adapter, &witness);

    // Set up delegation store: beginCycle=5, endCycle=6 (no pending reward cycles)
    // This means the account is already at the current cycle, so no reward to compute.
    storage_adapter
        .set_delegation_begin_cycle(&owner, 5)
        .unwrap();
    storage_adapter.set_delegation_end_cycle(&owner, 6).unwrap();

    let data = encode_vote_witness_contract(&owner_tron, &[(&witness_tron, 1)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());
    let exec_result = result.unwrap();
    assert!(exec_result.success);

    // With beginCycle == currentCycle (5), withdrawal should find no reward
    let account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(
        account.allowance, 0,
        "No reward when beginCycle == currentCycle"
    );
}

/// When CHANGE_DELEGATION == 1 but delegation is enabled with no votes (empty account),
/// withdrawReward should be a no-op (returns 0).
#[test]
fn test_vote_witness_withdraw_reward_noop_no_prior_votes() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put("properties", b"CHANGE_DELEGATION", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"CURRENT_CYCLE_NUMBER", &10i64.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x11; 20]);
    let witness = Address::from([0x21; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness_tron = make_from_raw(&witness);

    seed_owner_with_tron_power(&mut storage_adapter, &owner, 10_000_000, 5_000_000);
    seed_witness(&mut storage_adapter, &witness);

    // No delegation cycles set (beginCycle defaults to 0, endCycle defaults to 0)
    // begin_cycle(0) < current_cycle(10), but no account_vote at cycle 0 and no votes in account.
    // This means: no votes → sets beginCycle to endCycle + 1 and returns 0.

    let data = encode_vote_witness_contract(&owner_tron, &[(&witness_tron, 1)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    let account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(account.allowance, 0, "No reward when no prior votes exist");
}

// ---------------------------------------------------------------------------
// Tests: oldTronPower initialization
// ---------------------------------------------------------------------------

/// When ALLOW_NEW_RESOURCE_MODEL == 1 and oldTronPower == 0 with non-zero legacy frozen,
/// VoteWitness should set oldTronPower to the legacy frozen amount.
#[test]
fn test_vote_witness_initializes_old_tron_power_positive() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // Enable new resource model
    storage_engine
        .put(
            "properties",
            b"ALLOW_NEW_RESOURCE_MODEL",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x30; 20]);
    let witness = Address::from([0x31; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness_tron = make_from_raw(&witness);

    // Seed with 5_000_000 SUN frozen bandwidth → tronPower = 5_000_000
    seed_owner_with_tron_power(&mut storage_adapter, &owner, 100_000_000, 5_000_000);
    seed_witness(&mut storage_adapter, &witness);

    // Verify oldTronPower starts at 0
    let before = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(before.old_tron_power, 0, "Should start as not initialized");

    let data = encode_vote_witness_contract(&owner_tron, &[(&witness_tron, 1)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    // Verify oldTronPower was set to the frozen balance (5_000_000)
    let after = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(
        after.old_tron_power, 5_000_000,
        "oldTronPower should be initialized to legacy frozen amount"
    );
}

/// When ALLOW_NEW_RESOURCE_MODEL == 1 and oldTronPower == 0 with zero tron power,
/// VoteWitness should set oldTronPower to -1 (Java's sentinel for "initialized but zero").
#[test]
fn test_vote_witness_initializes_old_tron_power_to_minus_one_when_zero_power() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put(
            "properties",
            b"ALLOW_NEW_RESOURCE_MODEL",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x32; 20]);
    let witness = Address::from([0x33; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness_tron = make_from_raw(&witness);

    // Seed with V2 tron power frozen (not legacy frozen), so legacy getTronPower() returns 0
    // But we need enough total tron power to be able to vote
    let account_info = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, account_info).unwrap();

    let mut proto = tron_backend_execution::protocol::Account::default();
    proto.balance = 100_000_000;
    proto.old_tron_power = 0;
    // Use frozenV2 with TRON_POWER type to give voting power under new model
    proto
        .frozen_v2
        .push(tron_backend_execution::protocol::account::FreezeV2 {
            amount: 5_000_000,
            r#type: 2, // TRON_POWER
        });
    storage_adapter.put_account_proto(&owner, &proto).unwrap();
    seed_witness(&mut storage_adapter, &witness);

    let data = encode_vote_witness_contract(&owner_tron, &[(&witness_tron, 1)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    // oldTronPower = getTronPower() which only counts legacy frozen (field 7), not frozenV2.
    // Since legacy frozen is 0, oldTronPower should be -1.
    let after = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(
        after.old_tron_power, -1,
        "oldTronPower should be -1 when legacy tron power is zero"
    );
}

/// When ALLOW_NEW_RESOURCE_MODEL is disabled, oldTronPower should NOT be modified.
#[test]
fn test_vote_witness_skips_old_tron_power_when_old_model() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // ALLOW_NEW_RESOURCE_MODEL defaults to 0 or not set → disabled
    storage_engine
        .put(
            "properties",
            b"ALLOW_NEW_RESOURCE_MODEL",
            &0i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x34; 20]);
    let witness = Address::from([0x35; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness_tron = make_from_raw(&witness);

    seed_owner_with_tron_power(&mut storage_adapter, &owner, 100_000_000, 5_000_000);
    seed_witness(&mut storage_adapter, &witness);

    let data = encode_vote_witness_contract(&owner_tron, &[(&witness_tron, 1)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    let after = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(
        after.old_tron_power, 0,
        "oldTronPower should remain 0 when new resource model is disabled"
    );
}

/// When oldTronPower is already initialized (non-zero), it should NOT be re-initialized.
#[test]
fn test_vote_witness_preserves_existing_old_tron_power() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine
        .put(
            "properties",
            b"ALLOW_NEW_RESOURCE_MODEL",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x36; 20]);
    let witness = Address::from([0x37; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness_tron = make_from_raw(&witness);

    // Seed with oldTronPower already set to -1 (initialized).
    // With old_tron_power == -1 and new_model, getAllTronPower only counts frozenV2 TRON_POWER.
    let account_info = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, account_info).unwrap();

    let mut proto = tron_backend_execution::protocol::Account::default();
    proto.balance = 100_000_000;
    proto.old_tron_power = -1; // Already initialized
                               // Use frozenV2 TRON_POWER to give voting power (legacy frozen doesn't count when old_tron_power == -1)
    proto
        .frozen_v2
        .push(tron_backend_execution::protocol::account::FreezeV2 {
            amount: 5_000_000,
            r#type: 2, // TRON_POWER
        });
    storage_adapter.put_account_proto(&owner, &proto).unwrap();
    seed_witness(&mut storage_adapter, &witness);

    let data = encode_vote_witness_contract(&owner_tron, &[(&witness_tron, 1)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    let after = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(
        after.old_tron_power, -1,
        "oldTronPower should remain -1 when already initialized"
    );
}

// ---------------------------------------------------------------------------
// Tests: Vote bookkeeping
// ---------------------------------------------------------------------------

/// First VoteWitness with non-empty Account.votes should seed old_votes correctly.
#[test]
fn test_vote_witness_seeds_old_votes_from_account_votes() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x40; 20]);
    let witness1 = Address::from([0x41; 20]);
    let witness2 = Address::from([0x42; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness1_tron = make_from_raw(&witness1);
    let witness2_tron = make_from_raw(&witness2);

    // Seed owner with existing Account.votes (simulating prior epoch votes)
    let account_info = AccountInfo {
        balance: U256::from(100_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, account_info).unwrap();

    let mut proto = tron_backend_execution::protocol::Account::default();
    proto.balance = 100_000_000;
    proto
        .frozen
        .push(tron_backend_execution::protocol::account::Frozen {
            frozen_balance: 10_000_000,
            expire_time: i64::MAX,
        });
    // Pre-existing votes in Account proto
    proto.votes.push(tron_backend_execution::protocol::Vote {
        vote_address: witness1_tron.clone(),
        vote_count: 3,
    });
    storage_adapter.put_account_proto(&owner, &proto).unwrap();

    seed_witness(&mut storage_adapter, &witness1);
    seed_witness(&mut storage_adapter, &witness2);

    // Vote for witness2 (different from existing vote)
    let data = encode_vote_witness_contract(&owner_tron, &[(&witness2_tron, 2)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    // Check VotesRecord: old_votes should be seeded from Account.votes
    let votes_record = storage_adapter
        .get_votes(&owner)
        .unwrap()
        .expect("VotesRecord should exist");

    assert_eq!(
        votes_record.old_votes.len(),
        1,
        "old_votes should contain the prior Account.votes entry"
    );
    assert_eq!(
        votes_record.old_votes[0].vote_count, 3,
        "old_votes should preserve the old vote count"
    );

    assert_eq!(
        votes_record.new_votes.len(),
        1,
        "new_votes should contain the new vote"
    );
    assert_eq!(
        votes_record.new_votes[0].vote_count, 2,
        "new_votes should have the new vote count"
    );

    // Account.votes should be replaced with new votes
    let updated_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(
        updated_account.votes.len(),
        1,
        "Account.votes should have exactly one entry after re-vote"
    );
    assert_eq!(
        updated_account.votes[0].vote_count, 2,
        "Account.votes should reflect the new vote count"
    );
}

/// Second VoteWitness in same epoch should NOT shift old_votes.
#[test]
fn test_vote_witness_second_vote_preserves_old_votes() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x50; 20]);
    let witness1 = Address::from([0x51; 20]);
    let witness2 = Address::from([0x52; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness1_tron = make_from_raw(&witness1);
    let witness2_tron = make_from_raw(&witness2);

    seed_owner_with_tron_power(&mut storage_adapter, &owner, 100_000_000, 10_000_000);
    seed_witness(&mut storage_adapter, &witness1);
    seed_witness(&mut storage_adapter, &witness2);

    // First vote: witness1 × 3
    let data1 = encode_vote_witness_contract(&owner_tron, &[(&witness1_tron, 3)]);
    let tx1 = make_vote_tx(owner, owner_tron.clone(), data1);
    let ctx = new_test_context();

    let result1 = service.execute_vote_witness_contract(&mut storage_adapter, &tx1, &ctx);
    assert!(
        result1.is_ok(),
        "First vote should succeed: {:?}",
        result1.err()
    );

    // After first vote, old_votes should be empty (no prior Account.votes)
    let votes1 = storage_adapter.get_votes(&owner).unwrap().unwrap();
    assert!(
        votes1.old_votes.is_empty(),
        "old_votes should be empty after first vote"
    );
    assert_eq!(votes1.new_votes.len(), 1);
    assert_eq!(votes1.new_votes[0].vote_count, 3);

    // Second vote: witness2 × 5
    let data2 = encode_vote_witness_contract(&owner_tron, &[(&witness2_tron, 5)]);
    let tx2 = make_vote_tx(owner, owner_tron.clone(), data2);

    let result2 = service.execute_vote_witness_contract(&mut storage_adapter, &tx2, &ctx);
    assert!(
        result2.is_ok(),
        "Second vote should succeed: {:?}",
        result2.err()
    );

    // After second vote, old_votes should still be empty (preserved from first VotesRecord)
    let votes2 = storage_adapter.get_votes(&owner).unwrap().unwrap();
    assert!(
        votes2.old_votes.is_empty(),
        "old_votes should remain empty (not shift) on second vote in same epoch"
    );
    assert_eq!(votes2.new_votes.len(), 1);
    assert_eq!(
        votes2.new_votes[0].vote_count, 5,
        "new_votes should be replaced with the second vote"
    );

    // Account.votes should reflect the second vote
    let updated_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(updated_account.votes.len(), 1);
    assert_eq!(updated_account.votes[0].vote_count, 5);
}

// ---------------------------------------------------------------------------
// Tests: Happy path
// ---------------------------------------------------------------------------

/// Basic happy path: single vote for a single witness.
#[test]
fn test_vote_witness_happy_path_single_vote() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x60; 20]);
    let witness = Address::from([0x61; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness_tron = make_from_raw(&witness);

    seed_owner_with_tron_power(&mut storage_adapter, &owner, 100_000_000, 10_000_000);
    seed_witness(&mut storage_adapter, &witness);

    let data = encode_vote_witness_contract(&owner_tron, &[(&witness_tron, 5)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());
    let exec_result = result.unwrap();

    assert!(exec_result.success);
    assert_eq!(exec_result.energy_used, 0);
    assert_eq!(
        exec_result.state_changes.len(),
        1,
        "Should have one AccountChange for CSV parity"
    );
    assert_eq!(
        exec_result.vote_changes.len(),
        1,
        "Should have one VoteChange"
    );
    assert_eq!(exec_result.vote_changes[0].votes.len(), 1);
    assert_eq!(exec_result.vote_changes[0].votes[0].vote_count, 5);
    assert!(exec_result.bandwidth_used > 0);

    // Verify Account.votes updated
    let account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(account.votes.len(), 1);
    assert_eq!(account.votes[0].vote_count, 5);
}

/// Multiple votes for different witnesses.
#[test]
fn test_vote_witness_multiple_witnesses() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x70; 20]);
    let witness1 = Address::from([0x71; 20]);
    let witness2 = Address::from([0x72; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness1_tron = make_from_raw(&witness1);
    let witness2_tron = make_from_raw(&witness2);

    // Need enough tron power for all votes: 3+7 = 10 TRX = 10_000_000 SUN
    seed_owner_with_tron_power(&mut storage_adapter, &owner, 100_000_000, 10_000_000);
    seed_witness(&mut storage_adapter, &witness1);
    seed_witness(&mut storage_adapter, &witness2);

    let data =
        encode_vote_witness_contract(&owner_tron, &[(&witness1_tron, 3), (&witness2_tron, 7)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    let account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    assert_eq!(account.votes.len(), 2, "Should have two vote entries");
}

// ---------------------------------------------------------------------------
// Tests: Validation
// ---------------------------------------------------------------------------

/// Empty votes list should fail.
#[test]
fn test_vote_witness_validation_empty_votes() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x80; 20]);
    let owner_tron = make_from_raw(&owner);

    seed_owner_with_tron_power(&mut storage_adapter, &owner, 100_000_000, 10_000_000);

    // No votes in the contract data
    let data = encode_vote_witness_contract(&owner_tron, &[]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("VoteNumber must more than 0"));
}

/// Votes exceeding tron power should fail.
#[test]
fn test_vote_witness_validation_exceeds_tron_power() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x81; 20]);
    let witness = Address::from([0x82; 20]);
    let owner_tron = make_from_raw(&owner);
    let witness_tron = make_from_raw(&witness);

    // Only 5 TRX tron power (5_000_000 SUN)
    seed_owner_with_tron_power(&mut storage_adapter, &owner, 100_000_000, 5_000_000);
    seed_witness(&mut storage_adapter, &witness);

    // Try to vote 6 (exceeds 5 TRX tron power)
    let data = encode_vote_witness_contract(&owner_tron, &[(&witness_tron, 6)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("greater than the tronPower"));
}

/// Voting for a non-witness address should fail.
#[test]
fn test_vote_witness_validation_not_a_witness() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x83; 20]);
    let non_witness = Address::from([0x84; 20]);
    let owner_tron = make_from_raw(&owner);
    let non_witness_tron = make_from_raw(&non_witness);

    seed_owner_with_tron_power(&mut storage_adapter, &owner, 100_000_000, 10_000_000);
    // Create account for non_witness but NOT a witness entry
    let account_info = AccountInfo {
        balance: U256::from(1_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter
        .set_account(non_witness, account_info)
        .unwrap();

    let data = encode_vote_witness_contract(&owner_tron, &[(&non_witness_tron, 1)]);
    let tx = make_vote_tx(owner, owner_tron.clone(), data);
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_err());
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("Witness") && err_msg.contains("not exists"),
        "Error should mention witness not exists, got: {}",
        err_msg
    );
}

/// Invalid owner address should fail.
#[test]
fn test_vote_witness_validation_invalid_owner() {
    let service = new_vote_witness_service();

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner = Address::from([0x85; 20]);

    // Use empty owner_address in the proto data
    let data = encode_vote_witness_contract(&[], &[]);
    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::VoteWitnessContract),
            from_raw: Some(make_from_raw(&owner)),
            ..Default::default()
        },
    };
    let ctx = new_test_context();

    let result = service.execute_vote_witness_contract(&mut storage_adapter, &tx, &ctx);
    assert!(result.is_err());
    // Empty address → "Invalid address" or "VoteNumber must more than 0"
    // (depends on which check fires first)
}
