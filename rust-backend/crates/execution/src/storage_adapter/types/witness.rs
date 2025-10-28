use anyhow::Result;
use revm::primitives::Address;

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
