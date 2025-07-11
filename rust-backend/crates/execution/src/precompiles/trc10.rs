use revm::primitives::{Bytes, PrecompileResult, PrecompileOutput, PrecompileErrors, PrecompileError, U256, Address};
use tracing::debug;

use super::TronPrecompile;

// TRC-10 precompile implementations
pub struct Trc10Transfer;
pub struct Trc10Balance;
pub struct Trc10Approve;
pub struct Trc10Allowance;

impl TronPrecompile for Trc10Transfer {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        const GAS_COST: u64 = 1000;
        
        if gas_limit < GAS_COST {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }
        
        if input.len() != 96 {
            return Err(PrecompileErrors::Fatal { 
                msg: "Invalid input length for TRC10 transfer".to_string() 
            });
        }
        
        // Parse input: token_id (32 bytes) + from (20 bytes) + to (20 bytes) + amount (32 bytes)
        let token_id = U256::from_be_slice(&input[0..32]);
        let from = Address::from_slice(&input[32..52]);
        let to = Address::from_slice(&input[52..72]);
        let amount = U256::from_be_slice(&input[72..104]);
        
        debug!("TRC10 Transfer: token_id={:?}, from={:?}, to={:?}, amount={:?}", 
               token_id, from, to, amount);
        
        // In a real implementation, this would interact with the storage to:
        // 1. Check balance of 'from' address
        // 2. Transfer tokens from 'from' to 'to'
        // 3. Update balances
        
        // For now, return success with empty output
        Ok(PrecompileOutput::new(GAS_COST, Bytes::new()))
    }
}

impl TronPrecompile for Trc10Balance {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        const GAS_COST: u64 = 500;
        
        if gas_limit < GAS_COST {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }
        
        if input.len() != 52 {
            return Err(PrecompileErrors::Fatal { 
                msg: "Invalid input length for TRC10 balance".to_string() 
            });
        }
        
        // Parse input: token_id (32 bytes) + address (20 bytes)
        let token_id = U256::from_be_slice(&input[0..32]);
        let address = Address::from_slice(&input[32..52]);
        
        debug!("TRC10 Balance: token_id={:?}, address={:?}", token_id, address);
        
        // In a real implementation, this would query the storage for the balance
        // For now, return a dummy balance
        let balance = U256::from(1000u64);
        let balance_bytes = balance.to_be_bytes_vec();
        
        Ok(PrecompileOutput::new(GAS_COST, Bytes::from(balance_bytes)))
    }
}

impl TronPrecompile for Trc10Approve {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        const GAS_COST: u64 = 1000;
        
        if gas_limit < GAS_COST {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }
        
        if input.len() != 84 {
            return Err(PrecompileErrors::Fatal { 
                msg: "Invalid input length for TRC10 approve".to_string() 
            });
        }
        
        // Parse input: token_id (32 bytes) + owner (20 bytes) + spender (20 bytes) + amount (32 bytes)
        let token_id = U256::from_be_slice(&input[0..32]);
        let owner = Address::from_slice(&input[32..52]);
        let spender = Address::from_slice(&input[52..72]);
        let amount = U256::from_be_slice(&input[72..104]);
        
        debug!("TRC10 Approve: token_id={:?}, owner={:?}, spender={:?}, amount={:?}", 
               token_id, owner, spender, amount);
        
        // In a real implementation, this would update the allowance in storage
        
        // Return success (1 for success)
        let mut result = vec![0u8; 31];
        result.push(1u8);
        
        Ok(PrecompileOutput::new(GAS_COST, Bytes::from(result)))
    }
}

impl TronPrecompile for Trc10Allowance {
    fn execute(&self, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        const GAS_COST: u64 = 500;
        
        if gas_limit < GAS_COST {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }
        
        if input.len() != 72 {
            return Err(PrecompileErrors::Fatal { 
                msg: "Invalid input length for TRC10 allowance".to_string() 
            });
        }
        
        // Parse input: token_id (32 bytes) + owner (20 bytes) + spender (20 bytes)
        let token_id = U256::from_be_slice(&input[0..32]);
        let owner = Address::from_slice(&input[32..52]);
        let spender = Address::from_slice(&input[52..72]);
        
        debug!("TRC10 Allowance: token_id={:?}, owner={:?}, spender={:?}", 
               token_id, owner, spender);
        
        // In a real implementation, this would query the allowance from storage
        // For now, return a dummy allowance
        let allowance = U256::from(500u64);
        let allowance_bytes = allowance.to_be_bytes_vec();
        
        Ok(PrecompileOutput::new(GAS_COST, Bytes::from(allowance_bytes)))
    }
}

// Data structures for TRC-10 tokens (moved to end to avoid conflicts)
#[derive(Debug, Clone)]
pub struct Trc10TokenInfo {
    pub id: U256,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: U256,
    pub owner: Address,
}

impl Trc10TokenInfo {
    pub fn new(
        id: U256,
        name: String,
        symbol: String,
        decimals: u8,
        total_supply: U256,
        owner: Address,
    ) -> Self {
        Self {
            id,
            name,
            symbol,
            decimals,
            total_supply,
            owner,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Trc10BalanceInfo {
    pub token_id: U256,
    pub address: Address,
    pub balance: U256,
}

impl Trc10BalanceInfo {
    pub fn new(token_id: U256, address: Address, balance: U256) -> Self {
        Self {
            token_id,
            address,
            balance,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Trc10AllowanceInfo {
    pub token_id: U256,
    pub owner: Address,
    pub spender: Address,
    pub allowance: U256,
}

impl Trc10AllowanceInfo {
    pub fn new(token_id: U256, owner: Address, spender: Address, allowance: U256) -> Self {
        Self {
            token_id,
            owner,
            spender,
            allowance,
        }
    }
} 