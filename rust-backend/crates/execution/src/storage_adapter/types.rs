//! Domain types for EVM state storage.
//!
//! This module contains all data structures used for TRON-specific state management:
//! - WitnessInfo: Witness (Super Representative) information
//! - FreezeRecord: Frozen balance tracking for resource acquisition
//! - AccountAext: Account extension fields for bandwidth/energy tracking
//! - Vote/VotesRecord: Voting mechanism for witness selection
//! - StateChangeRecord: State change tracking for debugging and verification

use anyhow::Result;
use revm::primitives::{AccountInfo, Address, U256};

/// TRON Witness information - equivalent to WitnessCapsule in Java
#[derive(Debug, Clone)]
pub struct WitnessInfo {
    pub address: Address,     // 20-byte witness address (owner address)
    pub url: String,          // Witness URL
    pub vote_count: u64,      // Total votes received
}

impl WitnessInfo {
    pub fn new(address: Address, url: String, vote_count: u64) -> Self {
        Self {
            address,
            url,
            vote_count,
        }
    }

    /// Serialize witness info to Java-compatible protobuf format
    pub fn serialize(&self) -> Vec<u8> {
        self.serialize_with_prefix(0x41)
    }

    /// Serialize witness info to Java-compatible protobuf format with a specific network prefix.
    ///
    /// java-tron stores witness addresses as 21-byte TRON addresses (prefix + 20 bytes).
    pub fn serialize_with_prefix(&self, address_prefix: u8) -> Vec<u8> {
        use prost::Message;
        use crate::protocol::Witness;

        // Build TRON address (21 bytes: network prefix + 20-byte address)
        let mut tron_address = Vec::with_capacity(21);
        tron_address.push(address_prefix);
        tron_address.extend_from_slice(self.address.as_slice());

        // Convert vote_count to i64 (panic if exceeds i64::MAX)
        let vote_count_i64 = self.vote_count.try_into()
            .expect("vote_count exceeds i64::MAX");

        // Build protobuf Witness message
        let witness = Witness {
            address: tron_address,
            vote_count: vote_count_i64,
            pub_key: vec![], // Empty, not used in current implementation
            url: self.url.clone(),
            total_produced: 0, // Default
            total_missed: 0,   // Default
            latest_block_num: 0, // Default
            latest_slot_num: 0,  // Default
            // java-tron does not set this field for WitnessCreate writes (default false).
            // Keep default for fixture parity.
            is_jobs: false,
        };

        // Encode to bytes
        witness.encode_to_vec()
    }

    /// Deserialize witness info from Java protobuf format
    /// Returns WitnessInfo if successful, otherwise returns error for fallback
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        use prost::Message;
        use crate::protocol::Witness;

        // Try to decode as protocol.Witness protobuf
        let witness = Witness::decode(data)
            .map_err(|e| anyhow::anyhow!("Protobuf decode failed: {}", e))?;

        // Extract and validate address
        let address = if witness.address.len() == 21 && (witness.address[0] == 0x41 || witness.address[0] == 0xa0) {
            // TRON format: 21 bytes with network prefix (0x41 mainnet / 0xa0 testnet)
            let mut addr_bytes = [0u8; 20];
            addr_bytes.copy_from_slice(&witness.address[1..21]);
            Address::from(addr_bytes)
        } else if witness.address.len() == 20 {
            // Already 20-byte format
            let mut addr_bytes = [0u8; 20];
            addr_bytes.copy_from_slice(&witness.address[..20]);
            Address::from(addr_bytes)
        } else {
            return Err(anyhow::anyhow!(
                "Invalid address length in protobuf: {} (expected 20 or 21)",
                witness.address.len()
            ));
        };

        // Extract URL (string field)
        let url = witness.url;

        // Extract voteCount (int64 -> u64)
        let vote_count = if witness.vote_count < 0 {
            return Err(anyhow::anyhow!("Negative voteCount in protobuf: {}", witness.vote_count));
        } else {
            witness.vote_count as u64
        };

        Ok(WitnessInfo::new(address, url, vote_count))
    }
}

/// TRON Freeze record - tracks frozen balance for resource acquisition
#[derive(Debug, Clone)]
pub struct FreezeRecord {
    pub frozen_amount: u64,        // Total frozen TRX in SUN
    pub expiration_timestamp: i64, // Milliseconds since epoch
}

impl FreezeRecord {
    pub fn new(frozen_amount: u64, expiration_timestamp: i64) -> Self {
        Self {
            frozen_amount,
            expiration_timestamp,
        }
    }

    /// Serialize freeze record to bytes for storage
    pub fn serialize(&self) -> Vec<u8> {
        // Format: [frozen_amount(8)] + [expiration_timestamp(8)]
        let mut result = Vec::with_capacity(16);

        // Add frozen amount (8 bytes, big-endian)
        result.extend_from_slice(&self.frozen_amount.to_be_bytes());

        // Add expiration timestamp (8 bytes, big-endian)
        result.extend_from_slice(&self.expiration_timestamp.to_be_bytes());

        result
    }

    /// Deserialize freeze record from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        if data.len() < 16 {
            return Err(anyhow::anyhow!("Insufficient data for freeze record"));
        }

        // Read frozen amount (8 bytes)
        let frozen_amount = u64::from_be_bytes([
            data[0], data[1], data[2], data[3],
            data[4], data[5], data[6], data[7]
        ]);

        // Read expiration timestamp (8 bytes)
        let expiration_timestamp = i64::from_be_bytes([
            data[8], data[9], data[10], data[11],
            data[12], data[13], data[14], data[15]
        ]);

        Ok(FreezeRecord::new(frozen_amount, expiration_timestamp))
    }
}

/// TRON Account AEXT (Account EXTension) - resource tracking fields
/// Mirrors Java's AccountInfo optional resource fields for bandwidth and energy tracking
#[derive(Debug, Clone, Default)]
pub struct AccountAext {
    /// Net (bandwidth) usage with windowed decay
    pub net_usage: i64,
    /// Free net usage with windowed decay
    pub free_net_usage: i64,
    /// Energy usage with windowed decay (Phase 2)
    pub energy_usage: i64,
    /// Latest consume time for ACCOUNT_NET (slot/block number)
    pub latest_consume_time: i64,
    /// Latest consume time for FREE_NET (slot/block number)
    pub latest_consume_free_time: i64,
    /// Latest consume time for energy (Phase 2)
    pub latest_consume_time_for_energy: i64,
    /// Net window size (default 28800 for EOAs)
    pub net_window_size: i64,
    /// Net window optimized flag (default false)
    pub net_window_optimized: bool,
    /// Energy window size (default 28800 for EOAs)
    pub energy_window_size: i64,
    /// Energy window optimized flag (default false)
    pub energy_window_optimized: bool,
}

impl AccountAext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with default window sizes (28800 for both net and energy)
    pub fn with_defaults() -> Self {
        Self {
            net_usage: 0,
            free_net_usage: 0,
            energy_usage: 0,
            latest_consume_time: 0,
            latest_consume_free_time: 0,
            latest_consume_time_for_energy: 0,
            net_window_size: 28800,
            net_window_optimized: false,
            energy_window_size: 28800,
            energy_window_optimized: false,
        }
    }

    /// Serialize AccountAext to bytes for storage
    /// Format: 8 i64 fields (8 bytes each) + 2 bool flags (1 byte each) = 66 bytes total
    pub fn serialize(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(66);

        // Serialize all i64 fields (8 bytes each, big-endian)
        result.extend_from_slice(&self.net_usage.to_be_bytes());
        result.extend_from_slice(&self.free_net_usage.to_be_bytes());
        result.extend_from_slice(&self.energy_usage.to_be_bytes());
        result.extend_from_slice(&self.latest_consume_time.to_be_bytes());
        result.extend_from_slice(&self.latest_consume_free_time.to_be_bytes());
        result.extend_from_slice(&self.latest_consume_time_for_energy.to_be_bytes());
        result.extend_from_slice(&self.net_window_size.to_be_bytes());
        result.extend_from_slice(&self.energy_window_size.to_be_bytes());

        // Serialize bool flags (1 byte each)
        result.push(if self.net_window_optimized { 1 } else { 0 });
        result.push(if self.energy_window_optimized { 1 } else { 0 });

        result
    }

    /// Deserialize AccountAext from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        if data.len() < 66 {
            return Err(anyhow::anyhow!(
                "Insufficient data for AccountAext: expected 66 bytes, got {}",
                data.len()
            ));
        }

        let mut pos = 0;

        // Helper to read i64
        let read_i64 = |data: &[u8], pos: &mut usize| -> i64 {
            let value = i64::from_be_bytes([
                data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3],
                data[*pos + 4], data[*pos + 5], data[*pos + 6], data[*pos + 7],
            ]);
            *pos += 8;
            value
        };

        let net_usage = read_i64(data, &mut pos);
        let free_net_usage = read_i64(data, &mut pos);
        let energy_usage = read_i64(data, &mut pos);
        let latest_consume_time = read_i64(data, &mut pos);
        let latest_consume_free_time = read_i64(data, &mut pos);
        let latest_consume_time_for_energy = read_i64(data, &mut pos);
        let net_window_size = read_i64(data, &mut pos);
        let energy_window_size = read_i64(data, &mut pos);

        // Read bool flags
        let net_window_optimized = data[pos] != 0;
        pos += 1;
        let energy_window_optimized = data[pos] != 0;

        Ok(Self {
            net_usage,
            free_net_usage,
            energy_usage,
            latest_consume_time,
            latest_consume_free_time,
            latest_consume_time_for_energy,
            net_window_size,
            net_window_optimized,
            energy_window_size,
            energy_window_optimized,
        })
    }
}

/// TRON Vote - single vote entry (vote_address, vote_count)
#[derive(Debug, Clone)]
pub struct Vote {
    pub vote_address: Address, // 20-byte witness address
    pub vote_count: u64,       // Number of votes
}

impl Vote {
    pub fn new(vote_address: Address, vote_count: u64) -> Self {
        Self {
            vote_address,
            vote_count,
        }
    }

    /// Serialize Vote to protobuf format
    /// message Vote {
    ///   bytes vote_address = 1;  // field 1, length-delimited
    ///   int64 vote_count = 2;    // field 2, varint
    /// }
    pub fn serialize(&self) -> Vec<u8> {
        self.serialize_with_prefix(0x41)
    }

    /// Serialize Vote to protobuf format with a specific network prefix byte.
    ///
    /// java-tron stores vote addresses as 21-byte TRON addresses (prefix + 20 bytes).
    /// Fixture DBs commonly use `0xa0` prefixes; mainnet uses `0x41`.
    pub fn serialize_with_prefix(&self, address_prefix: u8) -> Vec<u8> {
        let mut data = Vec::new();

        // Field 1: vote_address (length-delimited, 21 bytes with 0x41 prefix)
        let mut tron_address = Vec::with_capacity(21);
        tron_address.push(address_prefix); // Tron address prefix
        tron_address.extend_from_slice(self.vote_address.as_slice());

        data.push(0x0a); // field 1, wire type 2 (length-delimited)
        Self::write_varint(&mut data, tron_address.len() as u64);
        data.extend_from_slice(&tron_address);

        // Field 2: vote_count (varint)
        data.push(0x10); // field 2, wire type 0 (varint)
        Self::write_varint(&mut data, self.vote_count);

        data
    }

    /// Deserialize Vote from protobuf format
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        let mut pos = 0;
        let mut vote_address: Option<Address> = None;
        let mut vote_count: Option<u64> = None;

        while pos < data.len() {
            // Read field header
            let (field_header, new_pos) = Self::read_varint(data, pos)?;
            pos = new_pos;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => { // vote_address (length-delimited)
                    let (length, new_pos) = Self::read_varint(data, pos)?;
                    pos = new_pos;
                    if pos + length as usize > data.len() {
                        return Err(anyhow::anyhow!("Invalid vote_address length"));
                    }
                    let addr_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    // Remove TRON prefix if present
                    let evm_addr = if addr_bytes.len() == 21 && (addr_bytes[0] == 0x41 || addr_bytes[0] == 0xa0) {
                        &addr_bytes[1..]
                    } else if addr_bytes.len() == 20 {
                        addr_bytes
                    } else {
                        return Err(anyhow::anyhow!("Invalid vote_address length: {}", addr_bytes.len()));
                    };

                    let mut addr = [0u8; 20];
                    addr.copy_from_slice(evm_addr);
                    vote_address = Some(Address::from(addr));
                },
                (2, 0) => { // vote_count (varint)
                    let (count, new_pos) = Self::read_varint(data, pos)?;
                    pos = new_pos;
                    vote_count = Some(count);
                },
                _ => {
                    // Skip unknown field
                    pos = Self::skip_field(data, pos, wire_type)?;
                }
            }
        }

        Ok(Vote::new(
            vote_address.ok_or_else(|| anyhow::anyhow!("Missing vote_address"))?,
            vote_count.ok_or_else(|| anyhow::anyhow!("Missing vote_count"))?,
        ))
    }

    fn write_varint(output: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            output.push(((value & 0x7F) | 0x80) as u8);
            value >>= 7;
        }
        output.push(value as u8);
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

        Err(anyhow::anyhow!("Unexpected end of data while reading varint"))
    }

    fn skip_field(data: &[u8], pos: usize, wire_type: u64) -> Result<usize> {
        match wire_type {
            0 => { // Varint
                let (_, new_pos) = Self::read_varint(data, pos)?;
                Ok(new_pos)
            },
            1 => { // 64-bit
                Ok(pos + 8)
            },
            2 => { // Length-delimited
                let (length, new_pos) = Self::read_varint(data, pos)?;
                Ok(new_pos + length as usize)
            },
            5 => { // 32-bit
                Ok(pos + 4)
            },
            _ => Err(anyhow::anyhow!("Unknown wire type: {}", wire_type))
        }
    }
}

/// TRON VotesRecord - tracks voting history for an account
/// Equivalent to VotesCapsule in java-tron
#[derive(Debug, Clone)]
pub struct VotesRecord {
    pub address: Address,         // 20-byte account address
    pub old_votes: Vec<Vote>,     // Previous votes
    pub new_votes: Vec<Vote>,     // Current votes
}

impl VotesRecord {
    pub fn new(address: Address, old_votes: Vec<Vote>, new_votes: Vec<Vote>) -> Self {
        Self {
            address,
            old_votes,
            new_votes,
        }
    }

    /// Create empty VotesRecord
    pub fn empty(address: Address) -> Self {
        Self::new(address, Vec::new(), Vec::new())
    }

    /// Clear new_votes
    pub fn clear_new_votes(&mut self) {
        self.new_votes.clear();
    }

    /// Add a new vote
    pub fn add_new_vote(&mut self, vote_address: Address, vote_count: u64) {
        self.new_votes.push(Vote::new(vote_address, vote_count));
    }

    /// Serialize VotesRecord to protobuf format
    /// message Votes {
    ///   bytes address = 1;           // field 1, length-delimited
    ///   repeated Vote old_votes = 2; // field 2, length-delimited
    ///   repeated Vote new_votes = 3; // field 3, length-delimited
    /// }
    pub fn serialize(&self) -> Vec<u8> {
        self.serialize_with_prefix(0x41)
    }

    /// Serialize VotesRecord to protobuf format with a specific network prefix byte.
    ///
    /// java-tron stores the owner address and vote addresses as 21-byte TRON addresses
    /// (prefix + 20 bytes). Fixture DBs commonly use `0xa0` prefixes; mainnet uses `0x41`.
    pub fn serialize_with_prefix(&self, address_prefix: u8) -> Vec<u8> {
        let mut data = Vec::new();

        // Field 1: address (length-delimited, 21 bytes with 0x41 prefix)
        let mut tron_address = Vec::with_capacity(21);
        tron_address.push(address_prefix); // Tron address prefix
        tron_address.extend_from_slice(self.address.as_slice());

        data.push(0x0a); // field 1, wire type 2 (length-delimited)
        Self::write_varint(&mut data, tron_address.len() as u64);
        data.extend_from_slice(&tron_address);

        // Field 2: old_votes (repeated, each is length-delimited)
        for vote in &self.old_votes {
            let vote_bytes = vote.serialize_with_prefix(address_prefix);
            data.push(0x12); // field 2, wire type 2 (length-delimited)
            Self::write_varint(&mut data, vote_bytes.len() as u64);
            data.extend_from_slice(&vote_bytes);
        }

        // Field 3: new_votes (repeated, each is length-delimited)
        for vote in &self.new_votes {
            let vote_bytes = vote.serialize_with_prefix(address_prefix);
            data.push(0x1a); // field 3, wire type 2 (length-delimited)
            Self::write_varint(&mut data, vote_bytes.len() as u64);
            data.extend_from_slice(&vote_bytes);
        }

        data
    }

    /// Deserialize VotesRecord from protobuf format
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        let mut pos = 0;
        let mut address: Option<Address> = None;
        let mut old_votes = Vec::new();
        let mut new_votes = Vec::new();

        while pos < data.len() {
            // Read field header
            let (field_header, new_pos) = Self::read_varint(data, pos)?;
            pos = new_pos;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => { // address (length-delimited)
                    let (length, new_pos) = Self::read_varint(data, pos)?;
                    pos = new_pos;
                    if pos + length as usize > data.len() {
                        return Err(anyhow::anyhow!("Invalid address length"));
                    }
                    let addr_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    // Remove TRON prefix if present
                    let evm_addr = if addr_bytes.len() == 21 && (addr_bytes[0] == 0x41 || addr_bytes[0] == 0xa0) {
                        &addr_bytes[1..]
                    } else if addr_bytes.len() == 20 {
                        addr_bytes
                    } else {
                        return Err(anyhow::anyhow!("Invalid address length: {}", addr_bytes.len()));
                    };

                    let mut addr = [0u8; 20];
                    addr.copy_from_slice(evm_addr);
                    address = Some(Address::from(addr));
                },
                (2, 2) => { // old_votes (length-delimited)
                    let (length, new_pos) = Self::read_varint(data, pos)?;
                    pos = new_pos;
                    if pos + length as usize > data.len() {
                        return Err(anyhow::anyhow!("Invalid old_votes length"));
                    }
                    let vote_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;
                    old_votes.push(Vote::deserialize(vote_bytes)?);
                },
                (3, 2) => { // new_votes (length-delimited)
                    let (length, new_pos) = Self::read_varint(data, pos)?;
                    pos = new_pos;
                    if pos + length as usize > data.len() {
                        return Err(anyhow::anyhow!("Invalid new_votes length"));
                    }
                    let vote_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;
                    new_votes.push(Vote::deserialize(vote_bytes)?);
                },
                _ => {
                    // Skip unknown field
                    pos = Self::skip_field(data, pos, wire_type)?;
                }
            }
        }

        Ok(VotesRecord::new(
            address.ok_or_else(|| anyhow::anyhow!("Missing address"))?,
            old_votes,
            new_votes,
        ))
    }

    fn write_varint(output: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            output.push(((value & 0x7F) | 0x80) as u8);
            value >>= 7;
        }
        output.push(value as u8);
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

        Err(anyhow::anyhow!("Unexpected end of data while reading varint"))
    }

    fn skip_field(data: &[u8], pos: usize, wire_type: u64) -> Result<usize> {
        match wire_type {
            0 => { // Varint
                let (_, new_pos) = Self::read_varint(data, pos)?;
                Ok(new_pos)
            },
            1 => { // 64-bit
                Ok(pos + 8)
            },
            2 => { // Length-delimited
                let (length, new_pos) = Self::read_varint(data, pos)?;
                Ok(new_pos + length as usize)
            },
            5 => { // 32-bit
                Ok(pos + 4)
            },
            _ => Err(anyhow::anyhow!("Unknown wire type: {}", wire_type))
        }
    }
}

/// State change tracking for debugging and verification.
/// Records old/new values for both storage slots and account-level changes.
#[derive(Debug, Clone)]
pub enum StateChangeRecord {
    /// Storage slot change within a contract
    StorageChange {
        address: Address,
        key: U256,
        old_value: U256,
        new_value: U256,
    },
    /// Account-level change (balance, nonce, code, etc.)
    AccountChange {
        address: Address,
        old_account: Option<AccountInfo>,
        new_account: Option<AccountInfo>,
    },
}
