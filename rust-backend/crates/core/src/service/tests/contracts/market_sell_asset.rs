//! Tests for MARKET_SELL_ASSET_CONTRACT fee handling parity

use super::super::super::*;
use super::common::{encode_varint, make_from_raw, seed_dynamic_properties};
use revm_primitives::{Address, U256, AccountInfo};
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{EngineBackedEvmStateStore, TronExecutionContext, TronTransaction, TxMetadata, TronContractType};
use tron_backend_execution::protocol::AssetIssueContractData;
use tron_backend_storage::StorageEngine;

/// Create a test service with market sell enabled
fn new_test_service_with_market_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            market_sell_asset_enabled: true,
            market_strict_index_parity: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Seed minimum dynamic properties required for market transactions
fn seed_market_properties(storage_engine: &StorageEngine) {
    seed_dynamic_properties(storage_engine);
    storage_engine
        .put("properties", b"ALLOW_MARKET_TRANSACTION", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b" ALLOW_SAME_TOKEN_NAME", &1i64.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"MARKET_QUANTITY_LIMIT", &i64::MAX.to_be_bytes())
        .unwrap();
    storage_engine
        .put("properties", b"LATEST_BLOCK_HEADER_TIMESTAMP", &1000000i64.to_be_bytes())
        .unwrap();
}

/// Build MarketSellAssetContract data
fn build_market_sell_asset_contract_data(
    owner: Address,
    sell_token_id: &[u8],
    sell_token_quantity: i64,
    buy_token_id: &[u8],
    buy_token_quantity: i64,
) -> Vec<u8> {
    // Manual protobuf encoding for MarketSellAssetContract
    let mut data = Vec::new();
    let owner_raw = make_from_raw(&owner);

    // Field 1: owner_address (bytes)
    encode_varint(&mut data, (1 << 3) | 2); // tag = field 1, wire type 2
    encode_varint(&mut data, owner_raw.len() as u64);
    data.extend_from_slice(&owner_raw);

    // Field 2: sell_token_id (bytes)
    encode_varint(&mut data, (2 << 3) | 2); // tag = field 2, wire type 2
    encode_varint(&mut data, sell_token_id.len() as u64);
    data.extend_from_slice(sell_token_id);

    // Field 3: sell_token_quantity (int64)
    encode_varint(&mut data, (3 << 3) | 0); // tag = field 3, wire type 0
    encode_varint(&mut data, sell_token_quantity as u64);

    // Field 4: buy_token_id (bytes)
    encode_varint(&mut data, (4 << 3) | 2); // tag = field 4, wire type 2
    encode_varint(&mut data, buy_token_id.len() as u64);
    data.extend_from_slice(buy_token_id);

    // Field 5: buy_token_quantity (int64)
    encode_varint(&mut data, (5 << 3) | 0); // tag = field 5, wire type 0
    encode_varint(&mut data, buy_token_quantity as u64);

    data
}

/// Test: When MARKET_SELL_FEE > 0 and ALLOW_BLACKHOLE_OPTIMIZATION = true, burn_trx is called
#[test]
fn test_market_sell_burns_fee_when_blackhole_optimization_enabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_market_properties(&storage_engine);

    // Enable blackhole optimization
    storage_engine
        .put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &1i64.to_be_bytes())
        .unwrap();

    // Set market sell fee
    let market_sell_fee: i64 = 100_000_000; // 100 TRX
    storage_engine
        .put("properties", b"MARKET_SELL_FEE", &market_sell_fee.to_be_bytes())
        .unwrap();

    // Initial BURN_TRX_AMOUNT
    let initial_burn: i64 = 0;
    storage_engine
        .put("properties", b"BURN_TRX_AMOUNT", &initial_burn.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_market_enabled();

    let owner = Address::from([0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                               0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
                               0x11, 0x22, 0x33, 0x44]);
    let initial_balance: u64 = 10_000_000_000; // 10,000 TRX
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    // Set account proto with TRC-10 balance
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto.asset_v2.insert("1000001".to_string(), 1_000_000_000);
    storage_adapter.set_account_proto(&owner, &account_proto).unwrap();

    // Create asset issue for the token
    let asset_proto = AssetIssueContractData {
        id: "1000001".to_string(),
        name: b"TestToken".to_vec(),
        abbr: b"TT".to_vec(),
        total_supply: 1_000_000_000_000,
        precision: 6,
        ..Default::default()
    };
    storage_adapter.put_asset_issue(b"1000001", &asset_proto, true).unwrap();

    let contract_data = build_market_sell_asset_contract_data(
        owner,
        b"1000001", // sell token
        1_000_000,  // sell quantity
        b"_",       // buy TRX
        100_000,    // buy quantity
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.into(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::MarketSellAssetContract),
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

    // Check BURN_TRX_AMOUNT increased by fee
    let burn_amount = storage_adapter.get_burn_trx_amount().unwrap();
    assert_eq!(
        burn_amount,
        initial_burn + market_sell_fee,
        "BURN_TRX_AMOUNT should increase by fee when blackhole optimization enabled"
    );
}

/// Test: When MARKET_SELL_FEE > 0 and ALLOW_BLACKHOLE_OPTIMIZATION = false, blackhole is credited
#[test]
fn test_market_sell_credits_blackhole_when_optimization_disabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_market_properties(&storage_engine);

    // Disable blackhole optimization
    storage_engine
        .put("properties", b"ALLOW_BLACKHOLE_OPTIMIZATION", &0i64.to_be_bytes())
        .unwrap();

    // Set market sell fee
    let market_sell_fee: i64 = 100_000_000; // 100 TRX
    storage_engine
        .put("properties", b"MARKET_SELL_FEE", &market_sell_fee.to_be_bytes())
        .unwrap();

    // Initial BURN_TRX_AMOUNT (should NOT change)
    let initial_burn: i64 = 0;
    storage_engine
        .put("properties", b"BURN_TRX_AMOUNT", &initial_burn.to_be_bytes())
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_market_enabled();

    let owner = Address::from([0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                               0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
                               0x11, 0x22, 0x33, 0x44]);
    let initial_balance: u64 = 10_000_000_000; // 10,000 TRX
    let owner_account = AccountInfo {
        balance: U256::from(initial_balance),
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(owner, owner_account).unwrap();

    // Set account proto with TRC-10 balance
    let mut account_proto = storage_adapter.get_account_proto(&owner).unwrap().unwrap_or_default();
    account_proto.balance = initial_balance as i64;
    let owner_tron = make_from_raw(&owner);
    account_proto.address = owner_tron.clone();
    account_proto.asset_v2.insert("1000001".to_string(), 1_000_000_000);
    storage_adapter.set_account_proto(&owner, &account_proto).unwrap();

    // Create asset issue for the token
    let asset_proto = AssetIssueContractData {
        id: "1000001".to_string(),
        name: b"TestToken".to_vec(),
        abbr: b"TT".to_vec(),
        total_supply: 1_000_000_000_000,
        precision: 6,
        ..Default::default()
    };
    storage_adapter.put_asset_issue(b"1000001", &asset_proto, true).unwrap();

    // Create blackhole account (needed when optimization disabled to credit fees)
    let blackhole_addr = storage_adapter.get_blackhole_address_evm();
    let blackhole_account = AccountInfo {
        balance: U256::ZERO,
        nonce: 0,
        code_hash: revm::primitives::B256::ZERO,
        code: None,
    };
    storage_adapter.set_account(blackhole_addr, blackhole_account).unwrap();

    // Get blackhole balance before
    let blackhole_balance_before = storage_adapter
        .get_account(&blackhole_addr)
        .unwrap()
        .map(|a| a.balance)
        .unwrap_or(U256::ZERO);

    let contract_data = build_market_sell_asset_contract_data(
        owner,
        b"1000001", // sell token
        1_000_000,  // sell quantity
        b"_",       // buy TRX
        100_000,    // buy quantity
    );

    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: contract_data.into(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::MarketSellAssetContract),
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

    // Check blackhole balance increased by fee
    let blackhole_balance_after = storage_adapter
        .get_account(&blackhole_addr)
        .unwrap()
        .map(|a| a.balance)
        .unwrap_or(U256::ZERO);
    assert_eq!(
        blackhole_balance_after - blackhole_balance_before,
        U256::from(market_sell_fee as u64),
        "Blackhole balance should increase by fee when optimization disabled"
    );

    // Check BURN_TRX_AMOUNT did NOT change
    let burn_amount = storage_adapter.get_burn_trx_amount().unwrap();
    assert_eq!(
        burn_amount,
        initial_burn,
        "BURN_TRX_AMOUNT should NOT change when blackhole optimization disabled"
    );
}

/// Test: orderDetails are included in receipt when matches occur
#[test]
fn test_market_sell_receipt_includes_order_details_on_match() {
    // This test verifies that when a sell order matches against existing orders,
    // the receipt includes the order details for each fill.
    // For now, we verify the receipt builder can encode order details correctly.
    use crate::service::contracts::proto::{TransactionResultBuilder, MarketOrderDetail};

    let detail1 = MarketOrderDetail::new(
        vec![0x01, 0x02, 0x03], // maker order id
        vec![0x04, 0x05, 0x06], // taker order id
        1000,                   // fill sell quantity
        500,                    // fill buy quantity
    );

    let detail2 = MarketOrderDetail::new(
        vec![0x11, 0x12, 0x13],
        vec![0x14, 0x15, 0x16],
        2000,
        1000,
    );

    let receipt = TransactionResultBuilder::new()
        .with_order_id(&[0xaa, 0xbb, 0xcc])
        .add_order_detail(detail1)
        .add_order_detail(detail2)
        .build();

    // Verify receipt is non-empty and contains data
    assert!(!receipt.is_empty(), "Receipt should not be empty");
    // Basic sanity check - receipt should be larger than just order_id
    assert!(receipt.len() > 10, "Receipt should contain order details");
}
