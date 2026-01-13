//! Core traits for EVM state storage.
//!
//! This module defines the `EvmStateStore` trait, which provides the essential
//! interface for account, code, and storage operations needed by the EVM execution engine.

use anyhow::Result;
use crate::protocol::AssetIssueContractData;
use revm::primitives::{AccountInfo, Bytecode, Address, SpecId, U256};

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

    /// Best-effort TVM/EVM fork selection for gas accounting parity.
    ///
    /// Engine-backed stores can read TRON dynamic properties (e.g. `ALLOW_TVM_CONSTANTINOPLE`)
    /// and return the matching REVM `SpecId` for the current block.
    ///
    /// Default: `None` (caller should fall back to config defaults).
    fn tvm_spec_id(&self) -> Result<Option<SpecId>> {
        Ok(None)
    }

    /// Best-effort ENERGY_FEE lookup (SUN per energy unit).
    ///
    /// Engine-backed stores can read TRON dynamic properties and return the effective energy fee
    /// used to convert a transaction's `fee_limit` (SUN) into an EVM gas limit (energy units).
    ///
    /// Default: `None` (caller should fall back to the raw value they were given).
    fn energy_fee_rate(&self) -> Result<Option<u64>> {
        Ok(None)
    }

    /// Best-effort TRON address prefix (0x41 mainnet, 0xa0 testnets).
    ///
    /// Engine-backed stores can detect the prefix from database keys for fixture parity.
    /// Default: 0x41.
    fn tron_address_prefix(&self) -> Result<u8> {
        Ok(0x41)
    }

    /// Best-effort TRON dynamic property lookup (big-endian i64 stored under string keys).
    ///
    /// Default: `None` (caller should fall back to safe defaults).
    fn tron_dynamic_property_i64(&self, _key: &[u8]) -> Result<Option<i64>> {
        Ok(None)
    }

    /// Best-effort TRC-10 asset issue lookup.
    ///
    /// Default: `None` (caller should treat as missing/unavailable).
    fn tron_get_asset_issue(
        &self,
        _key: &[u8],
        _allow_same_token_name: i64,
    ) -> Result<Option<AssetIssueContractData>> {
        Ok(None)
    }

    /// Best-effort TRC-10 asset balance lookup (V2 format; allowSameTokenName=1).
    ///
    /// Default: 0.
    fn tron_get_asset_balance_v2(&self, _address: &Address, _token_id: &[u8]) -> Result<i64> {
        Ok(0)
    }
}
