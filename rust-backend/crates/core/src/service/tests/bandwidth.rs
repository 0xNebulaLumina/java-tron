use super::super::*;
use tron_backend_execution::{AccountAext, EngineBackedEvmStateStore, TronTransaction, TxMetadata};
use tron_backend_storage::StorageEngine;
use revm_primitives::{AccountInfo, Address, Bytes, U256};

#[test]
fn test_calculate_bandwidth_usage() {
    // Test basic transaction
    let tx = TronTransaction {
        from: Address::ZERO,
        to: Some(Address::ZERO),
        value: U256::from(100),
        data: Bytes::new(),
        gas_limit: 21000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: None,
            asset_id: None,
            ..Default::default()
        },
    };

    let bandwidth = BackendService::calculate_bandwidth_usage(&tx);
    assert_eq!(bandwidth, 60 + 0 + 65); // base_size + data_size + signature_size

    // Test transaction with data
    let tx_with_data = TronTransaction {
        from: Address::ZERO,
        to: Some(Address::ZERO),
        value: U256::from(100),
        data: Bytes::from(vec![0x60, 0x60, 0x60, 0x40]), // 4 bytes of data
        gas_limit: 21000,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: None,
            asset_id: None,
            ..Default::default()
        },
    };

    let bandwidth_with_data = BackendService::calculate_bandwidth_usage(&tx_with_data);
    assert_eq!(bandwidth_with_data, 60 + 4 + 65); // base_size + data_size + signature_size
}

#[test]
fn test_delegate_resource_window_normalization_for_optimized_window_size() {
    use tron_backend_execution::protocol::account::FreezeV2;
    use tron_backend_execution::protocol::Account;

    // If window_size is stored in V2/optimized form (slots * 1000), delegate validation must
    // normalize it back to logical slots to match Java's AccountCapsule.getWindowSize().
    let head_slot: i64 = 1_000_000;

    let mut account = Account::default();
    account.net_usage = 1000;
    account.latest_consume_time = head_slot - 14_400; // half window (28800 / 2)
    account.net_window_size = 28_800_000; // 28800 * 1000 (WINDOW_SIZE_PRECISION)
    account.net_window_optimized = true;
    account.frozen_v2.push(FreezeV2 {
        r#type: 0, // BANDWIDTH
        amount: 800_000_000,
    });

    // With correct normalization, decayed usage is ~500 bytes, scaled to 500_000_000 SUN usage,
    // leaving 300_000_000 SUN available.
    let available = BackendService::compute_available_freeze_v2_bandwidth(&account, 1, 1, head_slot);
    assert_eq!(available, 300_000_000);
}

#[test]
fn test_apply_bandwidth_aext_updates_account_proto_window_raw() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let address = Address::from([3u8; 20]);
    storage_adapter
        .set_account(
            address,
            AccountInfo {
                balance: U256::from(1u64),
                nonce: 0,
                code_hash: revm_primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let aext = AccountAext {
        net_usage: 123,
        free_net_usage: 456,
        energy_usage: 0,
        latest_consume_time: 111,
        latest_consume_free_time: 222,
        latest_consume_time_for_energy: 0,
        net_window_size: 28800, // logical slots (normalized)
        net_window_optimized: true,
        energy_window_size: 0,
        energy_window_optimized: false,
    };

    storage_adapter
        .apply_bandwidth_aext_to_account_proto(&address, &aext)
        .unwrap();

    let proto = storage_adapter.get_account_proto(&address).unwrap().unwrap();
    assert_eq!(proto.net_usage, 123);
    assert_eq!(proto.free_net_usage, 456);
    assert_eq!(proto.latest_consume_time, 111);
    assert_eq!(proto.latest_consume_free_time, 222);
    assert!(proto.net_window_optimized);
    assert_eq!(proto.net_window_size, 28_800_000);
}
