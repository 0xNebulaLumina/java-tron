use revm::primitives::{
    Bytes, PrecompileError, PrecompileErrors, PrecompileOutput, PrecompileResult, U256,
};
use tracing::debug;

use super::TronPrecompile;

// System precompile implementations
pub struct SystemEnergyPrice;
pub struct SystemBandwidthPrice;
pub struct SystemBlockInfo;

impl TronPrecompile for SystemEnergyPrice {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        const GAS_COST: u64 = 100;

        if gas_limit < GAS_COST {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }

        if !input.is_empty() {
            return Err(PrecompileErrors::Fatal {
                msg: "No input expected for energy price query".to_string(),
            });
        }

        debug!("System Energy Price query");

        // Return current energy price (in SUN per energy unit)
        let energy_price = U256::from(420u64); // 420 SUN per energy unit (typical mainnet value)
        let energy_price_bytes = energy_price.to_be_bytes_vec();

        Ok(PrecompileOutput::new(
            GAS_COST,
            Bytes::from(energy_price_bytes),
        ))
    }
}

impl TronPrecompile for SystemBandwidthPrice {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        const GAS_COST: u64 = 100;

        if gas_limit < GAS_COST {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }

        if !input.is_empty() {
            return Err(PrecompileErrors::Fatal {
                msg: "No input expected for bandwidth price query".to_string(),
            });
        }

        debug!("System Bandwidth Price query");

        // Return current bandwidth price (in SUN per byte)
        let bandwidth_price = U256::from(1000u64); // 1000 SUN per byte (typical mainnet value)
        let bandwidth_price_bytes = bandwidth_price.to_be_bytes_vec();

        Ok(PrecompileOutput::new(
            GAS_COST,
            Bytes::from(bandwidth_price_bytes),
        ))
    }
}

impl TronPrecompile for SystemBlockInfo {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        const GAS_COST: u64 = 200;

        if gas_limit < GAS_COST {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }

        if !input.is_empty() {
            return Err(PrecompileErrors::Fatal {
                msg: "No input expected for block info query".to_string(),
            });
        }

        debug!("System Block Info query");

        // Return current block info: [block_number (32 bytes)][block_timestamp (32 bytes)]
        let mut result = vec![0u8; 64];

        // Mock block number and timestamp
        let block_number = U256::from(12345678u64);
        let block_timestamp = U256::from(1640995200u64); // 2022-01-01 00:00:00 UTC

        let block_number_bytes = block_number.to_be_bytes_vec();
        let block_timestamp_bytes = block_timestamp.to_be_bytes_vec();

        result[0..32].copy_from_slice(&block_number_bytes);
        result[32..64].copy_from_slice(&block_timestamp_bytes);

        Ok(PrecompileOutput::new(GAS_COST, Bytes::from(result)))
    }
}

// System configuration constants
pub const MAINNET_ENERGY_PRICE: u64 = 420; // SUN per energy unit
pub const MAINNET_BANDWIDTH_PRICE: u64 = 1000; // SUN per byte
pub const TESTNET_ENERGY_PRICE: u64 = 10; // SUN per energy unit
pub const TESTNET_BANDWIDTH_PRICE: u64 = 100; // SUN per byte

// Resource type definitions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    Energy,
    Bandwidth,
}

impl ResourceType {
    pub fn price(&self, is_mainnet: bool) -> u64 {
        match (self, is_mainnet) {
            (ResourceType::Energy, true) => MAINNET_ENERGY_PRICE,
            (ResourceType::Energy, false) => TESTNET_ENERGY_PRICE,
            (ResourceType::Bandwidth, true) => MAINNET_BANDWIDTH_PRICE,
            (ResourceType::Bandwidth, false) => TESTNET_BANDWIDTH_PRICE,
        }
    }
}

// Resource utilization tracking
#[derive(Debug, Clone)]
pub struct ResourceUtilization {
    pub energy_used: u64,
    pub energy_limit: u64,
    pub bandwidth_used: u64,
    pub bandwidth_limit: u64,
}

impl ResourceUtilization {
    pub fn new(energy_limit: u64, bandwidth_limit: u64) -> Self {
        Self {
            energy_used: 0,
            energy_limit,
            bandwidth_used: 0,
            bandwidth_limit,
        }
    }

    pub fn use_energy(&mut self, amount: u64) -> Result<(), String> {
        if self.energy_used + amount > self.energy_limit {
            return Err("Energy limit exceeded".to_string());
        }
        self.energy_used += amount;
        Ok(())
    }

    pub fn use_bandwidth(&mut self, amount: u64) -> Result<(), String> {
        if self.bandwidth_used + amount > self.bandwidth_limit {
            return Err("Bandwidth limit exceeded".to_string());
        }
        self.bandwidth_used += amount;
        Ok(())
    }

    pub fn energy_remaining(&self) -> u64 {
        self.energy_limit.saturating_sub(self.energy_used)
    }

    pub fn bandwidth_remaining(&self) -> u64 {
        self.bandwidth_limit.saturating_sub(self.bandwidth_used)
    }
}
