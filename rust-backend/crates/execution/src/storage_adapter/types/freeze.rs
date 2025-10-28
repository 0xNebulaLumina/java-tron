use anyhow::Result;

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
