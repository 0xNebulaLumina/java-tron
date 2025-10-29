use super::super::*;
use tron_backend_execution::{TronTransaction, TxMetadata};
use revm_primitives::{Address, U256, Bytes};

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
        },
    };

    let bandwidth_with_data = BackendService::calculate_bandwidth_usage(&tx_with_data);
    assert_eq!(bandwidth_with_data, 60 + 4 + 65); // base_size + data_size + signature_size
}
