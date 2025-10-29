//! Core traits for EVM state storage.
//!
//! This module defines the `EvmStateStore` trait, which provides the essential
//! interface for account, code, and storage operations needed by the EVM execution engine.

use anyhow::Result;
use revm::primitives::{AccountInfo, Bytecode, Address, U256};

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
