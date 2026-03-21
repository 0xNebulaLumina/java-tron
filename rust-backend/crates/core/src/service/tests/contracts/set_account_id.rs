//! SetAccountIdContract tests.
//!
//! These tests verify Java parity for SET_ACCOUNT_ID_CONTRACT execution.
//! Key parity points tested:
//! 1. owner_address parsed from contract bytes (not transaction.from)
//! 2. Strict 21-byte address validation matching DecodeUtil.addressValid
//! 3. ASCII-only lowercasing for account ID index keys
//! 4. Validation order matches Java: accountId first, then ownerAddress
//! 5. Correct error strings for all validation failures

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, seed_dynamic_properties};
use revm_primitives::{Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{
    EngineBackedEvmStateStore, TronContractParameter, TronContractType, TronExecutionContext,
    TronTransaction, TxMetadata,
};

/// Helper to build a SetAccountIdContract protobuf
fn build_set_account_id_contract(account_id: &[u8], owner_address: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();

    // Field 1: account_id (bytes, wire type 2)
    if !account_id.is_empty() {
        encode_varint(&mut buf, (1 << 3) | 2);
        encode_varint(&mut buf, account_id.len() as u64);
        buf.extend_from_slice(account_id);
    }

    // Field 2: owner_address (bytes, wire type 2)
    if !owner_address.is_empty() {
        encode_varint(&mut buf, (2 << 3) | 2);
        encode_varint(&mut buf, owner_address.len() as u64);
        buf.extend_from_slice(owner_address);
    }

    buf
}

/// Create a TronContractParameter for SetAccountIdContract
fn make_contract_parameter(contract_data: Vec<u8>) -> TronContractParameter {
    TronContractParameter {
        type_url: "type.googleapis.com/protocol.SetAccountIdContract".to_string(),
        value: contract_data,
    }
}

/// Create a test service with set_account_id enabled
fn new_test_service() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            set_account_id_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Create a test context
fn new_test_context() -> TronExecutionContext {
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

/// Helper: create a storage adapter with owner account seeded
fn setup_storage_with_owner(
    temp_dir: &tempfile::TempDir,
    owner: &Address,
) -> EngineBackedEvmStateStore {
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let account = revm_primitives::AccountInfo {
        balance: U256::from(1_000_000_000_000i64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(*owner, account).unwrap();
    storage_adapter
}

// =====================================================================
// Fix B: owner_address parsed from contract bytes
// =====================================================================

/// Test successful SetAccountId execution (happy path).
#[test]
fn test_set_account_id_success() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    let account_id = b"myaccount123";
    let contract_data = build_set_account_id_contract(account_id, &owner_tron);
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);

    assert!(
        result.is_ok(),
        "SetAccountId should succeed: {:?}",
        result.err()
    );
    let exec_result = result.unwrap();
    assert!(exec_result.success);
    assert_eq!(exec_result.energy_used, 0);
}

/// Test that owner_address is extracted from contract bytes.
/// When contract owner_address differs from transaction.from, the contract bytes
/// determine which account is used (matching Java behavior).
#[test]
fn test_set_account_id_uses_contract_owner_address() {
    let temp_dir = tempfile::tempdir().unwrap();

    // The contract will reference owner_b, but transaction.from is owner_a
    let owner_a = Address::from([1u8; 20]);
    let owner_b = Address::from([2u8; 20]);
    let owner_b_tron = make_from_raw(&owner_b);

    // Only seed owner_b's account (the contract owner)
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_b);
    let service = new_test_service();

    let account_id = b"testaccount1";
    let contract_data = build_set_account_id_contract(account_id, &owner_b_tron);
    let contract_param = make_contract_parameter(contract_data);

    let transaction = TronTransaction {
        from: owner_a, // Different from contract.owner_address
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_a)),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);

    // Should succeed — it uses owner_b from contract bytes, not owner_a from tx.from
    assert!(
        result.is_ok(),
        "Should use contract owner_address, not tx.from: {:?}",
        result.err()
    );
}

// =====================================================================
// Fix C: Strict 21-byte address validation
// =====================================================================

/// Test that 20-byte owner_address in contract is rejected.
/// Java's DecodeUtil.addressValid requires exactly 21 bytes with 0x41 prefix.
#[test]
fn test_set_account_id_rejects_20_byte_owner_address() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    let account_id = b"testaccount1";
    // Use 20-byte address (without 0x41 prefix) — Java would reject this
    let contract_data = build_set_account_id_contract(account_id, owner_address.as_slice());
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_address.as_slice().to_vec()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err(), "Should reject 20-byte owner_address");
    assert_eq!(result.unwrap_err(), "Invalid ownerAddress");
}

/// Test that owner_address with wrong prefix is rejected.
#[test]
fn test_set_account_id_rejects_wrong_prefix() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    let account_id = b"testaccount1";
    // Use 21-byte address with wrong prefix (0x42 instead of 0x41)
    let mut wrong_prefix = vec![0x42u8];
    wrong_prefix.extend_from_slice(owner_address.as_slice());
    let contract_data = build_set_account_id_contract(account_id, &wrong_prefix);
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err(), "Should reject wrong address prefix");
    assert_eq!(result.unwrap_err(), "Invalid ownerAddress");
}

/// Test that empty owner_address is rejected.
#[test]
fn test_set_account_id_rejects_empty_owner_address() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    let account_id = b"testaccount1";
    // Build contract with no owner_address (empty bytes = proto3 default)
    let contract_data = build_set_account_id_contract(account_id, &[]);
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err(), "Should reject empty owner_address");
    assert_eq!(result.unwrap_err(), "Invalid ownerAddress");
}

// =====================================================================
// Validation order: accountId validated BEFORE ownerAddress
// =====================================================================

/// Test that invalid accountId is reported before invalid ownerAddress.
/// Java validates account_id first via TransactionUtil.validAccountId(),
/// then ownerAddress via DecodeUtil.addressValid().
#[test]
fn test_set_account_id_validates_account_id_before_owner_address() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    // Both account_id and owner_address are invalid.
    // Should get "Invalid accountId" (not "Invalid ownerAddress") per Java validation order.
    let bad_account_id = b"short"; // Too short (< 8 bytes)
    let bad_owner = vec![0x42u8; 21]; // Wrong prefix
    let contract_data = build_set_account_id_contract(bad_account_id, &bad_owner);
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "Invalid accountId",
        "accountId should be validated before ownerAddress"
    );
}

// =====================================================================
// Existing validation tests (parity error strings)
// =====================================================================

/// Test that account_id too short is rejected.
#[test]
fn test_set_account_id_too_short() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    let account_id = b"short12"; // 7 bytes, minimum is 8
    let contract_data = build_set_account_id_contract(account_id, &owner_tron);
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid accountId");
}

/// Test that account_id too long is rejected (> 32 bytes).
#[test]
fn test_set_account_id_too_long() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    // 33 bytes of readable ASCII
    let account_id = b"abcdefghijklmnopqrstuvwxyz1234567";
    assert_eq!(account_id.len(), 33);
    let contract_data = build_set_account_id_contract(account_id, &owner_tron);
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid accountId");
}

/// Test that account_id with space (0x20) is rejected.
#[test]
fn test_set_account_id_with_space_rejected() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    let account_id = b"test acct"; // Contains space (0x20), below valid range
    let contract_data = build_set_account_id_contract(account_id, &owner_tron);
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Invalid accountId");
}

/// Test that non-existent account returns correct error.
#[test]
fn test_set_account_id_account_not_existed() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service();

    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);
    // Do NOT seed the account — it should not exist

    let account_id = b"testaccount1";
    let contract_data = build_set_account_id_contract(account_id, &owner_tron);
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Account has not existed");
}

/// Test that duplicate account_id returns "This id has existed".
#[test]
fn test_set_account_id_duplicate_id() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_a = Address::from([1u8; 20]);
    let owner_a_tron = make_from_raw(&owner_a);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_a);
    let service = new_test_service();

    let account_id = b"myuniqueid12";

    // First, set account_id on owner_a (should succeed)
    let contract_data = build_set_account_id_contract(account_id, &owner_a_tron);
    let contract_param = make_contract_parameter(contract_data);

    let transaction = TronTransaction {
        from: owner_a,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_a_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "First SetAccountId should succeed: {:?}",
        result.err()
    );

    // Now create owner_b and try to use the same account_id
    let owner_b = Address::from([2u8; 20]);
    let owner_b_tron = make_from_raw(&owner_b);
    let account_b = revm_primitives::AccountInfo {
        balance: U256::from(1_000_000_000_000i64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_b, account_b).unwrap();

    let contract_data2 = build_set_account_id_contract(account_id, &owner_b_tron);
    let contract_param2 = make_contract_parameter(contract_data2);

    let transaction2 = TronTransaction {
        from: owner_b,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_b_tron.clone()),
            contract_parameter: Some(contract_param2),
            ..Default::default()
        },
    };

    let result2 =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction2, &context);

    assert!(result2.is_err());
    assert_eq!(result2.unwrap_err(), "This id has existed");
}

/// Test that setting account_id twice on the same account returns "This account id already set".
#[test]
fn test_set_account_id_already_set() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    let account_id = b"firstaccid12";
    let contract_data = build_set_account_id_contract(account_id, &owner_tron);
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "First SetAccountId should succeed: {:?}",
        result.err()
    );

    // Try to set a different account_id on the same account
    let account_id2 = b"secondaccid1";
    let contract_data2 = build_set_account_id_contract(account_id2, &owner_tron);
    let contract_param2 = make_contract_parameter(contract_data2);

    let transaction2 = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param2),
            ..Default::default()
        },
    };

    let result2 =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction2, &context);

    assert!(result2.is_err());
    assert_eq!(result2.unwrap_err(), "This account id already set");
}

/// Test wrong contract type error string matches Java.
#[test]
fn test_set_account_id_wrong_contract_type() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    let account_id = b"testaccount1";
    let contract_data = build_set_account_id_contract(account_id, &owner_tron);

    // Use wrong type_url
    let contract_param = TronContractParameter {
        type_url: "type.googleapis.com/protocol.TransferContract".to_string(),
        value: contract_data,
    };

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "contract type error,expected type [SetAccountIdContract],real type[class com.google.protobuf.Any]"
    );
}

// =====================================================================
// Fix D: ASCII-only lowercasing (case-insensitive uniqueness)
// =====================================================================

/// Test that account IDs are case-insensitively unique.
/// "MYACCOUNT123" and "myaccount123" should collide.
#[test]
fn test_set_account_id_case_insensitive_uniqueness() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_a = Address::from([1u8; 20]);
    let owner_a_tron = make_from_raw(&owner_a);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_a);
    let service = new_test_service();

    // Set "MYACCOUNT123" on owner_a
    let account_id_upper = b"MYACCOUNT123";
    let contract_data = build_set_account_id_contract(account_id_upper, &owner_a_tron);
    let contract_param = make_contract_parameter(contract_data);

    let transaction = TronTransaction {
        from: owner_a,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_a_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "First SetAccountId should succeed: {:?}",
        result.err()
    );

    // Now try "myaccount123" (lowercase) on owner_b — should fail
    let owner_b = Address::from([2u8; 20]);
    let owner_b_tron = make_from_raw(&owner_b);
    let account_b = revm_primitives::AccountInfo {
        balance: U256::from(1_000_000_000_000i64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_b, account_b).unwrap();

    let account_id_lower = b"myaccount123";
    let contract_data2 = build_set_account_id_contract(account_id_lower, &owner_b_tron);
    let contract_param2 = make_contract_parameter(contract_data2);

    let transaction2 = TronTransaction {
        from: owner_b,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_b_tron.clone()),
            contract_parameter: Some(contract_param2),
            ..Default::default()
        },
    };

    let result2 =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction2, &context);

    assert!(result2.is_err());
    assert_eq!(
        result2.unwrap_err(),
        "This id has existed",
        "Case-insensitive uniqueness: MYACCOUNT123 and myaccount123 should collide"
    );
}

/// Test account_id_key ASCII lowercasing directly.
/// Verify it lowercases only A-Z and leaves other ASCII untouched.
#[test]
fn test_account_id_key_ascii_lowercasing() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    let storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Mixed case
    let key = storage_adapter.account_id_key(b"AbCdEfGh");
    assert_eq!(key, b"abcdefgh");

    // All uppercase
    let key = storage_adapter.account_id_key(b"ALLUPPERCASEXX123456");
    assert_eq!(key, b"alluppercasexx123456");

    // Already lowercase
    let key = storage_adapter.account_id_key(b"alreadylower");
    assert_eq!(key, b"alreadylower");

    // Digits and special chars (should remain unchanged)
    let key = storage_adapter.account_id_key(b"Test!@#$%^&*()123");
    assert_eq!(key, b"test!@#$%^&*()123");

    // Boundary: only b'A' and b'Z'
    let key = storage_adapter.account_id_key(b"AZ");
    assert_eq!(key, b"az");
}

/// Test that has_account_id is case-insensitive via ASCII lowercasing.
#[test]
fn test_has_account_id_case_insensitive() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    let storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_bytes = vec![0x41u8; 21];

    // Store "ABC" (uppercase)
    storage_adapter
        .put_account_id_index(b"ABCDEFGH", &owner_bytes)
        .unwrap();

    // Should find it via "abcdefgh" (lowercase)
    assert!(
        storage_adapter.has_account_id(b"abcdefgh").unwrap(),
        "has_account_id('abcdefgh') should return true after put_account_id_index('ABCDEFGH')"
    );

    // Should find it via "AbCdEfGh" (mixed case)
    assert!(
        storage_adapter.has_account_id(b"AbCdEfGh").unwrap(),
        "has_account_id('AbCdEfGh') should return true"
    );

    // Should NOT find a different ID
    assert!(
        !storage_adapter.has_account_id(b"XYZXYZXY").unwrap(),
        "has_account_id('XYZXYZXY') should return false"
    );
}

/// Test that put_account_id_index("ABC") is retrievable via "abc".
#[test]
fn test_put_account_id_index_retrievable_via_lowercase() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    let storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_bytes = vec![0x41u8; 21];

    // Store with uppercase key
    storage_adapter
        .put_account_id_index(b"TESTIDXX", &owner_bytes)
        .unwrap();

    // Retrieve with lowercase key
    let retrieved = storage_adapter
        .get_address_by_account_id(b"testidxx")
        .unwrap();

    assert!(retrieved.is_some(), "Should retrieve via lowercase key");
    assert_eq!(retrieved.unwrap(), owner_bytes);
}

// =====================================================================
// Edge case: account_id at boundary lengths
// =====================================================================

/// Test minimum valid account_id length (8 bytes).
#[test]
fn test_set_account_id_min_length() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    let account_id = b"exactly8"; // exactly 8 bytes
    assert_eq!(account_id.len(), 8);
    let contract_data = build_set_account_id_contract(account_id, &owner_tron);
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "8-byte account_id should be valid: {:?}",
        result.err()
    );
}

/// Test maximum valid account_id length (32 bytes).
#[test]
fn test_set_account_id_max_length() {
    let temp_dir = tempfile::tempdir().unwrap();
    let owner_address = Address::from([1u8; 20]);
    let owner_tron = make_from_raw(&owner_address);
    let mut storage_adapter = setup_storage_with_owner(&temp_dir, &owner_address);
    let service = new_test_service();

    let account_id = b"abcdefghijklmnopqrstuvwxyz123456"; // exactly 32 bytes
    assert_eq!(account_id.len(), 32);
    let contract_data = build_set_account_id_contract(account_id, &owner_tron);
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
            contract_type: Some(TronContractType::SetAccountIdContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(contract_param),
            ..Default::default()
        },
    };

    let context = new_test_context();
    let result =
        service.execute_set_account_id_contract(&mut storage_adapter, &transaction, &context);
    assert!(
        result.is_ok(),
        "32-byte account_id should be valid: {:?}",
        result.err()
    );
}
