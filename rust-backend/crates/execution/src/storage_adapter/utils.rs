//! Utility functions for storage adapter.
//!
//! This module provides common utility functions used across the storage adapter:
//! - keccak256: Hash function for EVM operations
//! - to_tron_address: Convert EVM address to TRON base58 format
//! - from_tron_address: Convert TRON base58 back to EVM address (test-only)

use revm::primitives::{Address, B256};

/// Compute Keccak256 hash of data
pub fn keccak256(data: &[u8]) -> B256 {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(data);
    B256::from_slice(&hasher.finalize())
}

/// Convert an EVM address to a proper Tron format address (base58 with checksum)
pub fn to_tron_address(address: &Address) -> String {
    use sha2::{Digest, Sha256};

    // Create 21-byte address with 0x41 prefix
    let mut tron_addr = Vec::with_capacity(21);
    tron_addr.push(0x41);
    tron_addr.extend_from_slice(address.as_slice());

    // Calculate double SHA256 for checksum
    let mut hasher1 = Sha256::new();
    hasher1.update(&tron_addr);
    let hash1 = hasher1.finalize();

    let mut hasher2 = Sha256::new();
    hasher2.update(&hash1);
    let hash2 = hasher2.finalize();

    // Take first 4 bytes as checksum
    let mut addr_with_checksum = tron_addr;
    addr_with_checksum.extend_from_slice(&hash2[..4]);

    // Encode with base58
    bs58::encode(&addr_with_checksum).into_string()
}

/// Convert a Tron format address (base58 with checksum) back to EVM address for testing
#[cfg(test)]
pub fn from_tron_address(tron_address: &str) -> anyhow::Result<Address> {
    use sha2::{Digest, Sha256};

    // Decode base58
    let decoded = bs58::decode(tron_address).into_vec()
        .map_err(|e| anyhow::anyhow!("Invalid base58: {}", e))?;

    if decoded.len() != 25 {
        return Err(anyhow::anyhow!("Invalid Tron address length: expected 25 bytes, got {}", decoded.len()));
    }

    // Split address and checksum
    let (addr_bytes, checksum) = decoded.split_at(21);

    // Verify checksum
    let mut hasher1 = Sha256::new();
    hasher1.update(addr_bytes);
    let hash1 = hasher1.finalize();

    let mut hasher2 = Sha256::new();
    hasher2.update(&hash1);
    let hash2 = hasher2.finalize();

    if &hash2[..4] != checksum {
        return Err(anyhow::anyhow!("Invalid checksum"));
    }

    // Check 0x41 prefix
    if addr_bytes[0] != 0x41 {
        return Err(anyhow::anyhow!("Invalid Tron address prefix: expected 0x41, got 0x{:02x}", addr_bytes[0]));
    }

    // Return the 20-byte EVM address (without the 0x41 prefix)
    let mut evm_addr = [0u8; 20];
    evm_addr.copy_from_slice(&addr_bytes[1..]);
    Ok(Address::from(evm_addr))
}
