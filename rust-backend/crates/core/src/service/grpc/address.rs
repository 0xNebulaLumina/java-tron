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
pub(in crate::service) fn add_tron_address_prefix(address: &Address) -> Vec<u8> {
    let mut result = Vec::with_capacity(21);
    result.push(0x41); // Add Tron address prefix
    result.extend_from_slice(address.as_slice());
    result
}
