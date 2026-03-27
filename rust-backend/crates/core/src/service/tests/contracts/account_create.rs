//! AccountCreateContract tests.

use super::super::super::*;
use super::common::{encode_varint, new_test_context, seed_dynamic_properties};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{EngineBackedEvmStateStore, TronContractParameter, TronTransaction, TxMetadata};
use tron_backend_storage::StorageEngine;

/// Helper function to create BackendService with account_create_enabled
fn new_test_service_with_account_create_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            account_create_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Helper function to create BackendService with account_create and AEXT tracking enabled
fn new_test_service_with_account_create_and_aext() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            account_create_enabled: true,
            accountinfo_aext_mode: "tracked".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Build AccountCreateContract protobuf data
/// Field 1: owner_address (bytes, 21-byte TRON address)
/// Field 2: account_address (bytes, 21-byte TRON address - target to create)
/// Field 3: type (varint, AccountType enum - optional)
fn build_account_create_contract_data(
    owner_address: &[u8],
    account_address: &[u8],
    account_type: Option<i32>,
) -> Bytes {
    let mut data = Vec::new();

    // Field 1: owner_address (tag = 0x0a, wire type 2 = length-delimited)
    data.push(0x0a);
    encode_varint(&mut data, owner_address.len() as u64);
    data.extend_from_slice(owner_address);

    // Field 2: account_address (tag = 0x12, wire type 2 = length-delimited)
    data.push(0x12);
    encode_varint(&mut data, account_address.len() as u64);
    data.extend_from_slice(account_address);

    // Field 3: type (tag = 0x18, wire type 0 = varint) - optional
    if let Some(t) = account_type {
        data.push(0x18);
        encode_varint(&mut data, t as u64);
    }

    Bytes::from(data)
}

/// Helper to create a 21-byte TRON address with given prefix
fn make_tron_address_21(prefix: u8, base: [u8; 20]) -> Vec<u8> {
    let mut addr = vec![prefix];
    addr.extend_from_slice(&base);
    addr
}

// -----------------------------------------------------------------------------
// Address Validation Tests
// -----------------------------------------------------------------------------

#[test]
fn test_account_create_reject_wrong_prefix_owner_address() {
    // Set up mainnet storage (prefix 0x41) by inserting a mainnet address
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Insert a mainnet account to set the detected prefix to 0x41
    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Verify prefix is detected as mainnet
    assert_eq!(
        storage_adapter.address_prefix(),
        0x41,
        "Should detect mainnet prefix"
    );

    // Set up owner account with mainnet prefix
    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Build contract with TESTNET prefix (0xa0) for owner - should be rejected
    let wrong_prefix_owner = make_tron_address_21(0xa0, [0x11u8; 20]);
    let target_address = make_tron_address_21(0x41, [0x22u8; 20]);
    let contract_data =
        build_account_create_contract_data(&wrong_prefix_owner, &target_address, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountCreateContract".to_string(), value: contract_data.to_vec() }),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    assert!(result.is_err(), "Should reject wrong prefix owner address");
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress");
}

#[test]
fn test_account_create_reject_wrong_prefix_target_address() {
    // Set up mainnet storage (prefix 0x41)
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    assert_eq!(storage_adapter.address_prefix(), 0x41);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Build contract with correct owner but TESTNET prefix (0xa0) for target
    let correct_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    let wrong_prefix_target = make_tron_address_21(0xa0, [0x22u8; 20]);
    let contract_data =
        build_account_create_contract_data(&correct_owner, &wrong_prefix_target, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountCreateContract".to_string(), value: contract_data.to_vec() }),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    assert!(result.is_err(), "Should reject wrong prefix target address");
    assert_eq!(result.err().unwrap(), "Invalid account address");
}

#[test]
fn test_account_create_reject_wrong_length_owner_address() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Test 20-byte owner address (too short)
    let short_owner = vec![0x41u8; 20]; // Missing one byte
    let target_address = make_tron_address_21(0x41, [0x22u8; 20]);
    let contract_data = build_account_create_contract_data(&short_owner, &target_address, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountCreateContract".to_string(), value: contract_data.to_vec() }),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    assert!(result.is_err(), "Should reject 20-byte owner address");
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress");
}

#[test]
fn test_account_create_reject_wrong_length_target_address() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Test 22-byte target address (too long)
    let correct_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    let long_target = vec![0x41u8; 22];
    let contract_data = build_account_create_contract_data(&correct_owner, &long_target, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountCreateContract".to_string(), value: contract_data.to_vec() }),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    assert!(result.is_err(), "Should reject 22-byte target address");
    assert_eq!(result.err().unwrap(), "Invalid account address");
}

// -----------------------------------------------------------------------------
// Contract Type Field Tests
// -----------------------------------------------------------------------------

#[test]
fn test_account_create_type_normal_default() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Set fee to 0 to simplify test
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &0u64.to_be_bytes(),
        )
        .unwrap();
    seed_dynamic_properties(&storage_engine);

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Create account without specifying type (should default to Normal = 0)
    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x22u8; 20]);
    let contract_data = build_account_create_contract_data(&owner_tron, &target_tron, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountCreateContract".to_string(), value: contract_data.to_vec() }),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(
        result.is_ok(),
        "Account create should succeed: {:?}",
        result.err()
    );

    // Verify the created account has type = 0 (Normal)
    let target_address = Address::from([0x22u8; 20]);
    let target_proto = storage_adapter.get_account_proto(&target_address).unwrap();
    assert!(target_proto.is_some(), "Target account should exist");
    let proto = target_proto.unwrap();
    assert_eq!(
        proto.r#type, 0,
        "Account type should be Normal (0) by default"
    );
}

#[test]
fn test_account_create_type_contract_persisted() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &0u64.to_be_bytes(),
        )
        .unwrap();
    seed_dynamic_properties(&storage_engine);

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    // Create account with type = 1 (Contract)
    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x33u8; 20]);
    let contract_data = build_account_create_contract_data(&owner_tron, &target_tron, Some(1));

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountCreateContract".to_string(), value: contract_data.to_vec() }),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(
        result.is_ok(),
        "Account create with type=Contract should succeed: {:?}",
        result.err()
    );

    // Verify the created account has type = 1 (Contract)
    let target_address = Address::from([0x33u8; 20]);
    let target_proto = storage_adapter.get_account_proto(&target_address).unwrap();
    assert!(target_proto.is_some(), "Target account should exist");
    let proto = target_proto.unwrap();
    assert_eq!(proto.r#type, 1, "Account type should be Contract (1)");
}

// -----------------------------------------------------------------------------
// Resource Path Tests (Bandwidth / Fee Fallback)
// -----------------------------------------------------------------------------

#[test]
fn test_account_create_bandwidth_path_free_net() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Set up dynamic properties
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &0u64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"FREE_NET_LIMIT",
            &100000i64.to_be_bytes(), // Large free net limit
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE",
            &1i64.to_be_bytes(), // 1x multiplier
        )
        .unwrap();
    seed_dynamic_properties(&storage_engine);

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_and_aext();

    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x44u8; 20]);
    let contract_data = build_account_create_contract_data(&owner_tron, &target_tron, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountCreateContract".to_string(), value: contract_data.to_vec() }),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(
        result.is_ok(),
        "Account create should succeed: {:?}",
        result.err()
    );
    let exec_result = result.unwrap();

    // Verify AEXT tracking happened
    assert!(
        exec_result.aext_map.contains_key(&owner_address),
        "AEXT map should contain owner"
    );
    let (before_aext, after_aext) = &exec_result.aext_map[&owner_address];

    // With large free_net_limit and small tx size, should use FREE_NET path
    // free_net_usage should increase
    assert!(
        after_aext.free_net_usage > before_aext.free_net_usage,
        "Free net usage should increase when using FREE_NET path"
    );
}

#[test]
fn test_account_create_fee_fallback_updates_total_cost() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Set up dynamic properties with very small free_net_limit to force fee path
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &0u64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"FREE_NET_LIMIT",
            &0i64.to_be_bytes(), // Zero free net - forces fee path
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"CREATE_ACCOUNT_FEE",
            &100000u64.to_be_bytes(), // 0.1 TRX fallback fee
        )
        .unwrap();
    seed_dynamic_properties(&storage_engine);

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    // Check initial TOTAL_CREATE_ACCOUNT_COST
    let initial_cost = storage_adapter.get_total_create_account_cost().unwrap();
    assert_eq!(
        initial_cost, 0,
        "Initial TOTAL_CREATE_ACCOUNT_COST should be 0"
    );

    let service = new_test_service_with_account_create_and_aext();

    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x55u8; 20]);
    let contract_data = build_account_create_contract_data(&owner_tron, &target_tron, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountCreateContract".to_string(), value: contract_data.to_vec() }),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(
        result.is_ok(),
        "Account create should succeed: {:?}",
        result.err()
    );

    // Verify TOTAL_CREATE_ACCOUNT_COST was incremented
    let final_cost = storage_adapter.get_total_create_account_cost().unwrap();
    assert_eq!(
        final_cost, 100000,
        "TOTAL_CREATE_ACCOUNT_COST should be incremented by CREATE_ACCOUNT_FEE"
    );
}

#[test]
fn test_account_create_receipt_contains_fee() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Set actuator fee
    let actuator_fee: u64 = 1_000_000; // 1 TRX
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &actuator_fee.to_be_bytes(),
        )
        .unwrap();
    seed_dynamic_properties(&storage_engine);

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let service = new_test_service_with_account_create_enabled();

    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x66u8; 20]);
    let contract_data = build_account_create_contract_data(&owner_tron, &target_tron, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountCreateContract".to_string(), value: contract_data.to_vec() }),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );
    assert!(
        result.is_ok(),
        "Account create should succeed: {:?}",
        result.err()
    );
    let exec_result = result.unwrap();

    // Verify receipt passthrough is present and contains fee
    assert!(
        exec_result.tron_transaction_result.is_some(),
        "tron_transaction_result should be set for receipt passthrough"
    );

    let receipt_bytes = exec_result.tron_transaction_result.unwrap();
    assert!(
        !receipt_bytes.is_empty(),
        "Receipt bytes should not be empty"
    );

    // Parse the receipt to verify fee field
    // Field 1 in Transaction.Result is 'fee' (int64, wire type 0 = varint)
    // The receipt should start with tag 0x08 (field 1, wire type 0)
    assert_eq!(
        receipt_bytes[0], 0x08,
        "Receipt should start with fee field tag"
    );
}

#[test]
fn test_account_create_insufficient_bandwidth_and_balance() {
    // Test case: bandwidth is insufficient AND owner doesn't have enough TRX for CREATE_ACCOUNT_FEE
    // Should return error matching Java: "account [%s] has insufficient bandwidth[%d] and balance[%d] to create new account"

    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Set up dynamic properties:
    // - Zero actuator fee (so validation passes at step 5)
    // - Zero free net limit (forces fee path)
    // - High CREATE_ACCOUNT_FEE (higher than owner balance)
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &0u64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"FREE_NET_LIMIT",
            &0i64.to_be_bytes(), // Zero free net - forces fee path
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE",
            &1i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"CREATE_ACCOUNT_FEE",
            &1_000_000_000u64.to_be_bytes(), // 1000 TRX - higher than owner balance
        )
        .unwrap();
    seed_dynamic_properties(&storage_engine);

    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Owner has low balance (less than CREATE_ACCOUNT_FEE)
    let owner_address = Address::from([0x11u8; 20]);
    let owner_balance: u64 = 100_000; // Only 0.1 TRX, not enough for 1000 TRX fee
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

    let service = new_test_service_with_account_create_and_aext();

    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x77u8; 20]);
    let contract_data = build_account_create_contract_data(&owner_tron, &target_tron, None);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::AccountCreateContract),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.AccountCreateContract".to_string(), value: contract_data.to_vec() }),
            ..Default::default()
        },
    };

    let result = service.execute_account_create_contract(
        &mut storage_adapter,
        &transaction,
        &new_test_context(),
    );

    // Should fail with the Java-parity error message
    assert!(
        result.is_err(),
        "Should fail when bandwidth and balance are both insufficient"
    );
    let error_msg = result.err().unwrap();

    // Verify error message matches Java format
    assert!(
        error_msg.contains("has insufficient bandwidth")
            && error_msg.contains("and balance")
            && error_msg.contains("to create new account"),
        "Error should match Java format: got '{}'",
        error_msg
    );
    assert!(
        error_msg.contains(&format!("{}", owner_balance)),
        "Error should include owner balance: got '{}'",
        error_msg
    );
}
