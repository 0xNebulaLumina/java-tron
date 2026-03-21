use super::super::grpc::address::{
    add_tron_address_prefix, add_tron_address_prefix_with, strip_tron_address_prefix,
    validate_tron_address_prefix,
};
use super::super::*;
use revm_primitives::Address;

#[test]
fn test_tx_kind_conversion() {
    // Test that TxKind enum values can be converted
    assert_eq!(crate::backend::TxKind::NonVm as i32, 0);
    assert_eq!(crate::backend::TxKind::Vm as i32, 1);

    // Test conversion from i32
    assert_eq!(
        crate::backend::TxKind::try_from(0).unwrap(),
        crate::backend::TxKind::NonVm
    );
    assert_eq!(
        crate::backend::TxKind::try_from(1).unwrap(),
        crate::backend::TxKind::Vm
    );
}

#[test]
fn test_address_conversion_helpers() {
    // Test Tron address prefix stripping
    let tron_address_with_prefix = vec![
        0x41, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
        0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
    ];
    let stripped = strip_tron_address_prefix(&tron_address_with_prefix).unwrap();
    assert_eq!(stripped.len(), 20);
    assert_eq!(stripped[0], 0x12);

    let evm_address_no_prefix = vec![
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde,
        0xf0, 0x12, 0x34, 0x56, 0x78,
    ];
    let already_stripped = strip_tron_address_prefix(&evm_address_no_prefix).unwrap();
    assert_eq!(already_stripped.len(), 20);
    assert_eq!(already_stripped, &evm_address_no_prefix);

    // Test adding Tron address prefix
    let address = Address::from_slice(&[
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde,
        0xf0, 0x12, 0x34, 0x56, 0x78,
    ]);
    let with_prefix = add_tron_address_prefix(&address);
    assert_eq!(with_prefix.len(), 21);
    assert_eq!(with_prefix[0], 0x41);
    assert_eq!(&with_prefix[1..], address.as_slice());
}

#[test]
fn test_add_tron_address_prefix_with_configurable() {
    let address = Address::from_slice(&[
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde,
        0xf0, 0x12, 0x34, 0x56, 0x78,
    ]);

    // Test with mainnet prefix (0x41)
    let mainnet_addr = add_tron_address_prefix_with(&address, 0x41);
    assert_eq!(mainnet_addr.len(), 21);
    assert_eq!(mainnet_addr[0], 0x41);
    assert_eq!(&mainnet_addr[1..], address.as_slice());

    // Test with testnet prefix (0xa0)
    let testnet_addr = add_tron_address_prefix_with(&address, 0xa0);
    assert_eq!(testnet_addr.len(), 21);
    assert_eq!(testnet_addr[0], 0xa0);
    assert_eq!(&testnet_addr[1..], address.as_slice());
}

#[test]
fn test_validate_tron_address_prefix() {
    // Test that matching prefix is accepted
    let mainnet_address = vec![
        0x41, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
        0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
    ];
    let result = validate_tron_address_prefix(&mainnet_address, 0x41);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 20);

    // Test that mismatched prefix is rejected
    let result = validate_tron_address_prefix(&mainnet_address, 0xa0);
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress");

    // Test that testnet address is accepted with testnet prefix
    let testnet_address = vec![
        0xa0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
        0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
    ];
    let result = validate_tron_address_prefix(&testnet_address, 0xa0);
    assert!(result.is_ok());

    // Test that testnet address is rejected with mainnet prefix
    let result = validate_tron_address_prefix(&testnet_address, 0x41);
    assert!(result.is_err());

    // Test that 20-byte address (no prefix) is accepted
    let no_prefix = vec![
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde,
        0xf0, 0x12, 0x34, 0x56, 0x78,
    ];
    let result = validate_tron_address_prefix(&no_prefix, 0x41);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 20);
}

#[test]
fn test_any_type_url_matches() {
    // Test exact match
    assert!(BackendService::any_type_url_matches(
        "protocol.MarketCancelOrderContract",
        "protocol.MarketCancelOrderContract"
    ));

    // Test with type.googleapis.com prefix (common in protobuf Any)
    assert!(BackendService::any_type_url_matches(
        "type.googleapis.com/protocol.MarketCancelOrderContract",
        "protocol.MarketCancelOrderContract"
    ));

    // Test with custom prefix
    assert!(BackendService::any_type_url_matches(
        "custom.prefix/protocol.MarketCancelOrderContract",
        "protocol.MarketCancelOrderContract"
    ));

    // Test mismatch - different contract type
    assert!(!BackendService::any_type_url_matches(
        "type.googleapis.com/protocol.MarketSellAssetContract",
        "protocol.MarketCancelOrderContract"
    ));

    // Test mismatch - completely different type
    assert!(!BackendService::any_type_url_matches(
        "type.googleapis.com/protocol.TransferContract",
        "protocol.MarketCancelOrderContract"
    ));

    // Test empty type_url
    assert!(!BackendService::any_type_url_matches(
        "",
        "protocol.MarketCancelOrderContract"
    ));

    // Test for MarketSellAssetContract
    assert!(BackendService::any_type_url_matches(
        "type.googleapis.com/protocol.MarketSellAssetContract",
        "protocol.MarketSellAssetContract"
    ));
    assert!(!BackendService::any_type_url_matches(
        "type.googleapis.com/protocol.MarketCancelOrderContract",
        "protocol.MarketSellAssetContract"
    ));
}

#[test]
fn test_create_pair_key_validates_token_id_length() {
    // Token IDs within 19 bytes should succeed
    let valid_sell = b"1000001";
    let valid_buy = b"1000002";
    let result = BackendService::create_pair_key(valid_sell, valid_buy);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 38); // 19 * 2

    // Token ID exactly 19 bytes should succeed
    let max_len_sell = b"1234567890123456789";
    let max_len_buy = b"9876543210987654321";
    let result = BackendService::create_pair_key(max_len_sell, max_len_buy);
    assert!(result.is_ok());

    // Token ID over 19 bytes should fail
    let oversized_sell = b"12345678901234567890"; // 20 bytes
    let result = BackendService::create_pair_key(oversized_sell, valid_buy);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("sellTokenId length 20 exceeds maximum 19"));

    // buyTokenId over 19 bytes should also fail
    let result = BackendService::create_pair_key(valid_sell, oversized_sell);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("buyTokenId length 20 exceeds maximum 19"));
}

#[test]
fn test_create_pair_price_key_validates_token_id_length() {
    // Token IDs within 19 bytes should succeed
    let valid_sell = b"1000001";
    let valid_buy = b"1000002";
    let result = BackendService::create_pair_price_key(valid_sell, valid_buy, 100, 200);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 54); // 19 * 2 + 8 + 8

    // Token ID over 19 bytes should fail
    let oversized_sell = b"12345678901234567890"; // 20 bytes
    let result = BackendService::create_pair_price_key(oversized_sell, valid_buy, 100, 200);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("sellTokenId length 20 exceeds maximum 19"));

    // buyTokenId over 19 bytes should also fail
    let result = BackendService::create_pair_price_key(valid_sell, oversized_sell, 100, 200);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("buyTokenId length 20 exceeds maximum 19"));
}
