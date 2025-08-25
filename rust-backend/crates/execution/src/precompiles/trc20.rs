use revm::primitives::{Bytes, PrecompileResult, PrecompileOutput, PrecompileErrors, PrecompileError, U256, Address};
use tracing::debug;

use super::TronPrecompile;

// TRC-20 precompile implementations
pub struct Trc20Transfer;
pub struct Trc20Balance;
pub struct Trc20Approve;
pub struct Trc20Allowance;

impl TronPrecompile for Trc20Transfer {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        const GAS_COST: u64 = 2000;
        
        if gas_limit < GAS_COST {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }
        
        if input.len() != 84 {
            return Err(PrecompileErrors::Fatal { 
                msg: "Invalid input length for TRC20 transfer".to_string() 
            });
        }
        
        // Parse input: contract_address (20 bytes) + from (20 bytes) + to (20 bytes) + amount (32 bytes)
        let contract_address = Address::from_slice(&input[0..20]);
        let from = Address::from_slice(&input[20..40]);
        let to = Address::from_slice(&input[40..60]);
        let amount = U256::from_be_slice(&input[60..92]);
        
        debug!("TRC20 Transfer: contract={:?}, from={:?}, to={:?}, amount={:?}", 
               contract_address, from, to, amount);
        
        // In a real implementation, this would interact with the TRC-20 contract
        // For now, return success with empty output
        Ok(PrecompileOutput::new(GAS_COST, Bytes::new()))
    }
}

impl TronPrecompile for Trc20Balance {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        const GAS_COST: u64 = 1000;
        
        if gas_limit < GAS_COST {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }
        
        if input.len() != 40 {
            return Err(PrecompileErrors::Fatal { 
                msg: "Invalid input length for TRC20 balance".to_string() 
            });
        }
        
        // Parse input: contract_address (20 bytes) + address (20 bytes)
        let contract_address = Address::from_slice(&input[0..20]);
        let address = Address::from_slice(&input[20..40]);
        
        debug!("TRC20 Balance: contract={:?}, address={:?}", contract_address, address);
        
        // In a real implementation, this would query the TRC-20 contract
        // For now, return a dummy balance
        let balance = U256::from(1000u64);
        let balance_bytes = balance.to_be_bytes_vec();
        
        Ok(PrecompileOutput::new(GAS_COST, Bytes::from(balance_bytes)))
    }
}

impl TronPrecompile for Trc20Approve {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        const GAS_COST: u64 = 2000;
        
        if gas_limit < GAS_COST {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }
        
        if input.len() != 72 {
            return Err(PrecompileErrors::Fatal { 
                msg: "Invalid input length for TRC20 approve".to_string() 
            });
        }
        
        // Parse input: contract_address (20 bytes) + owner (20 bytes) + spender (20 bytes) + amount (32 bytes)
        let contract_address = Address::from_slice(&input[0..20]);
        let owner = Address::from_slice(&input[20..40]);
        let spender = Address::from_slice(&input[40..60]);
        let amount = U256::from_be_slice(&input[60..92]);
        
        debug!("TRC20 Approve: contract={:?}, owner={:?}, spender={:?}, amount={:?}", 
               contract_address, owner, spender, amount);
        
        // In a real implementation, this would update the allowance in the TRC-20 contract
        
        // Return success (1 for success)
        let mut result = vec![0u8; 31];
        result.push(1u8);
        
        Ok(PrecompileOutput::new(GAS_COST, Bytes::from(result)))
    }
}

impl TronPrecompile for Trc20Allowance {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        const GAS_COST: u64 = 1000;
        
        if gas_limit < GAS_COST {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }
        
        if input.len() != 60 {
            return Err(PrecompileErrors::Fatal { 
                msg: "Invalid input length for TRC20 allowance".to_string() 
            });
        }
        
        // Parse input: contract_address (20 bytes) + owner (20 bytes) + spender (20 bytes)
        let contract_address = Address::from_slice(&input[0..20]);
        let owner = Address::from_slice(&input[20..40]);
        let spender = Address::from_slice(&input[40..60]);
        
        debug!("TRC20 Allowance: contract={:?}, owner={:?}, spender={:?}", 
               contract_address, owner, spender);
        
        // In a real implementation, this would query the allowance from the TRC-20 contract
        // For now, return a dummy allowance
        let allowance = U256::from(500u64);
        let allowance_bytes = allowance.to_be_bytes_vec();
        
        Ok(PrecompileOutput::new(GAS_COST, Bytes::from(allowance_bytes)))
    }
}

// ERC-20/TRC-20 function selectors
pub const TRANSFER_SELECTOR: [u8; 4] = [0xa9, 0x05, 0x9c, 0xbb];
pub const BALANCE_OF_SELECTOR: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];
pub const APPROVE_SELECTOR: [u8; 4] = [0x09, 0x5e, 0xa7, 0xb3];
pub const ALLOWANCE_SELECTOR: [u8; 4] = [0xdd, 0x62, 0xed, 0x3e];

// Helper functions for encoding/decoding ERC-20 calls
pub fn encode_transfer_call(to: Address, amount: U256) -> Bytes {
    let mut data = Vec::with_capacity(68);
    data.extend_from_slice(&TRANSFER_SELECTOR);
    
    // Encode address (padded to 32 bytes)
    let mut to_bytes = vec![0u8; 12];
    to_bytes.extend_from_slice(to.as_slice());
    data.extend_from_slice(&to_bytes);
    
    // Encode amount
    let amount_bytes = amount.to_be_bytes_vec();
    data.extend_from_slice(&amount_bytes);
    
    Bytes::from(data)
}

pub fn encode_balance_of_call(account: Address) -> Bytes {
    let mut data = Vec::with_capacity(36);
    data.extend_from_slice(&BALANCE_OF_SELECTOR);
    
    // Encode address (padded to 32 bytes)
    let mut account_bytes = vec![0u8; 12];
    account_bytes.extend_from_slice(account.as_slice());
    data.extend_from_slice(&account_bytes);
    
    Bytes::from(data)
}

pub fn encode_approve_call(spender: Address, amount: U256) -> Bytes {
    let mut data = Vec::with_capacity(68);
    data.extend_from_slice(&APPROVE_SELECTOR);
    
    // Encode address (padded to 32 bytes)
    let mut spender_bytes = vec![0u8; 12];
    spender_bytes.extend_from_slice(spender.as_slice());
    data.extend_from_slice(&spender_bytes);
    
    // Encode amount
    let amount_bytes = amount.to_be_bytes_vec();
    data.extend_from_slice(&amount_bytes);
    
    Bytes::from(data)
}

pub fn encode_allowance_call(owner: Address, spender: Address) -> Bytes {
    let mut data = Vec::with_capacity(68);
    data.extend_from_slice(&ALLOWANCE_SELECTOR);
    
    // Encode owner address (padded to 32 bytes)
    let mut owner_bytes = vec![0u8; 12];
    owner_bytes.extend_from_slice(owner.as_slice());
    data.extend_from_slice(&owner_bytes);
    
    // Encode spender address (padded to 32 bytes)
    let mut spender_bytes = vec![0u8; 12];
    spender_bytes.extend_from_slice(spender.as_slice());
    data.extend_from_slice(&spender_bytes);
    
    Bytes::from(data)
}

// Data structures for TRC-20 tokens (moved to end to avoid conflicts)
#[derive(Debug, Clone)]
pub struct Trc20TokenInfo {
    pub contract_address: Address,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: U256,
}

impl Trc20TokenInfo {
    pub fn new(
        contract_address: Address,
        name: String,
        symbol: String,
        decimals: u8,
        total_supply: U256,
    ) -> Self {
        Self {
            contract_address,
            name,
            symbol,
            decimals,
            total_supply,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Trc20BalanceInfo {
    pub contract_address: Address,
    pub address: Address,
    pub balance: U256,
}

impl Trc20BalanceInfo {
    pub fn new(contract_address: Address, address: Address, balance: U256) -> Self {
        Self {
            contract_address,
            address,
            balance,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Trc20AllowanceInfo {
    pub contract_address: Address,
    pub owner: Address,
    pub spender: Address,
    pub allowance: U256,
}

impl Trc20AllowanceInfo {
    pub fn new(contract_address: Address, owner: Address, spender: Address, allowance: U256) -> Self {
        Self {
            contract_address,
            owner,
            spender,
            allowance,
        }
    }
} 