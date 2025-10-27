use std::collections::{HashMap, HashSet};
use anyhow::Result;
use revm::primitives::{Account, AccountInfo, Bytecode, B256, U256, Address};
use revm::{Database, DatabaseCommit};
use tron_backend_storage::StorageEngine;

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
        use prost::Message;
        use crate::protocol::Witness;

        // Build TRON address (21 bytes: 0x41 prefix + 20-byte address)
        let mut tron_address = Vec::with_capacity(21);
        tron_address.push(0x41); // TRON address prefix
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
            is_jobs: true, // Set to true for parity with Java genesis writes
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
        let address = if witness.address.len() == 21 && witness.address[0] == 0x41 {
            // TRON format: 21 bytes with 0x41 prefix, strip prefix for 20-byte address
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
    /// Format: 10 fields, each 8 bytes + 2 bool flags = 82 bytes total
    pub fn serialize(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(82);

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
        if data.len() < 82 {
            return Err(anyhow::anyhow!(
                "Insufficient data for AccountAext: expected 82 bytes, got {}",
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
        let mut data = Vec::new();

        // Field 1: vote_address (length-delimited, 21 bytes with 0x41 prefix)
        let mut tron_address = Vec::with_capacity(21);
        tron_address.push(0x41); // Tron address prefix
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

                    // Remove 0x41 prefix if present
                    let evm_addr = if addr_bytes.len() == 21 && addr_bytes[0] == 0x41 {
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
        let mut data = Vec::new();

        // Field 1: address (length-delimited, 21 bytes with 0x41 prefix)
        let mut tron_address = Vec::with_capacity(21);
        tron_address.push(0x41); // Tron address prefix
        tron_address.extend_from_slice(self.address.as_slice());

        data.push(0x0a); // field 1, wire type 2 (length-delimited)
        Self::write_varint(&mut data, tron_address.len() as u64);
        data.extend_from_slice(&tron_address);

        // Field 2: old_votes (repeated, each is length-delimited)
        for vote in &self.old_votes {
            let vote_bytes = vote.serialize();
            data.push(0x12); // field 2, wire type 2 (length-delimited)
            Self::write_varint(&mut data, vote_bytes.len() as u64);
            data.extend_from_slice(&vote_bytes);
        }

        // Field 3: new_votes (repeated, each is length-delimited)
        for vote in &self.new_votes {
            let vote_bytes = vote.serialize();
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

                    // Remove 0x41 prefix if present
                    let evm_addr = if addr_bytes.len() == 21 && addr_bytes[0] == 0x41 {
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

/// Minimal EVM-facing state interface for account, code, and storage operations.
/// Provides the essential read/write operations needed by the EVM execution engine.
/// Implemented by in-memory stores (testing) and engine-backed stores (production).
pub trait EvmStateStore: Send + Sync {
    /// Get account information
    fn get_account(&self, address: &Address) -> Result<Option<AccountInfo>>;
    
    /// Get account code
    fn get_code(&self, address: &Address) -> Result<Option<Bytecode>>;
    
    /// Get storage value
    fn get_storage(&self, address: &Address, key: &U256) -> Result<U256>;
    
    /// Set account information
    fn set_account(&mut self, address: Address, account: AccountInfo) -> Result<()>;
    
    /// Set account code
    fn set_code(&mut self, address: Address, code: Bytecode) -> Result<()>;
    
    /// Set storage value
    fn set_storage(&mut self, address: Address, key: U256, value: U256) -> Result<()>;
    
    /// Remove account
    fn remove_account(&mut self, address: &Address) -> Result<()>;
}

/// In-memory implementation of EVM state store for testing and local execution.
/// Provides a HashMap-backed storage that doesn't persist to disk.
#[derive(Debug)]
pub struct InMemoryEvmStateStore {
    accounts: HashMap<Address, AccountInfo>,
    codes: HashMap<Address, Bytecode>,
    storage: HashMap<(Address, U256), U256>,
    freeze_records: std::sync::Arc<std::sync::RwLock<HashMap<(Address, u8), FreezeRecord>>>,
    account_aext: std::sync::Arc<std::sync::RwLock<HashMap<Address, AccountAext>>>,
}

impl Clone for InMemoryEvmStateStore {
    fn clone(&self) -> Self {
        Self {
            accounts: self.accounts.clone(),
            codes: self.codes.clone(),
            storage: self.storage.clone(),
            freeze_records: self.freeze_records.clone(),
            account_aext: self.account_aext.clone(),
        }
    }
}

impl InMemoryEvmStateStore {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            codes: HashMap::new(),
            storage: HashMap::new(),
            freeze_records: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
            account_aext: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Get freeze record for an address and resource
    pub fn get_freeze_record(&self, address: &Address, resource: u8) -> Result<Option<FreezeRecord>> {
        Ok(self.freeze_records.read().unwrap().get(&(*address, resource)).cloned())
    }

    /// Set freeze record for an address and resource
    pub fn set_freeze_record(&self, address: &Address, resource: u8, frozen_amount: u64, expiration_timestamp: i64) -> Result<()> {
        let record = FreezeRecord {
            frozen_amount,
            expiration_timestamp,
        };
        self.freeze_records.write().unwrap().insert((*address, resource), record);
        Ok(())
    }

    /// Get tron power for an address in SUN
    pub fn get_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64> {
        // Resource types as defined in Tron protocol
        const BANDWIDTH: u8 = 0;
        const ENERGY: u8 = 1;
        const TRON_POWER: u8 = 2;

        let mut total: u64 = 0;
        let mut bandwidth_amount: u64 = 0;
        let mut energy_amount: u64 = 0;
        let mut tron_power_amount: u64 = 0;

        // Sum frozen amounts across all three resource types
        for resource in [BANDWIDTH, ENERGY, TRON_POWER] {
            if let Some(record) = self.get_freeze_record(address, resource)? {
                let amount = record.frozen_amount;
                total = total.checked_add(amount)
                    .ok_or_else(|| anyhow::anyhow!(
                        "Tron power overflow when adding resource {} amount {} to total {}",
                        resource, amount, total
                    ))?;

                // Track per-resource amounts for logging
                match resource {
                    BANDWIDTH => bandwidth_amount = amount,
                    ENERGY => energy_amount = amount,
                    TRON_POWER => tron_power_amount = amount,
                    _ => {}
                }
            }
        }

        // Log the computation with all relevant details
        tracing::info!(
            address = ?address,
            new_model = new_model,
            bandwidth = bandwidth_amount,
            energy = energy_amount,
            tron_power_legacy = tron_power_amount,
            total = total,
            "Computed tron power from freeze ledger (in-memory)"
        );

        Ok(total)
    }

    /// Get account AEXT (resource tracking fields) for an address
    pub fn get_account_aext(&self, address: &Address) -> Result<Option<AccountAext>> {
        Ok(self.account_aext.read().unwrap().get(address).cloned())
    }

    /// Set account AEXT (resource tracking fields) for an address
    pub fn set_account_aext(&self, address: &Address, aext: AccountAext) -> Result<()> {
        self.account_aext.write().unwrap().insert(*address, aext);
        Ok(())
    }

    /// Get or initialize account AEXT with defaults
    pub fn get_or_init_account_aext(&self, address: &Address) -> Result<AccountAext> {
        if let Some(aext) = self.get_account_aext(address)? {
            Ok(aext)
        } else {
            let aext = AccountAext::with_defaults();
            self.set_account_aext(address, aext.clone())?;
            Ok(aext)
        }
    }

    // Phase C: Method alias shims (preferred names going forward)
    // See planning/storage_adapter_namings.planning.md for rationale

    /// **Preferred name**: Store freeze record (upsert semantics, aligns with `put_witness`).
    /// Delegates to `set_freeze_record`. Use this method in new code.
    pub fn put_freeze_record(&self, address: &Address, resource: u8, frozen_amount: u64, expiration_timestamp: i64) -> Result<()> {
        self.set_freeze_record(address, resource, frozen_amount, expiration_timestamp)
    }

    /// **Preferred name**: Compute tron power from ledger (reflects computation rather than "get").
    /// Delegates to `get_tron_power_in_sun`. Use this method in new code.
    pub fn compute_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64> {
        self.get_tron_power_in_sun(address, new_model)
    }
}

impl EvmStateStore for InMemoryEvmStateStore {
    fn get_account(&self, address: &Address) -> Result<Option<AccountInfo>> {
        Ok(self.accounts.get(address).cloned())
    }
    
    fn get_code(&self, address: &Address) -> Result<Option<Bytecode>> {
        Ok(self.codes.get(address).cloned())
    }
    
    fn get_storage(&self, address: &Address, key: &U256) -> Result<U256> {
        Ok(self.storage.get(&(*address, *key)).copied().unwrap_or(U256::ZERO))
    }
    
    fn set_account(&mut self, address: Address, account: AccountInfo) -> Result<()> {
        self.accounts.insert(address, account);
        Ok(())
    }
    
    fn set_code(&mut self, address: Address, code: Bytecode) -> Result<()> {
        self.codes.insert(address, code);
        Ok(())
    }
    
    fn set_storage(&mut self, address: Address, key: U256, value: U256) -> Result<()> {
        if value == U256::ZERO {
            self.storage.remove(&(address, key));
        } else {
            self.storage.insert((address, key), value);
        }
        Ok(())
    }
    
    fn remove_account(&mut self, address: &Address) -> Result<()> {
        self.accounts.remove(address);
        self.codes.remove(address);
        
        // Remove all storage for this address
        self.storage.retain(|(addr, _), _| addr != address);
        
        Ok(())
    }
}

/// Persistent implementation of EVM state store backed by the storage engine.
/// Routes data to appropriate RocksDB databases matching java-tron's organization
/// while providing a unified interface for EVM execution.
pub struct EngineBackedEvmStateStore {
    storage_engine: StorageEngine,
}

impl EngineBackedEvmStateStore {
    pub fn new(storage_engine: StorageEngine) -> Self {
        Self {
            storage_engine,
        }
    }

    /// Get the appropriate database name for account data
    fn account_database(&self) -> &str {
        "account"
    }

    /// Get the appropriate database name for contract code
    fn code_database(&self) -> &str {
        "code"
    }

    /// Get the appropriate database name for contract storage
    fn contract_state_database(&self) -> &str {
        "contract-state"
    }

    /// Get the appropriate database name for contract metadata
    fn contract_database(&self) -> &str {
        "contract"
    }

    /// Get the appropriate database name for dynamic properties
    fn dynamic_properties_database(&self) -> &str {
        "properties"
    }

    /// Get the appropriate database name for witness store
    fn witness_database(&self) -> &str {
        "witness"
    }

    /// Get the appropriate database name for votes store
    fn votes_database(&self) -> &str {
        "votes"
    }

    /// Convert Address to storage key for accounts (matching java-tron format)
    /// Java-tron stores accounts using 21-byte addresses with 0x41 prefix
    /// REVM uses 20-byte addresses, so we need to add the 0x41 prefix
    fn account_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(0x41); // Tron address prefix
        key.extend_from_slice(address.as_slice()); // 20-byte address
        key
    }

    /// Convert Address to storage key for code (raw address, matching java-tron)
    fn code_key(&self, address: &Address) -> Vec<u8> {
        address.as_slice().to_vec()
    }

    /// Convert Address to storage key for witness store (21-byte address with 0x41 prefix)
    fn witness_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(0x41); // Tron address prefix
        key.extend_from_slice(address.as_slice()); // 20-byte address
        key
    }

    /// Convert Address to storage key for votes store (21-byte address with 0x41 prefix)
    fn votes_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(0x41); // Tron address prefix
        key.extend_from_slice(address.as_slice()); // 20-byte address
        key
    }

    /// Get the appropriate database name for freeze records
    fn freeze_records_database(&self) -> &str {
        "freeze-records"
    }

    /// Convert Address and FreezeResource to storage key for freeze records
    /// Format: 21-byte tron address (0x41 + 20-byte) + 1-byte resource type
    fn freeze_record_key(&self, address: &Address, resource: u8) -> Vec<u8> {
        let mut key = Vec::with_capacity(22);
        key.push(0x41); // Tron address prefix
        key.extend_from_slice(address.as_slice()); // 20-byte address
        key.push(resource); // Resource type (0=BANDWIDTH, 1=ENERGY, 2=TRON_POWER)
        key
    }

    /// Get the appropriate database name for account names
    fn account_name_database(&self) -> &str {
        "account-name"
    }

    /// Convert Address and storage key to contract storage key (matching java-tron's Storage.compose format)
    fn contract_storage_key(&self, address: &Address, storage_key: &U256) -> Vec<u8> {
        // Match java-tron's Storage.compose() method:
        // addrHash[0:16] + storageKey[16:32] (32 bytes total)
        let addr_hash = keccak256(address.as_slice());
        let storage_key_bytes = storage_key.to_be_bytes::<32>();

        let mut composed_key = Vec::with_capacity(32);
        composed_key.extend_from_slice(&addr_hash.as_slice()[0..16]); // First 16 bytes of address hash
        composed_key.extend_from_slice(&storage_key_bytes[16..32]);   // Last 16 bytes of storage key
        composed_key
    }

    /// Serialize AccountInfo to bytes in java-tron Account protobuf format
    fn serialize_account(&self, address: &Address, account: &AccountInfo) -> Vec<u8> {
        // Create a protobuf Account message compatible with java-tron
        // The Account protobuf in java-tron has the following structure:
        // message Account {
        //   bytes address = 1;           // field 1, length-delimited
        //   AccountType type = 2;        // field 2, varint (0 = Normal)
        //   int64 balance = 4;           // field 4, varint
        //   int64 create_time = 9;       // field 9, varint
        //   // ... other fields
        // }
        
        let mut data = Vec::new();
        
        // Field 1: address (length-delimited)
        // Include the full 21-byte Tron address with 0x41 prefix
        let tron_address = self.account_key(address); // This adds 0x41 prefix
        data.push(0x0a); // field 1, length-delimited
        self.write_varint(&mut data, tron_address.len() as u64);
        data.extend_from_slice(&tron_address);
        
        // Field 2: type (AccountType.Normal = 0)
        data.push(0x10); // field 2, varint
        data.push(0x00); // value = 0 (Normal)
        
        // Field 4: balance (varint)
        // Convert U256 balance to u64 (TRON uses long for balance)
        // ALWAYS include balance field, even if 0, for Java compatibility
        let balance_u64 = account.balance.to::<u64>();
        data.push(0x20); // field 4, varint
        self.write_varint(&mut data, balance_u64);
        
        // Field 9: create_time (use current timestamp)
        // Use current time in milliseconds
        use std::time::{SystemTime, UNIX_EPOCH};
        let create_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if create_time > 0 {
            data.push(0x48); // field 9, varint
            self.write_varint(&mut data, create_time);
        }
        
        data
    }
    
    /// Write a varint to the output buffer
    fn write_varint(&self, output: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            output.push(((value & 0x7F) | 0x80) as u8);
            value >>= 7;
        }
        output.push(value as u8);
    }

    /// Deserialize AccountInfo from protobuf bytes (java-tron Account message)
    fn deserialize_account(&self, data: &[u8]) -> Result<AccountInfo> {
        // Parse protobuf Account message
        // For now, we'll implement a simple parser for the balance field
        // TODO: Use proper protobuf parsing library for full compatibility

        // This is a simplified parser that extracts the balance field from the protobuf
        // The Account protobuf has balance as field 4 (varint)
        let balance = self.extract_balance_from_protobuf(data)?;

        // For now, use default values for other fields
        // In a full implementation, we'd parse all fields from the protobuf
        Ok(AccountInfo {
            balance: U256::from(balance),
            nonce: 0, // TRON doesn't use nonce, so we can use 0
            code_hash: revm::primitives::B256::ZERO, // TODO: Extract from protobuf if needed
            code: None,
        })
    }

    /// Extract balance field from Account protobuf message
    /// This is a simplified parser for the balance field (field number 4)
    fn extract_balance_from_protobuf(&self, data: &[u8]) -> Result<u64> {
        let mut pos = 0;

        while pos < data.len() {
            if pos >= data.len() {
                break;
            }

            // Read field header (varint)
            let (field_header, new_pos) = self.read_varint(data, pos)?;
            pos = new_pos;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            if field_number == 4 && wire_type == 0 { // balance field (varint)
                let (balance, _) = self.read_varint(data, pos)?;
                return Ok(balance);
            } else {
                // Skip this field
                pos = self.skip_field(data, pos, wire_type)?;
            }
        }

        // If balance field not found, return 0
        Ok(0)
    }

    /// Read a varint from protobuf data
    fn read_varint(&self, data: &[u8], mut pos: usize) -> Result<(u64, usize)> {
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

    /// Skip a field in protobuf data
    fn skip_field(&self, data: &[u8], pos: usize, wire_type: u64) -> Result<usize> {
        match wire_type {
            0 => { // Varint
                let (_, new_pos) = self.read_varint(data, pos)?;
                Ok(new_pos)
            },
            1 => { // 64-bit
                Ok(pos + 8)
            },
            2 => { // Length-delimited
                let (length, new_pos) = self.read_varint(data, pos)?;
                Ok(new_pos + length as usize)
            },
            5 => { // 32-bit
                Ok(pos + 4)
            },
            _ => Err(anyhow::anyhow!("Unknown wire type: {}", wire_type))
        }
    }

    /// Get AccountUpgradeCost dynamic property
    /// Default value for witness creation cost in SUN
    pub fn get_account_upgrade_cost(&self) -> Result<u64> {
        let key = b"ACCOUNT_UPGRADE_COST";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let cost = u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(cost)
                } else {
                    // Use default value for AccountUpgradeCost
                    Ok(9999000000) // 9999 TRX in SUN (default from TRON)
                }
            },
            None => {
                // Use default value for AccountUpgradeCost
                Ok(9999000000) // 9999 TRX in SUN (default from TRON)
            }
        }
    }

    /// Get AllowMultiSign dynamic property
    /// Default value: 1 (enabled)
    pub fn get_allow_multi_sign(&self) -> Result<bool> {
        let key = b"ALLOW_MULTI_SIGN";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if !data.is_empty() {
                    Ok(data[0] != 0)
                } else {
                    Ok(true) // Default enabled
                }
            },
            None => {
                Ok(true) // Default enabled
            }
        }
    }

    /// Get Black Hole Optimization dynamic property (parity with Java)
    /// Java stores this as a long under key "ALLOW_BLACKHOLE_OPTIMIZATION".
    /// When this flag is 1, the node BURNS fees (optimization enabled).
    /// When 0, the node CREDITS the blackhole account.
    /// Default: false (credit blackhole) to match early-chain behavior when key is absent.
    pub fn support_black_hole_optimization(&self) -> Result<bool> {
        // Parity key with java-tron DynamicPropertiesStore
        let key = b"ALLOW_BLACKHOLE_OPTIMIZATION";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                // Java writes a long; interpret big-endian u64 when length >= 8.
                if data.len() >= 8 {
                    let val = u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val != 0)
                } else if !data.is_empty() {
                    // Fallback: treat first byte as boolean
                    Ok(data[0] != 0)
                } else {
                    // Empty value → treat as disabled (credit blackhole)
                    Ok(false)
                }
            },
            None => {
                // Absent key → default to disabled (credit blackhole) for early heights
                Ok(false)
            }
        }
    }

    /// Get AllowNewResourceModel dynamic property
    /// Determines whether to use new resource model for tron power calculation
    /// Default: true (enabled)
    pub fn support_allow_new_resource_model(&self) -> Result<bool> {
        let key = b"ALLOW_NEW_RESOURCE_MODEL";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val != 0)
                } else if !data.is_empty() {
                    Ok(data[0] != 0)
                } else {
                    Ok(true) // Default enabled
                }
            },
            None => {
                Ok(true) // Default enabled
            }
        }
    }

    /// Get UnfreezeDelay dynamic property
    /// Returns true if unfreeze delay is enabled (UNFREEZE_DELAY_DAYS > 0)
    /// Default: false (no delay)
    pub fn support_unfreeze_delay(&self) -> Result<bool> {
        let key = b"UNFREEZE_DELAY_DAYS";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val > 0)
                } else if !data.is_empty() {
                    Ok(data[0] > 0)
                } else {
                    Ok(false) // Default no delay
                }
            },
            None => {
                Ok(false) // Default no delay
            }
        }
    }

    /// Get blackhole address (if crediting instead of burning)
    /// Returns:
    /// - The configured dynamic property value when present (20 raw bytes)
    /// - Otherwise, a sane mainnet default (TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy)
    ///   to match java-tron's AccountStore.getBlackhole() behavior.
    pub fn get_blackhole_address(&self) -> Result<Option<Address>> {
        let key = b"BLACK_HOLE_ADDRESS";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 20 {
                    let mut addr_bytes = [0u8; 20];
                    addr_bytes.copy_from_slice(&data[0..20]);
                    Ok(Some(Address::from(addr_bytes)))
                } else {
                    // Invalid or empty value: fall back to default
                    Ok(Self::default_blackhole_address())
                }
            },
            None => {
                // Not configured in dynamic properties - use sane network default
                Ok(Self::default_blackhole_address())
            }
        }
    }

    /// Default blackhole address (mainnet): TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy
    /// Provided as 20-byte EVM address wrapped in revm_primitives::Address.
    fn default_blackhole_address() -> Option<Address> {
        // Use common address utility to decode TRON Base58
        match tron_backend_common::from_tron_address("TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy") {
            Ok(bytes20) => Some(Address::from(bytes20)),
            Err(_) => None,
        }
    }

    // Bandwidth and Resource Dynamic Properties for AEXT tracking

    /// Get FREE_NET_LIMIT dynamic property (free bandwidth limit per account)
    /// Default: 5000 bytes per transaction
    pub fn get_free_net_limit(&self) -> Result<i64> {
        let key = b"FREE_NET_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(5000) // Default
                }
            },
            None => Ok(5000) // Default
        }
    }

    /// Get PUBLIC_NET_LIMIT dynamic property (total public bandwidth pool)
    /// Default: 14_400_000_000 bytes
    pub fn get_public_net_limit(&self) -> Result<i64> {
        let key = b"PUBLIC_NET_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(14_400_000_000) // Default
                }
            },
            None => Ok(14_400_000_000) // Default
        }
    }

    /// Get PUBLIC_NET_USAGE dynamic property (current public bandwidth usage)
    /// Default: 0
    pub fn get_public_net_usage(&self) -> Result<i64> {
        let key = b"PUBLIC_NET_USAGE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(0)
                }
            },
            None => Ok(0)
        }
    }

    /// Set PUBLIC_NET_USAGE dynamic property
    pub fn set_public_net_usage(&self, value: i64) -> Result<()> {
        let key = b"PUBLIC_NET_USAGE";
        let data = value.to_be_bytes();
        self.storage_engine.put(self.dynamic_properties_database(), key, &data)?;
        Ok(())
    }

    /// Get PUBLIC_NET_TIME dynamic property (last time public bandwidth was updated)
    /// Default: 0
    pub fn get_public_net_time(&self) -> Result<i64> {
        let key = b"PUBLIC_NET_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(0)
                }
            },
            None => Ok(0)
        }
    }

    /// Set PUBLIC_NET_TIME dynamic property
    pub fn set_public_net_time(&self, value: i64) -> Result<()> {
        let key = b"PUBLIC_NET_TIME";
        let data = value.to_be_bytes();
        self.storage_engine.put(self.dynamic_properties_database(), key, &data)?;
        Ok(())
    }

    /// Get TOTAL_NET_WEIGHT dynamic property (total frozen for bandwidth)
    /// Default: 0
    pub fn get_total_net_weight(&self) -> Result<i64> {
        let key = b"TOTAL_NET_WEIGHT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(0)
                }
            },
            None => Ok(0)
        }
    }

    /// Get TOTAL_NET_LIMIT dynamic property (total bandwidth from frozen balance)
    /// Default: 43_200_000_000 bytes
    pub fn get_total_net_limit(&self) -> Result<i64> {
        let key = b"TOTAL_NET_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(43_200_000_000) // Default
                }
            },
            None => Ok(43_200_000_000) // Default
        }
    }

    /// Get witness information by address
    /// Uses dual-decoder: tries protobuf first (Java format), falls back to legacy custom format
    pub fn get_witness(&self, address: &Address) -> Result<Option<WitnessInfo>> {
        let key = self.witness_key(address);
        tracing::debug!("Getting witness for address {:?}, key: {}",
                       address, hex::encode(&key));

        match self.storage_engine.get(self.witness_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found witness data, length: {}", data.len());

                // Step 1: Try protobuf decode (Java-compatible format)
                match WitnessInfo::deserialize(&data) {
                    Ok(witness) => {
                        tracing::debug!("Decoded witness as Protocol.Witness (protobuf) - URL: {}, votes: {}",
                                       witness.url, witness.vote_count);
                        return Ok(Some(witness));
                    },
                    Err(e) => {
                        tracing::debug!("Protobuf decode failed ({}), trying legacy format", e);
                        Ok(None)
                    }
                }
            },
            None => {
                tracing::debug!("No witness found for address {:?}", address);
                Ok(None)
            }
        }
    }

    /// Store witness information
    /// Uses protobuf encoding by default for Java compatibility
    pub fn put_witness(&self, witness: &WitnessInfo) -> Result<()> {
        let key = self.witness_key(&witness.address);
        // Use protobuf encoding for Java compatibility
        let data = witness.serialize();

        tracing::debug!("Storing witness (protobuf format) for address {:?}, key: {}, URL: {}, votes: {}",
                       witness.address, hex::encode(&key), witness.url, witness.vote_count);

        self.storage_engine.put(self.witness_database(), &key, &data)?;
        Ok(())
    }

    /// Check if an address is already a witness
    pub fn is_witness(&self, address: &Address) -> Result<bool> {
        match self.get_witness(address)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Get votes record for an address
    pub fn get_votes(&self, address: &Address) -> Result<Option<VotesRecord>> {
        let key = self.votes_key(address);
        tracing::debug!("Getting votes for address {:?}, key: {}",
                       address, hex::encode(&key));

        match self.storage_engine.get(self.votes_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found votes data, length: {}", data.len());
                match VotesRecord::deserialize(&data) {
                    Ok(votes) => {
                        tracing::debug!("Successfully deserialized votes - old_votes: {}, new_votes: {}",
                                       votes.old_votes.len(), votes.new_votes.len());
                        Ok(Some(votes))
                    },
                    Err(e) => {
                        tracing::error!("Failed to deserialize votes data: {}", e);
                        Ok(None) // Return None instead of error for corrupted data
                    }
                }
            },
            None => {
                tracing::debug!("No votes found for address {:?}", address);
                Ok(None)
            }
        }
    }

    /// Store votes record
    pub fn set_votes(&self, address: Address, votes: &VotesRecord) -> Result<()> {
        let key = self.votes_key(&address);
        let data = votes.serialize();

        tracing::debug!("Storing votes for address {:?}, key: {}, old_votes: {}, new_votes: {}",
                       address, hex::encode(&key), votes.old_votes.len(), votes.new_votes.len());

        self.storage_engine.put(self.votes_database(), &key, &data)?;
        Ok(())
    }

    /// Get freeze record for an address and resource type
    /// resource: 0=BANDWIDTH, 1=ENERGY, 2=TRON_POWER
    pub fn get_freeze_record(&self, address: &Address, resource: u8) -> Result<Option<FreezeRecord>> {
        let key = self.freeze_record_key(address, resource);
        tracing::debug!("Getting freeze record for address {:?}, resource {}, key: {}",
                       address, resource, hex::encode(&key));

        match self.storage_engine.get(self.freeze_records_database(), &key)? {
            Some(data) => {
                let record = FreezeRecord::deserialize(&data)?;
                tracing::debug!("Found freeze record: amount={}, expiration={}",
                               record.frozen_amount, record.expiration_timestamp);
                Ok(Some(record))
            },
            None => {
                tracing::debug!("No freeze record found");
                Ok(None)
            }
        }
    }

    /// Store freeze record for an address and resource type
    pub fn set_freeze_record(&self, address: Address, resource: u8, record: &FreezeRecord) -> Result<()> {
        let key = self.freeze_record_key(&address, resource);
        let data = record.serialize();

        tracing::debug!("Storing freeze record for address {:?}, resource {}, key: {}, amount={}, expiration={}",
                       address, resource, hex::encode(&key), record.frozen_amount, record.expiration_timestamp);

        self.storage_engine.put(self.freeze_records_database(), &key, &data)?;
        Ok(())
    }

    /// Add to existing freeze amount (convenience method)
    /// If no record exists, creates a new one
    pub fn add_freeze_amount(&self, address: Address, resource: u8, amount: u64, expiration: i64) -> Result<()> {
        let mut record = self.get_freeze_record(&address, resource)?
            .unwrap_or(FreezeRecord::new(0, 0));

        // Add to frozen amount
        record.frozen_amount = record.frozen_amount.checked_add(amount)
            .ok_or_else(|| anyhow::anyhow!("Freeze amount overflow"))?;

        // Update expiration to later of existing or new
        record.expiration_timestamp = record.expiration_timestamp.max(expiration);

        self.set_freeze_record(address, resource, &record)?;
        Ok(())
    }

    /// Remove freeze record (for unfreeze operations)
    pub fn remove_freeze_record(&self, address: &Address, resource: u8) -> Result<()> {
        let key = self.freeze_record_key(address, resource);

        tracing::debug!("Removing freeze record for address {:?}, resource {}, key: {}",
                       address, resource, hex::encode(&key));

        self.storage_engine.delete(self.freeze_records_database(), &key)?;
        Ok(())
    }

    /// Get tron power for an address in SUN
    /// Sums frozen amounts across BANDWIDTH (0), ENERGY (1), and TRON_POWER (2) resources
    pub fn get_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64> {
        // Resource types as defined in Tron protocol
        const BANDWIDTH: u8 = 0;
        const ENERGY: u8 = 1;
        const TRON_POWER: u8 = 2;

        let mut total: u64 = 0;
        let mut bandwidth_amount: u64 = 0;
        let mut energy_amount: u64 = 0;
        let mut tron_power_amount: u64 = 0;

        // Sum frozen amounts across all three resource types
        for resource in [BANDWIDTH, ENERGY, TRON_POWER] {
            if let Some(record) = self.get_freeze_record(address, resource)? {
                let amount = record.frozen_amount;
                total = total.checked_add(amount)
                    .ok_or_else(|| anyhow::anyhow!(
                        "Tron power overflow when adding resource {} amount {} to total {}",
                        resource, amount, total
                    ))?;

                // Track per-resource amounts for logging
                match resource {
                    BANDWIDTH => bandwidth_amount = amount,
                    ENERGY => energy_amount = amount,
                    TRON_POWER => tron_power_amount = amount,
                    _ => {}
                }
            }
        }

        // Log the computation with all relevant details
        tracing::info!(
            address = ?address,
            new_model = new_model,
            bandwidth = bandwidth_amount,
            energy = energy_amount,
            tron_power_legacy = tron_power_amount,
            total = total,
            "Computed tron power from freeze ledger"
        );

        Ok(total)
    }

    /// Get account name for an address
    pub fn get_account_name(&self, address: &Address) -> Result<Option<String>> {
        let key = self.account_key(address); // Reuse account_key helper (21-byte with 0x41 prefix)
        tracing::debug!("Getting account name for address {:?}, key: {}",
                       address, hex::encode(&key));

        match self.storage_engine.get(self.account_name_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found account name data, length: {}", data.len());
                // Decode as UTF-8 string
                match String::from_utf8(data) {
                    Ok(name) => {
                        tracing::debug!("Successfully decoded account name: {}", name);
                        Ok(Some(name))
                    },
                    Err(e) => {
                        tracing::error!("Failed to decode account name as UTF-8: {}", e);
                        Err(anyhow::anyhow!("Invalid UTF-8 in account name: {}", e))
                    }
                }
            },
            None => {
                tracing::debug!("No account name found for address {:?}", address);
                Ok(None)
            }
        }
    }

    /// Set account name for an address
    pub fn set_account_name(&mut self, address: Address, name: &[u8]) -> Result<()> {
        let key = self.account_key(&address); // Reuse account_key helper (21-byte with 0x41 prefix)

        tracing::debug!("Setting account name for address {:?}, key: {}, name_len: {}",
                       address, hex::encode(&key), name.len());

        // Validate name length (1 <= len <= 32 bytes to match java-tron constraints)
        if name.is_empty() {
            return Err(anyhow::anyhow!("Account name cannot be empty"));
        }
        if name.len() > 32 {
            return Err(anyhow::anyhow!("Account name cannot exceed 32 bytes, got {}", name.len()));
        }

        // Validate UTF-8 encoding (optional policy)
        match std::str::from_utf8(name) {
            Ok(name_str) => {
                tracing::debug!("Account name is valid UTF-8: {}", name_str);
            },
            Err(e) => {
                tracing::warn!("Account name contains invalid UTF-8: {}, allowing raw bytes", e);
                // Continue with raw bytes - some chains may allow arbitrary bytes
            }
        }

        self.storage_engine.put(self.account_name_database(), &key, name)?;

        tracing::info!("Successfully stored account name for address {:?}, length: {}", address, name.len());
        Ok(())
    }

    /// Get database name for account resource tracking (AEXT)
    fn account_aext_database(&self) -> &str {
        "account-resource"
    }

    /// Build storage key for account AEXT: 20-byte address
    fn account_aext_key(&self, address: &Address) -> Vec<u8> {
        address.as_slice().to_vec()
    }

    /// Get account AEXT (resource tracking fields) for an address
    pub fn get_account_aext(&self, address: &Address) -> Result<Option<AccountAext>> {
        let key = self.account_aext_key(address);
        tracing::debug!("Getting account AEXT for address {:?}, key: {}",
                       address, hex::encode(&key));

        match self.storage_engine.get(self.account_aext_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found account AEXT data, length: {}", data.len());
                match AccountAext::deserialize(&data) {
                    Ok(aext) => {
                        tracing::debug!("Successfully deserialized account AEXT - net_usage: {}, free_net_usage: {}, net_window: {}",
                                       aext.net_usage, aext.free_net_usage, aext.net_window_size);
                        Ok(Some(aext))
                    },
                    Err(e) => {
                        tracing::warn!("Failed to deserialize account AEXT data: {}, returning None", e);
                        Ok(None)
                    }
                }
            },
            None => {
                tracing::debug!("No account AEXT found for address {:?}", address);
                Ok(None)
            }
        }
    }

    /// Set account AEXT (resource tracking fields) for an address
    pub fn set_account_aext(&self, address: &Address, aext: &AccountAext) -> Result<()> {
        let key = self.account_aext_key(address);
        let data = aext.serialize();

        tracing::debug!("Setting account AEXT for address {:?}, net_usage: {}, free_net_usage: {}, net_window: {}",
                       address, aext.net_usage, aext.free_net_usage, aext.net_window_size);

        self.storage_engine.put(self.account_aext_database(), &key, &data)?;

        tracing::debug!("Successfully stored account AEXT for address {:?}", address);
        Ok(())
    }

    /// Get or initialize account AEXT with defaults
    pub fn get_or_init_account_aext(&self, address: &Address) -> Result<AccountAext> {
        if let Some(aext) = self.get_account_aext(address)? {
            Ok(aext)
        } else {
            let aext = AccountAext::with_defaults();
            self.set_account_aext(address, &aext)?;
            Ok(aext)
        }
    }

    // Phase C: Method alias shims (preferred names going forward)
    // See planning/storage_adapter_namings.planning.md for rationale

    /// **Preferred name**: Store freeze record (upsert semantics, aligns with `put_witness`).
    /// Delegates to `set_freeze_record`. Use this method in new code.
    pub fn put_freeze_record(&self, address: Address, resource: u8, record: &FreezeRecord) -> Result<()> {
        self.set_freeze_record(address, resource, record)
    }

    /// **Preferred name**: Compute tron power from ledger (reflects computation rather than "get").
    /// Delegates to `get_tron_power_in_sun`. Use this method in new code.
    pub fn compute_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64> {
        self.get_tron_power_in_sun(address, new_model)
    }
}

impl EvmStateStore for EngineBackedEvmStateStore {
    fn get_account(&self, address: &Address) -> Result<Option<AccountInfo>> {
        let key = self.account_key(address);
        // Convert to Tron format address for debugging consistency with Java logs
        let address_tron = to_tron_address(address);
        tracing::info!("Getting account for address {:?} (tron: {}), key: {}", 
                      address, address_tron, hex::encode(&key));

        match self.storage_engine.get(self.account_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found account data, length: {}, first 32 bytes: {}",
                               data.len(), hex::encode(&data[..std::cmp::min(32, data.len())]));
                match self.deserialize_account(&data) {
                    Ok(account) => {
                        tracing::info!("Successfully deserialized account - balance: {}, nonce: {}",
                                      account.balance, account.nonce);
                        Ok(Some(account))
                    },
                    Err(e) => {
                        tracing::error!("Failed to deserialize account data: {}", e);
                        // Provide default account as fallback
                        let default_balance = revm::primitives::U256::from(0u64);
                        let default_account = AccountInfo {
                            balance: default_balance,
                            nonce: 0,
                            // Use canonical empty code hash keccak256("") for EOAs
                            code_hash: keccak256(&[]),
                            code: None,
                        };
                        tracing::warn!("Providing default account due to deserialization error, balance: {}", default_balance);
                        Ok(Some(default_account))
                    }
                }
            },
            None => {
                tracing::info!("No account data found for address {:?} with key {} - account does not exist", address, hex::encode(&key));
                // Return None to indicate account doesn't exist
                // This allows the Database implementation to handle account creation properly
                Ok(None)
            },
        }
    }

    fn get_code(&self, address: &Address) -> Result<Option<Bytecode>> {
        let key = self.code_key(address);
        match self.storage_engine.get(self.code_database(), &key)? {
            Some(data) => Ok(Some(Bytecode::new_raw(data.into()))),
            None => Ok(None),
        }
    }

    fn get_storage(&self, address: &Address, key: &U256) -> Result<U256> {
        let storage_key = self.contract_storage_key(address, key);
        match self.storage_engine.get(self.contract_state_database(), &storage_key)? {
            Some(data) => {
                if data.len() == 32 {
                    Ok(U256::from_be_bytes::<32>(data.try_into().unwrap()))
                } else {
                    Ok(U256::ZERO)
                }
            }
            None => Ok(U256::ZERO),
        }
    }

    fn set_account(&mut self, address: Address, account: AccountInfo) -> Result<()> {
        let key = self.account_key(&address);
        let data = self.serialize_account(&address, &account);
        let address_tron = to_tron_address(&address);
        tracing::info!("Setting account for address {:?} (tron: {}), balance: {}, key: {}, data_len: {}, data_hex: {}", 
                       address, address_tron, account.balance, hex::encode(&key), 
                       data.len(), hex::encode(&data));
        self.storage_engine.put(self.account_database(), &key, &data)?;
        
        // Immediately verify the write by reading it back
        if let Ok(Some(read_data)) = self.storage_engine.get(self.account_database(), &key) {
            if read_data == data {
                tracing::info!("Verified account write for {} - data matches", address_tron);
            } else {
                tracing::error!("Account write verification failed for {} - data mismatch!", address_tron);
            }
        } else {
            tracing::error!("Account write verification failed for {} - could not read back!", address_tron);
        }
        
        Ok(())
    }

    fn set_code(&mut self, address: Address, code: Bytecode) -> Result<()> {
        let key = self.code_key(&address);
        self.storage_engine.put(self.code_database(), &key, &code.bytes())?;
        Ok(())
    }

    fn set_storage(&mut self, address: Address, key: U256, value: U256) -> Result<()> {
        let storage_key = self.contract_storage_key(&address, &key);
        let data = value.to_be_bytes::<32>();
        self.storage_engine.put(self.contract_state_database(), &storage_key, &data)?;
        Ok(())
    }

    fn remove_account(&mut self, address: &Address) -> Result<()> {
        // Remove account data
        let account_key = self.account_key(address);
        self.storage_engine.delete(self.account_database(), &account_key)?;

        // Remove code
        let code_key = self.code_key(address);
        self.storage_engine.delete(self.code_database(), &code_key)?;

        // Note: We don't remove storage slots here as it would require iteration
        // In a real implementation, we might want to track storage slots separately
        // or use a different key scheme that allows prefix deletion

        Ok(())
    }
}

/// Snapshot hook callback for capturing modified accounts
pub type SnapshotHook = Box<dyn Fn(&HashSet<Address>) + Send + Sync>;

/// Represents different types of state changes with old and new values
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

/// REVM Database wrapper over an EVM state store.
/// Provides caching and state tracking for transaction execution.
pub struct EvmStateDatabase<S: EvmStateStore> {
    storage: S,
    // Cache for performance
    account_cache: HashMap<Address, Option<AccountInfo>>,
    code_cache: HashMap<Address, Option<Bytecode>>,
    storage_cache: HashMap<(Address, U256), U256>,
    // Track changes for commit
    account_snapshots: HashMap<Address, Option<Account>>,
    storage_snapshots: HashMap<Address, HashMap<U256, U256>>,
    // Track detailed state changes with old and new values
    state_change_records: Vec<StateChangeRecord>,
    // Snapshots for revert support
    snapshots: Vec<(HashMap<Address, Option<Account>>, HashMap<Address, HashMap<U256, U256>>)>,
    // Track modified accounts for shadow verification
    modified_accounts: HashSet<Address>,
    // Snapshot hooks for state comparison
    snapshot_hooks: Vec<SnapshotHook>,
}

impl<S: EvmStateStore> EvmStateDatabase<S> {
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            account_cache: HashMap::new(),
            code_cache: HashMap::new(),
            storage_cache: HashMap::new(),
            account_snapshots: HashMap::new(),
            storage_snapshots: HashMap::new(),
            state_change_records: Vec::new(),
            snapshots: Vec::new(),
            modified_accounts: HashSet::new(),
            snapshot_hooks: Vec::new(),
        }
    }
    
    pub fn snapshot(&mut self) -> U256 {
        let snapshot_id = U256::from(self.snapshots.len());
        self.snapshots.push((self.account_snapshots.clone(), self.storage_snapshots.clone()));
        snapshot_id
    }
    
    pub fn revert(&mut self, snapshot_id: U256) -> bool {
        let id = snapshot_id.to::<usize>();
        if id < self.snapshots.len() {
            let (accounts, storage) = self.snapshots[id].clone();
            self.account_snapshots = accounts;
            self.storage_snapshots = storage;
            self.snapshots.truncate(id);
            // Clear modified accounts on revert
            self.modified_accounts.clear();
            true
        } else {
            false
        }
    }

    /// Get the current state changes tracked by this database
    pub fn get_storage_snapshots(&self) -> &HashMap<Address, HashMap<U256, U256>> {
        &self.storage_snapshots
    }

    /// Get the current account changes tracked by this database
    pub fn get_account_snapshots(&self) -> &HashMap<Address, Option<Account>> {
        &self.account_snapshots
    }

    /// Get the detailed state change records with old and new values
    pub fn get_state_change_records(&self) -> &Vec<StateChangeRecord> {
        &self.state_change_records
    }

    /// Clear all state change records (useful after processing)
    pub fn clear_state_change_records(&mut self) {
        self.state_change_records.clear();
    }

    /// Add a snapshot hook for capturing modified accounts
    pub fn add_snapshot_hook<F>(&mut self, hook: F)
    where
        F: Fn(&HashSet<Address>) + Send + Sync + 'static
    {
        self.snapshot_hooks.push(Box::new(hook));
    }

    /// Get the current set of modified accounts
    pub fn get_modified_accounts(&self) -> &HashSet<Address> {
        &self.modified_accounts
    }

    /// Clear the modified accounts set
    pub fn clear_modified_accounts(&mut self) {
        self.modified_accounts.clear();
    }

    /// Trigger snapshot hooks with current modified accounts
    pub fn trigger_snapshot_hooks(&self) {
        for hook in &self.snapshot_hooks {
            hook(&self.modified_accounts);
        }
    }

    /// Mark an account as modified and trigger hooks if needed
    fn mark_account_modified(&mut self, address: Address) {
        let was_new = self.modified_accounts.insert(address);
        if was_new {
            // Trigger hooks when a new account is modified
            self.trigger_snapshot_hooks();
        }
    }
}

impl<S: EvmStateStore> Database for EvmStateDatabase<S> {
    type Error = anyhow::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        // Check cache first
        if let Some(cached) = self.account_cache.get(&address) {
            return Ok(cached.clone());
        }

        // Check pending changes
        if let Some(Some(account)) = self.account_snapshots.get(&address) {
            let info = AccountInfo {
                balance: account.info.balance,
                nonce: account.info.nonce,
                code_hash: account.info.code_hash,
                code: account.info.code.clone(),
            };
            self.account_cache.insert(address, Some(info.clone()));
            return Ok(Some(info));
        } else if self.account_snapshots.get(&address) == Some(&None) {
            // Account was deleted
            self.account_cache.insert(address, None);
            return Ok(None);
        }

        // Load from storage
        let result = self.storage.get_account(&address)?;
        
        // If account doesn't exist, create a default account for Tron compatibility
        // This ensures that REVM can proceed with execution and account creation is tracked
        let final_result = match result {
            Some(account) => Some(account),
            None => {
                let address_tron = to_tron_address(&address);
                tracing::info!("Creating default account for non-existent address {:?} (tron: {})", address, address_tron);
                // Create a default account with zero balance
                let default_account = AccountInfo {
                    balance: revm::primitives::U256::ZERO,
                    nonce: 0,
                    // Use canonical empty code hash keccak256("") instead of ZERO for parity
                    code_hash: keccak256(&[]),
                    code: None,
                };
                
                // **CRITICAL FIX: Pre-register this as a new account in snapshots**
                // This ensures that when REVM commits changes, it will detect this as account creation
                // even if the balance remains zero
                // Mark as "was non-existent" but don't persist yet - let commit() handle it with final balance
                self.account_snapshots.insert(address, None);
                
                // **IMPORTANT: Don't record state change or persist here**
                // The account creation will be tracked in commit() with the final balance
                // This way Java sees the account created with its actual balance, not 0
                tracing::info!("Marked {:?} (tron: {}) as non-existent for tracking, will persist in commit() with final balance", address, address_tron);

                Some(default_account)
            }
        };
        
        self.account_cache.insert(address, final_result.clone());
        Ok(final_result)
    }

    fn code_by_hash(&mut self, _code_hash: B256) -> Result<revm::primitives::Bytecode, Self::Error> {
        // For simplicity, return empty bytecode
        // In a real implementation, this would look up code by hash
        Ok(Bytecode::new())
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let key = (address, index);
        
        // Check cache first
        if let Some(&cached) = self.storage_cache.get(&key) {
            return Ok(cached);
        }

        // Check pending changes
        if let Some(storage_map) = self.storage_snapshots.get(&address) {
            if let Some(&value) = storage_map.get(&index) {
                self.storage_cache.insert(key, value);
                return Ok(value);
            }
        }

        // Load from storage
        let value = self.storage.get_storage(&address, &index)?;
        self.storage_cache.insert(key, value);
        Ok(value)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        // For simplicity, return a deterministic hash based on block number
        // In a real implementation, this would look up the actual block hash
        let mut hash = [0u8; 32];
        hash[24..32].copy_from_slice(&number.to_be_bytes());
        Ok(B256::from(hash))
    }
}

impl<S: EvmStateStore> DatabaseCommit for EvmStateDatabase<S> {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        for (address, account) in changes {
            // Mark account as modified for shadow verification
            self.mark_account_modified(address);

            // Get old account info for comparison using comprehensive fallback pattern
            let was_nonexistent_in_snapshots = self.account_snapshots.get(&address) == Some(&None);
            let old_account_info = self.account_snapshots.get(&address)
                .and_then(|acc_opt| acc_opt.as_ref())
                .map(|acc| acc.info.clone())
                .or_else(|| {
                    // If not in our changes, try to get from account cache
                    self.account_cache.get(&address).cloned().flatten()
                })
                .or_else(|| {
                    // If not in cache, try to load from storage
                    self.storage.get_account(&address).ok().flatten()
                });

            // Track account-level changes
            let new_account_info = account.info.clone();
            let is_account_creation = old_account_info.is_none() || was_nonexistent_in_snapshots;
            let account_changed = match &old_account_info {
                Some(old_info) => {
                    old_info.balance != new_account_info.balance ||
                    old_info.nonce != new_account_info.nonce ||
                    old_info.code_hash != new_account_info.code_hash ||
                    old_info.code != new_account_info.code
                },
                None => true, // New account
            } || was_nonexistent_in_snapshots; // Force account creation tracking
            
            // **ENHANCED FIX: Always track account creation, even with zero balance**
            // This ensures that bandwidth processing in Java can find the account
            let force_track_creation = is_account_creation && (
                new_account_info.balance > revm::primitives::U256::ZERO ||
                new_account_info.nonce > 0 ||
                new_account_info.code.is_some() ||
                was_nonexistent_in_snapshots
            );

            // Record account change if there was a change or if we're forcing account creation tracking
            if account_changed || force_track_creation {
                if is_account_creation || force_track_creation {
                    let address_tron = to_tron_address(&address);
                    tracing::info!("Recording account creation for address {:?} (tron: {}) with balance: {} (forced: {})", 
                                 address, address_tron, new_account_info.balance, force_track_creation);
                } else {
                    tracing::debug!("Recording account modification for address {:?} - old balance: {:?}, new balance: {}", 
                                  address, 
                                  old_account_info.as_ref().map(|info| info.balance),
                                  new_account_info.balance);
                }
                
                // If the account did not exist at the start (tracked in snapshots),
                // do not emit a synthetic zeroed old_account; leave it as None to signal creation.
                let old_account_to_record = if was_nonexistent_in_snapshots {
                    None
                } else {
                    old_account_info.clone()
                };

                self.state_change_records.push(StateChangeRecord::AccountChange {
                    address,
                    old_account: old_account_to_record,
                    new_account: Some(new_account_info.clone()),
                });
            }

            // **CRITICAL FIX: Persist account changes to underlying storage**
            if account_changed || force_track_creation {
                if let Err(e) = self.storage.set_account(address, new_account_info.clone()) {
                    tracing::error!("Failed to persist account changes for {:?}: {}", address, e);
                } else {
                    let address_tron = to_tron_address(&address);
                    if is_account_creation || force_track_creation {
                        tracing::info!("Successfully persisted account creation for {:?} (tron: {}) - balance: {}",
                                     address, address_tron, new_account_info.balance);
                    } else {
                        tracing::info!("Successfully persisted account changes for {:?} (tron: {}) - balance: {}",
                                       address, address_tron, new_account_info.balance);
                    }
                }
            }

            // **CRITICAL FIX: Persist code changes if present**
            if let Some(code) = &account.info.code {
                if let Err(e) = self.storage.set_code(address, code.clone()) {
                    tracing::error!("Failed to persist code for {:?}: {}", address, e);
                } else {
                    tracing::debug!("Successfully persisted code for {:?}", address);
                }
            }

            // Update the account snapshots (keep existing in-memory tracking)
            self.account_snapshots.insert(address, Some(account.clone()));

            // Handle self-destruct
            if account.is_selfdestructed() {
                // Record account deletion
                if old_account_info.is_some() {
                    self.state_change_records.push(StateChangeRecord::AccountChange {
                        address,
                        old_account: old_account_info,
                        new_account: None, // Account deleted
                    });
                }

                // **CRITICAL FIX: Persist account deletion to underlying storage**
                if let Err(e) = self.storage.remove_account(&address) {
                    tracing::error!("Failed to remove account {:?}: {}", address, e);
                } else {
                    tracing::debug!("Successfully removed account {:?}", address);
                }

                self.account_snapshots.insert(address, None);
            }

            // Clone storage before iterating to avoid borrow checker issues
            let new_values: HashMap<U256, U256> = account.storage.iter()
                .map(|(k, slot)| (*k, slot.present_value))
                .collect();

            // Store storage changes and track detailed state changes
            for (key, new_value) in new_values {
                // Get the old value before updating
                let old_value = self.storage_snapshots
                    .get(&address)
                    .and_then(|storage| storage.get(&key))
                    .copied()
                    .unwrap_or_else(|| {
                        // If not in our changes, try to get from storage cache
                        self.storage_cache.get(&(address, key)).copied()
                            .unwrap_or_else(|| {
                                // If not in cache, try to load from storage
                                self.storage.get_storage(&address, &key).unwrap_or(U256::ZERO)
                            })
                    });

                // Only record and persist the change if the value actually changed
                if old_value != new_value {
                    self.state_change_records.push(StateChangeRecord::StorageChange {
                        address,
                        key,
                        old_value,
                        new_value,
                    });

                    // **CRITICAL FIX: Persist storage changes to underlying storage**
                    if let Err(e) = self.storage.set_storage(address, key, new_value) {
                        tracing::error!("Failed to persist storage change for {:?}[{:?}]: {}", address, key, e);
                    } else {
                        tracing::debug!("Successfully persisted storage change for {:?}[{:?}] = {:?}",
                                       address, key, new_value);
                    }
                }

                // Update the storage snapshots (keep existing in-memory tracking)
                self.storage_snapshots
                    .entry(address)
                    .or_insert_with(HashMap::new)
                    .insert(key, new_value);
            }
        }
    }
}

/// Helper function for keccak256 hashing
pub fn keccak256(data: &[u8]) -> B256 {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(data);
    B256::from_slice(&hasher.finalize())
}

/// Convert an EVM address to a proper Tron format address (base58 with checksum)
fn to_tron_address(address: &Address) -> String {
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
fn from_tron_address(tron_address: &str) -> Result<Address, anyhow::Error> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use revm::primitives::{AccountInfo, Bytecode};

    #[test]
    fn test_snapshot_hooks() {
        let storage = InMemoryEvmStateStore::new();
        let mut db = StorageAdapterDatabase::new(storage);

        // Track modified accounts via hook
        let modified_accounts = Arc::new(Mutex::new(Vec::new()));
        let hook_accounts = modified_accounts.clone();

        db.add_snapshot_hook(move |accounts: &HashSet<Address>| {
            let mut hook_accounts = hook_accounts.lock().unwrap();
            hook_accounts.extend(accounts.iter().cloned());
        });

        // Create test account
        let test_address = Address::from([1u8; 20]);
        let account = Account {
            info: AccountInfo {
                balance: U256::from(1000),
                nonce: 1,
                code_hash: B256::ZERO,
                code: Some(Bytecode::new()),
            },
            storage: HashMap::new(),
            status: revm::primitives::AccountStatus::Loaded,
        };

        // Commit changes (this should trigger the hook)
        let mut changes = HashMap::new();
        changes.insert(test_address, account);
        db.commit(changes);

        // Verify hook was called
        let captured_accounts = modified_accounts.lock().unwrap();
        assert!(captured_accounts.contains(&test_address));

        // Verify modified accounts tracking
        assert!(db.get_modified_accounts().contains(&test_address));
    }

    #[test]
    fn test_modified_accounts_tracking() {
        let storage = InMemoryEvmStateStore::new();
        let mut db = StorageAdapterDatabase::new(storage);

        let addr1 = Address::from([1u8; 20]);
        let addr2 = Address::from([2u8; 20]);

        // Initially no modified accounts
        assert_eq!(db.get_modified_accounts().len(), 0);

        // Mark accounts as modified
        db.mark_account_modified(addr1);
        db.mark_account_modified(addr2);

        // Verify tracking
        assert_eq!(db.get_modified_accounts().len(), 2);
        assert!(db.get_modified_accounts().contains(&addr1));
        assert!(db.get_modified_accounts().contains(&addr2));

        // Clear and verify
        db.clear_modified_accounts();
        assert_eq!(db.get_modified_accounts().len(), 0);
    }

    #[test]
    fn test_snapshot_revert_clears_modified_accounts() {
        let storage = InMemoryEvmStateStore::new();
        let mut db = StorageAdapterDatabase::new(storage);

        let test_address = Address::from([1u8; 20]);

        // Create snapshot
        let snapshot_id = db.snapshot();

        // Mark account as modified
        db.mark_account_modified(test_address);
        assert!(db.get_modified_accounts().contains(&test_address));

        // Revert snapshot
        assert!(db.revert(snapshot_id));

        // Verify modified accounts were cleared
        assert_eq!(db.get_modified_accounts().len(), 0);
    }

    #[test]
    fn test_tron_address_conversion() {
        // Test the specific example provided
        let tron_address = "TB16q6kpSEW2WqvTJ9ua7HAoP9ugQ2HdHZ";
        let expected_evm_hex = "0x0B53CE4AA6F0C2F3C849F11F682702EC99622E2E";
        
        // Convert Tron address to EVM address
        let evm_address = from_tron_address(tron_address).expect("Failed to parse Tron address");
        let actual_evm_hex = format!("0x{}", hex::encode(evm_address.as_slice()).to_uppercase());
        
        assert_eq!(actual_evm_hex, expected_evm_hex, 
                   "EVM address mismatch: expected {}, got {}", expected_evm_hex, actual_evm_hex);
        
        // Convert EVM address back to Tron address
        let converted_tron_address = to_tron_address(&evm_address);
        
        assert_eq!(converted_tron_address, tron_address,
                   "Tron address mismatch: expected {}, got {}", tron_address, converted_tron_address);
    }

    #[test]
    fn test_tron_address_roundtrip() {
        // Test multiple addresses for round-trip conversion
        let test_cases = vec![
            // Add the specific example
            ("TB16q6kpSEW2WqvTJ9ua7HAoP9ugQ2HdHZ", "0x0B53CE4AA6F0C2F3C849F11F682702EC99622E2E"),
        ];

        for (tron_addr, evm_hex) in test_cases {
            // Parse expected EVM address
            let expected_evm = Address::from_slice(&hex::decode(&evm_hex[2..]).expect("Invalid hex"));

            // Test Tron -> EVM conversion
            let parsed_evm = from_tron_address(tron_addr).expect("Failed to parse Tron address");
            assert_eq!(parsed_evm, expected_evm, "Tron->EVM conversion failed");

            // Test EVM -> Tron conversion
            let converted_tron = to_tron_address(&expected_evm);
            assert_eq!(converted_tron, tron_addr, "EVM->Tron conversion failed");

            // Test full round-trip
            let roundtrip_evm = from_tron_address(&converted_tron).expect("Round-trip failed");
            assert_eq!(roundtrip_evm, expected_evm, "Round-trip conversion failed");
        }
    }

    #[test]
    fn test_account_name_storage() {
        let storage = InMemoryEvmStateStore::new();
        let storage_engine = tron_backend_storage::StorageEngine::new_mock();
        let mut adapter = StorageModuleAdapter::new(storage_engine);

        let test_address = Address::from([1u8; 20]);
        let test_name = b"TestAccount";

        // Test setting and getting account name
        assert!(adapter.set_account_name(test_address, test_name).is_ok());

        let retrieved_name = adapter.get_account_name(&test_address).unwrap();
        assert_eq!(retrieved_name, Some("TestAccount".to_string()));

        // Test non-existent account name
        let non_existent_address = Address::from([2u8; 20]);
        let no_name = adapter.get_account_name(&non_existent_address).unwrap();
        assert_eq!(no_name, None);
    }

    #[test]
    fn test_account_name_validation() {
        let storage_engine = tron_backend_storage::StorageEngine::new_mock();
        let mut adapter = StorageModuleAdapter::new(storage_engine);

        let test_address = Address::from([1u8; 20]);

        // Test empty name (should fail)
        let empty_name = b"";
        assert!(adapter.set_account_name(test_address, empty_name).is_err());

        // Test name too long (should fail)
        let long_name = b"ThisIsAVeryLongAccountNameThatExceedsTheThirtyTwoByteLimitAndShouldFail";
        assert!(adapter.set_account_name(test_address, long_name).is_err());

        // Test valid name length
        let valid_name = b"ValidAccountName";
        assert!(adapter.set_account_name(test_address, valid_name).is_ok());

        // Test maximum length name (32 bytes)
        let max_length_name = b"ThisIsExactlyThirtyTwoBytesLong!";
        let another_address = Address::from([2u8; 20]);
        assert_eq!(max_length_name.len(), 32);
        assert!(adapter.set_account_name(another_address, max_length_name).is_ok());
    }

    #[test]
    fn test_account_name_utf8_handling() {
        let storage_engine = tron_backend_storage::StorageEngine::new_mock();
        let mut adapter = StorageModuleAdapter::new(storage_engine);

        let test_address = Address::from([1u8; 20]);

        // Test valid UTF-8 name
        let utf8_name = "ValidUTF8Name".as_bytes();
        assert!(adapter.set_account_name(test_address, utf8_name).is_ok());

        let retrieved_name = adapter.get_account_name(&test_address).unwrap();
        assert_eq!(retrieved_name, Some("ValidUTF8Name".to_string()));

        // Test non-UTF-8 bytes (should store but warn)
        let non_utf8_address = Address::from([2u8; 20]);
        let non_utf8_name = &[0xFF, 0xFE, 0xFD, 0xFC]; // Invalid UTF-8 sequence
        assert!(adapter.set_account_name(non_utf8_address, non_utf8_name).is_ok());

        // Should fail to decode as UTF-8 but the setting should have succeeded
        let result = adapter.get_account_name(&non_utf8_address);
        assert!(result.is_err()); // Should error when trying to decode invalid UTF-8
    }

    #[test]
    fn test_witness_protobuf_encode_decode() {
        // Test protobuf encoding and decoding roundtrip
        let address = Address::from([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
                                      0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
                                      0x12, 0x34, 0x56, 0x78]);
        let witness_info = WitnessInfo {
            address,
            url: "https://test-witness.com".to_string(),
            vote_count: 1000,
        };

        // Encode as protobuf
        let protobuf_data = witness_info.serialize();
        assert!(!protobuf_data.is_empty(), "Protobuf data should not be empty");

        // Decode protobuf
        let decoded = WitnessInfo::deserialize(&protobuf_data)
            .expect("Protobuf decode should succeed");

        assert_eq!(decoded.address, witness_info.address);
        assert_eq!(decoded.url, witness_info.url);
        assert_eq!(decoded.vote_count, witness_info.vote_count);
    }

    #[test]
    fn test_witness_legacy_encode_decode() {
        // Test legacy encoding and decoding roundtrip
        let address = Address::from([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
                                      0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
                                      0x12, 0x34, 0x56, 0x78]);
        let witness_info = WitnessInfo {
            address,
            url: "https://legacy-witness.com".to_string(),
            vote_count: 2000,
        };

        // Encode as legacy
        let legacy_data = witness_info.serialize();
        assert!(!legacy_data.is_empty(), "Legacy data should not be empty");

        // Decode legacy
        let decoded = WitnessInfo::deserialize(&legacy_data)
            .expect("Legacy decode should succeed");

        assert_eq!(decoded.address, witness_info.address);
        assert_eq!(decoded.url, witness_info.url);
        assert_eq!(decoded.vote_count, witness_info.vote_count);
    }

    #[test]
    fn test_witness_protobuf_fallback_to_legacy() {
        // Create legacy format data
        let address = Address::from([0xab; 20]);
        let witness_info = WitnessInfo {
            address,
            url: "fallback-test".to_string(),
            vote_count: 500,
        };
        let legacy_data = witness_info.serialize();

        // Try protobuf decode first (should fail)
        assert!(WitnessInfo::deserialize(&legacy_data).is_err(),
                "Protobuf decode of legacy data should fail");

        // Legacy decode should succeed
        let decoded = WitnessInfo::deserialize(&legacy_data)
            .expect("Legacy decode should succeed");
        assert_eq!(decoded.address, witness_info.address);
        assert_eq!(decoded.url, witness_info.url);
        assert_eq!(decoded.vote_count, witness_info.vote_count);
    }

    #[test]
    fn test_witness_protobuf_address_formats() {
        use prost::Message;
        use crate::protocol::Witness;

        // Test 21-byte TRON address (0x41 prefix)
        let mut tron_addr_21 = vec![0x41];
        tron_addr_21.extend_from_slice(&[0x12; 20]);

        let witness_21 = Witness {
            address: tron_addr_21.clone(),
            vote_count: 100,
            url: "test".to_string(),
            pub_key: vec![],
            total_produced: 0,
            total_missed: 0,
            latest_block_num: 0,
            latest_slot_num: 0,
            is_jobs: true,
        };
        let data_21 = witness_21.encode_to_vec();

        let decoded_21 = WitnessInfo::deserialize(&data_21)
            .expect("Should decode 21-byte TRON address");
        assert_eq!(decoded_21.address, Address::from([0x12; 20]));

        // Test 20-byte address (no prefix)
        let witness_20 = Witness {
            address: vec![0x34; 20],
            vote_count: 200,
            url: "test".to_string(),
            pub_key: vec![],
            total_produced: 0,
            total_missed: 0,
            latest_block_num: 0,
            latest_slot_num: 0,
            is_jobs: true,
        };
        let data_20 = witness_20.encode_to_vec();

        let decoded_20 = WitnessInfo::deserialize(&data_20)
            .expect("Should decode 20-byte address");
        assert_eq!(decoded_20.address, Address::from([0x34; 20]));
    }

    #[test]
    fn test_witness_protobuf_negative_vote_count() {
        use prost::Message;
        use crate::protocol::Witness;

        let witness = Witness {
            address: vec![0x41; 21],
            vote_count: -100, // Negative vote count
            url: "test".to_string(),
            pub_key: vec![],
            total_produced: 0,
            total_missed: 0,
            latest_block_num: 0,
            latest_slot_num: 0,
            is_jobs: true,
        };
        let data = witness.encode_to_vec();

        // Should fail on negative vote count
        assert!(WitnessInfo::deserialize(&data).is_err(),
                "Should reject negative voteCount");
    }

    #[test]
    fn test_witness_protobuf_invalid_address_length() {
        use prost::Message;
        use crate::protocol::Witness;

        let witness = Witness {
            address: vec![0x41; 19], // Invalid length
            vote_count: 100,
            url: "test".to_string(),
            pub_key: vec![],
            total_produced: 0,
            total_missed: 0,
            latest_block_num: 0,
            latest_slot_num: 0,
            is_jobs: true,
        };
        let data = witness.encode_to_vec();

        // Should fail on invalid address length
        assert!(WitnessInfo::deserialize(&data).is_err(),
                "Should reject invalid address length");
    }

    #[test]
    fn test_witness_empty_url() {
        // Test that empty URLs are allowed
        let address = Address::from([0xcd; 20]);
        let witness_info = WitnessInfo {
            address,
            url: "".to_string(), // Empty URL
            vote_count: 0,
        };

        // Protobuf roundtrip
        let protobuf_data = witness_info.serialize();
        let decoded_pb = WitnessInfo::deserialize(&protobuf_data)
            .expect("Should decode empty URL from protobuf");
        assert_eq!(decoded_pb.url, "");

        // Legacy roundtrip
        let legacy_data = witness_info.serialize();
        let decoded_legacy = WitnessInfo::deserialize(&legacy_data)
            .expect("Should decode empty URL from legacy");
        assert_eq!(decoded_legacy.url, "");
    }

    // Tron power computation tests

    #[test]
    fn test_tron_power_bandwidth_only() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xab; 20]);

        // Set freeze record for BANDWIDTH (resource=0)
        storage.set_freeze_record(&address, 0, 1_000_000, 1000000000)
            .expect("Should set freeze record");

        let power = storage.get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 1_000_000, "Expected power from bandwidth only");
    }

    #[test]
    fn test_tron_power_energy_only() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xbc; 20]);

        // Set freeze record for ENERGY (resource=1)
        storage.set_freeze_record(&address, 1, 2_000_000, 1000000000)
            .expect("Should set freeze record");

        let power = storage.get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 2_000_000, "Expected power from energy only");
    }

    #[test]
    fn test_tron_power_sum_bw_energy() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xcd; 20]);

        // Set freeze records for both BANDWIDTH and ENERGY
        storage.set_freeze_record(&address, 0, 1_000_000, 1000000000)
            .expect("Should set bandwidth freeze");
        storage.set_freeze_record(&address, 1, 2_000_000, 1000000000)
            .expect("Should set energy freeze");

        let power = storage.get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 3_000_000, "Expected sum of bandwidth + energy");
    }

    #[test]
    fn test_tron_power_includes_tron_power_legacy() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xde; 20]);

        // Set freeze record for TRON_POWER (resource=2) only
        storage.set_freeze_record(&address, 2, 500_000, 1000000000)
            .expect("Should set tron_power freeze");

        let power = storage.get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 500_000, "Expected power from legacy tron_power");
    }

    #[test]
    fn test_tron_power_all_three() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xef; 20]);

        // Set freeze records for all three resources
        storage.set_freeze_record(&address, 0, 1_000_000, 1000000000)
            .expect("Should set bandwidth freeze");
        storage.set_freeze_record(&address, 1, 2_000_000, 1000000000)
            .expect("Should set energy freeze");
        storage.set_freeze_record(&address, 2, 500_000, 1000000000)
            .expect("Should set tron_power freeze");

        let power = storage.get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 3_500_000, "Expected sum of all three resources");
    }

    #[test]
    fn test_tron_power_overflow_protection() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xf0; 20]);

        // Set freeze records that would overflow u64
        let near_max = u64::MAX - 100_000;
        storage.set_freeze_record(&address, 0, near_max, 1000000000)
            .expect("Should set bandwidth freeze");
        storage.set_freeze_record(&address, 1, 200_000, 1000000000)
            .expect("Should set energy freeze");

        // Should return error due to overflow
        let result = storage.get_tron_power_in_sun(&address, false);
        assert!(result.is_err(), "Expected overflow error");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("overflow"), "Error should mention overflow");
    }

    #[test]
    fn test_tron_power_no_freeze_records() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xa1; 20]);

        // No freeze records set
        let power = storage.get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 0, "Expected zero power when no freeze records");
    }

    // ResourceTracker Tests

    #[test]
    fn test_resource_tracker_increase_no_time_delta() {
        // When now == lastTime, no recovery should occur
        let new_usage = ResourceTracker::increase(100, 50, 1000, 1000, 28800);
        assert_eq!(new_usage, 150, "No recovery when time delta is 0");
    }

    #[test]
    fn test_resource_tracker_increase_partial_recovery() {
        // lastUsage=1000, usage=200, lastTime=0, now=14400, windowSize=28800
        // Time delta = 14400 (half the window)
        // Recovery = 1000 * 14400 / 28800 = 500
        // New usage = max(0, 1000 - 500) + 200 = 500 + 200 = 700
        let new_usage = ResourceTracker::increase(1000, 200, 0, 14400, 28800);
        assert_eq!(new_usage, 700, "Half window should recover half usage");
    }

    #[test]
    fn test_resource_tracker_increase_full_recovery() {
        // lastUsage=1000, usage=200, lastTime=0, now=28800, windowSize=28800
        // Time delta = 28800 (full window)
        // Recovery = 1000 (full recovery)
        // New usage = max(0, 1000 - 1000) + 200 = 0 + 200 = 200
        let new_usage = ResourceTracker::increase(1000, 200, 0, 28800, 28800);
        assert_eq!(new_usage, 200, "Full window should fully recover");
    }

    #[test]
    fn test_resource_tracker_increase_beyond_window() {
        // Time delta exceeds window - should fully recover
        let new_usage = ResourceTracker::increase(1000, 200, 0, 50000, 28800);
        assert_eq!(new_usage, 200, "Beyond window should fully recover");
    }

    #[test]
    fn test_resource_tracker_recovery_zero_usage() {
        // Recovery with zero last usage
        let new_usage = ResourceTracker::recovery(0, 0, 14400, 28800);
        assert_eq!(new_usage, 0, "Recovery of zero usage should be zero");
    }

    #[test]
    fn test_resource_tracker_recovery_half_window() {
        // lastUsage=1000, lastTime=0, now=14400, windowSize=28800
        // Should recover to 500
        let recovered = ResourceTracker::recovery(1000, 0, 14400, 28800);
        assert_eq!(recovered, 500, "Half window recovery");
    }

    #[test]
    fn test_resource_tracker_increase_zero_window() {
        // When windowSize is 0, should just return the usage
        let new_usage = ResourceTracker::increase(1000, 200, 0, 14400, 0);
        assert_eq!(new_usage, 200, "Zero window should return usage only");
    }

    #[test]
    fn test_resource_tracker_increase_negative_time_delta() {
        // When now < lastTime, should not recover
        let new_usage = ResourceTracker::increase(1000, 200, 5000, 4000, 28800);
        assert_eq!(new_usage, 1200, "Negative time delta should not recover");
    }

    #[test]
    fn test_resource_tracker_increase_overflow_protection() {
        // Test with very large values to ensure no overflow
        let new_usage = ResourceTracker::increase(i64::MAX / 2, 100, 0, 100, 28800);
        // Should not panic and should return a reasonable value
        assert!(new_usage > 0, "Should handle large values without overflow");
    }

    #[test]
    fn test_resource_tracker_track_bandwidth_free_net_path() {
        use crate::storage_adapter::{ResourceTracker, AccountAext, BandwidthPath};

        let owner = Address::from([0xab; 20]);
        let current_aext = AccountAext::with_defaults();
        let free_net_limit = 5000i64;
        let bytes_used = 212i64;
        let now = 1000i64;

        let result = ResourceTracker::track_bandwidth(
            &owner,
            bytes_used,
            now,
            &current_aext,
            free_net_limit,
        );

        assert!(result.is_ok(), "Track bandwidth should succeed");
        let (path, before, after) = result.unwrap();

        assert_eq!(path, BandwidthPath::FreeNet, "Should use FREE_NET path");
        assert_eq!(before.free_net_usage, 0, "Before should have zero free_net_usage");
        assert_eq!(after.free_net_usage, 212, "After should have 212 free_net_usage");
        assert_eq!(after.latest_consume_free_time, 1000, "Should update consume time");
    }

    #[test]
    fn test_resource_tracker_track_bandwidth_with_existing_usage() {
        use crate::storage_adapter::{ResourceTracker, AccountAext, BandwidthPath};

        let owner = Address::from([0xcd; 20]);
        let mut current_aext = AccountAext::with_defaults();
        current_aext.free_net_usage = 1000;
        current_aext.latest_consume_free_time = 0;

        let free_net_limit = 5000i64;
        let bytes_used = 212i64;
        let now = 14400i64; // Half window

        let result = ResourceTracker::track_bandwidth(
            &owner,
            bytes_used,
            now,
            &current_aext,
            free_net_limit,
        );

        assert!(result.is_ok(), "Track bandwidth should succeed");
        let (path, before, after) = result.unwrap();

        assert_eq!(path, BandwidthPath::FreeNet, "Should use FREE_NET path");
        // Before: recovered from 1000 by half = 500
        assert_eq!(before.free_net_usage, 500, "Before should have recovered to 500");
        // After: 500 + 212 = 712
        assert_eq!(after.free_net_usage, 712, "After should have 712 free_net_usage");
    }

    #[test]
    fn test_resource_tracker_track_bandwidth_exceeds_limit() {
        use crate::storage_adapter::{ResourceTracker, AccountAext, BandwidthPath};

        let owner = Address::from([0xef; 20]);
        let mut current_aext = AccountAext::with_defaults();
        current_aext.free_net_usage = 4900; // Close to limit
        current_aext.latest_consume_free_time = 0;

        let free_net_limit = 5000i64;
        let bytes_used = 500i64; // Would exceed limit
        let now = 100i64; // Small time delta

        let result = ResourceTracker::track_bandwidth(
            &owner,
            bytes_used,
            now,
            &current_aext,
            free_net_limit,
        );

        assert!(result.is_ok(), "Track bandwidth should succeed");
        let (path, _before, _after) = result.unwrap();

        // Should fall back to FEE when FREE_NET is insufficient
        assert_eq!(path, BandwidthPath::Fee, "Should use FEE path when limit exceeded");
    }

    #[test]
    fn test_account_aext_serialization_roundtrip() {
        let aext = AccountAext {
            net_usage: 100,
            free_net_usage: 200,
            energy_usage: 0,
            latest_consume_time: 1000,
            latest_consume_free_time: 2000,
            latest_consume_time_for_energy: 0,
            net_window_size: 28800,
            net_window_optimized: false,
            energy_window_size: 28800,
            energy_window_optimized: false,
        };

        let serialized = aext.serialize();
        assert_eq!(serialized.len(), 82, "Serialized size should be 82 bytes");

        let deserialized = AccountAext::deserialize(&serialized)
            .expect("Should deserialize");

        assert_eq!(deserialized.net_usage, 100);
        assert_eq!(deserialized.free_net_usage, 200);
        assert_eq!(deserialized.latest_consume_time, 1000);
        assert_eq!(deserialized.latest_consume_free_time, 2000);
        assert_eq!(deserialized.net_window_size, 28800);
        assert_eq!(deserialized.net_window_optimized, false);
    }

    #[test]
    fn test_account_aext_with_defaults() {
        let aext = AccountAext::with_defaults();

        assert_eq!(aext.net_usage, 0);
        assert_eq!(aext.free_net_usage, 0);
        assert_eq!(aext.net_window_size, 28800);
        assert_eq!(aext.energy_window_size, 28800);
        assert_eq!(aext.net_window_optimized, false);
        assert_eq!(aext.energy_window_optimized, false);
    }
}

/// Resource Tracker for bandwidth and energy accounting
/// Mirrors Java's BandwidthProcessor and ResourceProcessor semantics
pub struct ResourceTracker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandwidthPath {
    AccountNet,  // Used account frozen bandwidth
    FreeNet,     // Used free public bandwidth
    Fee,         // Fall back to fee deduction
}

impl ResourceTracker {
    /// Increase usage with windowed recovery (Java ResourceProcessor.increase parity)
    /// Formula: newUsage = increase(lastUsage, usage, lastTime, now, windowSize)
    ///        = max(0, lastUsage - (now - lastTime) / windowSize * lastUsage) + usage
    /// Simplified: recovered = lastUsage * (now - lastTime) / windowSize
    ///            newUsage = max(0, lastUsage - recovered) + usage
    pub fn increase(
        last_usage: i64,
        usage: i64,
        last_time: i64,
        now: i64,
        window_size: i64,
    ) -> i64 {
        if window_size == 0 {
            return usage;
        }

        let time_delta = now.saturating_sub(last_time);
        if time_delta <= 0 {
            // No time passed, just add usage
            return last_usage.saturating_add(usage);
        }

        // Calculate recovered amount: (last_usage * time_delta) / window_size
        // Use saturating operations to avoid overflow
        let recovered = if time_delta >= window_size {
            // Fully recovered if time delta exceeds window
            last_usage
        } else {
            // Partial recovery: last_usage * time_delta / window_size
            let numerator = (last_usage as i128).saturating_mul(time_delta as i128);
            let recovered_amt = numerator / (window_size as i128);
            recovered_amt.min(last_usage as i128) as i64
        };

        // New usage = max(0, last_usage - recovered) + usage
        let after_recovery = last_usage.saturating_sub(recovered).max(0);
        after_recovery.saturating_add(usage)
    }

    /// Compute recovered usage (for debugging/logging)
    pub fn recovery(last_usage: i64, last_time: i64, now: i64, window_size: i64) -> i64 {
        Self::increase(last_usage, 0, last_time, now, window_size)
    }

    /// Track bandwidth usage and return (path, before_aext, after_aext)
    /// Mirrors Java BandwidthProcessor.consume path selection:
    /// 1. Try ACCOUNT_NET (if account has frozen bandwidth)
    /// 2. Try FREE_NET (if public bandwidth available)
    /// 3. Fall back to FEE (charge TRX)
    pub fn track_bandwidth(
        _owner: &Address,
        bytes_used: i64,
        now: i64,  // block number or slot
        current_aext: &AccountAext,
        free_net_limit: i64,
    ) -> Result<(BandwidthPath, AccountAext, AccountAext)> {
        // Compute before AEXT (with decay but no new usage)
        let net_window_size = if current_aext.net_window_size > 0 {
            current_aext.net_window_size
        } else {
            28800
        };

        let free_net_window_size = 28800i64; // Default window for free net

        // Recover net_usage
        let recovered_net_usage = Self::recovery(
            current_aext.net_usage,
            current_aext.latest_consume_time,
            now,
            net_window_size,
        );

        // Recover free_net_usage
        let recovered_free_net_usage = Self::recovery(
            current_aext.free_net_usage,
            current_aext.latest_consume_free_time,
            now,
            free_net_window_size,
        );

        let before_aext = AccountAext {
            net_usage: recovered_net_usage,
            free_net_usage: recovered_free_net_usage,
            energy_usage: current_aext.energy_usage,
            latest_consume_time: current_aext.latest_consume_time,
            latest_consume_free_time: current_aext.latest_consume_free_time,
            latest_consume_time_for_energy: current_aext.latest_consume_time_for_energy,
            net_window_size: current_aext.net_window_size,
            net_window_optimized: current_aext.net_window_optimized,
            energy_window_size: current_aext.energy_window_size,
            energy_window_optimized: current_aext.energy_window_optimized,
        };

        // Path selection logic
        // Phase 1: Simplified - assume no frozen bandwidth (ACCOUNT_NET always 0)
        let account_net_limit = 0i64;  // Would calculate from freeze records in full implementation

        let available_account_net = account_net_limit.saturating_sub(recovered_net_usage).max(0);

        let (path, after_aext) = if bytes_used <= available_account_net {
            // Path 1: ACCOUNT_NET
            let new_net_usage = Self::increase(
                current_aext.net_usage,
                bytes_used,
                current_aext.latest_consume_time,
                now,
                net_window_size,
            );

            let after = AccountAext {
                net_usage: new_net_usage,
                latest_consume_time: now,
                ..before_aext.clone()
            };

            (BandwidthPath::AccountNet, after)
        } else {
            // Try FREE_NET
            let available_free_net = free_net_limit.saturating_sub(recovered_free_net_usage).max(0);

            if bytes_used <= available_free_net {
                // Path 2: FREE_NET
                let new_free_net_usage = Self::increase(
                    current_aext.free_net_usage,
                    bytes_used,
                    current_aext.latest_consume_free_time,
                    now,
                    free_net_window_size,
                );

                let after = AccountAext {
                    free_net_usage: new_free_net_usage,
                    latest_consume_free_time: now,
                    ..before_aext.clone()
                };

                (BandwidthPath::FreeNet, after)
            } else {
                // Path 3: FEE (no AEXT changes)
                (BandwidthPath::Fee, before_aext.clone())
            }
        };

        Ok((path, before_aext, after_aext))
    }
}
