use revm::primitives::{Address, Bytes, PrecompileResult};
use std::collections::HashMap;

// TRC-10 and TRC-20 precompile modules
pub mod system;
pub mod trc10;
pub mod trc20;

/// Tron precompile manager
pub struct TronPrecompiles {
    precompiles: HashMap<Address, Box<dyn TronPrecompile>>,
}

impl TronPrecompiles {
    pub fn new() -> Self {
        let mut precompiles: HashMap<Address, Box<dyn TronPrecompile>> = HashMap::new();

        // TRC-10 precompiles
        precompiles.insert(TRC10_TRANSFER_ADDR, Box::new(trc10::Trc10Transfer));
        precompiles.insert(TRC10_BALANCE_ADDR, Box::new(trc10::Trc10Balance));
        precompiles.insert(TRC10_APPROVE_ADDR, Box::new(trc10::Trc10Approve));
        precompiles.insert(TRC10_ALLOWANCE_ADDR, Box::new(trc10::Trc10Allowance));

        // TRC-20 precompiles
        precompiles.insert(TRC20_TRANSFER_ADDR, Box::new(trc20::Trc20Transfer));
        precompiles.insert(TRC20_BALANCE_ADDR, Box::new(trc20::Trc20Balance));
        precompiles.insert(TRC20_APPROVE_ADDR, Box::new(trc20::Trc20Approve));
        precompiles.insert(TRC20_ALLOWANCE_ADDR, Box::new(trc20::Trc20Allowance));

        // System precompiles
        precompiles.insert(
            SYSTEM_ENERGY_PRICE_ADDR,
            Box::new(system::SystemEnergyPrice),
        );
        precompiles.insert(
            SYSTEM_BANDWIDTH_PRICE_ADDR,
            Box::new(system::SystemBandwidthPrice),
        );
        precompiles.insert(SYSTEM_BLOCK_INFO_ADDR, Box::new(system::SystemBlockInfo));

        Self { precompiles }
    }

    pub fn get(&self, address: &Address) -> Option<&Box<dyn TronPrecompile>> {
        self.precompiles.get(address)
    }

    pub fn contains(&self, address: &Address) -> bool {
        self.precompiles.contains_key(address)
    }

    pub fn execute(
        &self,
        address: &Address,
        input: &Bytes,
        gas_limit: u64,
    ) -> Option<PrecompileResult> {
        if let Some(precompile) = self.get(address) {
            Some(precompile.execute(input, gas_limit))
        } else {
            None
        }
    }
}

/// Trait for Tron-specific precompiles
pub trait TronPrecompile: Send + Sync {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult;
}

// Precompile addresses using Address::new() for const compatibility
pub const TRC10_TRANSFER_ADDR: Address = Address::new([
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01,
]);
pub const TRC10_BALANCE_ADDR: Address = Address::new([
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x02,
]);
pub const TRC10_APPROVE_ADDR: Address = Address::new([
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x03,
]);
pub const TRC10_ALLOWANCE_ADDR: Address = Address::new([
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x04,
]);

pub const TRC20_TRANSFER_ADDR: Address = Address::new([
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x01,
]);
pub const TRC20_BALANCE_ADDR: Address = Address::new([
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x02,
]);
pub const TRC20_APPROVE_ADDR: Address = Address::new([
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x03,
]);
pub const TRC20_ALLOWANCE_ADDR: Address = Address::new([
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x04,
]);

pub const SYSTEM_ENERGY_PRICE_ADDR: Address = Address::new([
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x02, 0x01,
]);
pub const SYSTEM_BANDWIDTH_PRICE_ADDR: Address = Address::new([
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x02, 0x02,
]);
pub const SYSTEM_BLOCK_INFO_ADDR: Address = Address::new([
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x02, 0x03,
]);

impl TronPrecompiles {
    /// Calculate gas cost for a precompile call
    pub fn gas_cost(&self, _input: &Bytes) -> u64 {
        // Simple gas cost calculation
        // In a real implementation, this would be more sophisticated
        3000
    }
}
