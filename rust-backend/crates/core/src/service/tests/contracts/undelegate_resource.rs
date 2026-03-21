//! UnDelegateResourceContract tests for resource usage transfer parity.
//!
//! Tests validate that Rust matches Java's `UnDelegateResourceActuator.execute()` behavior,
//! including transferUsage computation, receiver usage reduction, and owner unDelegateIncrease.
//!
//! See: planning/review_again/UNDELEGATE_RESOURCE_CONTRACT.planning.md

use super::super::super::*;
use super::common::{encode_varint, make_from_raw};

// ==========================================================================
// Unit tests for helper functions (accessible from child module)
// ==========================================================================

#[test]
fn test_divide_ceil_resource() {
    assert_eq!(BackendService::divide_ceil_resource(10, 3), 4);
    assert_eq!(BackendService::divide_ceil_resource(9, 3), 3);
    assert_eq!(BackendService::divide_ceil_resource(0, 3), 0);
    assert_eq!(BackendService::divide_ceil_resource(10, 0), 0);
    assert_eq!(BackendService::divide_ceil_resource(1, 1), 1);
}

#[test]
fn test_normalize_window_size_slots() {
    assert_eq!(BackendService::normalize_window_size_slots(0, false), 28800);
    assert_eq!(BackendService::normalize_window_size_slots(0, true), 28800);
    assert_eq!(
        BackendService::normalize_window_size_slots(25000, false),
        25000
    );
    assert_eq!(
        BackendService::normalize_window_size_slots(28800, false),
        28800
    );
    assert_eq!(
        BackendService::normalize_window_size_slots(500, true),
        28800
    );
    assert_eq!(
        BackendService::normalize_window_size_slots(999, true),
        28800
    );
    assert_eq!(
        BackendService::normalize_window_size_slots(28800000, true),
        28800
    );
    assert_eq!(
        BackendService::normalize_window_size_slots(25000000, true),
        25000
    );
    assert_eq!(BackendService::normalize_window_size_slots(1000, true), 1);
}

#[test]
fn test_normalize_window_size_v2_raw() {
    assert_eq!(
        BackendService::normalize_window_size_v2_raw(0, false),
        28_800_000
    );
    assert_eq!(
        BackendService::normalize_window_size_v2_raw(0, true),
        28_800_000
    );
    assert_eq!(
        BackendService::normalize_window_size_v2_raw(28_800_000, true),
        28_800_000
    );
    assert_eq!(
        BackendService::normalize_window_size_v2_raw(25_000_000, true),
        25_000_000
    );
    assert_eq!(
        BackendService::normalize_window_size_v2_raw(28800, false),
        28_800_000
    );
    assert_eq!(
        BackendService::normalize_window_size_v2_raw(25000, false),
        25_000_000
    );
}

#[test]
fn test_resource_increase_v2_recovery_no_usage() {
    let (new_usage, new_window, new_optimized) =
        BackendService::resource_increase_v2_fn(0, 0, 1000, 2000, 28_800_000, true);
    assert_eq!(new_usage, 0);
    assert_eq!(new_window, 28_800_000);
    assert!(new_optimized);
}

#[test]
fn test_resource_increase_v2_recovery_with_usage() {
    // Java ResourceProcessor.increaseV2(account, BANDWIDTH, 1000, 0, 10000, 10100)
    // oldWindowSize = 28800, oldWindowSizeV2 = 28800000
    // averageLastUsage = divideCeil(1000 * 1000000, 28800) = 34723
    // delta = 100, decay = 28700/28800 = 0.99652777...
    // averageLastUsage = round(34723 * 0.99652...) = round(34602.39...) = 34602
    // newUsage = (34602 * 28800) / 1000000 = 996537600/1000000 = 996
    // remainUsage = 996
    // remainWindowSize = 28800000 - 100*1000 = 28700000
    // newWindowSize = divideCeil(996 * 28700000, 996) = 28700000
    // min(28700000, 28800000) = 28700000
    let (new_usage, new_window, new_optimized) =
        BackendService::resource_increase_v2_fn(1000, 0, 10000, 10100, 28_800_000, true);
    assert_eq!(new_usage, 996);
    assert_eq!(new_window, 28_700_000);
    assert!(new_optimized);
}

#[test]
fn test_resource_increase_v2_fully_expired() {
    // windowSize=28800, last_time=1000, now=30000 => 1000+28800=29800 <= 30000 => fully expired
    let (new_usage, new_window, new_optimized) =
        BackendService::resource_increase_v2_fn(5000, 0, 1000, 30000, 28_800_000, true);
    assert_eq!(new_usage, 0);
    assert_eq!(new_window, 28_800_000);
    assert!(new_optimized);
}

#[test]
fn test_resource_increase_v1_recovery() {
    let (new_usage, new_window, new_optimized) =
        BackendService::resource_increase_v1(1000, 0, 10000, 10100, 28800, false);
    assert_eq!(new_usage, 996);
    assert_eq!(new_window, 28700);
    assert!(!new_optimized);
}

#[test]
fn test_resource_increase_v1_same_slot() {
    // When lastTime == now, no decay happens
    let (new_usage, new_window, _) =
        BackendService::resource_increase_v1(1000, 0, 10000, 10000, 28800, false);
    assert_eq!(new_usage, 1000); // No decay
                                 // remainUsage=1000, remainWindowSize=28800-(10000-10000)=28800
    assert_eq!(new_window, 28800);
}

#[test]
fn test_resource_increase_with_window_dispatches() {
    // V1 dispatch
    let (u1, w1, o1) =
        BackendService::resource_increase_with_window(1000, 0, 10000, 10100, 28800, false, false);
    let (u1_ref, w1_ref, o1_ref) =
        BackendService::resource_increase_v1(1000, 0, 10000, 10100, 28800, false);
    assert_eq!((u1, w1, o1), (u1_ref, w1_ref, o1_ref));

    // V2 dispatch
    let (u2, w2, o2) = BackendService::resource_increase_with_window(
        1000, 0, 10000, 10100, 28_800_000, true, true,
    );
    let (u2_ref, w2_ref, o2_ref) =
        BackendService::resource_increase_v2_fn(1000, 0, 10000, 10100, 28_800_000, true);
    assert_eq!((u2, w2, o2), (u2_ref, w2_ref, o2_ref));
}

#[test]
fn test_un_delegate_increase_v2_basic() {
    let (new_usage, new_window, new_optimized, new_time) =
        BackendService::un_delegate_increase_v2_fn(
            500, 10000, 28_800_000, true, 28_800_000, true, 200, 10100,
        );
    assert!(new_usage > 200);
    assert!(new_window > 0);
    assert!(new_window <= 28_800_000);
    assert!(new_optimized);
    assert_eq!(new_time, 10100);
}

#[test]
fn test_un_delegate_increase_v2_zero_owner_usage() {
    let (new_usage, new_window, new_optimized, new_time) =
        BackendService::un_delegate_increase_v2_fn(
            0, 10000, 28_800_000, true, 28_800_000, true, 300, 10100,
        );
    assert_eq!(new_usage, 300);
    // Window = divideCeil(0 * ownerW + 300 * 28800000, 300) = 28800000
    assert_eq!(new_window, 28_800_000);
    assert!(new_optimized);
    assert_eq!(new_time, 10100);
}

#[test]
fn test_un_delegate_increase_v2_zero_total() {
    let (new_usage, new_window, new_optimized, new_time) =
        BackendService::un_delegate_increase_v2_fn(
            0, 10000, 28_800_000, true, 28_800_000, true, 0, 10100,
        );
    assert_eq!(new_usage, 0);
    assert_eq!(new_window, 28_800_000);
    assert!(new_optimized);
    assert_eq!(new_time, 10100);
}

#[test]
fn test_un_delegate_increase_v1_basic() {
    let (new_usage, new_window, new_optimized, new_time) =
        BackendService::un_delegate_increase_v1(500, 10000, 28800, false, 28800, false, 200, 10100);
    assert!(new_usage > 200);
    assert!(new_window > 0);
    assert!(!new_optimized);
    assert_eq!(new_time, 10100);
}

#[test]
fn test_un_delegate_increase_dispatches() {
    let result_v2 = BackendService::un_delegate_increase(
        500, 10000, 28_800_000, true, 28_800_000, true, 200, 10100, true,
    );
    let expected_v2 = BackendService::un_delegate_increase_v2_fn(
        500, 10000, 28_800_000, true, 28_800_000, true, 200, 10100,
    );
    assert_eq!(result_v2, expected_v2);

    let result_v1 = BackendService::un_delegate_increase(
        500, 10000, 28800, false, 28800, false, 200, 10100, false,
    );
    let expected_v1 =
        BackendService::un_delegate_increase_v1(500, 10000, 28800, false, 28800, false, 200, 10100);
    assert_eq!(result_v1, expected_v1);
}

#[test]
fn test_get_all_frozen_balance_for_bandwidth() {
    use tron_backend_execution::protocol::account::{FreezeV2, Frozen};
    use tron_backend_execution::protocol::Account;

    let mut account = Account::default();
    account.frozen.push(Frozen {
        frozen_balance: 1_000_000,
        expire_time: 0,
    });
    account.frozen.push(Frozen {
        frozen_balance: 2_000_000,
        expire_time: 0,
    });
    account.acquired_delegated_frozen_balance_for_bandwidth = 500_000;
    account.frozen_v2.push(FreezeV2 {
        r#type: 0,
        amount: 3_000_000,
    });
    account.frozen_v2.push(FreezeV2 {
        r#type: 1,
        amount: 999_999,
    }); // energy, shouldn't count
    account.acquired_delegated_frozen_v2_balance_for_bandwidth = 750_000;

    let total = BackendService::get_all_frozen_balance_for_bandwidth(&account);
    assert_eq!(total, 7_250_000);
}

#[test]
fn test_get_all_frozen_balance_for_energy() {
    use tron_backend_execution::protocol::account::{AccountResource, FreezeV2, Frozen};
    use tron_backend_execution::protocol::Account;

    let mut account = Account::default();
    account.account_resource = Some(AccountResource {
        frozen_balance_for_energy: Some(Frozen {
            frozen_balance: 1_500_000,
            expire_time: 0,
        }),
        acquired_delegated_frozen_balance_for_energy: 400_000,
        acquired_delegated_frozen_v2_balance_for_energy: 600_000,
        ..Default::default()
    });
    account.frozen_v2.push(FreezeV2 {
        r#type: 1,
        amount: 2_000_000,
    });
    account.frozen_v2.push(FreezeV2 {
        r#type: 0,
        amount: 888_888,
    }); // bandwidth, shouldn't count

    let total = BackendService::get_all_frozen_balance_for_energy(&account);
    assert_eq!(total, 4_500_000);
}

#[test]
fn test_transfer_usage_computation_bandwidth() {
    // Simulate the transferUsage computation for BANDWIDTH
    let un_delegate_balance: i64 = 10_000_000;
    let total_net_limit: i64 = 43_200_000_000;
    let total_net_weight: i64 = 50_000_000_000;
    let receiver_net_usage: i64 = 500;
    let all_frozen_bw: i64 = 20_000_000;
    let trx_precision: f64 = 1_000_000.0;

    // Java: (long) ((double) unDelegateBalance / TRX_PRECISION * ((double) totalNetLimit / totalNetWeight))
    let un_delegate_max_usage = (un_delegate_balance as f64 / trx_precision
        * (total_net_limit as f64 / total_net_weight as f64))
        as i64;
    assert_eq!(un_delegate_max_usage, 8);

    // Java: (long) (receiverNetUsage * ((double) unDelegateBalance / allFrozenBW))
    let mut transfer_usage =
        (receiver_net_usage as f64 * (un_delegate_balance as f64 / all_frozen_bw as f64)) as i64;
    assert_eq!(transfer_usage, 250);

    transfer_usage = std::cmp::min(un_delegate_max_usage, transfer_usage);
    assert_eq!(transfer_usage, 8);
}

#[test]
fn test_transfer_usage_computation_energy() {
    // Simulate the transferUsage computation for ENERGY
    let un_delegate_balance: i64 = 5_000_000; // 5 TRX
    let total_energy_limit: i64 = 50_000_000_000;
    let total_energy_weight: i64 = 10_000_000_000;
    let receiver_energy_usage: i64 = 1000;
    let all_frozen_energy: i64 = 10_000_000; // 10 TRX
    let trx_precision: f64 = 1_000_000.0;

    // unDelegateMaxUsage = (5000000/1000000) * (50000000000/10000000000) = 5 * 5 = 25
    let un_delegate_max_usage = (un_delegate_balance as f64 / trx_precision
        * (total_energy_limit as f64 / total_energy_weight as f64))
        as i64;
    assert_eq!(un_delegate_max_usage, 25);

    // transferUsage = (1000 * (5000000/10000000)) = 1000 * 0.5 = 500
    let mut transfer_usage = (receiver_energy_usage as f64
        * (un_delegate_balance as f64 / all_frozen_energy as f64))
        as i64;
    assert_eq!(transfer_usage, 500);

    transfer_usage = std::cmp::min(un_delegate_max_usage, transfer_usage);
    assert_eq!(transfer_usage, 25);
}

#[test]
fn test_resource_increase_v2_with_new_usage() {
    // Test increaseV2 with non-zero new usage (simulating a consumption, not just recovery)
    // last_usage=0, usage=500, last_time=10000, now=10000, window=28800000, optimized=true
    // Since lastTime == now, no decay. averageLastUsage = 0.
    // averageUsage = divideCeil(500*1000000, 28800) = divideCeil(500000000, 28800) = 17362
    // newUsage = (0*28800 + 17362*28800) / 1000000 = 500025600/1000000 = 500
    // remainUsage = 0 * 28800 / 1000000 = 0
    // Since remainUsage==0: return (500, 28800000, true)
    let (new_usage, new_window, new_optimized) =
        BackendService::resource_increase_v2_fn(0, 500, 10000, 10000, 28_800_000, true);
    assert_eq!(new_usage, 500);
    assert_eq!(new_window, 28_800_000);
    assert!(new_optimized);
}
