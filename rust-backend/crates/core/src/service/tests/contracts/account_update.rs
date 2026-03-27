//! AccountUpdateContract tests.
//!
//! These tests verify parity with Java's UpdateAccountActuator:
//! - TransactionUtil.validAccountName: allows empty, max 200 bytes
//! - DecodeUtil.addressValid: requires 21 bytes with correct prefix
//! - Only-set-once behavior when ALLOW_UPDATE_ACCOUNT_NAME == 0
//! - Duplicate name check via account-index when updates disabled

use super::super::super::*;
use super::common::{
    make_from_raw, new_test_context, new_test_service_with_system_enabled, seed_dynamic_properties,
};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_execution::{EngineBackedEvmStateStore, TronContractParameter, TronTransaction, TxMetadata};

/// Helper to seed ALLOW_UPDATE_ACCOUNT_NAME dynamic property
fn seed_allow_update_account_name(
    storage_engine: &tron_backend_storage::StorageEngine,
    value: i64,
) {
    storage_engine
        .put(
            "properties",
            b"ALLOW_UPDATE_ACCOUNT_NAME",
            &value.to_be_bytes(),
        )
        .unwrap();
}

#[test]
fn test_account_update_happy_path_with_valid_from_raw() {
    // Setup
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0); // Updates disabled (only-set-once)
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    // Create test account (owner must exist)
    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account.clone())
        .is_ok());

    // Create transaction with proper 21-byte from_raw (0x41 prefix + 20-byte address)
    let account_name = "TestAccount";
    let proto_value = build_account_update_contract_proto(&make_from_raw(&owner_address), account_name.as_bytes());
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(account_name.as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: proto_value }),
            ..Default::default()
        },
    };

    // Execute
    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);

    // Assert success
    assert!(
        result.is_ok(),
        "Account update should succeed: {:?}",
        result.err()
    );
    let execution_result = result.unwrap();

    assert!(execution_result.success, "Execution should be successful");
    assert_eq!(execution_result.energy_used, 0, "Energy used should be 0");
    assert!(execution_result.logs.is_empty(), "Should have no logs");
    assert!(execution_result.error.is_none(), "Should have no error");

    // Verify account name was stored
    let stored_name = storage_adapter.get_account_name(&owner_address).unwrap();
    assert_eq!(stored_name, Some("TestAccount".to_string()));
}

#[test]
fn test_account_update_allows_empty_name() {
    // Java: TransactionUtil.validAccountName allows empty (allowEmpty=true)
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    // Empty name should be allowed per Java TransactionUtil.validAccountName
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(vec![]), // Empty name
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "Empty name should be allowed: {:?}",
        result.err()
    );
}

#[test]
fn test_account_update_allows_200_byte_name() {
    // Java: MAX_ACCOUNT_NAME_LEN = 200
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    // 200 bytes should succeed
    let name_200 = vec![b'a'; 200];
    let proto_value = build_account_update_contract_proto(&make_from_raw(&owner_address), &name_200);
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(name_200.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: proto_value }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "200-byte name should be allowed: {:?}",
        result.err()
    );
}

#[test]
fn test_account_update_rejects_201_byte_name() {
    // Java: MAX_ACCOUNT_NAME_LEN = 200, so 201 bytes should fail
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    // 201 bytes should fail
    let name_201 = vec![b'a'; 201];
    let proto_value = build_account_update_contract_proto(&make_from_raw(&owner_address), &name_201);
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(name_201.clone()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: proto_value }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "201-byte name should be rejected");
    assert_eq!(result.unwrap_err(), "Invalid accountName");
}

#[test]
fn test_account_update_rejects_missing_from_raw() {
    // Java: DecodeUtil.addressValid requires address to be provided
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    // No from_raw provided
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("ValidName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: None, // Missing from_raw
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "Missing from_raw should be rejected");
    assert_eq!(result.unwrap_err(), "Invalid ownerAddress");
}

#[test]
fn test_account_update_rejects_20_byte_address() {
    // Java: DecodeUtil.addressValid requires exactly 21 bytes
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    // 20-byte address (missing TRON prefix)
    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("ValidName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(owner_address.as_slice().to_vec()), // 20 bytes only
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "20-byte address should be rejected");
    assert_eq!(result.unwrap_err(), "Invalid ownerAddress");
}

#[test]
fn test_account_update_rejects_wrong_prefix() {
    // Java: DecodeUtil.addressValid requires prefix == addressPreFixByte (0x41 for mainnet)
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    // Wrong prefix (0xa0 when mainnet expects 0x41)
    let mut from_raw_wrong_prefix = vec![0xa0u8];
    from_raw_wrong_prefix.extend_from_slice(owner_address.as_slice());

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("ValidName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(from_raw_wrong_prefix),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "Wrong prefix should be rejected");
    assert_eq!(result.unwrap_err(), "Invalid ownerAddress");
}

#[test]
fn test_account_update_rejects_nonexistent_account() {
    // Java: Account must exist
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    // Don't create the account

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("ValidName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "Non-existent account should be rejected");
    assert_eq!(result.unwrap_err(), "Account does not exist");
}

#[test]
fn test_account_update_only_set_once_when_updates_disabled() {
    // Java: When ALLOW_UPDATE_ACCOUNT_NAME == 0, can only set name once
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0); // Updates disabled
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    // First name set should succeed
    let first_proto = build_account_update_contract_proto(&make_from_raw(&owner_address), b"FirstName");
    let first_tx = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("FirstName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: first_proto }),
            ..Default::default()
        },
    };

    let result = service.execute_account_update_contract(&mut storage_adapter, &first_tx, &context);
    assert!(
        result.is_ok(),
        "First name set should succeed: {:?}",
        result.err()
    );

    // Second attempt should fail with Java error message
    let second_proto = build_account_update_contract_proto(&make_from_raw(&owner_address), b"SecondName");
    let second_tx = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("SecondName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: second_proto }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &second_tx, &context);
    assert!(
        result.is_err(),
        "Second name set should fail when updates disabled"
    );
    assert_eq!(result.unwrap_err(), "This account name is already existed");

    // Verify original name is still there
    let stored_name = storage_adapter.get_account_name(&owner_address).unwrap();
    assert_eq!(stored_name, Some("FirstName".to_string()));
}

#[test]
fn test_account_update_allows_repeated_updates_when_enabled() {
    // Java: When ALLOW_UPDATE_ACCOUNT_NAME == 1, can update name multiple times
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 1); // Updates enabled
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    // First name set
    let first_proto = build_account_update_contract_proto(&make_from_raw(&owner_address), b"FirstName");
    let first_tx = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("FirstName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: first_proto }),
            ..Default::default()
        },
    };

    let result = service.execute_account_update_contract(&mut storage_adapter, &first_tx, &context);
    assert!(result.is_ok(), "First name set should succeed");

    // Second update should also succeed when ALLOW_UPDATE_ACCOUNT_NAME == 1
    let second_proto = build_account_update_contract_proto(&make_from_raw(&owner_address), b"SecondName");
    let second_tx = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("SecondName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: second_proto }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &second_tx, &context);
    assert!(
        result.is_ok(),
        "Second name set should succeed when updates enabled: {:?}",
        result.err()
    );

    // Verify new name was stored
    let stored_name = storage_adapter.get_account_name(&owner_address).unwrap();
    assert_eq!(stored_name, Some("SecondName".to_string()));
}

#[test]
fn test_account_update_duplicate_name_check_when_updates_disabled() {
    // Java: When ALLOW_UPDATE_ACCOUNT_NAME == 0, cannot use a name that already exists in account-index
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0); // Updates disabled
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    // Create first account and set its name
    let owner1 = Address::from([1u8; 20]);
    let owner1_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner1, owner1_account).is_ok());

    let tx1 = TronTransaction {
        from: owner1,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("SharedName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner1)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result = service.execute_account_update_contract(&mut storage_adapter, &tx1, &context);
    assert!(result.is_ok(), "First account name set should succeed");

    // Create second account and try to use the same name
    let owner2 = Address::from([2u8; 20]);
    let owner2_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner2, owner2_account).is_ok());

    let tx2 = TronTransaction {
        from: owner2,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("SharedName".as_bytes()), // Same name
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner2)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result = service.execute_account_update_contract(&mut storage_adapter, &tx2, &context);
    assert!(
        result.is_err(),
        "Duplicate name should be rejected when updates disabled"
    );
    assert_eq!(result.unwrap_err(), "This name is existed");
}

#[test]
fn test_account_update_duplicate_name_allowed_when_updates_enabled() {
    // Java: When ALLOW_UPDATE_ACCOUNT_NAME == 1, the duplicate name check is skipped
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 1); // Updates enabled
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    // Create first account and set its name
    let owner1 = Address::from([1u8; 20]);
    let owner1_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner1, owner1_account).is_ok());

    let tx1 = TronTransaction {
        from: owner1,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("SharedName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner1)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result = service.execute_account_update_contract(&mut storage_adapter, &tx1, &context);
    assert!(result.is_ok(), "First account name set should succeed");

    // Create second account and use the same name - should succeed when updates enabled
    let owner2 = Address::from([2u8; 20]);
    let owner2_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter.set_account(owner2, owner2_account).is_ok());

    let tx2 = TronTransaction {
        from: owner2,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("SharedName".as_bytes()), // Same name
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(make_from_raw(&owner2)),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountUpdateContract".to_string(), value: vec![] }),
            ..Default::default()
        },
    };

    let result = service.execute_account_update_contract(&mut storage_adapter, &tx2, &context);
    assert!(
        result.is_ok(),
        "Duplicate name should be allowed when updates enabled: {:?}",
        result.err()
    );
}

// ============================================================================
// Contract-parameter unpack parity tests
// ============================================================================

/// Helper to build AccountUpdateContract protobuf bytes
fn build_account_update_contract_proto(owner_address: &[u8], account_name: &[u8]) -> Vec<u8> {
    use super::common::encode_varint;
    let mut buf = Vec::new();

    // Field 1: account_name (bytes, wire type 2)
    if !account_name.is_empty() {
        encode_varint(&mut buf, (1 << 3) | 2); // tag
        encode_varint(&mut buf, account_name.len() as u64);
        buf.extend_from_slice(account_name);
    }

    // Field 2: owner_address (bytes, wire type 2)
    if !owner_address.is_empty() {
        encode_varint(&mut buf, (2 << 3) | 2); // tag
        encode_varint(&mut buf, owner_address.len() as u64);
        buf.extend_from_slice(owner_address);
    }

    buf
}

#[test]
fn test_account_update_with_contract_parameter() {
    // Test that contract_parameter is properly parsed and validated
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    let from_raw = make_from_raw(&owner_address);
    let account_name = b"ProtoName";

    // Build proper protobuf
    let proto_value = build_account_update_contract_proto(&from_raw, account_name);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(account_name.to_vec()), // Must match proto
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(from_raw),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountUpdateContract".to_string(),
                value: proto_value,
            }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "Should succeed with valid contract_parameter: {:?}",
        result.err()
    );

    let stored_name = storage_adapter.get_account_name(&owner_address).unwrap();
    assert_eq!(stored_name, Some("ProtoName".to_string()));
}

#[test]
fn test_account_update_rejects_wrong_type_url() {
    // Test that wrong type URL in contract_parameter is rejected
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    let from_raw = make_from_raw(&owner_address);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("TestName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(from_raw),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.WrongContract".to_string(), // Wrong type
                value: vec![],
            }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "Should reject wrong type URL");
    assert!(result.unwrap_err().contains("contract type error"));
}

#[test]
fn test_account_update_with_malformed_proto() {
    // Test that malformed protobuf in contract_parameter is rejected
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    let from_raw = make_from_raw(&owner_address);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("TestName".as_bytes()),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(from_raw),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountUpdateContract".to_string(),
                value: vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF], // Invalid protobuf - varint too long
            }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_err(), "Should reject malformed protobuf");
    assert!(result.unwrap_err().contains("Protocol buffer parse error"));
}

#[test]
fn test_account_update_proto_name_takes_precedence() {
    // Test that name from decoded proto is used even if transaction.data differs
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    seed_allow_update_account_name(&storage_engine, 0);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_system_enabled();
    let context = new_test_context();

    let owner_address = Address::from([1u8; 20]);
    let owner_account = AccountInfo {
        balance: U256::from(1000000),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    assert!(storage_adapter
        .set_account(owner_address, owner_account)
        .is_ok());

    let from_raw = make_from_raw(&owner_address);

    // Proto has "ProtoName", but transaction.data has "DataName"
    let proto_value = build_account_update_contract_proto(&from_raw, b"ProtoName");

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::from("DataName".as_bytes()), // Different from proto
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountUpdateContract),
            from_raw: Some(from_raw),
            contract_parameter: Some(tron_backend_execution::TronContractParameter {
                type_url: "type.googleapis.com/protocol.AccountUpdateContract".to_string(),
                value: proto_value,
            }),
            ..Default::default()
        },
    };

    let result =
        service.execute_account_update_contract(&mut storage_adapter, &transaction, &context);
    assert!(result.is_ok(), "Should succeed: {:?}", result.err());

    // Verify proto name was used (not transaction.data)
    let stored_name = storage_adapter.get_account_name(&owner_address).unwrap();
    assert_eq!(
        stored_name,
        Some("ProtoName".to_string()),
        "Should use name from decoded proto"
    );
}
