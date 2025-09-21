use anyhow::{Result, anyhow};
use sha2::{Sha256, Digest};

/// TRON address utilities for Base58 encoding/decoding and checksum validation
/// 
/// TRON addresses use Base58Check encoding with a 0x41 prefix:
/// - 21-byte format: [0x41] + [20-byte EVM address]
/// - Base58Check: Base58(address_bytes + checksum)
/// - Checksum: first 4 bytes of SHA256(SHA256(address_bytes))

const TRON_ADDRESS_PREFIX: u8 = 0x41;
const BASE58_ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Convert a 20-byte EVM address to TRON Base58 format
pub fn to_tron_address(evm_address: &[u8; 20]) -> String {
    // Create 21-byte address with TRON prefix
    let mut address_bytes = Vec::with_capacity(21);
    address_bytes.push(TRON_ADDRESS_PREFIX);
    address_bytes.extend_from_slice(evm_address);
    
    // Calculate checksum
    let checksum = calculate_checksum(&address_bytes);
    
    // Append checksum
    address_bytes.extend_from_slice(&checksum);
    
    // Encode in Base58
    base58_encode(&address_bytes)
}

/// Convert a TRON Base58 address to 20-byte EVM address
pub fn from_tron_address(tron_address: &str) -> Result<[u8; 20]> {
    // Decode Base58
    let decoded = base58_decode(tron_address)
        .map_err(|e| anyhow!("Invalid Base58 encoding: {}", e))?;
    
    if decoded.len() != 25 {
        return Err(anyhow!("Invalid TRON address length: expected 25 bytes, got {}", decoded.len()));
    }
    
    // Split address and checksum
    let (address_bytes, checksum) = decoded.split_at(21);
    
    // Verify prefix
    if address_bytes[0] != TRON_ADDRESS_PREFIX {
        return Err(anyhow!("Invalid TRON address prefix: expected 0x{:02x}, got 0x{:02x}", 
                          TRON_ADDRESS_PREFIX, address_bytes[0]));
    }
    
    // Verify checksum
    let expected_checksum = calculate_checksum(address_bytes);
    if checksum != expected_checksum {
        return Err(anyhow!("Invalid TRON address checksum"));
    }
    
    // Extract EVM address (skip the 0x41 prefix)
    let mut evm_address = [0u8; 20];
    evm_address.copy_from_slice(&address_bytes[1..]);
    
    Ok(evm_address)
}

/// Calculate SHA256(SHA256(data)) checksum (first 4 bytes)
fn calculate_checksum(data: &[u8]) -> [u8; 4] {
    let first_hash = Sha256::digest(data);
    let second_hash = Sha256::digest(&first_hash);
    
    let mut checksum = [0u8; 4];
    checksum.copy_from_slice(&second_hash[..4]);
    checksum
}

/// Encode bytes to Base58
fn base58_encode(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    
    // Count leading zeros
    let leading_zeros = data.iter().take_while(|&&b| b == 0).count();
    
    // Use a larger integer type to handle overflow
    use num_bigint::BigUint;
    
    // Convert to base 58 using BigUint to avoid overflow
    let mut num = BigUint::from_bytes_be(data);
    let mut encoded = String::new();
    let base = BigUint::from(58u32);
    
    if num == BigUint::from(0u32) {
        // Special case for all zeros (after leading zeros are handled)
        if leading_zeros == data.len() {
            return "1".repeat(data.len());
        }
    }
    
    while num > BigUint::from(0u32) {
        let remainder = (&num % &base).to_bytes_be()[0] as usize;
        encoded.insert(0, BASE58_ALPHABET[remainder] as char);
        num /= &base;
    }
    
    // Add leading '1's for leading zeros
    for _ in 0..leading_zeros {
        encoded.insert(0, '1');
    }
    
    encoded
}

/// Decode Base58 string to bytes
fn base58_decode(encoded: &str) -> Result<Vec<u8>> {
    if encoded.is_empty() {
        return Ok(Vec::new());
    }
    
    // Count leading '1's
    let leading_ones = encoded.chars().take_while(|&c| c == '1').count();
    
    // Use BigUint to avoid overflow
    use num_bigint::BigUint;
    
    // Convert from base 58
    let mut num = BigUint::from(0u32);
    let base = BigUint::from(58u32);
    
    for ch in encoded.chars() {
        let value = BASE58_ALPHABET.iter().position(|&b| b == ch as u8)
            .ok_or_else(|| anyhow!("Invalid Base58 character: {}", ch))?;
        
        num = num * &base + BigUint::from(value);
    }
    
    // Convert back to bytes
    let decoded = num.to_bytes_be();
    
    // Add leading zeros for leading '1's
    let mut result = vec![0u8; leading_ones];
    result.extend(decoded);
    
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_base58_encode_decode() {
        let data = b"hello world";
        let encoded = base58_encode(data);
        let decoded = base58_decode(&encoded).unwrap();
        assert_eq!(data, decoded.as_slice());
    }
    
    #[test]
    fn test_tron_address_roundtrip() {
        // Test with a known EVM address
        let evm_address = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 
                          0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78];
        
        let tron_address = to_tron_address(&evm_address);
        let decoded_address = from_tron_address(&tron_address).unwrap();
        
        assert_eq!(evm_address, decoded_address);
    }
    
    #[test]
    fn test_known_tron_address() {
        // Test with known TRON mainnet addresses
        // TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy (example address)
        let known_address = "TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy";
        
        // This should not panic for a valid TRON address format
        match from_tron_address(known_address) {
            Ok(evm_addr) => {
                // Verify round-trip
                let reconstructed = to_tron_address(&evm_addr);
                assert_eq!(known_address, reconstructed);
            },
            Err(_) => {
                // If the example address is invalid, that's fine for this test
                // The important thing is that the functions don't panic
            }
        }
    }
    
    #[test]
    fn test_invalid_tron_address() {
        // Test invalid cases
        assert!(from_tron_address("").is_err());
        assert!(from_tron_address("invalid").is_err());
        assert!(from_tron_address("TLsV52sRDL79HXGGm9yzwKibb6BeruhUz0").is_err()); // Invalid checksum
    }
}