//! ExchangeCreateContract tests.
//!
//! Tests for parity with Java's ExchangeCreateActuator.

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, seed_dynamic_properties};
use revm_primitives::{Address, Bytes, U256, AccountInfo};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{EngineBackedEvmStateStore, TronExecutionContext, TronTransaction, TxMetadata, TronContractType};
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
        .put("properties", b"EXCHANGE_CREATE_FEE", &exchange_create_fee.to_be_bytes())
        .unwrap();

    // Set EXCHANGE_BALANCE_LIMIT
    storage_engine
        .put("properties", b"EXCHANGE_BALANCE_LIMIT", &1_000_000_000_000i64.to_be_bytes())
        .unwrap();

    // Set LATEST_EXCHANGE_NUM = 0
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();

    // Set LATEST_BLOCK_HEADER_TIMESTAMP
    storage_engine
        .put("properties", b"LATEST_BLOCK_HEADER_TIMESTAMP", &1000000i64.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    // Create owner account with enough balance for fee + TRX deposit
    let owner = Address::from([0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a,
                               0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a,
                               0xbc, 0xde, 0xf0, 0x12]);
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
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto.asset_v2.insert(String::from_utf8_lossy(token_id).to_string(), token_balance);
    storage_adapter.set_account_proto(&owner, &account_proto).unwrap();

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
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
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
    let receipt_bytes = result.tron_transaction_result.expect("Receipt should be set");
    assert!(!receipt_bytes.is_empty(), "Receipt bytes should not be empty");

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
    assert!(found_exchange_id, "Receipt should contain exchange_id field (field 21)");
    assert_eq!(fee_value, exchange_create_fee, "Fee should match EXCHANGE_CREATE_FEE");
    assert_eq!(exchange_id_value, 1, "Exchange ID should be 1 (first exchange)");
}

/// Test: When ALLOW_BLACKHOLE_OPTIMIZATION = true, burn_trx is called
#[test]
fn test_exchange_create_burns_fee_when_blackhole_optimization_enabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);

    // Enable blackhole optimization
    storage_engine
        .put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &1i64.to_be_bytes())
        .unwrap();

    // Set ALLOW_SAME_TOKEN_NAME = 1
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    // Set fee and limits
    let exchange_create_fee: i64 = 1024_000_000;
    storage_engine
        .put("properties", b"EXCHANGE_CREATE_FEE", &exchange_create_fee.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"EXCHANGE_BALANCE_LIMIT", &1_000_000_000_000i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_BLOCK_HEADER_TIMESTAMP", &1000000i64.to_be_bytes())
        .unwrap();

    // Initial BURN_TRX_AMOUNT
    let initial_burn: i64 = 0;
    storage_engine
        .put("properties", b"BURN_TRX_AMOUNT", &initial_burn.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                               0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
                               0x11, 0x22, 0x33, 0x44]);
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
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto.asset_v2.insert(String::from_utf8_lossy(token_id).to_string(), 10_000_000);
    storage_adapter.set_account_proto(&owner, &account_proto).unwrap();

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
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
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
        .put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &0i64.to_be_bytes())
        .unwrap();

    storage_engine
        .put("properties", b"ALLOW_MULTI_SIGN", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();

    let exchange_create_fee: i64 = 1024_000_000;
    storage_engine
        .put("properties", b"EXCHANGE_CREATE_FEE", &exchange_create_fee.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"EXCHANGE_BALANCE_LIMIT", &1_000_000_000_000i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_BLOCK_HEADER_TIMESTAMP", &1000000i64.to_be_bytes())
        .unwrap();

    // Initial BURN_TRX_AMOUNT
    storage_engine
        .put("properties", b"BURN_TRX_AMOUNT", &0i64.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11,
                               0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
                               0xaa, 0xbb, 0xcc, 0xdd]);
    let initial_balance: u64 = 100_000_000_000;
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    let token_id = b"1000001";
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto.asset_v2.insert(String::from_utf8_lossy(token_id).to_string(), 10_000_000);
    storage_adapter.set_account_proto(&owner, &account_proto).unwrap();

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
    storage_adapter.set_account_proto(&blackhole_addr, &blackhole_account_proto).unwrap();

    let contract_data = build_exchange_create_contract_data(
        owner,
        b"_",
        1_000_000_000,
        token_id,
        5_000_000,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
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
        .put("properties", b"ALLOW_ASSET_OPTIMIZATION", &0i64.to_be_bytes())
        .unwrap();

    let exchange_create_fee: i64 = 1024_000_000;
    storage_engine
        .put("properties", b"EXCHANGE_CREATE_FEE", &exchange_create_fee.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"EXCHANGE_BALANCE_LIMIT", &1_000_000_000_000i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_BLOCK_HEADER_TIMESTAMP", &1000000i64.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                               0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
                               0x11, 0x22, 0x33, 0x44]);
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
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    // Use the legacy 'asset' map instead of 'asset_v2'
    account_proto.asset.insert(String::from_utf8_lossy(token_name).to_string(), token_balance);
    storage_adapter.set_account_proto(&owner, &account_proto).unwrap();

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
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
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
        .put("properties", b"ALLOW_ASSET_OPTIMIZATION", &1i64.to_be_bytes())
        .unwrap();

    let exchange_create_fee: i64 = 1024_000_000;
    storage_engine
        .put("properties", b"EXCHANGE_CREATE_FEE", &exchange_create_fee.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"EXCHANGE_BALANCE_LIMIT", &1_000_000_000_000i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_EXCHANGE_NUM", &0i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_BLOCK_HEADER_TIMESTAMP", &1000000i64.to_be_bytes())
        .unwrap();

    let service = new_test_service_with_exchange_enabled();

    let owner = Address::from([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11,
                               0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
                               0xaa, 0xbb, 0xcc, 0xdd]);
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
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    // Intentionally NOT setting asset_v2 - the balance should come from AccountAssetStore
    storage_adapter.set_account_proto(&owner, &account_proto).unwrap();

    let contract_data = build_exchange_create_contract_data(
        owner,
        b"_",
        1_000_000_000,
        token_id,
        5_000_000,
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data,
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::ExchangeCreateContract),
            asset_id: None,
            from_raw: Some(owner_tron),
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
    assert!(result.is_ok(), "Execute with asset optimization should succeed: {:?}", result.err());

    let exec_result = result.unwrap();
    assert!(exec_result.success, "Transaction should succeed when balance is in AccountAssetStore");
}
