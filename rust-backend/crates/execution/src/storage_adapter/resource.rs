//! Resource tracking for bandwidth and energy accounting.
//!
//! This module provides windowed resource usage tracking matching TRON's
//! ResourceProcessor logic, including bandwidth path selection.
//!
//! ## Java Parity
//!
//! The `increase()` function uses Java's exact precision-scaled algorithm:
//! - `divideCeil()` for usage normalization
//! - `f64` decay with `.round()` for `Math.round()` parity
//! - `PRECISION` constant (1_000_000) for fixed-point arithmetic
//! - `DEFAULT_WINDOW_SIZE` (28800 slots = 86400s / 3s)

use anyhow::Result;
use revm::primitives::Address;
use super::types::AccountAext;

/// Precision constant matching Java's ResourceProcessor.PRECISION
const PRECISION: i64 = 1_000_000;

/// Default window size in slots (86400 seconds / 3 seconds per slot)
const DEFAULT_WINDOW_SIZE: i64 = 28800;

pub struct ResourceTracker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandwidthPath {
    AccountNet,    // Used account frozen bandwidth
    FreeNet,       // Used free public bandwidth
    CreateAccount, // Used bandwidth with create-account rate multiplier
    Fee,           // Fall back to fee deduction
}

/// Parameters for bandwidth tracking (expanded interface).
/// Provides all the inputs needed to match Java's BandwidthProcessor.consume().
pub struct BandwidthParams {
    /// Transaction size in bytes
    pub bytes_used: i64,
    /// Current head slot: (block_timestamp_ms - genesis_block_timestamp_ms) / 3000
    pub now: i64,
    /// Current AEXT state from storage
    pub current_aext: AccountAext,
    /// Account's frozen bandwidth limit (from calculateGlobalNetLimit)
    pub account_net_limit: i64,
    /// FREE_NET_LIMIT dynamic property (per-account free bandwidth cap)
    pub free_net_limit: i64,
    /// PUBLIC_NET_LIMIT dynamic property (global public bandwidth pool)
    pub public_net_limit: i64,
    /// PUBLIC_NET_USAGE dynamic property (current global public bandwidth usage)
    pub public_net_usage: i64,
    /// PUBLIC_NET_TIME dynamic property (last update time for global usage)
    pub public_net_time: i64,
    /// Whether this transaction creates a new account (TransferContract to non-existent recipient)
    pub creates_new_account: bool,
    /// CREATE_NEW_ACCOUNT_BANDWIDTH_RATE dynamic property (multiplier for create-account bandwidth)
    pub create_account_bandwidth_rate: i64,
    /// TRANSACTION_FEE dynamic property (fee per byte for FEE path), default 10 SUN/byte
    pub transaction_fee: i64,
}

/// Result from bandwidth tracking (expanded interface).
pub struct BandwidthResult {
    /// Which bandwidth path was selected
    pub path: BandwidthPath,
    /// AEXT state before bandwidth consumption (after decay)
    pub before_aext: AccountAext,
    /// AEXT state after bandwidth consumption
    pub after_aext: AccountAext,
    /// Updated global PUBLIC_NET_USAGE (set when FREE_NET path is used)
    pub new_public_net_usage: Option<i64>,
    /// Updated global PUBLIC_NET_TIME (set when FREE_NET path is used)
    pub new_public_net_time: Option<i64>,
    /// Fee amount in SUN (set when FEE path is used: bytes * transaction_fee)
    pub fee_amount: i64,
}

/// Java-parity `divideCeil`: integer division rounding up for positive numerators.
/// Matches `divideCeil(long numerator, long denominator)` in Java ResourceProcessor.
fn divide_ceil(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return 0;
    }
    numerator / denominator + if numerator % denominator > 0 { 1 } else { 0 }
}

impl ResourceTracker {
    /// Increase usage with windowed recovery (Java ResourceProcessor.increase parity).
    ///
    /// This implements Java's exact precision-scaled algorithm:
    /// ```text
    /// averageLastUsage  = divideCeil(lastUsage * PRECISION, windowSize)
    /// averageNewUsage   = divideCeil(usage * PRECISION, windowSize)
    /// if lastTime != now && lastTime + windowSize > now:
    ///   decay = (windowSize - delta) / windowSize   // floating point
    ///   averageLastUsage = Math.round(averageLastUsage * decay)
    /// elif lastTime + windowSize <= now:
    ///   averageLastUsage = 0
    /// // else lastTime == now: keep averageLastUsage as-is
    /// return getUsage((averageLastUsage + averageNewUsage) * windowSize / PRECISION)
    /// ```
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

        let avg_last = divide_ceil(last_usage.saturating_mul(PRECISION), window_size);
        let avg_new = divide_ceil(usage.saturating_mul(PRECISION), window_size);

        let decayed = if last_time != now {
            if last_time + window_size > now {
                let delta = now - last_time;
                let decay = (window_size - delta) as f64 / window_size as f64;
                (avg_last as f64 * decay).round() as i64 // Java Math.round parity
            } else {
                0 // Fully expired
            }
        } else {
            avg_last // Same slot, no decay
        };

        let total = decayed + avg_new;
        // getUsage(): total * windowSize / PRECISION
        total * window_size / PRECISION
    }

    /// Compute recovered usage (decay only, no new consumption).
    pub fn recovery(last_usage: i64, last_time: i64, now: i64, window_size: i64) -> i64 {
        Self::increase(last_usage, 0, last_time, now, window_size)
    }

    /// Track bandwidth usage with full Java parity (expanded interface).
    ///
    /// Path selection matches Java's BandwidthProcessor.consume():
    /// 1. If `creates_new_account`: netCost = bytes * rate, try ACCOUNT_NET with netCost
    /// 2. Try ACCOUNT_NET: check `netCost <= account_net_limit - recovered_net_usage`
    /// 3. Try FREE_NET: check both account free_net_limit AND global public_net_limit
    /// 4. FEE fallback: charge bytes * transaction_fee SUN
    pub fn track_bandwidth_v2(params: &BandwidthParams) -> Result<BandwidthResult> {
        let net_window_size = if params.current_aext.net_window_size > 0 {
            params.current_aext.net_window_size
        } else {
            DEFAULT_WINDOW_SIZE
        };
        let free_net_window_size = DEFAULT_WINDOW_SIZE;

        // Recover net_usage (decay to current slot)
        let recovered_net_usage = Self::recovery(
            params.current_aext.net_usage,
            params.current_aext.latest_consume_time,
            params.now,
            net_window_size,
        );

        // Recover free_net_usage (decay to current slot)
        let recovered_free_net_usage = Self::recovery(
            params.current_aext.free_net_usage,
            params.current_aext.latest_consume_free_time,
            params.now,
            free_net_window_size,
        );

        let before_aext = AccountAext {
            net_usage: recovered_net_usage,
            free_net_usage: recovered_free_net_usage,
            energy_usage: params.current_aext.energy_usage,
            latest_consume_time: params.current_aext.latest_consume_time,
            latest_consume_free_time: params.current_aext.latest_consume_free_time,
            latest_consume_time_for_energy: params.current_aext.latest_consume_time_for_energy,
            net_window_size: params.current_aext.net_window_size,
            net_window_optimized: params.current_aext.net_window_optimized,
            energy_window_size: params.current_aext.energy_window_size,
            energy_window_optimized: params.current_aext.energy_window_optimized,
        };

        // Compute netCost: for create-account txns, multiply bytes by rate
        let net_cost = if params.creates_new_account {
            params.bytes_used.saturating_mul(params.create_account_bandwidth_rate)
        } else {
            params.bytes_used
        };

        // Path 1: ACCOUNT_NET (try frozen bandwidth first)
        let available_account_net = params.account_net_limit.saturating_sub(recovered_net_usage).max(0);
        if net_cost <= available_account_net {
            let new_net_usage = Self::increase(
                params.current_aext.net_usage,
                net_cost,
                params.current_aext.latest_consume_time,
                params.now,
                net_window_size,
            );

            let after_aext = AccountAext {
                net_usage: new_net_usage,
                latest_consume_time: params.now,
                ..before_aext.clone()
            };

            let path = if params.creates_new_account {
                BandwidthPath::CreateAccount
            } else {
                BandwidthPath::AccountNet
            };

            return Ok(BandwidthResult {
                path,
                before_aext,
                after_aext,
                new_public_net_usage: None,
                new_public_net_time: None,
                fee_amount: 0,
            });
        }

        // Path 2: FREE_NET (check both account free_net_limit and global public_net_limit)
        let available_free_net = params.free_net_limit.saturating_sub(recovered_free_net_usage).max(0);

        if net_cost <= available_free_net {
            // Also check global PUBLIC_NET pool
            let recovered_public_net = Self::recovery(
                params.public_net_usage,
                params.public_net_time,
                params.now,
                free_net_window_size,
            );
            let available_public_net = params.public_net_limit.saturating_sub(recovered_public_net).max(0);

            if net_cost <= available_public_net {
                // Both account and global limits allow it
                let new_free_net_usage = Self::increase(
                    params.current_aext.free_net_usage,
                    net_cost,
                    params.current_aext.latest_consume_free_time,
                    params.now,
                    free_net_window_size,
                );

                let new_public_net_usage = Self::increase(
                    params.public_net_usage,
                    net_cost,
                    params.public_net_time,
                    params.now,
                    free_net_window_size,
                );

                let after_aext = AccountAext {
                    free_net_usage: new_free_net_usage,
                    latest_consume_free_time: params.now,
                    ..before_aext.clone()
                };

                return Ok(BandwidthResult {
                    path: BandwidthPath::FreeNet,
                    before_aext,
                    after_aext,
                    new_public_net_usage: Some(new_public_net_usage),
                    new_public_net_time: Some(params.now),
                    fee_amount: 0,
                });
            }
        }

        // Path 3: FEE fallback (no AEXT changes, charge fee)
        let fee_amount = params.bytes_used.saturating_mul(params.transaction_fee);

        Ok(BandwidthResult {
            path: BandwidthPath::Fee,
            before_aext: before_aext.clone(),
            after_aext: before_aext,
            new_public_net_usage: None,
            new_public_net_time: None,
            fee_amount,
        })
    }

    /// Legacy track_bandwidth interface (backward-compatible wrapper).
    ///
    /// Mirrors the old simplified API for callers that don't need full bandwidth params.
    /// Uses account_net_limit=0, no public_net check, no create-account path.
    pub fn track_bandwidth(
        _owner: &Address,
        bytes_used: i64,
        now: i64,
        current_aext: &AccountAext,
        free_net_limit: i64,
    ) -> Result<(BandwidthPath, AccountAext, AccountAext)> {
        let result = Self::track_bandwidth_v2(&BandwidthParams {
            bytes_used,
            now,
            current_aext: current_aext.clone(),
            account_net_limit: 0,
            free_net_limit,
            public_net_limit: i64::MAX, // No global limit check in legacy mode
            public_net_usage: 0,
            public_net_time: 0,
            creates_new_account: false,
            create_account_bandwidth_rate: 1,
            transaction_fee: 10,
        })?;

        Ok((result.path, result.before_aext, result.after_aext))
    }
}
