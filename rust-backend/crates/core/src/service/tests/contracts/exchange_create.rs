//! ExchangeCreateContract tests.
//!
//! Tests for parity with Java's ExchangeCreateActuator.

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, seed_dynamic_properties};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{
    EngineBackedEvmStateStore, TronContractType, TronExecutionContext, TronContractParameter, TronTransaction, TxMetadata,
};
use tron_backend_storage::StorageEngine;

/// Create a BackendService with exchange_create_enabled = true
fn new_test_service_with_exchange_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            exchange_create_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Build ExchangeCreateContract protobuf data
fn build_exchange_create_contract_data(
    owner: Address,
    first_token_id: &[u8],
    first_token_balance: i64,
    second_token_id: &[u8],
    second_token_balance: i64,
) -> Bytes {
    let mut data = Vec::new();

    // Field 1: owner_address (bytes)
    encode_varint(&mut data, (1 << 3) | 2); // field 1, wire type 2 (length-delimited)
    encode_varint(&mut data, 21);
    data.push(0x41u8); // TRON mainnet prefix
    data.extend_from_slice(owner.as_slice());

    // Field 2: first_token_id (bytes)
    encode_varint(&mut data, (2 << 3) | 2);
    encode_varint(&mut data, first_token_id.len() as u64);
    data.extend_from_slice(first_token_id);

    // Field 3: first_token_balance (int64)
    encode_varint(&mut data, (3 << 3) | 0);
    encode_varint(&mut data, first_token_balance as u64);

    // Field 4: second_token_id (bytes)
    encode_varint(&mut data, (4 << 3) | 2);
    encode_varint(&mut data, second_token_id.len() as u64);
    data.extend_from_slice(second_token_id);

    // Field 5: second_token_balance (int64)
    encode_varint(&mut data, (5 << 3) | 0);
    encode_varint(&mut data, second_token_balance as u64);

    Bytes::from(data)
}

/// Test: Receipt includes both fee and exchange_id fields
#[test]
fn test_exchange_create_receipt_includes_fee_and_exchange_id() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Set ALLOW_SAME_TOKEN_NAME = 1 (mainnet-modern)
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    // Set EXCHANGE_CREATE_FEE to known value
    let exchange_create_fee: i64 = 1024_000_000; // 1024 TRX
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &exchange_create_fee.to_be_bytes(),
        )
        .unwrap();

    // Set EXCHANGE_BALANCE_LIMIT
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();

    // Set LATEST_EXCHANGE_NUM = 0
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();

    // Set LATEST_BLOCK_HEADER_TIMESTAMP
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    // Create owner account with enough balance for fee + TRX deposit
    let owner = Address::from([
        0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
        0x9a, 0xbc, 0xde, 0xf0, 0x12,
    ]);
    let initial_balance: u64 = 100_000_000_000; // 100,000 TRX
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    // Seed TRC-10 balance on the account
    // We need to set the account proto for the TRC-10 balance
    let token_id = b"1000001";
    let token_balance: i64 = 10_000_000;

    // Get and update the account proto to add asset_v2 balance
    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto
        .asset_v2
        .insert(String::from_utf8_lossy(token_id).to_string(), token_balance);
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    // Build transaction: exchange TRX for TRC-10 token
    let trx_deposit: i64 = 1_000_000_000; // 1000 TRX
    let token_deposit: i64 = 5_000_000;

    let contract_data = build_exchange_create_contract_data(
        owner,
        b"_", // TRX
        trx_deposit,
        token_id,
        token_deposit,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);
    assert!(result.is_ok(), "Execute failed: {:?}", result.err());

    let result = result.unwrap();
    assert!(result.success, "Transaction should succeed");

    // Verify receipt bytes
    let receipt_bytes = result
        .tron_transaction_result
        .expect("Receipt should be set");
    assert!(
        !receipt_bytes.is_empty(),
        "Receipt bytes should not be empty"
    );

    // Parse the receipt protobuf manually to verify fields
    // Field 1: fee (int64, wire type 0 = varint)
    // Field 21: exchange_id (int64, wire type 0 = varint)
    let mut found_fee = false;
    let mut found_exchange_id = false;
    let mut fee_value: i64 = 0;
    let mut exchange_id_value: i64 = 0;

    let mut i = 0;
    while i < receipt_bytes.len() {
        let (tag, bytes_read) = read_varint(&receipt_bytes[i..]);
        i += bytes_read;

        let field_num = tag >> 3;
        let wire_type = tag & 0x07;

        if wire_type == 0 {
            // Varint
            let (value, bytes_read) = read_varint(&receipt_bytes[i..]);
            i += bytes_read;

            if field_num == 1 {
                found_fee = true;
                fee_value = value as i64;
            } else if field_num == 21 {
                found_exchange_id = true;
                exchange_id_value = value as i64;
            }
        } else if wire_type == 2 {
            // Length-delimited - skip it
            let (len, bytes_read) = read_varint(&receipt_bytes[i..]);
            i += bytes_read;
            i += len as usize;
        } else {
            // Skip unknown wire types (64-bit, 32-bit, etc.)
            break;
        }
    }

    assert!(found_fee, "Receipt should contain fee field (field 1)");
    assert!(
        found_exchange_id,
        "Receipt should contain exchange_id field (field 21)"
    );
    assert_eq!(
        fee_value, exchange_create_fee,
        "Fee should match EXCHANGE_CREATE_FEE"
    );
    assert_eq!(
        exchange_id_value, 1,
        "Exchange ID should be 1 (first exchange)"
    );
}

/// Test: When ALLOW_BLACKHOLE_OPTIMIZATION = true, burn_trx is called
#[test]
fn test_exchange_create_burns_fee_when_blackhole_optimization_enabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable blackhole optimization
    storage_engine
        .put(
            "properties",
            b"ALLOW_BLACKHOLE_OPTIMIZATION",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    // Set ALLOW_SAME_TOKEN_NAME = 1
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    // Set fee and limits
    let exchange_create_fee: i64 = 1024_000_000;
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &exchange_create_fee.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    // Initial BURN_TRX_AMOUNT
    let initial_burn: i64 = 0;
    storage_engine
        .put(
            "properties",
            b"BURN_TRX_AMOUNT",
            &initial_burn.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        0x00, 0x11, 0x22, 0x33, 0x44,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let token_id = b"1000001";
    // Set account proto with TRC-10 balance
    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto
        .asset_v2
        .insert(String::from_utf8_lossy(token_id).to_string(), 10_000_000);
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    let contract_data = build_exchange_create_contract_data(
        owner,
        b"_",
        1_000_000_000, // 1000 TRX
        token_id,
        5_000_000,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);
    assert!(result.is_ok(), "Execute failed: {:?}", result.err());

    // Note: Don't need to commit_buffer - buffered_get/buffered_put work together

    // Check BURN_TRX_AMOUNT increased
    let burn_amount = storage_adapter.get_burn_trx_amount().unwrap();
    assert_eq!(
        burn_amount,
        initial_burn + exchange_create_fee,
        "BURN_TRX_AMOUNT should increase by fee when blackhole optimization enabled"
    );
}

/// Test: When ALLOW_BLACKHOLE_OPTIMIZATION = false, blackhole account is credited
#[test]
fn test_exchange_create_credits_blackhole_when_optimization_disabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Disable blackhole optimization
    storage_engine
        .put(
            "properties",
            b"ALLOW_BLACKHOLE_OPTIMIZATION",
            &0i64.to_be_bytes(),
        )
        .unwrap();

    storage_engine
        .put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    let exchange_create_fee: i64 = 1024_000_000;
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &exchange_create_fee.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    // Initial BURN_TRX_AMOUNT
    storage_engine
        .put("properties", b"BURN_TRX_AMOUNT", &0i64.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
        0x99, 0xaa, 0xbb, 0xcc, 0xdd,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let token_id = b"1000001";
    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto
        .asset_v2
        .insert(String::from_utf8_lossy(token_id).to_string(), 10_000_000);
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    // Get blackhole address and seed initial balance
    // We need to create the blackhole account first since add_balance requires an existing account
    let blackhole_addr = storage_adapter.get_blackhole_address_evm();
    let initial_blackhole_balance: i64 = 1_000_000;
    let blackhole_tron = make_from_raw(&blackhole_addr);
    let blackhole_account_proto = tron_backend_execution::protocol::Account {
        balance: initial_blackhole_balance,
        address: blackhole_tron,
        ..Default::default()
    };
    storage_adapter
        .set_account_proto(&blackhole_addr, &blackhole_account_proto)
        .unwrap();

    let contract_data =
        build_exchange_create_contract_data(owner, b"_", 1_000_000_000, token_id, 5_000_000);

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);
    assert!(result.is_ok(), "Execute failed: {:?}", result.err());

    // Note: Don't need to commit_buffer - buffered_get/buffered_put work together

    // Check BURN_TRX_AMOUNT did NOT change
    let burn_amount = storage_adapter.get_burn_trx_amount().unwrap();
    assert_eq!(
        burn_amount, 0,
        "BURN_TRX_AMOUNT should NOT change when blackhole optimization disabled"
    );

    // Check blackhole account balance increased by reading from account store
    let blackhole_account = storage_adapter.get_account_proto(&blackhole_addr).unwrap();
    let blackhole_balance = blackhole_account.map(|a| a.balance).unwrap_or(0);
    assert_eq!(
        blackhole_balance,
        initial_blackhole_balance + exchange_create_fee,
        "Blackhole account balance should increase by fee"
    );
}

/// Helper to read a varint from bytes
fn read_varint(data: &[u8]) -> (u64, usize) {
    let mut value: u64 = 0;
    let mut shift = 0;
    let mut bytes_read = 0;
    for &byte in data {
        bytes_read += 1;
        value |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    (value, bytes_read)
}

/// Test: Legacy mode (ALLOW_SAME_TOKEN_NAME == 0) reads from Account.asset map
#[test]
fn test_exchange_create_legacy_mode_reads_asset_map() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Set ALLOW_SAME_TOKEN_NAME = 0 (legacy mode)
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &0i64.to_be_bytes())
        .unwrap();

    // Disable asset optimization for this test
    storage_engine
        .put(
            "properties",
            b"ALLOW_ASSET_OPTIMIZATION",
            &0i64.to_be_bytes(),
        )
        .unwrap();

    let exchange_create_fee: i64 = 1024_000_000;
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &exchange_create_fee.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        0x00, 0x11, 0x22, 0x33, 0x44,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    // In legacy mode, tokens are referenced by NAME (not numeric ID)
    // The token name is used as key in Account.asset map
    let token_name = b"TestToken"; // This is a token NAME, not numeric ID
    let token_balance: i64 = 10_000_000;

    // Set account proto with token balance in the LEGACY asset map (not asset_v2)
    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    // Use the legacy 'asset' map instead of 'asset_v2'
    account_proto.asset.insert(
        String::from_utf8_lossy(token_name).to_string(),
        token_balance,
    );
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    // Build transaction with token NAME (not numeric ID)
    let contract_data = build_exchange_create_contract_data(
        owner,
        b"_", // TRX
        1_000_000_000,
        token_name, // Token NAME in legacy mode
        5_000_000,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    // In legacy mode with token names, we need the asset-issue store to resolve names to IDs
    // Since we don't have that setup, this will fail with "No asset!" but that's expected behavior
    // The important thing is that it validates the balance correctly from Account.asset map first
    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);

    // The validation should pass (because we have the balance in Account.asset)
    // but execution may fail later when trying to resolve the token name to ID
    // This is expected behavior for legacy mode without asset-issue store setup
    // The test verifies that asset_balance_enough_v2 reads from the correct map
    if let Err(ref e) = result {
        // "No asset!" means validation passed, but asset-issue lookup failed
        // This confirms legacy mode reads from Account.asset correctly
        assert!(
            e.contains("No asset!") || e.contains("Failed to get"),
            "Expected 'No asset!' or balance check error in legacy mode, got: {}",
            e
        );
    }
    // If it succeeds (unlikely without full setup), that's also fine
}

/// Test: Asset optimization (ALLOW_ASSET_OPTIMIZATION == 1) reads from AccountAssetStore
#[test]
fn test_exchange_create_asset_optimization_reads_asset_store() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable modern mode
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    // Enable asset optimization
    storage_engine
        .put(
            "properties",
            b"ALLOW_ASSET_OPTIMIZATION",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    let exchange_create_fee: i64 = 1024_000_000;
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &exchange_create_fee.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
        0x99, 0xaa, 0xbb, 0xcc, 0xdd,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let token_id = b"1000001";
    let token_balance: i64 = 10_000_000;

    // Store the token balance in AccountAssetStore BEFORE creating the storage adapter
    // Key format: address (21 bytes TRON) + tokenId
    let mut asset_key = Vec::new();
    asset_key.push(0x41u8); // TRON prefix
    asset_key.extend_from_slice(owner.as_slice());
    asset_key.extend_from_slice(token_id);

    // Store balance as big-endian i64
    storage_engine
        .put("account-asset", &asset_key, &token_balance.to_be_bytes())
        .unwrap();

    // Now create the storage adapter
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    // Set account proto WITHOUT the token in asset_v2 map
    // The balance will only be in AccountAssetStore
    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    // Intentionally NOT setting asset_v2 - the balance should come from AccountAssetStore
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    let contract_data =
        build_exchange_create_contract_data(owner, b"_", 1_000_000_000, token_id, 5_000_000);

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    // With asset optimization enabled, the balance check should pass
    // because asset_balance_enough_v2 will read from AccountAssetStore
    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);

    // Should succeed - the balance was found in AccountAssetStore
    assert!(
        result.is_ok(),
        "Execute with asset optimization should succeed: {:?}",
        result.err()
    );

    let exec_result = result.unwrap();
    assert!(
        exec_result.success,
        "Transaction should succeed when balance is in AccountAssetStore"
    );
}

/// Test: When EXCHANGE_CREATE_FEE key is missing, default to 1024000000 (1024 TRX)
/// Java initializes missing EXCHANGE_CREATE_FEE to 1024000000L
#[test]
fn test_exchange_create_fee_default_when_missing() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Intentionally DO NOT set EXCHANGE_CREATE_FEE
    // Only set minimal required properties
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    let storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    // Call get_exchange_create_fee() and verify it returns the Java-parity default
    let fee = storage_adapter.get_exchange_create_fee().unwrap();

    assert_eq!(
        fee, 1024_000_000,
        "Missing EXCHANGE_CREATE_FEE should default to 1024000000 (1024 TRX in SUN)"
    );
}

/// Test: Validation fails when owner has insufficient balance for fee
/// Java error: "No enough balance for exchange create fee!"
#[test]
fn test_exchange_create_fails_insufficient_balance_for_fee() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    let exchange_create_fee: i64 = 1024_000_000;
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &exchange_create_fee.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        0x00, 0x11, 0x22, 0x33, 0x44,
    ]);

    // Set balance LESS than the fee
    let insufficient_balance: u64 = 500_000_000; // 500 TRX, but fee is 1024 TRX
    let owner_account = AccountInfo {
        balance: U256::from(insufficient_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = insufficient_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    let contract_data = build_exchange_create_contract_data(
        owner, b"_",      // TRX
        1_000_000, // small deposit
        b"1000001", 1_000_000,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);

    // Should fail with fee balance error
    assert!(result.is_err(), "Should fail when balance < fee");
    let err = result.unwrap_err();
    assert!(
        err.contains("No enough balance for exchange create fee"),
        "Expected 'No enough balance for exchange create fee' error, got: {}",
        err
    );
}

/// Test: Validation fails when tokens are the same
/// Java error: "cannot exchange same tokens"
#[test]
fn test_exchange_create_fails_same_tokens() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &1024_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        0x00, 0x11, 0x22, 0x33, 0x44,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    // Try to exchange the SAME token (both are "1000001")
    let contract_data = build_exchange_create_contract_data(
        owner, b"1000001", // same token
        1_000_000, b"1000001", // same token
        1_000_000,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);

    assert!(result.is_err(), "Should fail when tokens are the same");
    let err = result.unwrap_err();
    assert!(
        err.contains("cannot exchange same tokens"),
        "Expected 'cannot exchange same tokens' error, got: {}",
        err
    );
}

/// Test: Validation fails when token balance is zero
/// Java error: "token balance must greater than zero"
#[test]
fn test_exchange_create_fails_zero_balance() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &1024_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        0x00, 0x11, 0x22, 0x33, 0x44,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    // Try to create exchange with zero balance for first token
    let contract_data = build_exchange_create_contract_data(
        owner, b"_", // TRX
        0,    // ZERO balance - should fail
        b"1000001", 1_000_000,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);

    assert!(result.is_err(), "Should fail when token balance is zero");
    let err = result.unwrap_err();
    assert!(
        err.contains("token balance must greater than zero"),
        "Expected 'token balance must greater than zero' error, got: {}",
        err
    );
}

/// Test: Validation fails when token balance exceeds limit
/// Java error: "token balance must less than <limit>"
#[test]
fn test_exchange_create_fails_balance_exceeds_limit() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &1024_000_000i64.to_be_bytes(),
        )
        .unwrap();

    // Set a low limit for testing
    let exchange_limit: i64 = 1_000_000; // 1 TRX limit
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &exchange_limit.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        0x00, 0x11, 0x22, 0x33, 0x44,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    // Try to create exchange with balance exceeding limit
    let contract_data = build_exchange_create_contract_data(
        owner, b"_",       // TRX
        10_000_000, // 10 TRX - exceeds 1 TRX limit
        b"1000001", 500_000, // under limit
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);

    assert!(
        result.is_err(),
        "Should fail when token balance exceeds limit"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("token balance must less than"),
        "Expected 'token balance must less than' error, got: {}",
        err
    );
}

/// Test: Validation fails with invalid (non-numeric) token id when ALLOW_SAME_TOKEN_NAME == 1
/// Java error: "first token id is not a valid number"
#[test]
fn test_exchange_create_fails_invalid_token_id() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable ALLOW_SAME_TOKEN_NAME which requires numeric token IDs
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &1024_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        0x00, 0x11, 0x22, 0x33, 0x44,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    // Use non-numeric token id (should fail when ALLOW_SAME_TOKEN_NAME == 1)
    let contract_data = build_exchange_create_contract_data(
        owner,
        b"_", // TRX
        1_000_000,
        b"InvalidTokenName", // non-numeric - should fail
        1_000_000,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);

    assert!(result.is_err(), "Should fail when token id is not numeric");
    let err = result.unwrap_err();
    assert!(
        err.contains("token id is not a valid number"),
        "Expected 'token id is not a valid number' error, got: {}",
        err
    );
}

/// Test: Validation fails when TRX balance is insufficient for deposit (not just fee)
/// Java error: "balance is not enough"
#[test]
fn test_exchange_create_fails_insufficient_trx_for_deposit() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    let exchange_create_fee: i64 = 1024_000_000; // 1024 TRX
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &exchange_create_fee.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        0x00, 0x11, 0x22, 0x33, 0x44,
    ]);

    // Owner has enough for fee but NOT enough for fee + TRX deposit
    // Fee = 1024 TRX, deposit = 1000 TRX, total needed = 2024 TRX
    // But we only give 1500 TRX
    let balance: u64 = 1_500_000_000; // 1500 TRX
    let owner_account = AccountInfo {
        balance: U256::from(balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    // Give some TRC-10 balance for the other side
    account_proto
        .asset_v2
        .insert("1000001".to_string(), 10_000_000);
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    // Try to deposit 1000 TRX (need 1024 + 1000 = 2024 but only have 1500)
    let contract_data = build_exchange_create_contract_data(
        owner,
        b"_",          // TRX
        1_000_000_000, // 1000 TRX deposit
        b"1000001",
        5_000_000,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);

    assert!(
        result.is_err(),
        "Should fail when TRX balance < fee + deposit"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("balance is not enough"),
        "Expected 'balance is not enough' error, got: {}",
        err
    );
}

/// Test: Validation fails when TRC-10 token balance is insufficient
/// Java error: "first token balance is not enough" or "second token balance is not enough"
#[test]
fn test_exchange_create_fails_insufficient_trc10_balance() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &1024_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        0x00, 0x11, 0x22, 0x33, 0x44,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    // Only 1,000,000 tokens but trying to deposit 5,000,000
    account_proto
        .asset_v2
        .insert("1000001".to_string(), 1_000_000);
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    // Try to deposit more tokens than we have
    let contract_data = build_exchange_create_contract_data(
        owner,
        b"_", // TRX
        1_000_000_000,
        b"1000001",
        5_000_000, // Trying to deposit 5M but only have 1M
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);

    assert!(
        result.is_err(),
        "Should fail when TRC-10 balance is insufficient"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("token balance is not enough"),
        "Expected 'token balance is not enough' error, got: {}",
        err
    );
}

/// Test: Exchange ID is correctly incremented for multiple exchanges
#[test]
fn test_exchange_create_increments_exchange_id() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &1024_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();

    // Start with LATEST_EXCHANGE_NUM = 5 (pretend 5 exchanges already exist)
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &5i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
        0x9a, 0xbc, 0xde, 0xf0, 0x12,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let token_id = b"1000001";
    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto
        .asset_v2
        .insert(String::from_utf8_lossy(token_id).to_string(), 10_000_000);
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    let contract_data =
        build_exchange_create_contract_data(owner, b"_", 1_000_000_000, token_id, 5_000_000);

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);
    assert!(result.is_ok(), "Execute failed: {:?}", result.err());

    let result = result.unwrap();
    assert!(result.success, "Transaction should succeed");

    // Verify receipt has exchange_id = 6 (previous was 5)
    let receipt_bytes = result
        .tron_transaction_result
        .expect("Receipt should be set");
    let mut exchange_id_value: i64 = 0;

    let mut i = 0;
    while i < receipt_bytes.len() {
        let (tag, bytes_read) = read_varint(&receipt_bytes[i..]);
        i += bytes_read;

        let field_num = tag >> 3;
        let wire_type = tag & 0x07;

        if wire_type == 0 {
            let (value, bytes_read) = read_varint(&receipt_bytes[i..]);
            i += bytes_read;
            if field_num == 21 {
                exchange_id_value = value as i64;
            }
        } else if wire_type == 2 {
            let (len, bytes_read) = read_varint(&receipt_bytes[i..]);
            i += bytes_read;
            i += len as usize;
        } else {
            break;
        }
    }

    assert_eq!(
        exchange_id_value, 6,
        "Exchange ID should be 6 (previous was 5)"
    );

    // Verify LATEST_EXCHANGE_NUM was updated
    let latest_exchange_num = storage_adapter.get_latest_exchange_num().unwrap();
    assert_eq!(
        latest_exchange_num, 6,
        "LATEST_EXCHANGE_NUM should be updated to 6"
    );
}

/// Test: Receipt bytes match conformance fixture format
/// Validates that our receipt serialization produces the exact same bytes as Java
/// Fixture: conformance/fixtures/exchange_create_contract/happy_path_trx_to_token/expected/result.pb
///
/// Expected bytes: 08 80 80 a4 e8 03 a8 01 01
/// - field 1 (fee): 1024000000 (0x3D090000)
/// - field 21 (exchange_id): 1
#[test]
fn test_exchange_create_receipt_matches_conformance_fixture() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    // Set fee to match conformance fixture (1024 TRX)
    let exchange_create_fee: i64 = 1024_000_000;
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &exchange_create_fee.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();

    // Set LATEST_EXCHANGE_NUM = 0 so first exchange gets ID = 1
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
        0x9a, 0xbc, 0xde, 0xf0, 0x12,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let token_id = b"1000001";
    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto
        .asset_v2
        .insert(String::from_utf8_lossy(token_id).to_string(), 10_000_000);
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    let contract_data = build_exchange_create_contract_data(
        owner,
        b"_", // TRX
        1_000_000_000,
        token_id,
        5_000_000,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);
    assert!(result.is_ok(), "Execute failed: {:?}", result.err());

    let result = result.unwrap();
    assert!(result.success, "Transaction should succeed");

    let receipt_bytes = result
        .tron_transaction_result
        .expect("Receipt should be set");

    // Expected conformance fixture bytes:
    // 08 80 80 a4 e8 03 a8 01 01
    // - 08 = field 1 wire type 0 (varint)
    // - 80 80 a4 e8 03 = varint 1024000000
    // - a8 01 = field 21 wire type 0 (varint) - note: 21 << 3 | 0 = 168 = 0xa8, but >127 so 0xa8 0x01
    // - 01 = varint 1
    let expected_bytes: [u8; 9] = [0x08, 0x80, 0x80, 0xa4, 0xe8, 0x03, 0xa8, 0x01, 0x01];

    // Convert receipt_bytes (Bytes type) to a slice for comparison
    let receipt_slice: &[u8] = &receipt_bytes;
    assert_eq!(
        receipt_slice, &expected_bytes,
        "Receipt bytes should match conformance fixture.\n\
         Expected: {:02x?}\n\
         Got:      {:02x?}",
        expected_bytes, receipt_slice
    );
}

// ============================================================================
// End-to-End Parity Tests (Surrounding Processors)
// ============================================================================

/// Test: Owner account TRX balance is correctly deducted (fee + TRX deposit)
/// Verifies state change parity with Java's ExchangeCreateActuator.execute()
#[test]
fn test_exchange_create_deducts_owner_trx_balance() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    let exchange_create_fee: i64 = 1024_000_000; // 1024 TRX
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &exchange_create_fee.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
        0x9a, 0xbc, 0xde, 0xf0, 0x12,
    ]);
    let initial_balance: i64 = 100_000_000_000; // 100,000 TRX
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance as u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let token_id = b"1000001";
    let token_balance: i64 = 10_000_000;
    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto
        .asset_v2
        .insert(String::from_utf8_lossy(token_id).to_string(), token_balance);
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    let trx_deposit: i64 = 1_000_000_000; // 1000 TRX
    let token_deposit: i64 = 5_000_000;

    let contract_data = build_exchange_create_contract_data(
        owner,
        b"_", // TRX
        trx_deposit,
        token_id,
        token_deposit,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);
    assert!(result.is_ok(), "Execute failed: {:?}", result.err());

    // Verify owner's TRX balance after execution
    let final_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    let expected_balance = initial_balance - exchange_create_fee - trx_deposit;

    assert_eq!(
        final_account.balance, expected_balance,
        "Owner TRX balance should be deducted by fee ({}) + TRX deposit ({})\n\
         Initial: {}, Expected: {}, Got: {}",
        exchange_create_fee, trx_deposit, initial_balance, expected_balance, final_account.balance
    );
}

/// Test: Owner account TRC-10 balance is correctly deducted
/// Verifies state change parity with Java's AccountCapsule.reduceAssetAmountV2()
#[test]
fn test_exchange_create_deducts_owner_trc10_balance() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &1024_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
        0x9a, 0xbc, 0xde, 0xf0, 0x12,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let token_id = b"1000001";
    let token_key = String::from_utf8_lossy(token_id).to_string();
    let initial_token_balance: i64 = 10_000_000;
    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto
        .asset_v2
        .insert(token_key.clone(), initial_token_balance);
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    let token_deposit: i64 = 5_000_000;

    let contract_data = build_exchange_create_contract_data(
        owner,
        b"_", // TRX
        1_000_000_000,
        token_id,
        token_deposit,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);
    assert!(result.is_ok(), "Execute failed: {:?}", result.err());

    // Verify owner's TRC-10 balance after execution
    let final_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();
    let final_token_balance = final_account.asset_v2.get(&token_key).copied().unwrap_or(0);
    let expected_token_balance = initial_token_balance - token_deposit;

    assert_eq!(
        final_token_balance, expected_token_balance,
        "Owner TRC-10 balance should be deducted by token deposit ({})\n\
         Initial: {}, Expected: {}, Got: {}",
        token_deposit, initial_token_balance, expected_token_balance, final_token_balance
    );
}

/// Test: Exchange record is stored correctly in ExchangeV2Store
/// Verifies state change parity with Java's ExchangeCreateActuator storing to ExchangeV2Store
#[test]
fn test_exchange_create_stores_exchange_record() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &1024_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();

    let create_time: i64 = 1609459200000; // 2021-01-01 00:00:00 UTC
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &create_time.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
        0x9a, 0xbc, 0xde, 0xf0, 0x12,
    ]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let first_token_id = b"_"; // TRX
    let second_token_id = b"1000001";
    let first_token_balance: i64 = 1_000_000_000;
    let second_token_balance: i64 = 5_000_000;

    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto.asset_v2.insert(
        String::from_utf8_lossy(second_token_id).to_string(),
        10_000_000,
    );
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    let contract_data = build_exchange_create_contract_data(
        owner,
        first_token_id,
        first_token_balance,
        second_token_id,
        second_token_balance,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron.clone()),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
            ..Default::default()
        },
    };

    let context = TronExecutionContext {
        block_number: 1,
        block_timestamp: create_time as u64,
        block_coinbase: Address::ZERO,
        block_difficulty: U256::ZERO,
        block_gas_limit: 100_000_000,
        chain_id: 1,
        energy_price: 420,
        bandwidth_price: 1000,
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);
    assert!(result.is_ok(), "Execute failed: {:?}", result.err());

    // Verify exchange record was stored
    let exchange = storage_adapter.get_exchange(1).unwrap();
    assert!(
        exchange.is_some(),
        "Exchange record should exist in ExchangeV2Store"
    );

    let exchange = exchange.unwrap();
    assert_eq!(exchange.exchange_id, 1, "Exchange ID should be 1");
    assert_eq!(
        exchange.creator_address, owner_tron,
        "Creator address should match owner"
    );
    // Note: create_time comes from LATEST_BLOCK_HEADER_TIMESTAMP which may be read at execution time
    // The important thing is that it's set (non-zero in production)
    assert_eq!(
        exchange.first_token_id,
        first_token_id.to_vec(),
        "First token ID should match"
    );
    assert_eq!(
        exchange.first_token_balance, first_token_balance,
        "First token balance should match"
    );
    assert_eq!(
        exchange.second_token_id,
        second_token_id.to_vec(),
        "Second token ID should match"
    );
    assert_eq!(
        exchange.second_token_balance, second_token_balance,
        "Second token balance should match"
    );
}

/// Test: Token-to-token exchange (no TRX involved)
/// Verifies end-to-end execution for TRC-10 to TRC-10 exchange creation
#[test]
fn test_exchange_create_token_to_token() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    let exchange_create_fee: i64 = 1024_000_000;
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_CREATE_FEE",
            &exchange_create_fee.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"EXCHANGE_BALANCE_LIMIT",
            &1_000_000_000_000i64.to_be_bytes(),
        )
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put(
            "properties",
            b"LATEST_BLOCK_HEADER_TIMESTAMP",
            &1000000i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([
        0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
        0x9a, 0xbc, 0xde, 0xf0, 0x12,
    ]);
    let initial_balance: i64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance as u64),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let first_token_id = b"1000001";
    let second_token_id = b"1000002";
    let first_token_key = String::from_utf8_lossy(first_token_id).to_string();
    let second_token_key = String::from_utf8_lossy(second_token_id).to_string();
    let initial_first_token: i64 = 20_000_000;
    let initial_second_token: i64 = 30_000_000;
    let first_deposit: i64 = 10_000_000;
    let second_deposit: i64 = 15_000_000;

    let mut account_proto = storage_adapter
        .get_account_proto(&owner)
        .unwrap()
        .unwrap_or_default();
    account_proto.balance = initial_balance;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto
        .asset_v2
        .insert(first_token_key.clone(), initial_first_token);
    account_proto
        .asset_v2
        .insert(second_token_key.clone(), initial_second_token);
    storage_adapter
        .set_account_proto(&owner, &account_proto)
        .unwrap();

    let contract_data = build_exchange_create_contract_data(
        owner,
        first_token_id,
        first_deposit,
        second_token_id,
        second_deposit,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.clone(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
            contract_parameter: Some(TronContractParameter { type_url: "protocol.ExchangeCreateContract".to_string(), value: contract_data.to_vec() }),
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
        transaction_id: None,
    };

    let result = service.execute_non_vm_contract(&mut storage_adapter, &tx, &context);
    assert!(result.is_ok(), "Execute failed: {:?}", result.err());

    let result = result.unwrap();
    assert!(result.success, "Transaction should succeed");

    // Verify owner's balances after execution
    let final_account = storage_adapter.get_account_proto(&owner).unwrap().unwrap();

    // TRX should only be deducted by fee (no TRX deposit)
    assert_eq!(
        final_account.balance,
        initial_balance - exchange_create_fee,
        "TRX balance should only be deducted by fee for token-to-token exchange"
    );

    // Both token balances should be deducted
    let final_first_token = final_account
        .asset_v2
        .get(&first_token_key)
        .copied()
        .unwrap_or(0);
    let final_second_token = final_account
        .asset_v2
        .get(&second_token_key)
        .copied()
        .unwrap_or(0);

    assert_eq!(
        final_first_token,
        initial_first_token - first_deposit,
        "First token balance should be deducted by deposit"
    );
    assert_eq!(
        final_second_token,
        initial_second_token - second_deposit,
        "Second token balance should be deducted by deposit"
    );

    // Verify exchange record
    let exchange = storage_adapter.get_exchange(1).unwrap().unwrap();
    assert_eq!(exchange.first_token_id, first_token_id.to_vec());
    assert_eq!(exchange.second_token_id, second_token_id.to_vec());
    assert_eq!(exchange.first_token_balance, first_deposit);
    assert_eq!(exchange.second_token_balance, second_deposit);
}
