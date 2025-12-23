//! Delegation store key generation functions.
//!
//! These functions generate storage keys matching java-tron's DelegationStore key formats.
//! All keys are string-based for compatibility with the Java implementation.
//!
//! Java reference: DelegationStore.java:148-170

/// Generate key for begin_cycle.
/// In Java, begin_cycle uses the raw address as key.
/// Java reference: DelegationStore.java:55-62
/// Key format: address bytes (21 bytes with 0x41 prefix)
pub fn delegation_begin_cycle_key(address: &[u8]) -> Vec<u8> {
    address.to_vec()
}

/// Generate key for end_cycle.
/// Java reference: DelegationStore.java:64-71, 160-162
/// Key format: "end-" + hex(address)
pub fn delegation_end_cycle_key(address: &[u8]) -> Vec<u8> {
    let hex_addr = hex::encode(address);
    format!("end-{}", hex_addr).into_bytes()
}

/// Generate key for account_vote snapshot.
/// Java reference: DelegationStore.java:86-97, 156-158
/// Key format: "{cycle}-{hex(address)}-account-vote"
pub fn delegation_account_vote_key(cycle: i64, address: &[u8]) -> Vec<u8> {
    let hex_addr = hex::encode(address);
    format!("{}-{}-account-vote", cycle, hex_addr).into_bytes()
}

/// Generate key for delegation reward.
/// Java reference: DelegationStore.java:35-53, 152-154
/// Key format: "{cycle}-{hex(address)}-reward"
pub fn delegation_reward_key(cycle: i64, witness_address: &[u8]) -> Vec<u8> {
    let hex_addr = hex::encode(witness_address);
    format!("{}-{}-reward", cycle, hex_addr).into_bytes()
}

/// Generate key for witness vote count.
/// Java reference: DelegationStore.java:73-84, 148-150
/// Key format: "{cycle}-{hex(address)}-vote"
pub fn delegation_witness_vote_key(cycle: i64, witness_address: &[u8]) -> Vec<u8> {
    let hex_addr = hex::encode(witness_address);
    format!("{}-{}-vote", cycle, hex_addr).into_bytes()
}

/// Generate key for witness Vi (vote index).
/// Java reference: DelegationStore.java:120-131, 168-170
/// Key format: "{cycle}-{hex(address)}-vi"
pub fn delegation_witness_vi_key(cycle: i64, witness_address: &[u8]) -> Vec<u8> {
    let hex_addr = hex::encode(witness_address);
    format!("{}-{}-vi", cycle, hex_addr).into_bytes()
}

/// Generate key for brokerage.
/// Java reference: DelegationStore.java:99-118, 164-166
/// Key format: "{cycle}-{hex(address)}-brokerage"
pub fn delegation_brokerage_key(cycle: i64, witness_address: &[u8]) -> Vec<u8> {
    let hex_addr = hex::encode(witness_address);
    format!("{}-{}-brokerage", cycle, hex_addr).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_begin_cycle_key() {
        let address = vec![0x41, 0x12, 0x34, 0x56, 0x78];
        let key = delegation_begin_cycle_key(&address);
        assert_eq!(key, address);
    }

    #[test]
    fn test_end_cycle_key() {
        let address = vec![0x41, 0x12, 0x34];
        let key = delegation_end_cycle_key(&address);
        let expected = b"end-411234".to_vec();
        assert_eq!(key, expected);
    }

    #[test]
    fn test_account_vote_key() {
        let address = vec![0x41, 0xab, 0xcd];
        let key = delegation_account_vote_key(100, &address);
        let expected = b"100-41abcd-account-vote".to_vec();
        assert_eq!(key, expected);
    }

    #[test]
    fn test_reward_key() {
        let address = vec![0x41, 0xef];
        let key = delegation_reward_key(50, &address);
        let expected = b"50-41ef-reward".to_vec();
        assert_eq!(key, expected);
    }

    #[test]
    fn test_witness_vote_key() {
        let address = vec![0x41, 0x00, 0x01];
        let key = delegation_witness_vote_key(25, &address);
        let expected = b"25-410001-vote".to_vec();
        assert_eq!(key, expected);
    }

    #[test]
    fn test_vi_key() {
        let address = vec![0x41, 0xff];
        let key = delegation_witness_vi_key(999, &address);
        let expected = b"999-41ff-vi".to_vec();
        assert_eq!(key, expected);
    }

    #[test]
    fn test_brokerage_key() {
        let address = vec![0x41, 0x01, 0x02];
        let key = delegation_brokerage_key(10, &address);
        let expected = b"10-410102-brokerage".to_vec();
        assert_eq!(key, expected);
    }

    #[test]
    fn test_brokerage_key_negative_cycle() {
        // Java uses cycle=-1 for default brokerage
        let address = vec![0x41, 0x01];
        let key = delegation_brokerage_key(-1, &address);
        let expected = b"-1-4101-brokerage".to_vec();
        assert_eq!(key, expected);
    }
}
