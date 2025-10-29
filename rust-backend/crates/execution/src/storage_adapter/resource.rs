//! Resource tracking for bandwidth and energy accounting.
//!
//! This module provides windowed resource usage tracking matching TRON's
//! ResourceProcessor logic, including bandwidth path selection.

use anyhow::Result;
use revm::primitives::Address;
use super::types::AccountAext;

pub struct ResourceTracker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandwidthPath {
    AccountNet,  // Used account frozen bandwidth
    FreeNet,     // Used free public bandwidth
    Fee,         // Fall back to fee deduction
}

impl ResourceTracker {
    /// Increase usage with windowed recovery (Java ResourceProcessor.increase parity)
    /// Formula: newUsage = increase(lastUsage, usage, lastTime, now, windowSize)
    ///        = max(0, lastUsage - (now - lastTime) / windowSize * lastUsage) + usage
    /// Simplified: recovered = lastUsage * (now - lastTime) / windowSize
    ///            newUsage = max(0, lastUsage - recovered) + usage
    pub fn increase(
        last_usage: i64,
        usage: i64,
        last_time: i64,
        now: i64,
        window_size: i64,
    ) -> i64 {
        if window_size == 0 {
            return usage;
        }

        let time_delta = now.saturating_sub(last_time);
        if time_delta <= 0 {
            // No time passed, just add usage
            return last_usage.saturating_add(usage);
        }

        // Calculate recovered amount: (last_usage * time_delta) / window_size
        // Use saturating operations to avoid overflow
        let recovered = if time_delta >= window_size {
            // Fully recovered if time delta exceeds window
            last_usage
        } else {
            // Partial recovery: last_usage * time_delta / window_size
            let numerator = (last_usage as i128).saturating_mul(time_delta as i128);
            let recovered_amt = numerator / (window_size as i128);
            recovered_amt.min(last_usage as i128) as i64
        };

        // New usage = max(0, last_usage - recovered) + usage
        let after_recovery = last_usage.saturating_sub(recovered).max(0);
        after_recovery.saturating_add(usage)
    }

    /// Compute recovered usage (for debugging/logging)
    pub fn recovery(last_usage: i64, last_time: i64, now: i64, window_size: i64) -> i64 {
        Self::increase(last_usage, 0, last_time, now, window_size)
    }

    /// Track bandwidth usage and return (path, before_aext, after_aext)
    /// Mirrors Java BandwidthProcessor.consume path selection:
    /// 1. Try ACCOUNT_NET (if account has frozen bandwidth)
    /// 2. Try FREE_NET (if public bandwidth available)
    /// 3. Fall back to FEE (charge TRX)
    pub fn track_bandwidth(
        _owner: &Address,
        bytes_used: i64,
        now: i64,  // block number or slot
        current_aext: &AccountAext,
        free_net_limit: i64,
    ) -> Result<(BandwidthPath, AccountAext, AccountAext)> {
        // Compute before AEXT (with decay but no new usage)
        let net_window_size = if current_aext.net_window_size > 0 {
            current_aext.net_window_size
        } else {
            28800
        };

        let free_net_window_size = 28800i64; // Default window for free net

        // Recover net_usage
        let recovered_net_usage = Self::recovery(
            current_aext.net_usage,
            current_aext.latest_consume_time,
            now,
            net_window_size,
        );

        // Recover free_net_usage
        let recovered_free_net_usage = Self::recovery(
            current_aext.free_net_usage,
            current_aext.latest_consume_free_time,
            now,
            free_net_window_size,
        );

        let before_aext = AccountAext {
            net_usage: recovered_net_usage,
            free_net_usage: recovered_free_net_usage,
            energy_usage: current_aext.energy_usage,
            latest_consume_time: current_aext.latest_consume_time,
            latest_consume_free_time: current_aext.latest_consume_free_time,
            latest_consume_time_for_energy: current_aext.latest_consume_time_for_energy,
            net_window_size: current_aext.net_window_size,
            net_window_optimized: current_aext.net_window_optimized,
            energy_window_size: current_aext.energy_window_size,
            energy_window_optimized: current_aext.energy_window_optimized,
        };

        // Path selection logic
        // Phase 1: Simplified - assume no frozen bandwidth (ACCOUNT_NET always 0)
        let account_net_limit = 0i64;  // Would calculate from freeze records in full implementation

        let available_account_net = account_net_limit.saturating_sub(recovered_net_usage).max(0);

        let (path, after_aext) = if bytes_used <= available_account_net {
            // Path 1: ACCOUNT_NET
            let new_net_usage = Self::increase(
                current_aext.net_usage,
                bytes_used,
                current_aext.latest_consume_time,
                now,
                net_window_size,
            );

            let after = AccountAext {
                net_usage: new_net_usage,
                latest_consume_time: now,
                ..before_aext.clone()
            };

            (BandwidthPath::AccountNet, after)
        } else {
            // Try FREE_NET
            let available_free_net = free_net_limit.saturating_sub(recovered_free_net_usage).max(0);

            if bytes_used <= available_free_net {
                // Path 2: FREE_NET
                let new_free_net_usage = Self::increase(
                    current_aext.free_net_usage,
                    bytes_used,
                    current_aext.latest_consume_free_time,
                    now,
                    free_net_window_size,
                );

                let after = AccountAext {
                    free_net_usage: new_free_net_usage,
                    latest_consume_free_time: now,
                    ..before_aext.clone()
                };

                (BandwidthPath::FreeNet, after)
            } else {
                // Path 3: FEE (no AEXT changes)
                (BandwidthPath::Fee, before_aext.clone())
            }
        };

        Ok((path, before_aext, after_aext))
    }
}
