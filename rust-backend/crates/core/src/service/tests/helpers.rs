use super::super::*;
use super::super::grpc::address::{strip_tron_address_prefix, add_tron_address_prefix, add_tron_address_prefix_with, validate_tron_address_prefix};
use revm_primitives::Address;

#[test]
fn test_tx_kind_conversion() {
    // Test that TxKind enum values can be converted
    assert_eq!(crate::backend::TxKind::NonVm as i32, 0);
    assert_eq!(crate::backend::TxKind::Vm as i32, 1);

    // Test conversion from i32
    assert_eq!(crate::backend::TxKind::try_from(0).unwrap(), crate::backend::TxKind::NonVm);
    assert_eq!(crate::backend::TxKind::try_from(1).unwrap(), crate::backend::TxKind::Vm);
}

#[test]
fn test_address_conversion_helpers() {
    // Test Tron address prefix stripping
    let tron_address_with_prefix = vec![0x41, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78];
    let stripped = strip_tron_address_prefix(&tron_address_with_prefix).unwrap();
    assert_eq!(stripped.len(), 20);
    assert_eq!(stripped[0], 0x12);

    let evm_address_no_prefix = vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78];
    let already_stripped = strip_tron_address_prefix(&evm_address_no_prefix).unwrap();
    assert_eq!(already_stripped.len(), 20);
    assert_eq!(already_stripped, &evm_address_no_prefix);

    // Test adding Tron address prefix
    let address = Address::from_slice(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78]);
    let with_prefix = add_tron_address_prefix(&address);
    assert_eq!(with_prefix.len(), 21);
    assert_eq!(with_prefix[0], 0x41);
    assert_eq!(&with_prefix[1..], address.as_slice());
}

#[test]
fn test_add_tron_address_prefix_with_configurable() {
    let address = Address::from_slice(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78]);

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
    let mainnet_address = vec![0x41, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78];
    let result = validate_tron_address_prefix(&mainnet_address, 0x41);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 20);

    // Test that mismatched prefix is rejected
    let result = validate_tron_address_prefix(&mainnet_address, 0xa0);
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress");

    // Test that testnet address is accepted with testnet prefix
    let testnet_address = vec![0xa0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78];
    let result = validate_tron_address_prefix(&testnet_address, 0xa0);
    assert!(result.is_ok());

    // Test that testnet address is rejected with mainnet prefix
    let result = validate_tron_address_prefix(&testnet_address, 0x41);
    assert!(result.is_err());

    // Test that 20-byte address (no prefix) is accepted
    let no_prefix = vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78];
    let result = validate_tron_address_prefix(&no_prefix, 0x41);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 20);
}
