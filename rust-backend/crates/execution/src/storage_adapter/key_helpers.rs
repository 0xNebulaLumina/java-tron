//! Key generation helpers for various TRON stores.
//!
//! ## Phase 0.2 Implementation
//!
//! This module provides key generation functions that match Java's capsule
//! key generation patterns exactly. Keys are typically either:
//! - Address-based (21-byte with 0x41 prefix)
//! - ID-based (8-byte big-endian long)
//! - Composite (prefix + address + suffix)
//!
//! ## References
//!
//! - ProposalCapsule.java: `createDbKey()` uses `ByteArray.fromLong(proposalId)`
//! - ExchangeCapsule.java: `createDbKey()` uses `ByteArray.fromLong(exchangeId)`
//! - DelegatedResourceCapsule.java: `createDbKeyV2()` uses prefix + from + to

/// Create a key from a big-endian long value (8 bytes).
/// Used by Proposal, Exchange, and other ID-based stores.
///
/// Java reference: `ByteArray.fromLong(long num)` in common/src/main/java/org/tron/common/utils/ByteArray.java
pub fn key_from_long(value: i64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// Create a key from an unsigned long value (8 bytes big-endian).
pub fn key_from_u64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// Proposal key: 8-byte big-endian proposal ID.
/// Java: ProposalCapsule.createDbKey() returns ByteArray.fromLong(proposalId)
pub fn proposal_key(proposal_id: i64) -> Vec<u8> {
    key_from_long(proposal_id)
}

/// Exchange key: 8-byte big-endian exchange ID.
/// Java: ExchangeCapsule.createDbKey() returns ByteArray.fromLong(exchangeId)
pub fn exchange_key(exchange_id: i64) -> Vec<u8> {
    key_from_long(exchange_id)
}

/// TRON address key: 21-byte with 0x41 prefix.
/// Standard format for account-based lookups.
pub fn tron_address_key(address: &[u8; 20]) -> Vec<u8> {
    let mut key = Vec::with_capacity(21);
    key.push(0x41);
    key.extend_from_slice(address);
    key
}

/// TRON address key from slice (assumes 20-byte input).
pub fn tron_address_key_from_slice(address: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(21);
    key.push(0x41);
    key.extend_from_slice(address);
    key
}

/// DelegatedResource V2 key prefix constants.
/// Java: DelegatedResourceCapsule.java
pub mod delegated_resource {
    /// V2 key prefix (unlock): 0x01 + from_address + to_address
    /// Java: DelegatedResourceCapsule.V2_PREFIX
    pub const V2_PREFIX: u8 = 0x01;
    /// V2 key prefix (lock): 0x02 + from_address + to_address
    /// Java: DelegatedResourceCapsule.V2_LOCK_PREFIX
    pub const V2_LOCK_PREFIX: u8 = 0x02;

    /// Create DelegatedResource V2 key (unlock).
    /// Format: 0x01 + from_address (21-byte) + to_address (21-byte)
    /// Total: 43 bytes
    pub fn create_db_key_v2_unlock(from_address: &[u8], to_address: &[u8]) -> Vec<u8> {
        let mut key = Vec::with_capacity(43);
        key.push(V2_PREFIX);
        key.extend_from_slice(from_address); // 21 bytes
        key.extend_from_slice(to_address); // 21 bytes
        key
    }

    /// Create DelegatedResource V2 key (lock).
    /// Format: 0x02 + from_address (21-byte) + to_address (21-byte)
    /// Total: 43 bytes
    pub fn create_db_key_v2_lock(from_address: &[u8], to_address: &[u8]) -> Vec<u8> {
        let mut key = Vec::with_capacity(43);
        key.push(V2_LOCK_PREFIX);
        key.extend_from_slice(from_address); // 21 bytes
        key.extend_from_slice(to_address); // 21 bytes
        key
    }

    /// Create DelegatedResource V2 key based on `lock` flag.
    pub fn create_db_key_v2(from_address: &[u8], to_address: &[u8], lock: bool) -> Vec<u8> {
        if lock {
            create_db_key_v2_lock(from_address, to_address)
        } else {
            create_db_key_v2_unlock(from_address, to_address)
        }
    }
}

/// DelegatedResourceAccountIndex key prefix constants.
/// Java: DelegatedResourceAccountIndexCapsule.java
pub mod delegated_resource_account_index {
    /// V2 key prefix for "from" index: 0x03 + from + to
    pub const V2_FROM_PREFIX: u8 = 0x03;
    /// V2 key prefix for "to" index: 0x04 + to + from
    pub const V2_TO_PREFIX: u8 = 0x04;

    /// Create index key for "from" direction.
    /// Format: 0x03 + from_address (21-byte) + to_address (21-byte)
    /// Total: 43 bytes
    pub fn create_db_key_v2_from(from_address: &[u8], to_address: &[u8]) -> Vec<u8> {
        let mut key = Vec::with_capacity(43);
        key.push(V2_FROM_PREFIX);
        key.extend_from_slice(from_address);
        key.extend_from_slice(to_address);
        key
    }

    /// Create index key for "to" direction.
    /// Format: 0x04 + to_address (21-byte) + from_address (21-byte)
    /// Total: 43 bytes
    pub fn create_db_key_v2_to(from_address: &[u8], to_address: &[u8]) -> Vec<u8> {
        let mut key = Vec::with_capacity(43);
        key.push(V2_TO_PREFIX);
        key.extend_from_slice(to_address);
        key.extend_from_slice(from_address);
        key
    }
}

/// Account ID index key: lowercase bytes of account ID.
/// Java: AccountIdIndexStore stores lowercase version of account ID.
pub fn account_id_index_key(account_id: &[u8]) -> Vec<u8> {
    // Java uses account_id.toLowerCase() but since we work with bytes,
    // we assume the caller provides already-normalized bytes.
    // If needed, implement ASCII lowercase conversion here.
    account_id.to_vec()
}

/// Market order key: 16-byte order ID.
/// Java: MarketOrderCapsule uses 16-byte order_id directly
pub fn market_order_key(order_id: &[u8; 16]) -> Vec<u8> {
    order_id.to_vec()
}

/// ABI store key: 20-byte contract address (no 0x41 prefix).
/// Java: AbiStore uses contract address directly
pub fn abi_key(contract_address: &[u8; 20]) -> Vec<u8> {
    contract_address.to_vec()
}

/// Contract store key: 20-byte contract address (no 0x41 prefix).
/// Java: ContractStore uses contract address directly
pub fn contract_key(contract_address: &[u8; 20]) -> Vec<u8> {
    contract_address.to_vec()
}

/// Code store key: 20-byte contract address (no 0x41 prefix).
/// Java: CodeStore uses contract address directly
pub fn code_key(contract_address: &[u8; 20]) -> Vec<u8> {
    contract_address.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_from_long() {
        // Test positive value
        let key = key_from_long(12345);
        assert_eq!(key.len(), 8);
        assert_eq!(key, [0, 0, 0, 0, 0, 0, 0x30, 0x39]);

        // Test zero
        let key = key_from_long(0);
        assert_eq!(key, [0, 0, 0, 0, 0, 0, 0, 0]);

        // Test negative (shouldn't happen for IDs but should work)
        let key = key_from_long(-1);
        assert_eq!(key, [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn test_tron_address_key() {
        let address = [0x12; 20];
        let key = tron_address_key(&address);
        assert_eq!(key.len(), 21);
        assert_eq!(key[0], 0x41);
        assert_eq!(&key[1..], &address);
    }

    #[test]
    fn test_delegated_resource_v2_keys() {
        let from_addr = vec![0x41; 21];
        let to_addr = vec![0x42; 21];

        let unlock_key = delegated_resource::create_db_key_v2_unlock(&from_addr, &to_addr);
        assert_eq!(unlock_key.len(), 43);
        assert_eq!(unlock_key[0], 0x01);
        assert_eq!(&unlock_key[1..22], from_addr.as_slice());
        assert_eq!(&unlock_key[22..43], to_addr.as_slice());

        let lock_key = delegated_resource::create_db_key_v2_lock(&from_addr, &to_addr);
        assert_eq!(lock_key.len(), 43);
        assert_eq!(lock_key[0], 0x02);
        assert_eq!(&lock_key[1..22], from_addr.as_slice());
        assert_eq!(&lock_key[22..43], to_addr.as_slice());
    }

    #[test]
    fn test_proposal_key() {
        let key = proposal_key(1);
        assert_eq!(key, [0, 0, 0, 0, 0, 0, 0, 1]);

        let key = proposal_key(256);
        assert_eq!(key, [0, 0, 0, 0, 0, 0, 1, 0]);
    }

    #[test]
    fn test_exchange_key() {
        let key = exchange_key(42);
        assert_eq!(key, [0, 0, 0, 0, 0, 0, 0, 42]);
    }
}
