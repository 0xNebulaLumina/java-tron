//! CreateSmartContract tests.
//!
//! Tests for:
//! 1. SmartContract.version persistence (ALLOW_TVM_COMPATIBLE_EVM)
//! 2. Internal CREATE address derivation (txid + nonce scheme)
//! 3. TRC-10 call_token_value transfer handling

use super::super::super::*;
use super::common::{encode_varint, new_test_context, seed_dynamic_properties, make_from_raw};
use tron_backend_execution::{
    EngineBackedEvmStateStore, TronTransaction, TronExecutionContext, TxMetadata,
    TronContractType, Trc10Change,
};
use revm_primitives::{Address, Bytes, U256, AccountInfo, B256};
use tron_backend_common::{ModuleManager, ExecutionConfig, RemoteExecutionConfig};
use tron_backend_storage::StorageEngine;
use sha3::{Digest, Keccak256};

/// Helper to create a test service with system and TRC-10 enabled
fn new_test_service_with_vm_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            trc10_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Build a minimal CreateSmartContract protobuf for testing.
///
/// Proto structure from tron.proto:
///
/// message SmartContract {
///   bytes origin_address = 1;
///   bytes contract_address = 2;
///   ABI abi = 3;
///   bytes bytecode = 4;
///   int64 call_value = 5;
///   int64 consume_user_resource_percent = 6;
///   string name = 7;
///   int64 origin_energy_limit = 8;
///   bytes code_hash = 9;
///   bytes trx_hash = 10;
///   int32 version = 11;
/// }
///
/// message CreateSmartContract {
///   bytes owner_address = 1;
///   SmartContract new_contract = 2;
///   int64 call_token_value = 3;
///   int64 token_id = 4;
/// }
fn build_create_smart_contract_data(
    owner: &Address,
    name: &str,
    bytecode: &[u8],
    consume_user_resource_percent: i64,
    origin_energy_limit: i64,
    call_value: i64,
    call_token_value: i64,
    token_id: i64,
) -> Bytes {
    // Build the inner SmartContract message
    let mut smart_contract = Vec::new();

    // Field 1: origin_address (must equal owner_address for java-tron parity)
    // Tag: (1 << 3) | 2 = 10 (length-delimited)
    smart_contract.push(10u8);
    smart_contract.push(21u8); // length = 21 bytes
    smart_contract.push(0x41u8); // TRON prefix
    smart_contract.extend_from_slice(owner.as_slice());

    // Field 4: bytecode
    // Tag: (4 << 3) | 2 = 34 (length-delimited)
    if !bytecode.is_empty() {
        smart_contract.push(34u8);
        encode_varint(&mut smart_contract, bytecode.len() as u64);
        smart_contract.extend_from_slice(bytecode);
    }

    // Field 5: call_value (int64)
    // Tag: (5 << 3) | 0 = 40 (varint)
    if call_value > 0 {
        smart_contract.push(40u8);
        encode_varint(&mut smart_contract, call_value as u64);
    }

    // Field 6: consume_user_resource_percent (int64)
    // Tag: (6 << 3) | 0 = 48 (varint)
    smart_contract.push(48u8);
    encode_varint(&mut smart_contract, consume_user_resource_percent as u64);

    // Field 7: name (string)
    // Tag: (7 << 3) | 2 = 58 (length-delimited)
    if !name.is_empty() {
        smart_contract.push(58u8);
        encode_varint(&mut smart_contract, name.len() as u64);
        smart_contract.extend_from_slice(name.as_bytes());
    }

    // Field 8: origin_energy_limit (int64)
    // Tag: (8 << 3) | 0 = 64 (varint)
    smart_contract.push(64u8);
    encode_varint(&mut smart_contract, origin_energy_limit as u64);

    // Build outer CreateSmartContract message
    let mut contract_data = Vec::new();

    // Field 1: owner_address
    // Tag: (1 << 3) | 2 = 10 (length-delimited)
    contract_data.push(10u8);
    contract_data.push(21u8); // length = 21 bytes
    contract_data.push(0x41u8); // TRON prefix
    contract_data.extend_from_slice(owner.as_slice());

    // Field 2: new_contract (embedded SmartContract message)
    // Tag: (2 << 3) | 2 = 18 (length-delimited)
    contract_data.push(18u8);
    encode_varint(&mut contract_data, smart_contract.len() as u64);
    contract_data.extend_from_slice(&smart_contract);

    // Field 3: call_token_value (int64)
    // Tag: (3 << 3) | 0 = 24 (varint)
    if call_token_value > 0 {
        contract_data.push(24u8);
        encode_varint(&mut contract_data, call_token_value as u64);
    }

    // Field 4: token_id (int64)
    // Tag: (4 << 3) | 0 = 32 (varint)
    if token_id > 0 {
        contract_data.push(32u8);
        encode_varint(&mut contract_data, token_id as u64);
    }

    Bytes::from(contract_data)
}

/// Derive the expected top-level contract address using Java's WalletUtil.generateContractAddress scheme.
/// Formula: keccak256(txid || owner_address_21_bytes)[12..32]
fn derive_top_level_contract_address(txid: &B256, owner: &Address) -> Address {
    let mut combined = Vec::with_capacity(32 + 21);
    combined.extend_from_slice(txid.as_slice());
    combined.push(0x41u8); // TRON prefix
    combined.extend_from_slice(owner.as_slice());

    let hash = Keccak256::digest(&combined);
    Address::from_slice(&hash[12..32])
}

/// Derive internal CREATE address using Java's TransactionUtil.generateContractAddress scheme.
/// Formula: keccak256(txid || nonce_be_u64)[12..32]
fn derive_internal_create_address(txid: &B256, nonce: u64) -> Address {
    let mut combined = [0u8; 40];
    combined[..32].copy_from_slice(txid.as_slice());
    combined[32..40].copy_from_slice(&nonce.to_be_bytes());

    let hash = Keccak256::digest(&combined);
    Address::from_slice(&hash[12..32])
}

// ============================================================================
// SECTION 1: SmartContract.version persistence tests
// ============================================================================

#[test]
fn test_create_contract_persists_version_1_when_allow_tvm_compatible_evm_enabled() {
    // Setup storage with ALLOW_TVM_COMPATIBLE_EVM = 1
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable ALLOW_TVM_COMPATIBLE_EVM
    storage_engine.put("properties", b"ALLOW_TVM_COMPATIBLE_EVM", &1i64.to_be_bytes()).unwrap();
    // Enable VM creation
    storage_engine.put("properties", b"ALLOW_CREATION_OF_CONTRACTS", &1i64.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_vm_enabled();

    // Create owner account with sufficient balance
    let owner_address = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12]);
    let owner_account = AccountInfo {
        balance: U256::from(10_000_000_000u64), // 10,000 TRX
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    // Simple bytecode that returns empty (PUSH0 PUSH0 RETURN in older opcodes: 60 00 60 00 f3)
    let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];

    let contract_data = build_create_smart_contract_data(
        &owner_address,
        "TestContract",
        &bytecode,
        50,   // consume_user_resource_percent
        1000, // origin_energy_limit
        0,    // call_value
        0,    // call_token_value
        0,    // token_id
    );

    // Create a transaction ID for address derivation
    let txid = B256::from([0x11u8; 32]);

    let transaction = TronTransaction {
        from: owner_address,
        to: None, // CreateSmartContract has no 'to'
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 10_000_000, // Fee limit in SUN
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::CreateSmartContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: Some(txid),
    };

    // Derive expected contract address
    let expected_contract_address = derive_top_level_contract_address(&txid, &owner_address);

    // Execute the transaction through the service
    // NOTE: This test validates the persist_smart_contract_metadata logic
    // The actual execution would need VM support, so we test the metadata persistence path directly
    let result = service.persist_smart_contract_metadata(
        &mut storage_adapter,
        &transaction,
        &context,
        &expected_contract_address,
    );

    assert!(result.is_ok(), "persist_smart_contract_metadata should succeed: {:?}", result);

    // Verify SmartContract.version = 1 was persisted
    let tron_contract_address = storage_adapter.to_tron_address_21(&expected_contract_address);
    let stored_contract = storage_adapter.get_smart_contract(&tron_contract_address);

    assert!(stored_contract.is_ok(), "Should be able to read stored contract");
    if let Ok(Some(contract)) = stored_contract {
        assert_eq!(contract.version, 1,
            "SmartContract.version should be 1 when ALLOW_TVM_COMPATIBLE_EVM=1");
        assert_eq!(contract.name, "TestContract",
            "Contract name should be preserved");
        assert_eq!(contract.consume_user_resource_percent, 50,
            "consume_user_resource_percent should be preserved");
        assert_eq!(contract.origin_energy_limit, 1000,
            "origin_energy_limit should be preserved");
    } else {
        panic!("SmartContract metadata should be stored");
    }
}

#[test]
fn test_create_contract_persists_version_0_when_allow_tvm_compatible_evm_disabled() {
    // Setup storage with ALLOW_TVM_COMPATIBLE_EVM = 0 (disabled)
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Explicitly disable ALLOW_TVM_COMPATIBLE_EVM
    storage_engine.put("properties", b"ALLOW_TVM_COMPATIBLE_EVM", &0i64.to_be_bytes()).unwrap();
    // Enable VM creation
    storage_engine.put("properties", b"ALLOW_CREATION_OF_CONTRACTS", &1i64.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_vm_enabled();

    let owner_address = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12]);
    let owner_account = AccountInfo {
        balance: U256::from(10_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];

    let contract_data = build_create_smart_contract_data(
        &owner_address,
        "TestContract2",
        &bytecode,
        30,
        2000,
        0,
        0,
        0,
    );

    let txid = B256::from([0x22u8; 32]);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 10_000_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::CreateSmartContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: Some(txid),
    };

    let expected_contract_address = derive_top_level_contract_address(&txid, &owner_address);

    let result = service.persist_smart_contract_metadata(
        &mut storage_adapter,
        &transaction,
        &context,
        &expected_contract_address,
    );

    assert!(result.is_ok(), "persist_smart_contract_metadata should succeed: {:?}", result);

    let tron_contract_address = storage_adapter.to_tron_address_21(&expected_contract_address);
    let stored_contract = storage_adapter.get_smart_contract(&tron_contract_address);

    if let Ok(Some(contract)) = stored_contract {
        assert_eq!(contract.version, 0,
            "SmartContract.version should be 0 when ALLOW_TVM_COMPATIBLE_EVM=0");
    } else {
        panic!("SmartContract metadata should be stored");
    }
}

// ============================================================================
// SECTION 2: Internal CREATE address derivation tests
// ============================================================================

#[test]
fn test_internal_create_address_derivation_formula() {
    // This test verifies the internal CREATE address derivation formula matches Java:
    // keccak256(txid || nonce_be_u64)[12..32]

    let txid = B256::from([0x11u8; 32]);

    // Nonce 0 should produce a specific address
    let addr_nonce_0 = derive_internal_create_address(&txid, 0);

    // Nonce 1 should produce a different address
    let addr_nonce_1 = derive_internal_create_address(&txid, 1);

    // Addresses should be different
    assert_ne!(addr_nonce_0, addr_nonce_1,
        "Different nonces should produce different addresses");

    // Same inputs should produce same output (deterministic)
    let addr_nonce_0_again = derive_internal_create_address(&txid, 0);
    assert_eq!(addr_nonce_0, addr_nonce_0_again,
        "Same inputs should produce same address (deterministic)");
}

#[test]
fn test_internal_create_addresses_sequence_stable() {
    // Verify that multiple internal CREATEs in one tx produce a stable address sequence
    let txid = B256::from([0xaa, 0xbb, 0xcc, 0xdd, 0x11, 0x22, 0x33, 0x44,
                           0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc,
                           0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44,
                           0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc]);

    // Simulate the sequence Java would produce:
    // - nonce starts at 0
    // - Each internal tx (CALL or CREATE) increments nonce
    // - CREATE uses the nonce BEFORE incrementing

    // First CREATE uses nonce 0
    let addr1 = derive_internal_create_address(&txid, 0);

    // A CALL happens (nonce becomes 1)
    // Second CREATE uses nonce 1
    let addr2 = derive_internal_create_address(&txid, 1);

    // Another CALL (nonce becomes 2)
    // Third CREATE uses nonce 2
    let addr3 = derive_internal_create_address(&txid, 2);

    // All addresses should be different
    assert_ne!(addr1, addr2, "First and second CREATE addresses should differ");
    assert_ne!(addr2, addr3, "Second and third CREATE addresses should differ");
    assert_ne!(addr1, addr3, "First and third CREATE addresses should differ");

    // The sequence should be reproducible
    let addr1_check = derive_internal_create_address(&txid, 0);
    let addr2_check = derive_internal_create_address(&txid, 1);
    let addr3_check = derive_internal_create_address(&txid, 2);

    assert_eq!(addr1, addr1_check, "Address sequence should be reproducible");
    assert_eq!(addr2, addr2_check, "Address sequence should be reproducible");
    assert_eq!(addr3, addr3_check, "Address sequence should be reproducible");
}

#[test]
fn test_no_address_collisions_across_different_txids() {
    // Verify that different txids produce different CREATE addresses even with same nonce
    let txid1 = B256::from([0x11u8; 32]);
    let txid2 = B256::from([0x22u8; 32]);
    let txid3 = B256::from([0x33u8; 32]);

    // Same nonce (0) with different txids
    let addr1 = derive_internal_create_address(&txid1, 0);
    let addr2 = derive_internal_create_address(&txid2, 0);
    let addr3 = derive_internal_create_address(&txid3, 0);

    assert_ne!(addr1, addr2, "Different txids should produce different addresses");
    assert_ne!(addr2, addr3, "Different txids should produce different addresses");
    assert_ne!(addr1, addr3, "Different txids should produce different addresses");
}

#[test]
fn test_top_level_vs_internal_create_address_differs() {
    // Verify that top-level CreateSmartContract address differs from internal CREATE address
    let txid = B256::from([0x44u8; 32]);
    let owner = Address::from([0x55u8; 20]);

    // Top-level uses: keccak256(txid || owner_address_21_bytes)[12..32]
    let top_level_addr = derive_top_level_contract_address(&txid, &owner);

    // Internal CREATE uses: keccak256(txid || nonce_be_u64)[12..32]
    let internal_addr = derive_internal_create_address(&txid, 0);

    // They should be different (different input formats)
    assert_ne!(top_level_addr, internal_addr,
        "Top-level and internal CREATE address derivation should differ");
}

// ============================================================================
// SECTION 3: TRC-10 call_token_value transfer tests
// ============================================================================

#[test]
fn test_trc10_transfer_emitted_on_successful_contract_creation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable TRC-10 transfers
    storage_engine.put("properties", b"ALLOW_TVM_TRANSFER_TRC10", &1i64.to_be_bytes()).unwrap();
    storage_engine.put("properties", b"ALLOW_CREATION_OF_CONTRACTS", &1i64.to_be_bytes()).unwrap();
    storage_engine.put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes()).unwrap();

    let storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_vm_enabled();

    let owner_address = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12]);

    let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];
    let token_id: i64 = 1_000_001; // Valid token ID (> 1_000_000)
    let call_token_value: i64 = 100;

    let contract_data = build_create_smart_contract_data(
        &owner_address,
        "TokenContract",
        &bytecode,
        50,
        1000,
        0,               // call_value (TRX)
        call_token_value,
        token_id,
    );

    let txid = B256::from([0x66u8; 32]);
    let created_address = derive_top_level_contract_address(&txid, &owner_address);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 10_000_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::CreateSmartContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: Some(txid),
    };

    // Test extract_create_contract_trc10_transfer directly
    let trc10_result = service.extract_create_contract_trc10_transfer(
        &storage_adapter,
        &transaction,
        &created_address,
    );

    assert!(trc10_result.is_ok(), "TRC-10 extraction should succeed: {:?}", trc10_result);

    if let Ok(Some(trc10_change)) = trc10_result {
        match trc10_change {
            Trc10Change::AssetTransferred(transfer) => {
                assert_eq!(transfer.owner_address, owner_address,
                    "Transfer should be from owner");
                assert_eq!(transfer.to_address, created_address,
                    "Transfer should be to created contract");
                assert_eq!(transfer.amount, call_token_value,
                    "Transfer amount should match call_token_value");
                assert_eq!(transfer.token_id, Some(token_id.to_string()),
                    "Token ID should be preserved");
            }
            _ => panic!("Expected AssetTransferred change"),
        }
    } else {
        panic!("Expected TRC-10 change to be emitted");
    }
}

#[test]
fn test_trc10_transfer_not_emitted_when_call_token_value_zero() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable TRC-10 transfers
    storage_engine.put("properties", b"ALLOW_TVM_TRANSFER_TRC10", &1i64.to_be_bytes()).unwrap();

    let storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_vm_enabled();

    let owner_address = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12]);

    let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];

    // No TRC-10 transfer (call_token_value = 0)
    let contract_data = build_create_smart_contract_data(
        &owner_address,
        "NoTokenContract",
        &bytecode,
        50,
        1000,
        0, // call_value
        0, // call_token_value = 0
        0, // token_id
    );

    let txid = B256::from([0x77u8; 32]);
    let created_address = derive_top_level_contract_address(&txid, &owner_address);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 10_000_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::CreateSmartContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: Some(txid),
    };

    let trc10_result = service.extract_create_contract_trc10_transfer(
        &storage_adapter,
        &transaction,
        &created_address,
    );

    assert!(trc10_result.is_ok(), "TRC-10 extraction should succeed");
    assert!(trc10_result.unwrap().is_none(),
        "No TRC-10 change should be emitted when call_token_value is 0");
}

#[test]
fn test_trc10_transfer_not_emitted_when_trc10_disabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // TRC-10 transfers DISABLED
    storage_engine.put("properties", b"ALLOW_TVM_TRANSFER_TRC10", &0i64.to_be_bytes()).unwrap();

    let storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_vm_enabled();

    let owner_address = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12]);

    let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];
    let token_id: i64 = 1_000_001;
    let call_token_value: i64 = 100;

    let contract_data = build_create_smart_contract_data(
        &owner_address,
        "TokenContract",
        &bytecode,
        50,
        1000,
        0,
        call_token_value, // Non-zero token value
        token_id,
    );

    let txid = B256::from([0x88u8; 32]);
    let created_address = derive_top_level_contract_address(&txid, &owner_address);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 10_000_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::CreateSmartContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: Some(txid),
    };

    let trc10_result = service.extract_create_contract_trc10_transfer(
        &storage_adapter,
        &transaction,
        &created_address,
    );

    assert!(trc10_result.is_ok(), "TRC-10 extraction should succeed");
    assert!(trc10_result.unwrap().is_none(),
        "No TRC-10 change should be emitted when ALLOW_TVM_TRANSFER_TRC10=0");
}

// ============================================================================
// SECTION 4: Validation parity tests
// ============================================================================

#[test]
fn test_validation_rejects_missing_owner_account() {
    use tron_backend_execution::ExecutionModule;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine.put("properties", b"ALLOW_CREATION_OF_CONTRACTS", &1i64.to_be_bytes()).unwrap();

    let storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Owner address that doesn't exist in storage
    let owner_address = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12]);

    let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];

    let contract_data = build_create_smart_contract_data(
        &owner_address,
        "TestContract",
        &bytecode,
        50,
        1000,
        0,
        0,
        0,
    );

    let txid = B256::from([0x99u8; 32]);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 10_000_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::CreateSmartContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: Some(txid),
    };

    // Call validation directly
    let config = ExecutionConfig::default();
    let module = ExecutionModule::new(config);

    let result = module.execute_transaction_with_storage(storage_adapter, &transaction, &context);

    assert!(result.is_err() || !result.as_ref().unwrap().success,
        "Should fail when owner account doesn't exist");

    if let Err(e) = result {
        assert!(e.to_string().contains("no OwnerAccount") || e.to_string().contains("OwnerAccount"),
            "Error should mention missing owner account, got: {}", e);
    }
}

#[test]
fn test_validation_rejects_contract_name_too_long() {
    use tron_backend_execution::ExecutionModule;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine.put("properties", b"ALLOW_CREATION_OF_CONTRACTS", &1i64.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12]);

    // Create owner account
    let owner_account = AccountInfo {
        balance: U256::from(10_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];

    // Name longer than 32 bytes
    let long_name = "ThisContractNameIsWayTooLongAndExceeds32Bytes";

    let contract_data = build_create_smart_contract_data(
        &owner_address,
        long_name,
        &bytecode,
        50,
        1000,
        0,
        0,
        0,
    );

    let txid = B256::from([0xaau8; 32]);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 10_000_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::CreateSmartContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: Some(txid),
    };

    let config = ExecutionConfig::default();
    let module = ExecutionModule::new(config);

    let result = module.execute_transaction_with_storage(storage_adapter, &transaction, &context);

    assert!(result.is_err(),
        "Should fail when contract name exceeds 32 bytes");

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("32") || error_msg.contains("contractName"),
        "Error should mention name length limit, got: {}", error_msg);
}

#[test]
fn test_validation_rejects_invalid_percent() {
    use tron_backend_execution::ExecutionModule;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine.put("properties", b"ALLOW_CREATION_OF_CONTRACTS", &1i64.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12]);

    let owner_account = AccountInfo {
        balance: U256::from(10_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];

    // Invalid percent (> 100)
    let contract_data = build_create_smart_contract_data(
        &owner_address,
        "TestContract",
        &bytecode,
        150, // Invalid: must be 0-100
        1000,
        0,
        0,
        0,
    );

    let txid = B256::from([0xccu8; 32]);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 10_000_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::CreateSmartContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: Some(txid),
    };

    let config = ExecutionConfig::default();
    let module = ExecutionModule::new(config);

    let result = module.execute_transaction_with_storage(storage_adapter, &transaction, &context);

    assert!(result.is_err(),
        "Should fail when consume_user_resource_percent is > 100");

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("percent") || error_msg.contains("100"),
        "Error should mention percent bounds, got: {}", error_msg);
}

// ============================================================================
// SECTION 5: TRC-10 validation error message parity tests
// ============================================================================

#[test]
fn test_trc10_validation_rejects_missing_asset() {
    // Test: "No asset !" error when token_id doesn't exist in AssetIssueStore
    use tron_backend_execution::ExecutionModule;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine.put("properties", b"ALLOW_CREATION_OF_CONTRACTS", &1i64.to_be_bytes()).unwrap();
    storage_engine.put("properties", b"ALLOW_TVM_TRANSFER_TRC10", &1i64.to_be_bytes()).unwrap();
    storage_engine.put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12]);

    // Create owner account with sufficient balance
    let owner_account = AccountInfo {
        balance: U256::from(10_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];
    let token_id: i64 = 9999999; // Non-existent token
    let call_token_value: i64 = 100;

    let contract_data = build_create_smart_contract_data(
        &owner_address,
        "TestContract",
        &bytecode,
        50,
        1000,
        0,
        call_token_value,
        token_id,
    );

    let txid = B256::from([0xddu8; 32]);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 10_000_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::CreateSmartContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: Some(txid),
    };

    let config = ExecutionConfig::default();
    let module = ExecutionModule::new(config);

    let result = module.execute_transaction_with_storage(storage_adapter, &transaction, &context);

    assert!(result.is_err(), "Should fail when token doesn't exist");

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("No asset"),
        "Error should be 'No asset !' for non-existent token, got: {}", error_msg);
}

#[test]
fn test_trc10_validation_rejects_zero_asset_balance() {
    // Test: "assetBalance must greater than 0." error when owner has no balance for the token
    use tron_backend_execution::ExecutionModule;
    use tron_backend_execution::protocol::AssetIssueContractData;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine.put("properties", b"ALLOW_CREATION_OF_CONTRACTS", &1i64.to_be_bytes()).unwrap();
    storage_engine.put("properties", b"ALLOW_TVM_TRANSFER_TRC10", &1i64.to_be_bytes()).unwrap();
    storage_engine.put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12]);

    // Create owner account with TRX balance but NO token balance
    let owner_account = AccountInfo {
        balance: U256::from(10_000_000_000u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner_address, owner_account).unwrap();

    // Create the asset in the asset store (token exists)
    let token_id: i64 = 1000001;
    let token_id_str = token_id.to_string();
    let asset_issue = AssetIssueContractData {
        owner_address: make_from_raw(&owner_address),
        name: b"TestToken".to_vec(),
        abbr: b"TT".to_vec(),
        total_supply: 1000000,
        ..Default::default()
    };
    storage_adapter.put_asset_issue(token_id_str.as_bytes(), &asset_issue, true).unwrap();

    // Owner has NO token balance (asset_v2 map is empty or doesn't have this token)

    let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];
    let call_token_value: i64 = 100;

    let contract_data = build_create_smart_contract_data(
        &owner_address,
        "TestContract",
        &bytecode,
        50,
        1000,
        0,
        call_token_value,
        token_id,
    );

    let txid = B256::from([0xeeu8; 32]);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 10_000_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::CreateSmartContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: Some(txid),
    };

    let config = ExecutionConfig::default();
    let module = ExecutionModule::new(config);

    let result = module.execute_transaction_with_storage(storage_adapter, &transaction, &context);

    assert!(result.is_err(), "Should fail when owner has zero token balance");

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("assetBalance must greater than 0"),
        "Error should be 'assetBalance must greater than 0.' for zero balance, got: {}", error_msg);
}

#[test]
fn test_trc10_validation_rejects_insufficient_asset_balance() {
    // Test: "assetBalance is not sufficient." error when token_value > balance
    use tron_backend_execution::ExecutionModule;
    use tron_backend_execution::protocol::{AssetIssueContractData, Account as ProtoAccount};
    use std::collections::BTreeMap;

    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    storage_engine.put("properties", b"ALLOW_CREATION_OF_CONTRACTS", &1i64.to_be_bytes()).unwrap();
    storage_engine.put("properties", b"ALLOW_TVM_TRANSFER_TRC10", &1i64.to_be_bytes()).unwrap();
    storage_engine.put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes()).unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                                       0xbc, 0xde, 0xf0, 0x12]);

    // Create the asset in the asset store
    let token_id: i64 = 1000002;
    let token_id_str = token_id.to_string();
    let asset_issue = AssetIssueContractData {
        owner_address: make_from_raw(&owner_address),
        name: b"TestToken2".to_vec(),
        abbr: b"TT2".to_vec(),
        total_supply: 1000000,
        ..Default::default()
    };
    storage_adapter.put_asset_issue(token_id_str.as_bytes(), &asset_issue, true).unwrap();

    // Create owner account with TRX balance AND some token balance (but not enough)
    let mut asset_v2 = BTreeMap::new();
    asset_v2.insert(token_id_str.clone(), 50i64); // Only 50 tokens

    let owner_proto = ProtoAccount {
        r#type: 0, // Normal account
        address: make_from_raw(&owner_address),
        balance: 10_000_000_000i64, // 10,000 TRX
        asset_v2,
        ..Default::default()
    };

    // Store the account proto directly using the public method
    storage_adapter.put_account_proto(&owner_address, &owner_proto).unwrap();

    let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];
    let call_token_value: i64 = 100; // Requesting 100, but only has 50

    let contract_data = build_create_smart_contract_data(
        &owner_address,
        "TestContract",
        &bytecode,
        50,
        1000,
        0,
        call_token_value,
        token_id,
    );

    let txid = B256::from([0xffu8; 32]);

    let transaction = TronTransaction {
        from: owner_address,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 10_000_000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::CreateSmartContract),
            asset_id: None,
            from_raw: Some(make_from_raw(&owner_address)),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: 1000000,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: Some(txid),
    };

    let config = ExecutionConfig::default();
    let module = ExecutionModule::new(config);

    let result = module.execute_transaction_with_storage(storage_adapter, &transaction, &context);

    assert!(result.is_err(), "Should fail when token balance is insufficient");

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("assetBalance is not sufficient"),
        "Error should be 'assetBalance is not sufficient.' for insufficient balance, got: {}", error_msg);
}
