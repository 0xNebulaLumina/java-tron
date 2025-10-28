use anyhow::Result;

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
