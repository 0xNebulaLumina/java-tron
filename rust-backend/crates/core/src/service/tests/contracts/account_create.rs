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

// -----------------------------------------------------------------------------
// Strict Dynamic-Property Missing-Key Tests
// -----------------------------------------------------------------------------

/// Helper function to create BackendService with account_create + strict_dynamic_properties
fn new_test_service_with_account_create_strict() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            account_create_enabled: true,
            strict_dynamic_properties: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Helper function to create BackendService with account_create + strict + AEXT tracking
fn new_test_service_with_account_create_strict_and_aext() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            account_create_enabled: true,
            strict_dynamic_properties: true,
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

/// Set up a common test storage with owner account, mainnet prefix, and a basic transaction.
/// Seeds ONLY the specified dynamic properties — caller controls which keys are present.
fn setup_strict_test_env(
    seed_props: impl FnOnce(&StorageEngine),
) -> (
    EngineBackedEvmStateStore,
    TronTransaction,
    Address,
) {
    let temp_dir = tempfile::tempdir().unwrap();
    // Leak the tempdir so it stays alive for the duration of the test
    let temp_dir = Box::leak(Box::new(temp_dir));
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed caller-specified properties
    seed_props(&storage_engine);

    // Set up mainnet prefix detection
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

    let owner_tron = make_tron_address_21(0x41, [0x11u8; 20]);
    let target_tron = make_tron_address_21(0x41, [0x99u8; 20]);
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
            contract_parameter: Some(TronContractParameter {
                type_url: "protocol.AccountCreateContract".to_string(),
                value: contract_data.to_vec(),
            }),
            ..Default::default()
        },
    };

    (storage_adapter, transaction, owner_address)
}

/// Seed all required dynamic properties for a successful account-create execution.
fn seed_all_account_create_props(se: &StorageEngine) {
    se.put("properties", b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", &0u64.to_be_bytes()).unwrap();
    se.put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
    se.put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &1i64.to_be_bytes()).unwrap();
    se.put("properties", b"latest_block_header_timestamp", &1000i64.to_be_bytes()).unwrap();
    se.put("properties", b"FREE_NET_LIMIT", &100000i64.to_be_bytes()).unwrap();
    se.put("properties", b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE", &1i64.to_be_bytes()).unwrap();
    se.put("properties", b"CREATE_ACCOUNT_FEE", &100000u64.to_be_bytes()).unwrap();
    se.put("properties", b"TOTAL_CREATE_ACCOUNT_COST", &0i64.to_be_bytes()).unwrap();
}

#[test]
fn test_strict_missing_create_new_account_fee_in_system_contract() {
    let (mut sa, tx, _) = setup_strict_test_env(|se| {
        // Seed everything EXCEPT CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT
        se.put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"latest_block_header_timestamp", &1000i64.to_be_bytes()).unwrap();
    });
    let service = new_test_service_with_account_create_strict();
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        err.contains("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT"),
        "Error should mention missing key: got '{}'", err
    );
}

#[test]
fn test_strict_missing_latest_block_header_timestamp() {
    let (mut sa, tx, _) = setup_strict_test_env(|se| {
        // Seed everything EXCEPT latest_block_header_timestamp
        se.put("properties", b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", &0u64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &1i64.to_be_bytes()).unwrap();
    });
    let service = new_test_service_with_account_create_strict();
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        err.contains("latest block header timestamp"),
        "Error should mention missing key: got '{}'", err
    );
}

#[test]
fn test_strict_missing_allow_multi_sign() {
    let (mut sa, tx, _) = setup_strict_test_env(|se| {
        // Seed everything EXCEPT ALLOW_MULTI_SIGN
        se.put("properties", b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", &0u64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"latest_block_header_timestamp", &1000i64.to_be_bytes()).unwrap();
    });
    let service = new_test_service_with_account_create_strict();
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        err.contains("ALLOW_MULTI_SIGN"),
        "Error should mention missing key: got '{}'", err
    );
}

#[test]
fn test_strict_missing_allow_blackhole_optimization() {
    let (mut sa, tx, _) = setup_strict_test_env(|se| {
        // Seed everything EXCEPT ALLOW_BLACKHOLE_OPTIMIZATION
        // Need fee > 0 to trigger blackhole read
        se.put("properties", b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", &1000000u64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"latest_block_header_timestamp", &1000i64.to_be_bytes()).unwrap();
    });
    let service = new_test_service_with_account_create_strict();
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        err.contains("ALLOW_BLACKHOLE_OPTIMIZATION"),
        "Error should mention missing key: got '{}'", err
    );
}

// --- Tracked-bandwidth strict missing-key tests ---

#[test]
fn test_strict_tracked_missing_free_net_limit() {
    let (mut sa, tx, _) = setup_strict_test_env(|se| {
        // Seed all actuator-path keys but NOT FREE_NET_LIMIT
        se.put("properties", b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", &0u64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"latest_block_header_timestamp", &1000i64.to_be_bytes()).unwrap();
        se.put("properties", b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE", &1i64.to_be_bytes()).unwrap();
    });
    let service = new_test_service_with_account_create_strict_and_aext();
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        err.contains("FREE_NET_LIMIT"),
        "Error should mention missing key: got '{}'", err
    );
}

#[test]
fn test_strict_tracked_missing_create_new_account_bandwidth_rate() {
    let (mut sa, tx, _) = setup_strict_test_env(|se| {
        // Seed all actuator-path keys but NOT CREATE_NEW_ACCOUNT_BANDWIDTH_RATE
        se.put("properties", b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", &0u64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"latest_block_header_timestamp", &1000i64.to_be_bytes()).unwrap();
        se.put("properties", b"FREE_NET_LIMIT", &100000i64.to_be_bytes()).unwrap();
    });
    let service = new_test_service_with_account_create_strict_and_aext();
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        err.contains("CREATE_NEW_ACCOUNT_BANDWIDTH_RATE"),
        "Error should mention missing key: got '{}'", err
    );
}

#[test]
fn test_strict_tracked_missing_create_account_fee() {
    let (mut sa, tx, _) = setup_strict_test_env(|se| {
        // Seed all keys but NOT CREATE_ACCOUNT_FEE; force fee path with FREE_NET_LIMIT=0
        se.put("properties", b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", &0u64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"latest_block_header_timestamp", &1000i64.to_be_bytes()).unwrap();
        se.put("properties", b"FREE_NET_LIMIT", &0i64.to_be_bytes()).unwrap();
        se.put("properties", b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"TOTAL_CREATE_ACCOUNT_COST", &0i64.to_be_bytes()).unwrap();
    });
    let service = new_test_service_with_account_create_strict_and_aext();
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        err.contains("CREATE_ACCOUNT_FEE"),
        "Error should mention missing key: got '{}'", err
    );
}

#[test]
fn test_strict_tracked_missing_total_create_account_cost() {
    let (mut sa, tx, _) = setup_strict_test_env(|se| {
        // Seed all keys but NOT TOTAL_CREATE_ACCOUNT_COST; force fee path with FREE_NET_LIMIT=0
        se.put("properties", b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", &0u64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"latest_block_header_timestamp", &1000i64.to_be_bytes()).unwrap();
        se.put("properties", b"FREE_NET_LIMIT", &0i64.to_be_bytes()).unwrap();
        se.put("properties", b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"CREATE_ACCOUNT_FEE", &100000u64.to_be_bytes()).unwrap();
    });
    let service = new_test_service_with_account_create_strict_and_aext();
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(
        err.contains("TOTAL_CREATE_ACCOUNT_COST"),
        "Error should mention missing key: got '{}'", err
    );
}

// --- Non-strict short-value decode regression tests ---
// These verify that present-but-short DB values are decoded via Java-parity
// helpers (decode_u64_java / decode_i64_java) rather than falling back to defaults.

/// Helper: create a minimal storage adapter with only the specified dynamic property set.
fn make_adapter_with_prop(key: &[u8], value: &[u8]) -> EngineBackedEvmStateStore {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_dir = Box::leak(Box::new(temp_dir));
    let se = StorageEngine::new(temp_dir.path()).unwrap();
    se.put("properties", key, value).unwrap();
    EngineBackedEvmStateStore::new(se)
}

/// Helper: create a minimal storage adapter with NO dynamic properties.
fn make_adapter_without_props() -> EngineBackedEvmStateStore {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_dir = Box::leak(Box::new(temp_dir));
    let se = StorageEngine::new(temp_dir.path()).unwrap();
    EngineBackedEvmStateStore::new(se)
}

#[test]
fn test_nonstrict_short_value_create_new_account_fee_in_system_contract() {
    // 4-byte value: 0x00000005 → should decode to 5, not fall back to default 1_000_000
    let sa = make_adapter_with_prop(
        b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
        &5u32.to_be_bytes(),
    );
    let fee = sa.get_create_new_account_fee_in_system_contract().unwrap();
    assert_eq!(fee, 5, "Short 4-byte value should decode to 5 via Java parity, not default");
}

#[test]
fn test_nonstrict_empty_value_create_new_account_fee_in_system_contract() {
    // Empty value → Java's ByteArray.toLong returns 0
    let sa = make_adapter_with_prop(b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", &[]);
    let fee = sa.get_create_new_account_fee_in_system_contract().unwrap();
    assert_eq!(fee, 0, "Empty value should decode to 0 via Java parity");
}

#[test]
fn test_nonstrict_absent_create_new_account_fee_in_system_contract() {
    // Key absent → should use default 1_000_000
    let sa = make_adapter_without_props();
    let fee = sa.get_create_new_account_fee_in_system_contract().unwrap();
    assert_eq!(fee, 1_000_000, "Absent key should use default 1_000_000");
}

#[test]
fn test_nonstrict_short_value_create_new_account_bandwidth_rate() {
    // 3-byte value: [0x00, 0x00, 0x07] → should decode to 7
    let sa = make_adapter_with_prop(
        b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE",
        &[0x00, 0x00, 0x07],
    );
    let rate = sa.get_create_new_account_bandwidth_rate().unwrap();
    assert_eq!(rate, 7, "Short 3-byte value should decode to 7 via Java parity, not default");
}

#[test]
fn test_nonstrict_empty_value_create_new_account_bandwidth_rate() {
    let sa = make_adapter_with_prop(b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE", &[]);
    let rate = sa.get_create_new_account_bandwidth_rate().unwrap();
    assert_eq!(rate, 0, "Empty value should decode to 0 via Java parity");
}

#[test]
fn test_nonstrict_absent_create_new_account_bandwidth_rate() {
    let sa = make_adapter_without_props();
    let rate = sa.get_create_new_account_bandwidth_rate().unwrap();
    assert_eq!(rate, 1, "Absent key should use default 1");
}

#[test]
fn test_nonstrict_short_value_create_account_fee() {
    // 2-byte value: [0x01, 0xF4] = 500 → should decode to 500
    let sa = make_adapter_with_prop(b"CREATE_ACCOUNT_FEE", &[0x01, 0xF4]);
    let fee = sa.get_create_account_fee().unwrap();
    assert_eq!(fee, 500, "Short 2-byte value should decode to 500 via Java parity, not default");
}

#[test]
fn test_nonstrict_empty_value_create_account_fee() {
    let sa = make_adapter_with_prop(b"CREATE_ACCOUNT_FEE", &[]);
    let fee = sa.get_create_account_fee().unwrap();
    assert_eq!(fee, 0, "Empty value should decode to 0 via Java parity");
}

#[test]
fn test_nonstrict_absent_create_account_fee() {
    let sa = make_adapter_without_props();
    let fee = sa.get_create_account_fee().unwrap();
    assert_eq!(fee, 100_000, "Absent key should use default 100_000");
}

#[test]
fn test_nonstrict_full_8byte_values_still_work() {
    // Full 8-byte values should still decode correctly
    let sa = make_adapter_with_prop(
        b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
        &42u64.to_be_bytes(),
    );
    assert_eq!(sa.get_create_new_account_fee_in_system_contract().unwrap(), 42);

    let sa = make_adapter_with_prop(
        b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE",
        &99i64.to_be_bytes(),
    );
    assert_eq!(sa.get_create_new_account_bandwidth_rate().unwrap(), 99);

    let sa = make_adapter_with_prop(b"CREATE_ACCOUNT_FEE", &200000u64.to_be_bytes());
    assert_eq!(sa.get_create_account_fee().unwrap(), 200000);
}

// --- >8-byte decode parity tests ---
// Java's `ByteArray.toLong` uses `new BigInteger(1, b).longValue()`, which
// interprets the full array as unsigned big-endian then truncates to the low
// 64 bits — equivalent to taking the **last** 8 bytes.  These tests lock that
// behaviour for both the signed (decode_i64_java) and unsigned (decode_u64_java)
// paths.

#[test]
fn test_long_value_u64_takes_last_8_bytes() {
    // 10-byte value: [0xFF, 0xEE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2A]
    // Last 8 bytes: [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2A] = 42
    let value: &[u8] = &[0xFF, 0xEE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2A];
    let sa = make_adapter_with_prop(b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", value);
    assert_eq!(
        sa.get_create_new_account_fee_in_system_contract().unwrap(),
        42,
        ">8-byte value should use last 8 bytes (u64 path)"
    );
}

#[test]
fn test_long_value_i64_takes_last_8_bytes() {
    // 9-byte value: [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x63]
    // Last 8 bytes: [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x63] = 99
    let value: &[u8] = &[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x63];
    let sa = make_adapter_with_prop(b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE", value);
    assert_eq!(
        sa.get_create_new_account_bandwidth_rate().unwrap(),
        99,
        ">8-byte value should use last 8 bytes (i64 path)"
    );
}

#[test]
fn test_long_value_u64_high_bits_ignored() {
    // 10-byte value where leading bytes are large but last 8 bytes form a small number.
    // Leading [0xAB, 0xCD] should be ignored.
    let value: &[u8] = &[0xAB, 0xCD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x86, 0xA0];
    // Last 8: [0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x86, 0xA0] = 100000
    let sa = make_adapter_with_prop(b"CREATE_ACCOUNT_FEE", value);
    assert_eq!(
        sa.get_create_account_fee().unwrap(),
        100_000,
        ">8-byte value with high leading bytes should use last 8 bytes (u64 path)"
    );
}

#[test]
fn test_long_value_i64_signed_last_8_bytes() {
    // 9-byte value: last 8 bytes represent a negative i64 (high bit set)
    // [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
    // Last 8: [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF] = -1 as i64
    let value: &[u8] = &[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let sa = make_adapter_with_prop(b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE", value);
    assert_eq!(
        sa.get_create_new_account_bandwidth_rate().unwrap(),
        -1,
        ">8-byte value with signed last-8-bytes should decode correctly (i64 path)"
    );
}

// --- Negative-value rejection tests ---
// Java parity: fee getters decode via i64 and cast to u64 without rejecting
// negative values.  Java's ByteArray.toLong returns signed long and the
// DynamicPropertiesStore getters pass the value through unchanged.

#[test]
fn test_negative_fee_create_new_account_fee_accepted() {
    // 0x80..00 → i64::MIN → cast to u64 = 9223372036854775808
    let value: &[u8] = &[0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let sa = make_adapter_with_prop(b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", value);
    let result = sa.get_create_new_account_fee_in_system_contract();
    assert_eq!(result.unwrap(), i64::MIN as u64);
}

#[test]
fn test_negative_fee_create_account_fee_accepted() {
    // All-ones → i64 = -1 → cast to u64 = u64::MAX
    let value: &[u8] = &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let sa = make_adapter_with_prop(b"CREATE_ACCOUNT_FEE", value);
    let result = sa.get_create_account_fee();
    assert_eq!(result.unwrap(), u64::MAX);
}

#[test]
fn test_negative_fee_strict_mode_accepted() {
    // Strict getter also accepts negative i64 values (Java parity)
    let value: &[u8] = &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let sa = make_adapter_with_prop(b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", value);
    let result = sa.get_create_new_account_fee_in_system_contract_strict();
    assert_eq!(result.unwrap(), u64::MAX);
}

#[test]
fn test_negative_fee_account_upgrade_cost_accepted() {
    // 0x80..00 → i64::MIN → cast to u64
    let value: &[u8] = &[0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let sa = make_adapter_with_prop(b"ACCOUNT_UPGRADE_COST", value);
    let result = sa.get_account_upgrade_cost();
    assert_eq!(result.unwrap(), i64::MIN as u64);
}

#[test]
fn test_negative_fee_asset_issue_fee_accepted() {
    // All-ones → i64 = -1 → cast to u64 = u64::MAX
    let value: &[u8] = &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let sa = make_adapter_with_prop(b"ASSET_ISSUE_FEE", value);
    let result = sa.get_asset_issue_fee();
    assert_eq!(result.unwrap(), u64::MAX);
}

// --- Control tests: non-strict mode still falls back ---

#[test]
fn test_nonstrict_missing_keys_still_succeeds() {
    // With strict=false (default), missing keys use defaults — should succeed
    let (mut sa, tx, _) = setup_strict_test_env(|se| {
        // Seed ONLY ALLOW_MULTI_SIGN (already strict in all modes)
        se.put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
        // All other keys are missing — non-strict mode should default them
    });
    let service = new_test_service_with_account_create_enabled(); // strict=false
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(
        result.is_ok(),
        "Non-strict mode should succeed with missing keys: {:?}",
        result.err()
    );
}

#[test]
fn test_strict_all_keys_present_succeeds() {
    // With strict=true but all keys present, should succeed
    let (mut sa, tx, _) = setup_strict_test_env(seed_all_account_create_props);
    let service = new_test_service_with_account_create_strict();
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(
        result.is_ok(),
        "Strict mode should succeed when all keys are present: {:?}",
        result.err()
    );
}

#[test]
fn test_strict_fee_zero_missing_blackhole_key_fails() {
    // Java's CreateAccountActuator.execute() reads supportBlackHoleOptimization()
    // unconditionally — even when fee=0.  If the key is missing, Java throws
    // IllegalArgumentException.  Strict mode must mirror this failure.
    let (mut sa, tx, _) = setup_strict_test_env(|se| {
        // fee = 0 (CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT = 0)
        se.put("properties", b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", &0u64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
        // Deliberately omit ALLOW_BLACKHOLE_OPTIMIZATION
        se.put("properties", b"latest_block_header_timestamp", &1000i64.to_be_bytes()).unwrap();
        se.put("properties", b"FREE_NET_LIMIT", &100000i64.to_be_bytes()).unwrap();
        se.put("properties", b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"CREATE_ACCOUNT_FEE", &100000u64.to_be_bytes()).unwrap();
        se.put("properties", b"TOTAL_CREATE_ACCOUNT_COST", &0i64.to_be_bytes()).unwrap();
    });
    let service = new_test_service_with_account_create_strict();
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(
        result.is_err(),
        "Strict mode with fee=0 should fail when ALLOW_BLACKHOLE_OPTIMIZATION is missing (Java parity)"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("ALLOW_BLACKHOLE_OPTIMIZATION"),
        "Error should reference the missing ALLOW_BLACKHOLE_OPTIMIZATION key, got: {}",
        err
    );
}

#[test]
fn test_nonstrict_fee_zero_missing_blackhole_key_succeeds() {
    // In non-strict mode the blackhole flag is read unconditionally (matching
    // Java's control flow), but missing keys default to false instead of
    // erroring.  With fee=0 the flag value is unused, so the test still passes.
    let (mut sa, tx, _) = setup_strict_test_env(|se| {
        se.put("properties", b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT", &0u64.to_be_bytes()).unwrap();
        se.put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes()).unwrap();
        // Deliberately omit ALLOW_BLACKHOLE_OPTIMIZATION
        se.put("properties", b"latest_block_header_timestamp", &1000i64.to_be_bytes()).unwrap();
        se.put("properties", b"FREE_NET_LIMIT", &100000i64.to_be_bytes()).unwrap();
        se.put("properties", b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE", &1i64.to_be_bytes()).unwrap();
        se.put("properties", b"CREATE_ACCOUNT_FEE", &100000u64.to_be_bytes()).unwrap();
        se.put("properties", b"TOTAL_CREATE_ACCOUNT_COST", &0i64.to_be_bytes()).unwrap();
    });
    let service = new_test_service_with_account_create_enabled();
    let result = service.execute_account_create_contract(&mut sa, &tx, &new_test_context());
    assert!(
        result.is_ok(),
        "Non-strict mode with fee=0 should succeed even when ALLOW_BLACKHOLE_OPTIMIZATION is missing: {:?}",
        result.err()
    );
}
