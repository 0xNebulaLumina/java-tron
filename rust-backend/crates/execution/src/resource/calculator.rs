//! Resource calculation logic for TRON bandwidth and fee semantics

use anyhow::Result;
use revm_primitives::{Address, U256};
use std::cmp::{min, max};
use tracing::debug;

use super::store::{DynamicProperties, ResourceUsageRecord, ResourceStateStore, DelegatedResource};

/// Calculates bandwidth usage, available resources, and required fees
pub struct ResourceCalculator;

impl ResourceCalculator {
    pub fn new() -> Self {
        Self
    }

    /// Calculate bandwidth used by a transaction
    /// Based on Java's transaction size calculation including signatures
    pub fn calculate_bandwidth_used(&self, tx_bytes: u64, _dynamic_props: &DynamicProperties) -> u64 {
        // For now, use the raw transaction bytes
        // In a full implementation, this would include:
        // - Base transaction size
        // - Signature overhead
        // - Any TRC10/TRC20 specific adjustments
        debug!("Calculated bandwidth used: {} bytes", tx_bytes);
        tx_bytes
    }

    /// Calculate free bandwidth available to an account
    /// Implements 24-hour rolling window logic matching Java
    pub fn calculate_free_bandwidth_available(
        &self,
        usage: &ResourceUsageRecord,
        dynamic_props: &DynamicProperties,
        current_timestamp: u64,
    ) -> u64 {
        // Check if the window has expired
        let window_start = current_timestamp.saturating_sub(dynamic_props.free_net_window_size);
        
        if usage.latest_consume_free_time < window_start {
            // Window expired, full limit available
            debug!("Free bandwidth window expired, full limit available: {}", dynamic_props.free_net_limit);
            return dynamic_props.free_net_limit;
        }

        // Window still active, calculate remaining quota
        let remaining = dynamic_props.free_net_limit.saturating_sub(usage.free_net_used);
        debug!("Free bandwidth remaining in current window: {}", remaining);
        remaining
    }

    /// Calculate staked bandwidth available to an account
    /// Includes own staking plus incoming delegations minus outgoing delegations,
    /// subtracting current-window net usage.
    pub fn calculate_staked_bandwidth_available<S>(
        &self,
        address: &Address,
        store: &mut ResourceStateStore<S>,
        usage: &ResourceUsageRecord,
        dynamic_props: &DynamicProperties,
        current_timestamp: u64,
    ) -> Result<u64>
    where
        S: crate::storage_adapter::StorageAdapter + 'static,
    {
        // Load own staked resources
        let own_delegated = store.load_delegated_resources(address)?;
        
        // Load incoming delegations (resources delegated TO this address)
        let incoming_delegations = store.load_incoming_delegations(address)?;
        
        // Load outgoing delegations (resources delegated BY this address)
        let outgoing_delegations = store.load_outgoing_delegations(address)?;

        // Calculate total staked bandwidth
        let mut total_staked_trx = own_delegated.frozen_balance_for_bandwidth;

        // Add incoming delegations
        for (_delegator, delegation) in incoming_delegations.iter() {
            if delegation.expire_time_for_bandwidth > current_timestamp {
                total_staked_trx += delegation.frozen_balance_for_bandwidth;
            }
        }

        // Subtract outgoing delegations
        for (_delegatee, delegation) in outgoing_delegations.iter() {
            if delegation.expire_time_for_bandwidth > current_timestamp {
                total_staked_trx = total_staked_trx.saturating_sub(delegation.frozen_balance_for_bandwidth);
            }
        }

        // Convert TRX to bandwidth (simplified mapping)
        let bandwidth_from_stake = if total_staked_trx > U256::ZERO {
            // Example: 1 TRX = 1000 bandwidth units (this needs to match Java's calculation)
            let bandwidth_per_trx = U256::from(1000);
            let total_bandwidth = total_staked_trx * bandwidth_per_trx;
            
            // Convert to u64, capping at u64::MAX
            if total_bandwidth > U256::from(u64::MAX) {
                u64::MAX
            } else {
                total_bandwidth.as_limbs()[0]
            }
        } else {
            0
        };

        // Subtract current-window usage (approximation of Java's moving window)
        let window_start = current_timestamp.saturating_sub(dynamic_props.free_net_window_size);
        let used_in_window = if usage.latest_consume_time < window_start { 0 } else { usage.net_used };
        let available = bandwidth_from_stake.saturating_sub(used_in_window);

        debug!(
            "Staked bandwidth available: {} (from {} TRX staked, used_in_window={})",
            available, total_staked_trx, used_in_window
        );

        Ok(available)
    }

    /// Calculate resource consumption and required TRX fee
    /// Returns (free_used, staked_used, fee_in_sun)
    pub fn calculate_resource_consumption(
        &self,
        bandwidth_needed: u64,
        free_available: u64,
        staked_available: u64,
        dynamic_props: &DynamicProperties,
    ) -> (u64, u64, U256) {
        let mut remaining_needed = bandwidth_needed;
        
        // First, consume staked bandwidth (Java order)
        let staked_used = min(remaining_needed, staked_available);
        remaining_needed = remaining_needed.saturating_sub(staked_used);

        // Then, consume free bandwidth
        let free_used = min(remaining_needed, free_available);
        remaining_needed = remaining_needed.saturating_sub(free_used);
        
        // Finally, calculate TRX fee for remaining bandwidth
        let fee_required = if remaining_needed > 0 {
            U256::from(remaining_needed) * dynamic_props.bandwidth_price
        } else {
            U256::ZERO
        };

        debug!("Resource consumption calculated: free={}, staked={}, fee={} SUN", 
               free_used, staked_used, fee_required);

        (free_used, staked_used, fee_required)
    }

    /// Update resource usage records after consumption
    pub fn update_resource_usage(
        &self,
        usage: &mut ResourceUsageRecord,
        free_used: u64,
        staked_used: u64,
        current_timestamp: u64,
    ) {
        // Update free bandwidth usage (cumulative within window)
        if free_used > 0 {
            usage.free_net_used = usage.free_net_used.saturating_add(free_used);
            usage.latest_consume_free_time = current_timestamp;
        }

        // Update net bandwidth usage
        if staked_used > 0 {
            usage.net_used = usage.net_used.saturating_add(staked_used);
            usage.latest_consume_time = current_timestamp;
        }

        debug!(
            "Updated resource usage: free_net_used={}, net_used={}, latest_consume_free_time={}, latest_consume_time={}",
            usage.free_net_used, usage.net_used, usage.latest_consume_free_time, usage.latest_consume_time
        );
    }

    /// Check if a window has expired and should be reset
    pub fn should_reset_window(
        &self,
        usage: &ResourceUsageRecord,
        dynamic_props: &DynamicProperties,
        current_timestamp: u64,
    ) -> bool {
        let window_start = current_timestamp.saturating_sub(dynamic_props.free_net_window_size);
        usage.latest_consume_free_time < window_start && usage.latest_consume_time < window_start
    }

    /// Reset resource usage for a new window
    pub fn reset_usage_for_new_window(
        &self,
        usage: &mut ResourceUsageRecord,
        current_timestamp: u64,
    ) {
        debug!("Resetting resource usage for new window at timestamp {}", current_timestamp);
        
        usage.free_net_used = 0;
        usage.net_used = 0;
        usage.energy_used = 0;
        usage.latest_consume_free_time = current_timestamp;
        usage.latest_consume_time = current_timestamp;
    }
}

impl Default for ResourceCalculator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_adapter::InMemoryStorageAdapter;

    fn create_test_calculator() -> ResourceCalculator {
        ResourceCalculator::new()
    }

    fn create_test_dynamic_props() -> DynamicProperties {
        DynamicProperties { 
            free_net_limit: 5000,
            free_net_window_size: 86400000, // 24 hours
            bandwidth_price: U256::from(1000), // 1000 SUN per byte
            total_energy_limit: 100_000_000,
            ..DynamicProperties::default()
        }
    }

    #[test]
    fn test_bandwidth_calculation() {
        let calculator = create_test_calculator();
        let dynamic_props = create_test_dynamic_props();
        
        let bandwidth = calculator.calculate_bandwidth_used(250, &dynamic_props);
        assert_eq!(bandwidth, 250);
    }

    #[test]
    fn test_free_bandwidth_available_fresh_account() {
        let calculator = create_test_calculator();
        let dynamic_props = create_test_dynamic_props();
        let usage = ResourceUsageRecord::default();
        let current_timestamp = 1000000000;
        
        let available = calculator.calculate_free_bandwidth_available(&usage, &dynamic_props, current_timestamp);
        assert_eq!(available, 5000); // Full limit for fresh account
    }

    #[test]
    fn test_free_bandwidth_available_with_usage() {
        let calculator = create_test_calculator();
        let dynamic_props = create_test_dynamic_props();
        let usage = ResourceUsageRecord {
            free_net_used: 2000,
            latest_consume_free_time: 999000000, // Recent timestamp
            net_used: 0,
            latest_consume_time: 0,
            energy_used: 0,
        };
        let current_timestamp = 1000000000;
        
        let available = calculator.calculate_free_bandwidth_available(&usage, &dynamic_props, current_timestamp);
        assert_eq!(available, 3000); // 5000 - 2000 used
    }

    #[test]
    fn test_free_bandwidth_window_expired() {
        let calculator = create_test_calculator();
        let dynamic_props = create_test_dynamic_props();
        let usage = ResourceUsageRecord {
            free_net_used: 5000,
            latest_consume_free_time: 1000, // Very old timestamp
            net_used: 0,
            latest_consume_time: 0,
            energy_used: 0,
        };
        let current_timestamp = 1000000000;
        
        let available = calculator.calculate_free_bandwidth_available(&usage, &dynamic_props, current_timestamp);
        assert_eq!(available, 5000); // Full limit since window expired
    }

    #[test]
    fn test_resource_consumption_free_only() {
        let calculator = create_test_calculator();
        let dynamic_props = create_test_dynamic_props();
        
        let (free_used, staked_used, fee) = calculator.calculate_resource_consumption(
            1000, // bandwidth needed
            5000, // free available
            0,    // staked available
            &dynamic_props,
        );
        
        assert_eq!(free_used, 1000);
        assert_eq!(staked_used, 0);
        assert_eq!(fee, U256::ZERO);
    }

    #[test]
    fn test_resource_consumption_with_fee() {
        let calculator = create_test_calculator();
        let dynamic_props = create_test_dynamic_props();
        
        let (free_used, staked_used, fee) = calculator.calculate_resource_consumption(
            7000, // bandwidth needed
            2000, // free available
            3000, // staked available
            &dynamic_props,
        );
        
        assert_eq!(free_used, 2000);
        assert_eq!(staked_used, 3000);
        assert_eq!(fee, U256::from(2000 * 1000)); // 2000 bytes * 1000 SUN/byte
    }

    #[test]
    fn test_usage_update() {
        let calculator = create_test_calculator();
        let mut usage = ResourceUsageRecord::default();
        let current_timestamp = 1000000000;
        
        calculator.update_resource_usage(&mut usage, 1000, 500, current_timestamp);
        
        assert_eq!(usage.free_net_used, 1000);
        assert_eq!(usage.net_used, 500);
        assert_eq!(usage.latest_consume_free_time, current_timestamp);
        assert_eq!(usage.latest_consume_time, current_timestamp);
    }

    #[test]
    fn test_window_reset_logic() {
        let calculator = create_test_calculator();
        let dynamic_props = create_test_dynamic_props();
        let current_timestamp = 1000000000;
        
        // Usage within window
        let recent_usage = ResourceUsageRecord { 
            latest_consume_free_time: current_timestamp - 1000, // 1 second ago
            latest_consume_time: current_timestamp - 1000,
            ..ResourceUsageRecord::default() 
        };
        assert!(!calculator.should_reset_window(&recent_usage, &dynamic_props, current_timestamp));
        
        // Usage outside window
        let old_usage = ResourceUsageRecord { 
            latest_consume_free_time: current_timestamp - 90000000, // More than 24 hours ago
            latest_consume_time: current_timestamp - 90000000,
            ..ResourceUsageRecord::default() 
        };
        assert!(calculator.should_reset_window(&old_usage, &dynamic_props, current_timestamp));
    }
}
