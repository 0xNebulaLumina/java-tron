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
        debug!(
            "Sender balance: {}, free_net_used: {}, latest_consume_free_time: {}, net_used: {}, latest_consume_time: {}",
            sender_balance,
            sender_usage.free_net_used,
            sender_usage.latest_consume_free_time,
            sender_usage.net_used,
            sender_usage.latest_consume_time
        );

        // Calculate bandwidth requirements
        let bandwidth_needed = self.calculator.calculate_bandwidth_used(tx_bytes, &dynamic_props);

        // Check if recipient account exists (Java semantics)
        let recipient_exists = self.store.account_exists(&to)?;

        // Special handling: new account creation by transfer
        if !recipient_exists {
            debug!("Recipient does not exist; applying create-new-account semantics");

            // Compute staked (net) available and the required net cost with multiplier
            let staked_available = self.calculator.calculate_staked_bandwidth_available(
                &from, &mut self.store, &sender_usage, &dynamic_props, block_timestamp
            )?;

            let net_cost_u128 = (bandwidth_needed as u128)
                .saturating_mul(dynamic_props.create_new_account_bandwidth_rate as u128);
            let net_cost = if net_cost_u128 > (u64::MAX as u128) { u64::MAX } else { net_cost_u128 as u64 };

            if staked_available >= net_cost {
                // Consume staked bandwidth only; no TRX fee
                self.calculator.update_resource_usage(&mut sender_usage, 0, net_cost, block_timestamp);

                // Apply balance changes (no fee)
                if sender_balance < value {
                    return Err(anyhow::anyhow!(
                        "Insufficient balance for value transfer: have {}, need {}", sender_balance, value
                    ));
                }

                let mut state_changes = Vec::new();
                let sender_changes = self.applier.apply_sender_changes(from, sender_balance, value, &sender_usage)?;
                state_changes.extend(sender_changes);

                let recipient_balance = self.store.load_account_balance(&to)?;
                let recipient_changes = self.applier.apply_recipient_changes(to, recipient_balance, value)?;
                state_changes.extend(recipient_changes);

                // Save updated resource usage
                self.store.save_resource_usage(&from, &sender_usage)?;

                info!(
                    "Create-account transfer processed using staked NET: net_cost={}, staked_available={}",
                    net_cost, staked_available
                );

                return Ok(state_changes);
            } else {
                // Fallback: fixed system contract fee for creating new account
                let fee_required = U256::from(dynamic_props.create_new_account_fee_in_system_contract);
                let total_cost = value + fee_required;
                if sender_balance < total_cost {
                    return Err(anyhow::anyhow!(
                        "Insufficient balance for create-account fee: have {}, need {}", sender_balance, total_cost
                    ));
                }

                let mut state_changes = Vec::new();
                let sender_changes = self.applier.apply_sender_changes(from, sender_balance, total_cost, &sender_usage)?;
                state_changes.extend(sender_changes);
                let recipient_balance = self.store.load_account_balance(&to)?;
                let recipient_changes = self.applier.apply_recipient_changes(to, recipient_balance, value)?;
                state_changes.extend(recipient_changes);

                // Apply fee handling according to dynamic property
                let effective_mode = if dynamic_props.allow_blackhole_optimization { "burn" } else { "blackhole" };
                let fee_changes = self.applier.apply_fee_changes_with_mode(fee_required, effective_mode)?;
                state_changes.extend(fee_changes);

                // No resource usage updates for this path (Java burns TRX instead)

                info!(
                    "Create-account transfer processed with system fee: fee={}, mode={}",
                    fee_required, if dynamic_props.allow_blackhole_optimization { "burn" } else { "blackhole" }
                );

                return Ok(state_changes);
            }
        }

        // Recipient exists: proceed with normal resource flow
        let free_available_account = self.calculator.calculate_free_bandwidth_available(
            &sender_usage, &dynamic_props, block_timestamp
        );
        let staked_available = self.calculator.calculate_staked_bandwidth_available(
            &from, &mut self.store, &sender_usage, &dynamic_props, block_timestamp
        )?;

        // Compute public free bandwidth remaining (24h window simplified)
        let public_remaining = if dynamic_props.public_net_limit == 0 {
            u64::MAX // no global cap configured
        } else {
            let window_start = block_timestamp.saturating_sub(dynamic_props.free_net_window_size);
            let refreshed_usage = if dynamic_props.public_net_time < window_start {
                0
            } else {
                dynamic_props.public_net_usage
            };
            dynamic_props.public_net_limit.saturating_sub(refreshed_usage)
        };

        // Effective free bandwidth is constrained by both account and public caps
        let free_available = std::cmp::min(free_available_account, public_remaining);

        debug!("Bandwidth calculation: needed={}, free_available={}, staked_available={}", 
               bandwidth_needed, free_available, staked_available);

        // Determine resource consumption and TRX fee (staked → free → fee)
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

        // Update resource usage records (account-level)
        self.calculator.update_resource_usage(
            &mut sender_usage, free_used, staked_used, block_timestamp
        );

        // Update public free bandwidth usage/time if used
        if free_used > 0 && dynamic_props.public_net_limit > 0 {
            // Refresh current usage per our simplified window model
            let window_start = block_timestamp.saturating_sub(dynamic_props.free_net_window_size);
            let current_public_usage = if dynamic_props.public_net_time < window_start {
                0
            } else {
                dynamic_props.public_net_usage
            };
            let new_public_usage = current_public_usage.saturating_add(free_used);
            self.store.save_public_net_usage_time(new_public_usage, block_timestamp)?;
        }

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

        // Apply fee handling (burn or blackhole). Follow Java's supportBlackHoleOptimization:
        // - true  => burn (no account delta)
        // - false => credit blackhole address
        if fee_required > U256::ZERO {
            let effective_mode = if dynamic_props.allow_blackhole_optimization { "burn" } else { "blackhole" };
            let fee_changes = self.applier.apply_fee_changes_with_mode(fee_required, effective_mode)?;
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
