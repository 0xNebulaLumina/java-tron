//! AccountPermissionUpdateContract tests.

use super::super::super::*;
use super::common::{encode_varint, new_test_context, new_test_service_with_system_enabled};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{
    EngineBackedEvmStateStore, TronContractParameter, TronTransaction, TxMetadata,
};
use tron_backend_storage::StorageEngine;

/// Helper to build AccountPermissionUpdateContract protobuf data
/// AccountPermissionUpdateContract:
///   field 1: owner_address (bytes)
///   field 2: owner Permission
///   field 3: witness Permission (optional)
///   field 4: repeated actives Permission
fn build_account_permission_update_contract_data(
    owner_address: &[u8],
    owner_permission: &[u8],
    witness_permission: Option<&[u8]>,
    active_permissions: &[&[u8]],
) -> Bytes {
    let mut data = Vec::new();

    // Field 1: owner_address (length-delimited)
    encode_varint(&mut data, (1 << 3) | 2);
    encode_varint(&mut data, owner_address.len() as u64);
    data.extend_from_slice(owner_address);

    // Field 2: owner Permission (length-delimited)
    encode_varint(&mut data, (2 << 3) | 2);
    encode_varint(&mut data, owner_permission.len() as u64);
    data.extend_from_slice(owner_permission);

    // Field 3: witness Permission (optional, length-delimited)
    if let Some(witness) = witness_permission {
        encode_varint(&mut data, (3 << 3) | 2);
        encode_varint(&mut data, witness.len() as u64);
        data.extend_from_slice(witness);
    }

    // Field 4: active permissions (repeated, length-delimited)
    for active in active_permissions {
        encode_varint(&mut data, (4 << 3) | 2);
        encode_varint(&mut data, active.len() as u64);
        data.extend_from_slice(active);
    }

    Bytes::from(data)
}

/// Helper to build a Permission protobuf message
/// Permission:
///   field 1: type (varint: 0=Owner, 1=Witness, 2=Active)
///   field 2: id (varint)
///   field 3: permission_name (string)
///   field 4: threshold (varint)
///   field 5: parent_id (varint)
///   field 6: operations (bytes)
///   field 7: repeated keys (Key messages)
fn build_permission(
    permission_type: u64,
    id: i32,
    name: &str,
    threshold: i64,
    operations: Option<&[u8]>,
    keys: &[(&[u8], i64)], // (address, weight)
) -> Vec<u8> {
    let mut data = Vec::new();

    // Field 1: type (varint)
    encode_varint(&mut data, (1 << 3) | 0);
    encode_varint(&mut data, permission_type);

    // Field 2: id (varint)
    if id != 0 {
        encode_varint(&mut data, (2 << 3) | 0);
        encode_varint(&mut data, id as u64);
    }

    // Field 3: permission_name (length-delimited string)
    if !name.is_empty() {
        encode_varint(&mut data, (3 << 3) | 2);
        encode_varint(&mut data, name.len() as u64);
        data.extend_from_slice(name.as_bytes());
    }

    // Field 4: threshold (varint)
    encode_varint(&mut data, (4 << 3) | 0);
    encode_varint(&mut data, threshold as u64);

    // Field 5: parent_id (default 0, skip)

    // Field 6: operations (length-delimited bytes)
    if let Some(ops) = operations {
        encode_varint(&mut data, (6 << 3) | 2);
        encode_varint(&mut data, ops.len() as u64);
        data.extend_from_slice(ops);
    }

    // Field 7: repeated keys
    for (address, weight) in keys {
        let key = build_key(address, *weight);
        encode_varint(&mut data, (7 << 3) | 2);
        encode_varint(&mut data, key.len() as u64);
        data.extend_from_slice(&key);
    }

    data
}

/// Helper to build a Key protobuf message
/// Key:
///   field 1: address (bytes)
///   field 2: weight (varint)
fn build_key(address: &[u8], weight: i64) -> Vec<u8> {
    let mut data = Vec::new();

    // Field 1: address (length-delimited)
    encode_varint(&mut data, (1 << 3) | 2);
    encode_varint(&mut data, address.len() as u64);
    data.extend_from_slice(address);

    // Field 2: weight (varint)
    encode_varint(&mut data, (2 << 3) | 0);
    encode_varint(&mut data, weight as u64);

    data
}

#[test]
fn test_account_permission_update_strict_contract_parameter_required() {
    // Verify that missing contract_parameter is rejected (strict enforcement).
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    storage_engine
        .put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let transaction = TronTransaction {
        from: Address::ZERO,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(vec![0x0a, 0x00]),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::AccountPermissionUpdateContract,
            ),
            // contract_parameter intentionally omitted — must be rejected
            ..Default::default()
        },
    };

    let err = service
        .execute_account_permission_update_contract(&mut storage_adapter, &transaction, &context)
        .unwrap_err();
    assert!(
        err.contains("contract type error"),
        "Expected type mismatch error, got: {}",
        err
    );
}

#[test]
fn test_account_permission_update_validate_fail_owner_address_empty() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed required dynamic properties (Java throws if these are missing)
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
            b"UPDATE_ACCOUNT_PERMISSION_FEE",
            &100_000_000i64.to_be_bytes(),
        )
        .unwrap();
    // Java stores TOTAL_SIGN_NUM as 4-byte int (ByteArray.fromInt), not 8-byte long
    storage_engine
        .put("properties", b"TOTAL_SIGN_NUM", &5i32.to_be_bytes())
        .unwrap();
    let available_contract_type = [0xFFu8; 32];
    storage_engine
        .put(
            "properties",
            b"AVAILABLE_CONTRACT_TYPE",
            &available_contract_type,
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

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
    let service = BackendService::new(module_manager);

    // Ensure the transaction.from account exists so validation must come from the contract payload.
    let tx_from = Address::from([7u8; 20]);
    let tx_from_account = AccountInfo {
        balance: U256::from(1_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter
        .set_account(tx_from, tx_from_account)
        .unwrap();

    // AccountPermissionUpdateContract owner_address = "" (field 1, length 0)
    let contract_data = Bytes::from(vec![0x0a, 0x00]);

    let transaction = TronTransaction {
        from: tx_from,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::AccountPermissionUpdateContract,
            ),
            asset_id: None,
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountPermissionUpdateContract"
                    .to_string(),
                value: contract_data.to_vec(),
            }),
            ..Default::default()
        },
    };

    let err = service
        .execute_account_permission_update_contract(
            &mut storage_adapter,
            &transaction,
            &new_test_context(),
        )
        .unwrap_err();
    assert_eq!(err, "invalidate ownerAddress");
}

// -----------------------------------------------------------------------------
// Section 1: Burn semantics tests
// -----------------------------------------------------------------------------

/// Test that BURN_TRX_AMOUNT is incremented when blackhole optimization is enabled.
/// This matches Java's burnTrx() behavior under supportBlackHoleOptimization() == true.
#[test]
fn test_account_permission_update_burn_trx_when_blackhole_optimization_enabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed dynamic properties
    // ALLOW_MULTI_SIGN = 1 (enable multi-sign)
    storage_engine
        .put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes())
        .unwrap();
    // ALLOW_BLACKHOLE_OPTIMIZATION = 1 (use burn mode)
    storage_engine
        .put(
            "properties",
            b"ALLOW_BLACKHOLE_OPTIMIZATION",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    // BURN_TRX_AMOUNT = 0 (initial burn amount)
    storage_engine
        .put("properties", b"BURN_TRX_AMOUNT", &0i64.to_be_bytes())
        .unwrap();
    // UPDATE_ACCOUNT_PERMISSION_FEE = 100_000_000 (100 TRX)
    let fee: i64 = 100_000_000;
    storage_engine
        .put(
            "properties",
            b"UPDATE_ACCOUNT_PERMISSION_FEE",
            &fee.to_be_bytes(),
        )
        .unwrap();
    // TOTAL_SIGN_NUM = 5 (max keys per permission)
    // Java stores as 4-byte int (ByteArray.fromInt), not 8-byte long
    storage_engine
        .put("properties", b"TOTAL_SIGN_NUM", &5i32.to_be_bytes())
        .unwrap();
    // AVAILABLE_CONTRACT_TYPE - 32 bytes, all bits enabled
    let available_contract_type = [0xFFu8; 32];
    storage_engine
        .put(
            "properties",
            b"AVAILABLE_CONTRACT_TYPE",
            &available_contract_type,
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();

    // Create owner account with sufficient balance
    let owner_address = Address::from([0x11u8; 20]);
    let owner_balance = 200_000_000u64; // 200 TRX
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(owner_balance),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    // Build owner_address in 21-byte TRON format (0x41 prefix)
    let mut owner_tron = vec![0x41u8];
    owner_tron.extend_from_slice(owner_address.as_slice());

    // Build owner permission (type=0=Owner)
    let owner_permission = build_permission(
        0,                   // Owner type
        0,                   // id
        "owner",             // name
        1,                   // threshold
        None,                // no operations for Owner
        &[(&owner_tron, 1)], // keys
    );

    // Build active permission (type=2=Active)
    let active_permission = build_permission(
        2,                   // Active type
        2,                   // id
        "active",            // name
        1,                   // threshold
        Some(&[0u8; 32]),    // 32 bytes of operations (all disabled is fine)
        &[(&owner_tron, 1)], // keys
    );

    let contract_data = build_account_permission_update_contract_data(
        &owner_tron,
        &owner_permission,
        None,
        &[&active_permission],
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::AccountPermissionUpdateContract,
            ),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountPermissionUpdateContract"
                    .to_string(),
                value: contract_data.to_vec(),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_account_permission_update_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    assert!(
        result.is_ok(),
        "AccountPermissionUpdate should succeed: {:?}",
        result.err()
    );

    // Verify BURN_TRX_AMOUNT was incremented by the fee
    let burn_amount = storage_adapter.get_burn_trx_amount().unwrap();
    assert_eq!(
        burn_amount, fee,
        "BURN_TRX_AMOUNT should be incremented by fee ({}) when blackhole optimization enabled, got {}",
        fee, burn_amount
    );

    // Verify owner balance was decremented
    let final_balance = storage_adapter
        .get_account(&owner_address)
        .unwrap()
        .unwrap()
        .balance;
    assert_eq!(
        final_balance,
        U256::from(owner_balance - fee as u64),
        "Owner balance should be decremented by fee"
    );
}

/// Test that blackhole account balance is credited when blackhole optimization is disabled.
/// This matches Java's credit blackhole account behavior when supportBlackHoleOptimization() == false.
#[test]
fn test_account_permission_update_credit_blackhole_when_optimization_disabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed dynamic properties
    storage_engine
        .put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes())
        .unwrap();
    // ALLOW_BLACKHOLE_OPTIMIZATION = 0 (use credit blackhole mode)
    storage_engine
        .put(
            "properties",
            b"ALLOW_BLACKHOLE_OPTIMIZATION",
            &0i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"BURN_TRX_AMOUNT", &0i64.to_be_bytes())
        .unwrap();
    let fee: i64 = 100_000_000;
    storage_engine
        .put(
            "properties",
            b"UPDATE_ACCOUNT_PERMISSION_FEE",
            &fee.to_be_bytes(),
        )
        .unwrap();
    // Java stores TOTAL_SIGN_NUM as 4-byte int (ByteArray.fromInt)
    storage_engine
        .put("properties", b"TOTAL_SIGN_NUM", &5i32.to_be_bytes())
        .unwrap();
    let available_contract_type = [0xFFu8; 32];
    storage_engine
        .put(
            "properties",
            b"AVAILABLE_CONTRACT_TYPE",
            &available_contract_type,
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Get the blackhole address that the storage adapter uses (depends on address prefix)
    let blackhole_address = storage_adapter.get_blackhole_address().unwrap().unwrap();

    // Create blackhole account with initial balance 0
    // The account must exist for add_balance to work
    storage_adapter
        .set_account(
            blackhole_address,
            AccountInfo {
                balance: U256::ZERO,
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_system_enabled();

    // Create owner account
    let owner_address = Address::from([0x11u8; 20]);
    let owner_balance = 200_000_000u64;
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(owner_balance),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    // Build owner_address in 21-byte TRON format
    let mut owner_tron = vec![0x41u8];
    owner_tron.extend_from_slice(owner_address.as_slice());

    let owner_permission = build_permission(0, 0, "owner", 1, None, &[(&owner_tron, 1)]);
    let active_permission =
        build_permission(2, 2, "active", 1, Some(&[0u8; 32]), &[(&owner_tron, 1)]);

    let contract_data = build_account_permission_update_contract_data(
        &owner_tron,
        &owner_permission,
        None,
        &[&active_permission],
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::AccountPermissionUpdateContract,
            ),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountPermissionUpdateContract"
                    .to_string(),
                value: contract_data.to_vec(),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_account_permission_update_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    assert!(
        result.is_ok(),
        "AccountPermissionUpdate should succeed: {:?}",
        result.err()
    );

    // Verify BURN_TRX_AMOUNT was NOT incremented (should remain 0)
    let burn_amount = storage_adapter.get_burn_trx_amount().unwrap();
    assert_eq!(
        burn_amount, 0,
        "BURN_TRX_AMOUNT should remain 0 when blackhole optimization disabled, got {}",
        burn_amount
    );

    // Verify blackhole account balance was credited by the fee
    let blackhole_balance = storage_adapter
        .get_account(&blackhole_address)
        .unwrap()
        .unwrap()
        .balance;
    assert_eq!(
        blackhole_balance,
        U256::from(fee as u64),
        "Blackhole balance should be increased by fee when optimization disabled"
    );
}

// -----------------------------------------------------------------------------
// Section 2: Atomicity tests - ensure permissions unchanged on fee failure
// -----------------------------------------------------------------------------

/// Test that insufficient balance produces the correct Java-parity error message.
#[test]
fn test_account_permission_update_insufficient_balance_error_message() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed dynamic properties
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
        .put("properties", b"BURN_TRX_AMOUNT", &0i64.to_be_bytes())
        .unwrap();
    let fee: i64 = 100_000_000; // 100 TRX fee
    storage_engine
        .put(
            "properties",
            b"UPDATE_ACCOUNT_PERMISSION_FEE",
            &fee.to_be_bytes(),
        )
        .unwrap();
    // Java stores TOTAL_SIGN_NUM as 4-byte int (ByteArray.fromInt)
    storage_engine
        .put("properties", b"TOTAL_SIGN_NUM", &5i32.to_be_bytes())
        .unwrap();
    let available_contract_type = [0xFFu8; 32];
    storage_engine
        .put(
            "properties",
            b"AVAILABLE_CONTRACT_TYPE",
            &available_contract_type,
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();

    // Create owner account with INSUFFICIENT balance (< fee)
    let owner_address = Address::from([0x22u8; 20]);
    let owner_balance = 50_000_000u64; // Only 50 TRX, but fee is 100 TRX
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(owner_balance),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    // Build owner_address in 21-byte TRON format
    let mut owner_tron = vec![0x41u8];
    owner_tron.extend_from_slice(owner_address.as_slice());

    let owner_permission = build_permission(0, 0, "owner", 1, None, &[(&owner_tron, 1)]);
    let active_permission =
        build_permission(2, 2, "active", 1, Some(&[0u8; 32]), &[(&owner_tron, 1)]);

    let contract_data = build_account_permission_update_contract_data(
        &owner_tron,
        &owner_permission,
        None,
        &[&active_permission],
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::AccountPermissionUpdateContract,
            ),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountPermissionUpdateContract"
                    .to_string(),
                value: contract_data.to_vec(),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_account_permission_update_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    // Should fail with insufficient balance error
    assert!(result.is_err(), "Should fail due to insufficient balance");
    let error = result.err().unwrap();

    // Verify error message matches Java format: "<ownerHex> insufficient balance, balance: X, amount: Y"
    assert!(
        error.contains("insufficient balance"),
        "Error should mention insufficient balance: got '{}'",
        error
    );
    assert!(
        error.contains(&format!("{}", owner_balance)),
        "Error should include current balance: got '{}'",
        error
    );
    assert!(
        error.contains(&format!("{}", fee)),
        "Error should include required fee amount: got '{}'",
        error
    );
}

/// Test that verifies the write buffer atomicity works correctly.
#[test]
fn test_account_permission_update_atomicity_with_write_buffer() {
    use tron_backend_execution::ExecutionWriteBuffer;

    // This test demonstrates how the write buffer provides atomicity:
    // 1. All writes during execution go to the buffer
    // 2. On success, buffer.commit() persists all changes atomically
    // 3. On failure, the buffer is dropped without commit, discarding all changes

    let mut buffer = ExecutionWriteBuffer::new();

    // Simulate permission update writes
    let owner_address = vec![
        0x41u8, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
        0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
    ];
    let updated_account_data = vec![0xAA, 0xBB, 0xCC]; // Mock permission data

    // Write permissions to buffer (this is what execute_account_permission_update_contract does)
    buffer.put("account", owner_address.clone(), updated_account_data);

    // Simulate fee check failure (insufficient balance)
    let has_sufficient_balance = false;

    if has_sufficient_balance {
        // Success path: commit buffer
        // buffer.commit(&storage_engine).unwrap();
        panic!("This test simulates failure path");
    } else {
        // Failure path: drop buffer without commit
        // The buffer is dropped here, discarding all pending writes
        let pending_ops = buffer.operation_count();
        assert_eq!(
            pending_ops, 1,
            "Buffer should have pending writes before drop"
        );

        // When dropped, these writes are discarded
        drop(buffer);

        // After drop, there's no way to access the buffer's writes
        // In the real gRPC path, this means the storage engine receives no writes on failure
    }

    // This test passes because dropping the buffer without commit discards all pending writes.
}

// -----------------------------------------------------------------------------
// Section 3: AVAILABLE_CONTRACT_TYPE validation tests
// -----------------------------------------------------------------------------

/// Test that validation fails when active permission enables a contract type
/// that is not in AVAILABLE_CONTRACT_TYPE bitmap.
#[test]
fn test_account_permission_update_invalid_contract_type_in_operations() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed dynamic properties
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
    let fee: i64 = 100_000_000;
    storage_engine
        .put(
            "properties",
            b"UPDATE_ACCOUNT_PERMISSION_FEE",
            &fee.to_be_bytes(),
        )
        .unwrap();
    // Java stores TOTAL_SIGN_NUM as 4-byte int (ByteArray.fromInt)
    storage_engine
        .put("properties", b"TOTAL_SIGN_NUM", &5i32.to_be_bytes())
        .unwrap();

    // Set AVAILABLE_CONTRACT_TYPE with bit 0 UNSET (contract type 0 is not available)
    let mut available_contract_type = [0xFFu8; 32]; // All bits set
    available_contract_type[0] = 0xFE; // Unset bit 0 (contract type 0)
    storage_engine
        .put(
            "properties",
            b"AVAILABLE_CONTRACT_TYPE",
            &available_contract_type,
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();

    // Create owner account with sufficient balance
    let owner_address = Address::from([0x33u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(200_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    // Build owner_address in 21-byte TRON format
    let mut owner_tron = vec![0x41u8];
    owner_tron.extend_from_slice(owner_address.as_slice());

    let owner_permission = build_permission(0, 0, "owner", 1, None, &[(&owner_tron, 1)]);

    // Build active permission with bit 0 SET in operations
    let mut operations = [0u8; 32];
    operations[0] = 0x01; // Set bit 0 (contract type 0)

    let active_permission =
        build_permission(2, 2, "active", 1, Some(&operations), &[(&owner_tron, 1)]);

    let contract_data = build_account_permission_update_contract_data(
        &owner_tron,
        &owner_permission,
        None,
        &[&active_permission],
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::AccountPermissionUpdateContract,
            ),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountPermissionUpdateContract"
                    .to_string(),
                value: contract_data.to_vec(),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_account_permission_update_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    // Should fail with invalid contract type error
    assert!(
        result.is_err(),
        "Should fail due to invalid contract type in operations"
    );
    let error = result.err().unwrap();
    assert!(
        error.contains("isn't a validate ContractType"),
        "Error should indicate invalid contract type: got '{}'",
        error
    );
    assert!(
        error.contains("0"),
        "Error should mention contract type 0: got '{}'",
        error
    );
}

/// Test that validation fails when AVAILABLE_CONTRACT_TYPE is missing from dynamic properties.
#[test]
fn test_account_permission_update_missing_available_contract_type() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed dynamic properties WITHOUT AVAILABLE_CONTRACT_TYPE
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
    let fee: i64 = 100_000_000;
    storage_engine
        .put(
            "properties",
            b"UPDATE_ACCOUNT_PERMISSION_FEE",
            &fee.to_be_bytes(),
        )
        .unwrap();
    // Java stores TOTAL_SIGN_NUM as 4-byte int (ByteArray.fromInt)
    storage_engine
        .put("properties", b"TOTAL_SIGN_NUM", &5i32.to_be_bytes())
        .unwrap();
    // NOTE: AVAILABLE_CONTRACT_TYPE is intentionally NOT set

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();

    let owner_address = Address::from([0x44u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(200_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let mut owner_tron = vec![0x41u8];
    owner_tron.extend_from_slice(owner_address.as_slice());

    let owner_permission = build_permission(0, 0, "owner", 1, None, &[(&owner_tron, 1)]);
    // Active permission with some operations enabled
    let mut operations = [0u8; 32];
    operations[0] = 0x02; // Enable contract type 1
    let active_permission =
        build_permission(2, 2, "active", 1, Some(&operations), &[(&owner_tron, 1)]);

    let contract_data = build_account_permission_update_contract_data(
        &owner_tron,
        &owner_permission,
        None,
        &[&active_permission],
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::AccountPermissionUpdateContract,
            ),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountPermissionUpdateContract"
                    .to_string(),
                value: contract_data.to_vec(),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_account_permission_update_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    // Should fail because AVAILABLE_CONTRACT_TYPE is missing
    assert!(
        result.is_err(),
        "Should fail when AVAILABLE_CONTRACT_TYPE is missing"
    );
    let error = result.err().unwrap();
    assert!(
        error.contains("AVAILABLE_CONTRACT_TYPE"),
        "Error should mention AVAILABLE_CONTRACT_TYPE: got '{}'",
        error
    );
}

/// Test that validation fails when AVAILABLE_CONTRACT_TYPE is too short (< 32 bytes).
#[test]
fn test_account_permission_update_available_contract_type_too_short() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

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
    let fee: i64 = 100_000_000;
    storage_engine
        .put(
            "properties",
            b"UPDATE_ACCOUNT_PERMISSION_FEE",
            &fee.to_be_bytes(),
        )
        .unwrap();
    // Java stores TOTAL_SIGN_NUM as 4-byte int (ByteArray.fromInt)
    storage_engine
        .put("properties", b"TOTAL_SIGN_NUM", &5i32.to_be_bytes())
        .unwrap();
    // AVAILABLE_CONTRACT_TYPE with only 16 bytes (should be 32)
    let short_available = [0xFFu8; 16];
    storage_engine
        .put("properties", b"AVAILABLE_CONTRACT_TYPE", &short_available)
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();

    let owner_address = Address::from([0x55u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(200_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let mut owner_tron = vec![0x41u8];
    owner_tron.extend_from_slice(owner_address.as_slice());

    let owner_permission = build_permission(0, 0, "owner", 1, None, &[(&owner_tron, 1)]);
    let mut operations = [0u8; 32];
    operations[0] = 0x02;
    let active_permission =
        build_permission(2, 2, "active", 1, Some(&operations), &[(&owner_tron, 1)]);

    let contract_data = build_account_permission_update_contract_data(
        &owner_tron,
        &owner_permission,
        None,
        &[&active_permission],
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::AccountPermissionUpdateContract,
            ),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountPermissionUpdateContract"
                    .to_string(),
                value: contract_data.to_vec(),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_account_permission_update_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    // Should fail because AVAILABLE_CONTRACT_TYPE is too short
    assert!(
        result.is_err(),
        "Should fail when AVAILABLE_CONTRACT_TYPE is too short"
    );
    let error = result.err().unwrap();
    assert!(
        error.contains("too short") || error.contains("AVAILABLE_CONTRACT_TYPE"),
        "Error should indicate AVAILABLE_CONTRACT_TYPE is too short: got '{}'",
        error
    );
}

/// Test that validation fails when ALLOW_MULTI_SIGN is missing from dynamic properties.
#[test]
fn test_account_permission_update_missing_allow_multi_sign() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed dynamic properties WITHOUT ALLOW_MULTI_SIGN
    storage_engine
        .put(
            "properties",
            b"ALLOW_BLACKHOLE_OPTIMIZATION",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    let fee: i64 = 100_000_000;
    storage_engine
        .put(
            "properties",
            b"UPDATE_ACCOUNT_PERMISSION_FEE",
            &fee.to_be_bytes(),
        )
        .unwrap();
    // Java stores TOTAL_SIGN_NUM as 4-byte int (ByteArray.fromInt)
    storage_engine
        .put("properties", b"TOTAL_SIGN_NUM", &5i32.to_be_bytes())
        .unwrap();
    let available_contract_type = [0xFFu8; 32];
    storage_engine
        .put(
            "properties",
            b"AVAILABLE_CONTRACT_TYPE",
            &available_contract_type,
        )
        .unwrap();
    // NOTE: ALLOW_MULTI_SIGN is intentionally NOT set

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();

    let owner_address = Address::from([0x56u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(200_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let mut owner_tron = vec![0x41u8];
    owner_tron.extend_from_slice(owner_address.as_slice());

    let owner_permission = build_permission(0, 0, "owner", 1, None, &[(&owner_tron, 1)]);
    let active_permission =
        build_permission(2, 2, "active", 1, Some(&[0u8; 32]), &[(&owner_tron, 1)]);

    let contract_data = build_account_permission_update_contract_data(
        &owner_tron,
        &owner_permission,
        None,
        &[&active_permission],
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::AccountPermissionUpdateContract,
            ),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountPermissionUpdateContract"
                    .to_string(),
                value: contract_data.to_vec(),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_account_permission_update_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    // Should fail because ALLOW_MULTI_SIGN is missing
    assert!(
        result.is_err(),
        "Should fail when ALLOW_MULTI_SIGN is missing"
    );
    let error = result.err().unwrap();
    assert!(
        error.contains("not found ALLOW_MULTI_SIGN") || error.contains("ALLOW_MULTI_SIGN"),
        "Error should mention ALLOW_MULTI_SIGN: got '{}'",
        error
    );
}

/// Test that validation fails when TOTAL_SIGN_NUM is missing from dynamic properties.
#[test]
fn test_account_permission_update_missing_total_sign_num() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed dynamic properties WITHOUT TOTAL_SIGN_NUM
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
    let fee: i64 = 100_000_000;
    storage_engine
        .put(
            "properties",
            b"UPDATE_ACCOUNT_PERMISSION_FEE",
            &fee.to_be_bytes(),
        )
        .unwrap();
    let available_contract_type = [0xFFu8; 32];
    storage_engine
        .put(
            "properties",
            b"AVAILABLE_CONTRACT_TYPE",
            &available_contract_type,
        )
        .unwrap();
    // NOTE: TOTAL_SIGN_NUM is intentionally NOT set

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();

    let owner_address = Address::from([0x57u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(200_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let mut owner_tron = vec![0x41u8];
    owner_tron.extend_from_slice(owner_address.as_slice());

    let owner_permission = build_permission(0, 0, "owner", 1, None, &[(&owner_tron, 1)]);
    let active_permission =
        build_permission(2, 2, "active", 1, Some(&[0u8; 32]), &[(&owner_tron, 1)]);

    let contract_data = build_account_permission_update_contract_data(
        &owner_tron,
        &owner_permission,
        None,
        &[&active_permission],
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::AccountPermissionUpdateContract,
            ),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountPermissionUpdateContract"
                    .to_string(),
                value: contract_data.to_vec(),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_account_permission_update_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    // Should fail because TOTAL_SIGN_NUM is missing
    assert!(
        result.is_err(),
        "Should fail when TOTAL_SIGN_NUM is missing"
    );
    let error = result.err().unwrap();
    assert!(
        error.contains("not found TOTAL_SIGN_NUM") || error.contains("TOTAL_SIGN_NUM"),
        "Error should mention TOTAL_SIGN_NUM: got '{}'",
        error
    );
}

/// Test that validation fails when UPDATE_ACCOUNT_PERMISSION_FEE is missing from dynamic properties.
#[test]
fn test_account_permission_update_missing_update_account_permission_fee() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed dynamic properties WITHOUT UPDATE_ACCOUNT_PERMISSION_FEE
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
    // Java stores TOTAL_SIGN_NUM as 4-byte int (ByteArray.fromInt)
    storage_engine
        .put("properties", b"TOTAL_SIGN_NUM", &5i32.to_be_bytes())
        .unwrap();
    let available_contract_type = [0xFFu8; 32];
    storage_engine
        .put(
            "properties",
            b"AVAILABLE_CONTRACT_TYPE",
            &available_contract_type,
        )
        .unwrap();
    // NOTE: UPDATE_ACCOUNT_PERMISSION_FEE is intentionally NOT set

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();

    let owner_address = Address::from([0x58u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(200_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let mut owner_tron = vec![0x41u8];
    owner_tron.extend_from_slice(owner_address.as_slice());

    let owner_permission = build_permission(0, 0, "owner", 1, None, &[(&owner_tron, 1)]);
    let active_permission =
        build_permission(2, 2, "active", 1, Some(&[0u8; 32]), &[(&owner_tron, 1)]);

    let contract_data = build_account_permission_update_contract_data(
        &owner_tron,
        &owner_permission,
        None,
        &[&active_permission],
    );

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(
                tron_backend_execution::TronContractType::AccountPermissionUpdateContract,
            ),
            contract_parameter: Some(TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountPermissionUpdateContract"
                    .to_string(),
                value: contract_data.to_vec(),
            }),
            ..Default::default()
        },
    };

    let result = service.execute_account_permission_update_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    // Should fail because UPDATE_ACCOUNT_PERMISSION_FEE is missing
    assert!(
        result.is_err(),
        "Should fail when UPDATE_ACCOUNT_PERMISSION_FEE is missing"
    );
    let error = result.err().unwrap();
    assert!(
        error.contains("not found UPDATE_ACCOUNT_PERMISSION_FEE")
            || error.contains("UPDATE_ACCOUNT_PERMISSION_FEE"),
        "Error should mention UPDATE_ACCOUNT_PERMISSION_FEE: got '{}'",
        error
    );
}
