//! CancelAllUnfreezeV2Contract tests.
//!
//! These tests verify parity with Java's CancelAllUnfreezeV2Actuator:
//! - Contract parameter parsing (owner_address from protobuf field 1)
//! - Address validation (21 bytes with correct prefix)
//! - Gate checks (ALLOW_CANCEL_ALL_UNFREEZE_V2 && UNFREEZE_DELAY_DAYS > 0)
//! - Error messages matching Java

use super::super::super::*;
use super::common::{
    encode_varint, make_from_raw, new_test_context, new_test_service_with_system_enabled,
    seed_dynamic_properties,
};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_execution::{
    EngineBackedEvmStateStore, TronContractParameter, TronTransaction, TxMetadata,
};

/// Seed CancelAllUnfreezeV2 gate properties
fn seed_cancel_all_unfreeze_v2_enabled(storage_engine: &tron_backend_storage::StorageEngine) {
    // ALLOW_CANCEL_ALL_UNFREEZE_V2 = 1
    storage_engine
        .put(
            "properties",
            b"ALLOW_CANCEL_ALL_UNFREEZE_V2",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    // UNFREEZE_DELAY_DAYS > 0 (e.g., 14 days)
    storage_engine
        .put("properties", b"UNFREEZE_DELAY_DAYS", &14i64.to_be_bytes())
        .unwrap();
    // Latest block timestamp
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000000i64.to_be_bytes(),
        )
        .unwrap();
}

/// Helper to build CancelAllUnfreezeV2Contract protobuf bytes
/// CancelAllUnfreezeV2Contract: bytes owner_address = 1
fn build_cancel_all_unfreeze_v2_contract_proto(owner_address: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();

    // Field 1: owner_address (bytes, wire type 2)
    if !owner_address.is_empty() {
        encode_varint(&mut buf, (1 << 3) | 2); // tag = (field_number << 3) | wire_type
        encode_varint(&mut buf, owner_address.len() as u64);
        buf.extend_from_slice(owner_address);
    }

    buf
}

/// Create a proper TronContractParameter for CancelAllUnfreezeV2Contract
fn make_contract_parameter(owner_address: &[u8]) -> TronContractParameter {
    TronContractParameter {
        type_url: "type.googleapis.com/protocol.CancelAllUnfreezeV2Contract".to_string(),
        value: build_cancel_all_unfreeze_v2_contract_proto(owner_address),
    }
}

/// Seed an account with unfrozenV2 entries for testing
fn seed_account_with_unfrozen_v2(
    storage_adapter: &mut EngineBackedEvmStateStore,
    owner: &Address,
    unfrozen_entries: Vec<(i32, i64, i64)>, // (resource_type, amount, expire_time)
) {
    // Create account proto with unfrozenV2 entries
    let mut account = tron_backend_execution::protocol::Account::default();
    account.address = {
        let mut addr = vec![0x41u8];
        addr.extend_from_slice(owner.as_slice());
        addr
    };
    account.balance = 1_000_000_000; // 1000 TRX

    for (resource_type, amount, expire_time) in unfrozen_entries {
        account
            .unfrozen_v2
            .push(tron_backend_execution::protocol::account::UnFreezeV2 {
                r#type: resource_type,
                unfreeze_amount: amount,
                unfreeze_expire_time: expire_time,
            });
    }

    storage_adapter.put_account_proto(owner, &account).unwrap();
}

// ============================================================================
// Edge-case tests for invalid owner_address
// ============================================================================

#[test]
fn test_cancel_all_unfreeze_v2_rejects_missing_contract_parameter() {
    // Java: any.is(CancelAllUnfreezeV2Contract.class) requires contract to be present
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_cancel_all_unfreeze_v2_enabled(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);

    // No contract_parameter provided
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: None, // Missing contract_parameter
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );
    assert!(
        result.is_err(),
        "Missing contract_parameter should be rejected"
    );
    assert_eq!(result.unwrap_err(), "No contract!");
}

#[test]
fn test_cancel_all_unfreeze_v2_rejects_wrong_type_url() {
    // Java: any.is(CancelAllUnfreezeV2Contract.class) checks type URL
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_cancel_all_unfreeze_v2_enabled(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let from_raw = make_from_raw(&owner_address);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(from_raw.clone()),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.WrongContract".to_string(), // Wrong type
                value: build_cancel_all_unfreeze_v2_contract_proto(&from_raw),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );
    assert!(result.is_err(), "Wrong type URL should be rejected");
    assert!(result.unwrap_err().contains("contract type error"));
}

#[test]
fn test_cancel_all_unfreeze_v2_rejects_empty_owner_address() {
    // Java: DecodeUtil.addressValid requires non-empty address
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_cancel_all_unfreeze_v2_enabled(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);

    // Empty owner_address in protobuf
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.CancelAllUnfreezeV2Contract".to_string(),
                value: build_cancel_all_unfreeze_v2_contract_proto(&[]), // Empty owner_address
            }),
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );
    assert!(result.is_err(), "Empty owner_address should be rejected");
    assert_eq!(result.unwrap_err(), "Invalid address");
}

#[test]
fn test_cancel_all_unfreeze_v2_rejects_20_byte_owner_address() {
    // Java: DecodeUtil.addressValid requires exactly 21 bytes
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_cancel_all_unfreeze_v2_enabled(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);

    // 20-byte address (missing TRON prefix)
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.CancelAllUnfreezeV2Contract".to_string(),
                value: build_cancel_all_unfreeze_v2_contract_proto(owner_address.as_slice()), // 20 bytes only
            }),
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );
    assert!(result.is_err(), "20-byte address should be rejected");
    assert_eq!(result.unwrap_err(), "Invalid address");
}

#[test]
fn test_cancel_all_unfreeze_v2_rejects_wrong_prefix() {
    // Java: DecodeUtil.addressValid requires prefix == addressPreFixByte (0x41 for mainnet)
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_cancel_all_unfreeze_v2_enabled(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);

    // Wrong prefix (0xa0 instead of 0x41)
    let mut wrong_prefix_address = vec![0xa0u8];
    wrong_prefix_address.extend_from_slice(owner_address.as_slice());

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.CancelAllUnfreezeV2Contract".to_string(),
                value: build_cancel_all_unfreeze_v2_contract_proto(&wrong_prefix_address),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );
    assert!(result.is_err(), "Wrong prefix should be rejected");
    assert_eq!(result.unwrap_err(), "Invalid address");
}

#[test]
fn test_cancel_all_unfreeze_v2_rejects_22_byte_owner_address() {
    // Java: DecodeUtil.addressValid requires exactly 21 bytes, not more
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_cancel_all_unfreeze_v2_enabled(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);

    // 22-byte address (too long)
    let mut too_long_address = vec![0x41u8];
    too_long_address.extend_from_slice(owner_address.as_slice());
    too_long_address.push(0x00); // Extra byte

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.CancelAllUnfreezeV2Contract".to_string(),
                value: build_cancel_all_unfreeze_v2_contract_proto(&too_long_address),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );
    assert!(result.is_err(), "22-byte address should be rejected");
    assert_eq!(result.unwrap_err(), "Invalid address");
}

#[test]
fn test_cancel_all_unfreeze_v2_rejects_malformed_protobuf() {
    // Test that malformed protobuf in contract_parameter is handled
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_cancel_all_unfreeze_v2_enabled(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.CancelAllUnfreezeV2Contract".to_string(),
                value: vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], // Invalid varint
            }),
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );
    assert!(result.is_err(), "Malformed protobuf should be rejected");
    // The error should indicate a parsing failure
    let err = result.unwrap_err();
    assert!(
        err.contains("varint") || err.contains("Varint") || err.contains("Failed"),
        "Expected parse error, got: {}",
        err
    );
}

#[test]
fn test_cancel_all_unfreeze_v2_rejects_nonexistent_account() {
    // Java: Account must exist
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_cancel_all_unfreeze_v2_enabled(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let from_raw = make_from_raw(&owner_address);
    // Don't create the account

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(from_raw.clone()),
            contract_parameter: Some(make_contract_parameter(&from_raw)),
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );
    assert!(result.is_err(), "Non-existent account should be rejected");
    assert!(result.unwrap_err().contains("not exists"));
}

#[test]
fn test_cancel_all_unfreeze_v2_rejects_empty_unfrozen_list() {
    // Java: "No unfreezeV2 list to cancel"
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_cancel_all_unfreeze_v2_enabled(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let from_raw = make_from_raw(&owner_address);

    // Create account with empty unfrozenV2 list
    seed_account_with_unfrozen_v2(&mut storage_adapter, &owner_address, vec![]);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(from_raw.clone()),
            contract_parameter: Some(make_contract_parameter(&from_raw)),
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );
    assert!(result.is_err(), "Empty unfrozen list should be rejected");
    assert_eq!(result.unwrap_err(), "No unfreezeV2 list to cancel");
}

#[test]
fn test_cancel_all_unfreeze_v2_rejects_when_feature_disabled() {
    // Java: "Not support CancelAllUnfreezeV2 transaction, need to be opened by the committee"
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // DON'T enable the feature - leave ALLOW_CANCEL_ALL_UNFREEZE_V2 = 0
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000000i64.to_be_bytes(),
        )
        .unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let from_raw = make_from_raw(&owner_address);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(from_raw.clone()),
            contract_parameter: Some(make_contract_parameter(&from_raw)),
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );
    assert!(result.is_err(), "Should reject when feature is disabled");
    assert!(result
        .unwrap_err()
        .contains("Not support CancelAllUnfreezeV2"));
}

// ============================================================================
// Happy path tests (owner_address parsed from protobuf)
// ============================================================================

#[test]
fn test_cancel_all_unfreeze_v2_happy_path_with_valid_proto() {
    // Test successful execution with owner_address parsed from protobuf
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_cancel_all_unfreeze_v2_enabled(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let from_raw = make_from_raw(&owner_address);

    // Create account with some unfrozenV2 entries
    // (resource_type, amount, expire_time)
    // expire_time > LATEST_BLOCK_HEADER_TIMESTAMP (1000000000) means unexpired
    seed_account_with_unfrozen_v2(
        &mut storage_adapter,
        &owner_address,
        vec![
            (0, 5_000_000_000, 2000000000), // BANDWIDTH, unexpired
        ],
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(from_raw.clone()),
            contract_parameter: Some(make_contract_parameter(&from_raw)),
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );
    assert!(
        result.is_ok(),
        "Should succeed with valid proto: {:?}",
        result.err()
    );

    let execution_result = result.unwrap();
    assert!(execution_result.success, "Execution should be successful");
    assert!(
        execution_result.tron_transaction_result.is_some(),
        "Should have receipt"
    );
}

#[test]
fn test_cancel_all_unfreeze_v2_proto_owner_takes_precedence() {
    // Test that owner_address from proto is used, not from_raw
    // (This is the key Java parity behavior we're testing)
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_cancel_all_unfreeze_v2_enabled(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let proto_owner = Address::from([1u8; 20]);
    let proto_owner_raw = make_from_raw(&proto_owner);

    let different_from = Address::from([2u8; 20]);
    let different_from_raw = make_from_raw(&different_from);

    // Create account for proto_owner (the one that should be used)
    seed_account_with_unfrozen_v2(
        &mut storage_adapter,
        &proto_owner,
        vec![
            (0, 5_000_000_000, 2000000000), // BANDWIDTH, unexpired
        ],
    );

    // transaction.from and from_raw point to different_from, but proto has proto_owner
    let transaction = TronTransaction {
        from: different_from,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract,
            ),
            from_raw: Some(different_from_raw), // Different from proto
            contract_parameter: Some(make_contract_parameter(&proto_owner_raw)), // Proto has proto_owner
            ..Default::default()
        },
    };

    let result = service.execute_cancel_all_unfreeze_v2_contract(
        &mut storage_adapter,
        &transaction,
        &context,
    );

    // Should succeed because we use proto_owner from protobuf, not from_raw
    assert!(
        result.is_ok(),
        "Should use owner from proto, not from_raw: {:?}",
        result.err()
    );
}
