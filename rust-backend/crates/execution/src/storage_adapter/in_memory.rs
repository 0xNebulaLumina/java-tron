//! In-memory implementation of EVM state store.
//!
//! Provides a HashMap-backed storage implementation for testing and local execution.
//! This implementation doesn't persist to disk and is suitable for unit tests.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use anyhow::Result;
use revm::primitives::{AccountInfo, Bytecode, Address, U256};
use super::traits::EvmStateStore;
use super::types::{FreezeRecord, AccountAext};

/// In-memory implementation of EVM state store for testing and local execution.
/// Provides a HashMap-backed storage that doesn't persist to disk.
#[derive(Debug)]
pub struct InMemoryEvmStateStore {
    accounts: HashMap<Address, AccountInfo>,
    codes: HashMap<Address, Bytecode>,
    storage: HashMap<(Address, U256), U256>,
    freeze_records: Arc<RwLock<HashMap<(Address, u8), FreezeRecord>>>,
    account_aext: Arc<RwLock<HashMap<Address, AccountAext>>>,
}

impl Clone for InMemoryEvmStateStore {
    fn clone(&self) -> Self {
        Self {
            accounts: self.accounts.clone(),
            codes: self.codes.clone(),
            storage: self.storage.clone(),
            freeze_records: self.freeze_records.clone(),
            account_aext: self.account_aext.clone(),
        }
    }
}

impl InMemoryEvmStateStore {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            codes: HashMap::new(),
            storage: HashMap::new(),
            freeze_records: Arc::new(RwLock::new(HashMap::new())),
            account_aext: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get freeze record for an address and resource
    pub fn get_freeze_record(&self, address: &Address, resource: u8) -> Result<Option<FreezeRecord>> {
        Ok(self.freeze_records.read().unwrap().get(&(*address, resource)).cloned())
    }

    /// Set freeze record for an address and resource
    pub fn set_freeze_record(&self, address: &Address, resource: u8, frozen_amount: u64, expiration_timestamp: i64) -> Result<()> {
        let record = FreezeRecord {
            frozen_amount,
            expiration_timestamp,
        };
        self.freeze_records.write().unwrap().insert((*address, resource), record);
        Ok(())
    }

    /// Get tron power for an address in SUN
    pub fn get_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64> {
        // Resource types as defined in Tron protocol
        const BANDWIDTH: u8 = 0;
        const ENERGY: u8 = 1;
        const TRON_POWER: u8 = 2;

        let mut total: u64 = 0;
        let mut bandwidth_amount: u64 = 0;
        let mut energy_amount: u64 = 0;
        let mut tron_power_amount: u64 = 0;

        // Sum frozen amounts across all three resource types
        for resource in [BANDWIDTH, ENERGY, TRON_POWER] {
            if let Some(record) = self.get_freeze_record(address, resource)? {
                let amount = record.frozen_amount;
                total = total.checked_add(amount)
                    .ok_or_else(|| anyhow::anyhow!(
                        "Tron power overflow when adding resource {} amount {} to total {}",
                        resource, amount, total
                    ))?;

                // Track per-resource amounts for logging
                match resource {
                    BANDWIDTH => bandwidth_amount = amount,
                    ENERGY => energy_amount = amount,
                    TRON_POWER => tron_power_amount = amount,
                    _ => {}
                }
            }
        }

        // Log the computation with all relevant details
        tracing::info!(
            address = ?address,
            new_model = new_model,
            bandwidth = bandwidth_amount,
            energy = energy_amount,
            tron_power_legacy = tron_power_amount,
            total = total,
            "Computed tron power from freeze ledger (in-memory)"
        );

        Ok(total)
    }

    /// Get account AEXT (resource tracking fields) for an address
    pub fn get_account_aext(&self, address: &Address) -> Result<Option<AccountAext>> {
        Ok(self.account_aext.read().unwrap().get(address).cloned())
    }

    /// Set account AEXT (resource tracking fields) for an address
    pub fn set_account_aext(&self, address: &Address, aext: AccountAext) -> Result<()> {
        self.account_aext.write().unwrap().insert(*address, aext);
        Ok(())
    }

    /// Get or initialize account AEXT with defaults
    pub fn get_or_init_account_aext(&self, address: &Address) -> Result<AccountAext> {
        if let Some(aext) = self.get_account_aext(address)? {
            Ok(aext)
        } else {
            let aext = AccountAext::with_defaults();
            self.set_account_aext(address, aext.clone())?;
            Ok(aext)
        }
    }

    // Phase C: Method alias shims (preferred names going forward)
    // See planning/storage_adapter_namings.planning.md for rationale

    /// **Preferred name**: Store freeze record (upsert semantics, aligns with `put_witness`).
    /// Delegates to `set_freeze_record`. Use this method in new code.
    pub fn put_freeze_record(&self, address: &Address, resource: u8, frozen_amount: u64, expiration_timestamp: i64) -> Result<()> {
        self.set_freeze_record(address, resource, frozen_amount, expiration_timestamp)
    }

    /// **Preferred name**: Compute tron power from ledger (reflects computation rather than "get").
    /// Delegates to `get_tron_power_in_sun`. Use this method in new code.
    pub fn compute_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64> {
        self.get_tron_power_in_sun(address, new_model)
    }
}

impl EvmStateStore for InMemoryEvmStateStore {
    fn get_account(&self, address: &Address) -> Result<Option<AccountInfo>> {
        Ok(self.accounts.get(address).cloned())
    }

    fn get_code(&self, address: &Address) -> Result<Option<Bytecode>> {
        Ok(self.codes.get(address).cloned())
    }

    fn get_storage(&self, address: &Address, key: &U256) -> Result<U256> {
        Ok(self.storage.get(&(*address, *key)).copied().unwrap_or(U256::ZERO))
    }

    fn set_account(&mut self, address: Address, account: AccountInfo) -> Result<()> {
        self.accounts.insert(address, account);
        Ok(())
    }

    fn set_code(&mut self, address: Address, code: Bytecode) -> Result<()> {
        self.codes.insert(address, code);
        Ok(())
    }

    fn set_storage(&mut self, address: Address, key: U256, value: U256) -> Result<()> {
        if value == U256::ZERO {
            self.storage.remove(&(address, key));
        } else {
            self.storage.insert((address, key), value);
        }
        Ok(())
    }

    fn remove_account(&mut self, address: &Address) -> Result<()> {
        self.accounts.remove(address);
        self.codes.remove(address);

        // Remove all storage for this address
        self.storage.retain(|(addr, _), _| addr != address);

        Ok(())
    }
}
