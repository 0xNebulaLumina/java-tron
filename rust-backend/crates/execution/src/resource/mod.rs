//! TRON Resource Management Module
//! 
//! This module implements TRON's bandwidth and fee semantics for non-VM transactions.
//! It handles:
//! - Free bandwidth allocation (24h rolling window)
//! - Staked/delegated bandwidth consumption
//! - TRX fee calculation and application
//! - State delta generation for account changes

use anyhow::Result;
use revm_primitives::{Address, U256};
use std::collections::HashMap;
use tracing::{debug, warn, info};

pub use config::ResourceConfig;
pub use store::{ResourceStateStore, ResourceUsageRecord, DynamicProperties};
pub use calculator::ResourceCalculator;
pub use applier::ResourceApplier;

mod config;
mod store;
mod calculator;
mod applier;

/// Resource manager that orchestrates bandwidth/fee calculations
pub struct ResourceManager<S> {
    config: ResourceConfig,
    store: ResourceStateStore<S>,
    calculator: ResourceCalculator,
    applier: ResourceApplier,
}

impl<S> ResourceManager<S> 
where
    S: crate::storage_adapter::StorageAdapter + 'static,
{
    pub fn new(storage: S, config: &crate::ExecutionConfig) -> Result<Self> {
        let resource_config = ResourceConfig::from_execution_config(config)?;
        let store = ResourceStateStore::new(storage)?;
        let calculator = ResourceCalculator::new();
        let applier = ResourceApplier::new(&resource_config);

        Ok(Self {
            config: resource_config,
            store,
            calculator,
            applier,
        })
    }

    /// Process bandwidth and fees for a non-VM TRX transfer
    /// Returns state changes to be applied by Java
    pub fn process_transfer(
        &mut self,
        from: Address,
        to: Address,
        value: U256,
        tx_bytes: u64,
        block_timestamp: u64,
    ) -> Result<Vec<crate::TronStateChange>> {
        debug!("Processing transfer: from={:?}, to={:?}, value={}, tx_bytes={}", from, to, value, tx_bytes);

        // Load dynamic properties
        let dynamic_props = self.store.load_dynamic_properties()?;
        debug!("Loaded dynamic properties: free_net_limit={}, bandwidth_price={}", 
               dynamic_props.free_net_limit, dynamic_props.bandwidth_price);

        // Load sender's resource usage and balance
        let mut sender_usage = self.store.load_resource_usage(&from)?;
        let sender_balance = self.store.load_account_balance(&from)?;
        debug!("Sender balance: {}, free_net_used: {}, latest_op_time: {}", 
               sender_balance, sender_usage.free_net_used, sender_usage.latest_op_time);

        // Calculate bandwidth requirements and available resources
        let bandwidth_needed = self.calculator.calculate_bandwidth_used(tx_bytes, &dynamic_props);
        let free_available = self.calculator.calculate_free_bandwidth_available(
            &sender_usage, &dynamic_props, block_timestamp
        );
        let staked_available = self.calculator.calculate_staked_bandwidth_available(
            &from, &mut self.store, &dynamic_props, block_timestamp
        )?;

        debug!("Bandwidth calculation: needed={}, free_available={}, staked_available={}", 
               bandwidth_needed, free_available, staked_available);

        // Determine resource consumption and TRX fee
        let (free_used, staked_used, fee_required) = self.calculator.calculate_resource_consumption(
            bandwidth_needed, free_available, staked_available, &dynamic_props
        );

        debug!("Resource consumption: free_used={}, staked_used={}, fee_required={}", 
               free_used, staked_used, fee_required);

        // Validate sender has sufficient funds
        let total_cost = value + fee_required;
        if sender_balance < total_cost {
            return Err(anyhow::anyhow!(
                "Insufficient balance: have {}, need {}", sender_balance, total_cost
            ));
        }

        // Update resource usage records
        self.calculator.update_resource_usage(
            &mut sender_usage, free_used, staked_used, block_timestamp
        );

        // Generate state changes
        let mut state_changes = Vec::new();

        // Apply sender changes (balance, resource usage)
        let sender_changes = self.applier.apply_sender_changes(
            from, sender_balance, total_cost, &sender_usage
        )?;
        state_changes.extend(sender_changes);

        // Apply recipient changes (balance, account creation if needed)
        let recipient_balance = self.store.load_account_balance(&to)?;
        let recipient_changes = self.applier.apply_recipient_changes(
            to, recipient_balance, value
        )?;
        state_changes.extend(recipient_changes);

        // Apply fee handling (burn or blackhole)
        if fee_required > U256::ZERO {
            let fee_changes = self.applier.apply_fee_changes(fee_required)?;
            state_changes.extend(fee_changes);
        }

        // Save updated resource usage
        self.store.save_resource_usage(&from, &sender_usage)?;

        info!("Transfer processed successfully: fee={}, free_used={}, staked_used={}", 
              fee_required, free_used, staked_used);

        Ok(state_changes)
    }

    /// Get current resource usage for an address (for diagnostics)
    pub fn get_resource_usage(&self, address: &Address) -> Result<ResourceUsageRecord> {
        self.store.load_resource_usage(address)
    }

    /// Get current dynamic properties (for diagnostics)
    pub fn get_dynamic_properties(&self) -> Result<DynamicProperties> {
        self.store.load_dynamic_properties()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_adapter::InMemoryStorageAdapter;
    use tron_backend_common::ExecutionConfig;

    fn create_test_manager() -> ResourceManager<InMemoryStorageAdapter> {
        let storage = InMemoryStorageAdapter::new();
        let config = ExecutionConfig::default();
        ResourceManager::new(storage, &config).unwrap()
    }

    #[test]
    fn test_resource_manager_creation() {
        let _manager = create_test_manager();
    }
}