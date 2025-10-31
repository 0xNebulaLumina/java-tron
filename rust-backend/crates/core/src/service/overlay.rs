//! Block Execution Overlay
//!
//! This module implements a per-block, in-memory state overlay that ensures
//! AccountChange.old_account reflects the true pre-transaction state, including:
//! - All prior writes within the same block
//! - Shadow TRC-10 ledger effects (asset issue/participate fees)
//!
//! The overlay sits between the execution logic and the storage adapter, feeding
//! account reads from the current block's view before falling back to DB.

use std::collections::HashMap;
use revm_primitives::{AccountInfo, Address, U256};
use tracing::{debug, warn};

/// Block identifier used to track overlay lifecycle
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockKey {
    pub block_number: u64,
    pub block_timestamp: u64,
    pub witness: Option<Address>,
}

impl BlockKey {
    pub fn new(block_number: u64, block_timestamp: u64, witness: Option<Address>) -> Self {
        Self {
            block_number,
            block_timestamp,
            witness,
        }
    }
}

/// Per-block state overlay that caches account state across transaction executions
/// within the same block, including shadow TRC-10 effects.
#[derive(Debug, Clone)]
pub struct BlockExecutionOverlay {
    /// Address -> AccountInfo mapping for the current block
    accounts: HashMap<Address, AccountInfo>,
    /// Block identifier for lifecycle management
    block_key: BlockKey,
}

impl BlockExecutionOverlay {
    /// Create a new overlay for the given block
    pub fn new(block_key: BlockKey) -> Self {
        debug!("Creating new BlockExecutionOverlay for block {}, timestamp {}",
               block_key.block_number, block_key.block_timestamp);
        Self {
            accounts: HashMap::new(),
            block_key,
        }
    }

    /// Get the block key for this overlay
    pub fn block_key(&self) -> &BlockKey {
        &self.block_key
    }

    /// Get account from overlay (returns clone)
    pub fn get_account(&self, address: &Address) -> Option<AccountInfo> {
        let result = self.accounts.get(address).cloned();
        if result.is_some() {
            debug!("Overlay HIT for address {:?}", address);
        } else {
            debug!("Overlay MISS for address {:?}", address);
        }
        result
    }

    /// Put account into overlay
    pub fn put_account(&mut self, address: Address, account: AccountInfo) {
        debug!("Overlay PUT: address={:?}, balance={}, nonce={}",
               address, account.balance, account.nonce);
        self.accounts.insert(address, account);
    }

    /// Apply a delta (positive or negative) to an account's balance
    /// Creates a default account if the address doesn't exist in the overlay
    pub fn apply_delta(&mut self, address: Address, delta_sun: i128) -> Result<(), String> {
        let current = self.accounts.get(&address).cloned().unwrap_or_else(|| {
            debug!("Creating default account in overlay for address {:?}", address);
            AccountInfo {
                balance: U256::ZERO,
                nonce: 0,
                code_hash: revm_primitives::B256::ZERO,
                code: None,
            }
        });

        // Perform safe balance arithmetic
        let current_balance = current.balance;
        let new_balance = if delta_sun >= 0 {
            // Positive delta: add
            let delta_u256 = U256::from(delta_sun as u128);
            current_balance.checked_add(delta_u256)
                .ok_or_else(|| format!("Balance overflow: {} + {}", current_balance, delta_u256))?
        } else {
            // Negative delta: subtract
            let delta_abs = (-delta_sun) as u128;
            let delta_u256 = U256::from(delta_abs);
            current_balance.checked_sub(delta_u256)
                .ok_or_else(|| {
                    warn!("Balance underflow attempt: {} - {} for address {:?}",
                          current_balance, delta_u256, address);
                    format!("Balance underflow: {} - {}", current_balance, delta_u256)
                })?
        };

        debug!("Overlay DELTA: address={:?}, old_balance={}, delta={}, new_balance={}",
               address, current_balance, delta_sun, new_balance);

        let updated = AccountInfo {
            balance: new_balance,
            nonce: current.nonce,
            code_hash: current.code_hash,
            code: current.code.clone(),
        };

        self.accounts.insert(address, updated);
        Ok(())
    }

    /// Clear all cached accounts (called on block boundary change)
    pub fn clear(&mut self) {
        debug!("Clearing overlay for block {}", self.block_key.block_number);
        self.accounts.clear();
    }

    /// Get the number of cached accounts
    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    /// Check if overlay is empty
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_get_put() {
        let block_key = BlockKey::new(100, 1234567890, None);
        let mut overlay = BlockExecutionOverlay::new(block_key);

        let addr = Address::from([1u8; 20]);
        let account = AccountInfo {
            balance: U256::from(1000u64),
            nonce: 5,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        // Put and get
        overlay.put_account(addr, account.clone());
        let retrieved = overlay.get_account(&addr).expect("Account should exist");
        assert_eq!(retrieved.balance, account.balance);
        assert_eq!(retrieved.nonce, account.nonce);
    }

    #[test]
    fn test_overlay_apply_delta_positive() {
        let block_key = BlockKey::new(100, 1234567890, None);
        let mut overlay = BlockExecutionOverlay::new(block_key);

        let addr = Address::from([1u8; 20]);
        let initial_account = AccountInfo {
            balance: U256::from(1000u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        overlay.put_account(addr, initial_account);

        // Apply positive delta
        overlay.apply_delta(addr, 500).expect("Delta should succeed");

        let updated = overlay.get_account(&addr).expect("Account should exist");
        assert_eq!(updated.balance, U256::from(1500u64));
    }

    #[test]
    fn test_overlay_apply_delta_negative() {
        let block_key = BlockKey::new(100, 1234567890, None);
        let mut overlay = BlockExecutionOverlay::new(block_key);

        let addr = Address::from([1u8; 20]);
        let initial_account = AccountInfo {
            balance: U256::from(1000u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        overlay.put_account(addr, initial_account);

        // Apply negative delta
        overlay.apply_delta(addr, -300).expect("Delta should succeed");

        let updated = overlay.get_account(&addr).expect("Account should exist");
        assert_eq!(updated.balance, U256::from(700u64));
    }

    #[test]
    fn test_overlay_apply_delta_underflow() {
        let block_key = BlockKey::new(100, 1234567890, None);
        let mut overlay = BlockExecutionOverlay::new(block_key);

        let addr = Address::from([1u8; 20]);
        let initial_account = AccountInfo {
            balance: U256::from(100u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        overlay.put_account(addr, initial_account);

        // Try to subtract more than available
        let result = overlay.apply_delta(addr, -200);
        assert!(result.is_err(), "Should fail with underflow");
    }

    #[test]
    fn test_overlay_apply_delta_creates_account() {
        let block_key = BlockKey::new(100, 1234567890, None);
        let mut overlay = BlockExecutionOverlay::new(block_key);

        let addr = Address::from([1u8; 20]);

        // Apply delta to non-existent account (should create default)
        overlay.apply_delta(addr, 1000).expect("Delta should succeed");

        let account = overlay.get_account(&addr).expect("Account should be created");
        assert_eq!(account.balance, U256::from(1000u64));
        assert_eq!(account.nonce, 0);
    }

    #[test]
    fn test_overlay_clear() {
        let block_key = BlockKey::new(100, 1234567890, None);
        let mut overlay = BlockExecutionOverlay::new(block_key);

        let addr = Address::from([1u8; 20]);
        let account = AccountInfo {
            balance: U256::from(1000u64),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };

        overlay.put_account(addr, account);
        assert!(!overlay.is_empty());

        overlay.clear();
        assert!(overlay.is_empty());
        assert!(overlay.get_account(&addr).is_none());
    }
}
