use anyhow::Result;
use revm::primitives::Address;

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
