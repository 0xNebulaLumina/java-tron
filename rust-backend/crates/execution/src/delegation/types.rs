//! Delegation types for TRON staking rewards.
//!
//! These types match the java-tron DelegationStore data structures.

use anyhow::Result;
use num_bigint::BigInt;
use revm::primitives::Address;

/// REMARK value used in delegation store to indicate uninitialized/invalid state.
/// Java reference: DelegationStore.java:20
pub const DELEGATION_STORE_REMARK: i64 = -1;

/// Default brokerage percentage for witnesses (20%).
/// Java reference: DelegationStore.java:21
pub const DEFAULT_BROKERAGE: i32 = 20;

/// Decimal precision for Vi reward calculation (10^18).
/// Java reference: DelegationStore.java:22
pub const DECIMAL_OF_VI_REWARD: u128 = 1_000_000_000_000_000_000;

/// Vote entry for delegation: witness address and vote count.
/// Mirrors java-tron's Vote protobuf message.
#[derive(Clone, Debug)]
pub struct DelegationVote {
    /// Witness address receiving the votes (20-byte EVM format)
    pub vote_address: Address,
    /// Number of votes cast for this witness
    pub vote_count: i64,
}

impl DelegationVote {
    pub fn new(vote_address: Address, vote_count: i64) -> Self {
        Self {
            vote_address,
            vote_count,
        }
    }
}

/// Account vote snapshot for delegation tracking.
/// Captures the voting state at a specific cycle for reward computation.
/// Mirrors java-tron's AccountCapsule used in DelegationStore.setAccountVote()
#[derive(Clone, Debug)]
pub struct AccountVoteSnapshot {
    /// Account address (owner of the votes)
    pub address: Address,
    /// List of votes (witness → vote count)
    pub votes: Vec<DelegationVote>,
}

impl AccountVoteSnapshot {
    pub fn new(address: Address, votes: Vec<DelegationVote>) -> Self {
        Self { address, votes }
    }

    /// Check if the account has any votes
    pub fn has_votes(&self) -> bool {
        !self.votes.is_empty()
    }

    /// Serialize to bytes for storage (Account protobuf format).
    /// We only need to preserve the votes field for delegation purposes.
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();

        // Field 1: address (length-delimited, 21 bytes with 0x41 prefix)
        let mut tron_address = Vec::with_capacity(21);
        tron_address.push(0x41); // Tron address prefix
        tron_address.extend_from_slice(self.address.as_slice());

        data.push(0x0a); // field 1, wire type 2 (length-delimited)
        write_varint(&mut data, tron_address.len() as u64);
        data.extend_from_slice(&tron_address);

        // Field 5: votes (repeated Vote)
        for vote in &self.votes {
            let vote_bytes = serialize_vote(vote);
            data.push(0x2a); // field 5, wire type 2 (length-delimited)
            write_varint(&mut data, vote_bytes.len() as u64);
            data.extend_from_slice(&vote_bytes);
        }

        data
    }

    /// Deserialize from bytes (Account protobuf format).
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        let mut pos = 0;
        let mut address: Option<Address> = None;
        let mut votes = Vec::new();

        while pos < data.len() {
            // Read field header
            let (field_header, new_pos) = read_varint(data, pos)?;
            pos = new_pos;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // address (length-delimited)
                    let (length, new_pos) = read_varint(data, pos)?;
                    pos = new_pos;
                    if pos + length as usize > data.len() {
                        return Err(anyhow::anyhow!("Invalid address length"));
                    }
                    let addr_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    // Remove 0x41 prefix if present
                    let evm_addr = if addr_bytes.len() == 21 && addr_bytes[0] == 0x41 {
                        &addr_bytes[1..]
                    } else if addr_bytes.len() == 20 {
                        addr_bytes
                    } else {
                        return Err(anyhow::anyhow!(
                            "Invalid address length: {}",
                            addr_bytes.len()
                        ));
                    };

                    let mut addr = [0u8; 20];
                    addr.copy_from_slice(evm_addr);
                    address = Some(Address::from(addr));
                }
                (5, 2) => {
                    // votes (length-delimited, repeated)
                    let (length, new_pos) = read_varint(data, pos)?;
                    pos = new_pos;
                    if pos + length as usize > data.len() {
                        return Err(anyhow::anyhow!("Invalid votes length"));
                    }
                    let vote_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;
                    votes.push(deserialize_vote(vote_bytes)?);
                }
                _ => {
                    // Skip other fields
                    pos = skip_field(data, pos, wire_type)?;
                }
            }
        }

        Ok(AccountVoteSnapshot::new(
            address.ok_or_else(|| anyhow::anyhow!("Missing address"))?,
            votes,
        ))
    }
}

/// Serialize a single Vote to protobuf format.
/// Vote protobuf:
///   bytes vote_address = 1;  (length-delimited, 21-byte Tron address)
///   int64 vote_count = 2;    (varint)
fn serialize_vote(vote: &DelegationVote) -> Vec<u8> {
    let mut data = Vec::new();

    // Field 1: vote_address (length-delimited, 21 bytes with 0x41 prefix)
    let mut tron_address = Vec::with_capacity(21);
    tron_address.push(0x41); // Tron address prefix
    tron_address.extend_from_slice(vote.vote_address.as_slice());

    data.push(0x0a); // field 1, wire type 2 (length-delimited)
    write_varint(&mut data, tron_address.len() as u64);
    data.extend_from_slice(&tron_address);

    // Field 2: vote_count (varint)
    data.push(0x10); // field 2, wire type 0 (varint)
    write_varint_signed(&mut data, vote.vote_count);

    data
}

/// Deserialize a single Vote from protobuf format.
fn deserialize_vote(data: &[u8]) -> Result<DelegationVote> {
    let mut pos = 0;
    let mut vote_address: Option<Address> = None;
    let mut vote_count: Option<i64> = None;

    while pos < data.len() {
        let (field_header, new_pos) = read_varint(data, pos)?;
        pos = new_pos;

        let field_number = field_header >> 3;
        let wire_type = field_header & 0x7;

        match (field_number, wire_type) {
            (1, 2) => {
                // vote_address (length-delimited)
                let (length, new_pos) = read_varint(data, pos)?;
                pos = new_pos;
                if pos + length as usize > data.len() {
                    return Err(anyhow::anyhow!("Invalid vote_address length"));
                }
                let addr_bytes = &data[pos..pos + length as usize];
                pos += length as usize;

                // Remove 0x41 prefix if present
                let evm_addr = if addr_bytes.len() == 21 && addr_bytes[0] == 0x41 {
                    &addr_bytes[1..]
                } else if addr_bytes.len() == 20 {
                    addr_bytes
                } else {
                    return Err(anyhow::anyhow!(
                        "Invalid vote_address length: {}",
                        addr_bytes.len()
                    ));
                };

                let mut addr = [0u8; 20];
                addr.copy_from_slice(evm_addr);
                vote_address = Some(Address::from(addr));
            }
            (2, 0) => {
                // vote_count (varint)
                let (count, new_pos) = read_varint(data, pos)?;
                pos = new_pos;
                vote_count = Some(count as i64);
            }
            _ => {
                // Skip unknown fields
                pos = skip_field(data, pos, wire_type)?;
            }
        }
    }

    Ok(DelegationVote::new(
        vote_address.ok_or_else(|| anyhow::anyhow!("Missing vote_address"))?,
        vote_count.ok_or_else(|| anyhow::anyhow!("Missing vote_count"))?,
    ))
}

// --- Protobuf helpers ---

fn write_varint(output: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        output.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }
    output.push(value as u8);
}

fn write_varint_signed(output: &mut Vec<u8>, value: i64) {
    // Protobuf int64 uses standard varint encoding (not zigzag)
    write_varint(output, value as u64);
}

fn read_varint(data: &[u8], mut pos: usize) -> Result<(u64, usize)> {
    let mut result = 0u64;
    let mut shift = 0;

    while pos < data.len() {
        let byte = data[pos];
        pos += 1;

        result |= ((byte & 0x7F) as u64) << shift;

        if (byte & 0x80) == 0 {
            return Ok((result, pos));
        }

        shift += 7;
        if shift >= 64 {
            return Err(anyhow::anyhow!("Varint too long"));
        }
    }

    Err(anyhow::anyhow!(
        "Unexpected end of data while reading varint"
    ))
}

fn skip_field(data: &[u8], pos: usize, wire_type: u64) -> Result<usize> {
    match wire_type {
        0 => {
            // Varint
            let (_, new_pos) = read_varint(data, pos)?;
            Ok(new_pos)
        }
        1 => {
            // 64-bit
            Ok(pos + 8)
        }
        2 => {
            // Length-delimited
            let (length, new_pos) = read_varint(data, pos)?;
            Ok(new_pos + length as usize)
        }
        5 => {
            // 32-bit
            Ok(pos + 4)
        }
        _ => Err(anyhow::anyhow!("Unknown wire type: {}", wire_type)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delegation_vote_creation() {
        let addr = Address::from_slice(&[0x12; 20]);
        let vote = DelegationVote::new(addr, 1000);
        assert_eq!(vote.vote_address, addr);
        assert_eq!(vote.vote_count, 1000);
    }

    #[test]
    fn test_account_vote_snapshot_serialization() {
        let owner = Address::from_slice(&[0x01; 20]);
        let witness1 = Address::from_slice(&[0x02; 20]);
        let witness2 = Address::from_slice(&[0x03; 20]);

        let snapshot = AccountVoteSnapshot::new(
            owner,
            vec![
                DelegationVote::new(witness1, 500),
                DelegationVote::new(witness2, 300),
            ],
        );

        let serialized = snapshot.serialize();
        assert!(!serialized.is_empty());

        let deserialized = AccountVoteSnapshot::deserialize(&serialized).unwrap();
        assert_eq!(deserialized.address, owner);
        assert_eq!(deserialized.votes.len(), 2);
        assert_eq!(deserialized.votes[0].vote_address, witness1);
        assert_eq!(deserialized.votes[0].vote_count, 500);
        assert_eq!(deserialized.votes[1].vote_address, witness2);
        assert_eq!(deserialized.votes[1].vote_count, 300);
    }

    #[test]
    fn test_constants() {
        assert_eq!(DELEGATION_STORE_REMARK, -1);
        assert_eq!(DEFAULT_BROKERAGE, 20);
        assert_eq!(DECIMAL_OF_VI_REWARD, 1_000_000_000_000_000_000);
    }
}
