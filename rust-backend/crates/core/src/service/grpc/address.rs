// Address conversion helpers
// Functions for handling Tron address prefix operations

use revm_primitives::Address;

/// Strip Tron address prefix (0x41) from 21-byte address to get 20-byte EVM address
pub(in crate::service) fn strip_tron_address_prefix(address_bytes: &[u8]) -> Result<&[u8], String> {
    if address_bytes.len() == 21 && (address_bytes[0] == 0x41 || address_bytes[0] == 0xa0) {
        Ok(&address_bytes[1..]) // Skip the 0x41 prefix, return 20 bytes
    } else if address_bytes.len() == 20 {
        Ok(address_bytes) // Already 20 bytes, no prefix
    } else {
        Err(format!(
            "Invalid address length: expected 20 or 21 bytes (with 0x41/0xa0 prefix), got {}",
            address_bytes.len()
        ))
    }
}

/// Add Tron address prefix (0x41) to 20-byte EVM address to get 21-byte Tron address
/// Note: For production code, prefer `add_tron_address_prefix_with()` to use the configured network prefix.
pub(in crate::service) fn add_tron_address_prefix(address: &Address) -> Vec<u8> {
    add_tron_address_prefix_with(address, 0x41)
}

/// Add a specific Tron address prefix to 20-byte EVM address to get 21-byte Tron address.
/// Use this when you need to match the network-specific prefix (0x41 for mainnet, 0xa0 for testnet).
pub(in crate::service) fn add_tron_address_prefix_with(address: &Address, prefix: u8) -> Vec<u8> {
    let mut result = Vec::with_capacity(21);
    result.push(prefix);
    result.extend_from_slice(address.as_slice());
    result
}

/// Validate that address has correct prefix for the network.
/// Returns the 20-byte EVM address if valid, or an error if prefix doesn't match.
pub(in crate::service) fn validate_tron_address_prefix(address_bytes: &[u8], expected_prefix: u8) -> Result<&[u8], String> {
    if address_bytes.len() == 21 && address_bytes[0] == expected_prefix {
        Ok(&address_bytes[1..])
    } else if address_bytes.len() == 21 {
        Err("Invalid ownerAddress".to_string())
    } else if address_bytes.len() == 20 {
        Ok(address_bytes) // Already 20 bytes, no prefix
    } else {
        Err("Invalid ownerAddress".to_string())
    }
}
