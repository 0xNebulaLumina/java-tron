//! TransferContract tests: validation edge cases for Java parity.

use super::super::super::*;
use super::common::{
    make_from_raw, new_test_context, new_test_service_with_system_enabled, seed_dynamic_properties,
};
use revm_primitives::{AccountInfo, Address, Bytes, U256};
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TxMetadata};
use tron_backend_storage::StorageEngine;

/// Helper to create a 21-byte TRON address with given prefix
fn make_tron_address_21(prefix: u8, base: [u8; 20]) -> Vec<u8> {
    let mut addr = vec![prefix];
    addr.extend_from_slice(&base);
    addr
}

/// Create a test storage adapter with mainnet prefix detected and an owner account seeded.
/// Returns (storage_adapter, owner_evm_address).
fn setup_storage_with_owner(balance: u64) -> (EngineBackedEvmStateStore, Address) {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed a mainnet-prefixed account so prefix detection returns 0x41
    let mainnet_owner = make_tron_address_21(0x41, [0x11u8; 20]);
    storage_engine
        .put("account", &mainnet_owner, b"dummy_account_data")
        .unwrap();
    seed_dynamic_properties(&storage_engine);

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(balance),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    // Leak temp_dir so it lives for the duration of the test
    std::mem::forget(temp_dir);

    (storage_adapter, owner_address)
}

/// Build a minimal TransferContract TronTransaction for testing.
fn build_transfer_tx(
    from: Address,
    from_raw: Option<Vec<u8>>,
    to_raw: Option<Vec<u8>>,
    to: Option<Address>,
    amount: i64,
) -> TronTransaction {
    TronTransaction {
        from,
        to,
        value: U256::from(amount as u64),
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::TransferContract),
            from_raw,
            to_raw,
            ..Default::default()
        },
    }
}

// -----------------------------------------------------------------------------
// Error Ordering Tests (Java parity)
// -----------------------------------------------------------------------------

#[test]
fn test_transfer_invalid_owner_and_invalid_to_returns_owner_error_first() {
    // Java: if both ownerAddress and toAddress are invalid, "Invalid ownerAddress!" is returned
    // because ownerAddress validation happens first in TransferActuator.validate().
    let (mut storage_adapter, owner_addr) = setup_storage_with_owner(10_000_000_000);
    let service = new_test_service_with_system_enabled();

    // Both addresses have wrong prefix (0xa0 on mainnet)
    let bad_owner_raw = make_tron_address_21(0xa0, [0x11u8; 20]);
    let bad_to_raw = make_tron_address_21(0xa0, [0x22u8; 20]);

    let tx = build_transfer_tx(
        owner_addr,
        Some(bad_owner_raw),
        Some(bad_to_raw.clone()),
        Some(Address::from([0x22u8; 20])),
        1_000_000,
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid ownerAddress!");
}

#[test]
fn test_transfer_valid_owner_and_invalid_to_returns_to_error() {
    // Java: valid owner + invalid to → "Invalid toAddress!"
    let (mut storage_adapter, owner_addr) = setup_storage_with_owner(10_000_000_000);
    let service = new_test_service_with_system_enabled();

    let good_owner_raw = make_from_raw(&owner_addr);
    let bad_to_raw = make_tron_address_21(0xa0, [0x22u8; 20]); // wrong prefix

    let tx = build_transfer_tx(
        owner_addr,
        Some(good_owner_raw),
        Some(bad_to_raw),
        Some(Address::from([0x22u8; 20])),
        1_000_000,
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid toAddress!");
}

#[test]
fn test_transfer_wrong_prefix_to_address_rejected() {
    // to_raw has testnet prefix 0xa0 on a mainnet chain → "Invalid toAddress!"
    let (mut storage_adapter, owner_addr) = setup_storage_with_owner(10_000_000_000);
    let service = new_test_service_with_system_enabled();

    let good_owner_raw = make_from_raw(&owner_addr);
    let wrong_prefix_to = make_tron_address_21(0xa0, [0x22u8; 20]);

    let tx = build_transfer_tx(
        owner_addr,
        Some(good_owner_raw),
        Some(wrong_prefix_to),
        None, // conversion would have set this to None for malformed address
        1_000_000,
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid toAddress!");
}

#[test]
fn test_transfer_empty_to_raw_rejected() {
    // to_raw is None (empty to field) → "Invalid toAddress!"
    let (mut storage_adapter, owner_addr) = setup_storage_with_owner(10_000_000_000);
    let service = new_test_service_with_system_enabled();

    let good_owner_raw = make_from_raw(&owner_addr);

    let tx = build_transfer_tx(
        owner_addr,
        Some(good_owner_raw),
        None, // empty to_raw
        None,
        1_000_000,
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid toAddress!");
}

#[test]
fn test_transfer_20_byte_to_without_prefix_rejected() {
    // to_raw is only 20 bytes (no prefix) → "Invalid toAddress!"
    // Java's DecodeUtil.addressValid requires exactly 21 bytes.
    let (mut storage_adapter, owner_addr) = setup_storage_with_owner(10_000_000_000);
    let service = new_test_service_with_system_enabled();

    let good_owner_raw = make_from_raw(&owner_addr);
    let short_to_raw: Vec<u8> = vec![0x22u8; 20]; // 20 bytes, no prefix

    let tx = build_transfer_tx(
        owner_addr,
        Some(good_owner_raw),
        Some(short_to_raw),
        Some(Address::from([0x22u8; 20])),
        1_000_000,
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid toAddress!");
}

#[test]
fn test_transfer_22_byte_to_rejected() {
    // to_raw is 22 bytes (too long) → "Invalid toAddress!"
    let (mut storage_adapter, owner_addr) = setup_storage_with_owner(10_000_000_000);
    let service = new_test_service_with_system_enabled();

    let good_owner_raw = make_from_raw(&owner_addr);
    let long_to_raw: Vec<u8> = vec![0x41u8; 22]; // 22 bytes, too long

    let tx = build_transfer_tx(
        owner_addr,
        Some(good_owner_raw),
        Some(long_to_raw),
        None,
        1_000_000,
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Invalid toAddress!");
}

// -----------------------------------------------------------------------------
// Transfer to Self Tests
// -----------------------------------------------------------------------------

#[test]
fn test_transfer_to_self_rejected() {
    // Java: "Cannot transfer TRX to yourself."
    let (mut storage_adapter, owner_addr) = setup_storage_with_owner(10_000_000_000);
    let service = new_test_service_with_system_enabled();

    let owner_raw = make_from_raw(&owner_addr);
    let to_raw = make_from_raw(&owner_addr); // same address

    let tx = build_transfer_tx(
        owner_addr,
        Some(owner_raw),
        Some(to_raw),
        Some(owner_addr),
        1_000_000,
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Cannot transfer TRX to yourself.");
}

// -----------------------------------------------------------------------------
// Amount Validation Tests
// -----------------------------------------------------------------------------

#[test]
fn test_transfer_zero_amount_rejected() {
    // Java: "Amount must be greater than 0."
    let (mut storage_adapter, owner_addr) = setup_storage_with_owner(10_000_000_000);
    let service = new_test_service_with_system_enabled();

    let to_addr = Address::from([0x22u8; 20]);
    let owner_raw = make_from_raw(&owner_addr);
    let to_raw = make_tron_address_21(0x41, [0x22u8; 20]);

    let tx = build_transfer_tx(
        owner_addr,
        Some(owner_raw),
        Some(to_raw),
        Some(to_addr),
        0, // zero amount
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Amount must be greater than 0.");
}

// -----------------------------------------------------------------------------
// Successful Transfer Tests
// -----------------------------------------------------------------------------

#[test]
fn test_transfer_normal_succeeds() {
    // Normal transfer: sender has enough balance, recipient exists
    let (mut storage_adapter, owner_addr) = setup_storage_with_owner(10_000_000_000);
    let service = new_test_service_with_system_enabled();

    let to_addr = Address::from([0x22u8; 20]);
    let owner_raw = make_from_raw(&owner_addr);
    let to_raw = make_tron_address_21(0x41, [0x22u8; 20]);

    // Create recipient account
    storage_adapter
        .set_account(
            to_addr,
            AccountInfo {
                balance: U256::from(5_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let amount = 1_000_000i64;
    let tx = build_transfer_tx(
        owner_addr,
        Some(owner_raw),
        Some(to_raw),
        Some(to_addr),
        amount,
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(
        result.is_ok(),
        "Normal transfer should succeed: {:?}",
        result.err()
    );

    let exec_result = result.unwrap();
    assert!(exec_result.success);
    assert_eq!(exec_result.energy_used, 0);

    // Verify balances
    let sender_account = storage_adapter.get_account(&owner_addr).unwrap().unwrap();
    assert_eq!(
        sender_account.balance,
        U256::from(10_000_000_000u64 - amount as u64)
    );

    let recipient_account = storage_adapter.get_account(&to_addr).unwrap().unwrap();
    assert_eq!(
        recipient_account.balance,
        U256::from(5_000_000_000u64 + amount as u64)
    );
}

#[test]
fn test_transfer_insufficient_balance_rejected() {
    // Java: "Validate TransferContract error, balance is not sufficient."
    let (mut storage_adapter, owner_addr) = setup_storage_with_owner(100); // very low balance
    let service = new_test_service_with_system_enabled();

    let to_addr = Address::from([0x22u8; 20]);
    let owner_raw = make_from_raw(&owner_addr);
    let to_raw = make_tron_address_21(0x41, [0x22u8; 20]);

    // Create recipient
    storage_adapter
        .set_account(
            to_addr,
            AccountInfo {
                balance: U256::from(5_000_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let tx = build_transfer_tx(
        owner_addr,
        Some(owner_raw),
        Some(to_raw),
        Some(to_addr),
        1_000_000, // more than balance
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap(),
        "Validate TransferContract error, balance is not sufficient."
    );
}

#[test]
fn test_transfer_no_owner_account_rejected() {
    // Java: "Validate TransferContract error, no OwnerAccount."
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();

    // Seed prefix detection but don't create owner account
    let mainnet_dummy = make_tron_address_21(0x41, [0xAAu8; 20]);
    storage_engine
        .put("account", &mainnet_dummy, b"dummy")
        .unwrap();
    seed_dynamic_properties(&storage_engine);

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);

    let service = new_test_service_with_system_enabled();

    let owner_addr = Address::from([0x11u8; 20]);
    let to_addr = Address::from([0x22u8; 20]);
    let owner_raw = make_tron_address_21(0x41, [0x11u8; 20]);
    let to_raw = make_tron_address_21(0x41, [0x22u8; 20]);

    let tx = build_transfer_tx(
        owner_addr,
        Some(owner_raw),
        Some(to_raw),
        Some(to_addr),
        1_000_000,
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap(),
        "Validate TransferContract error, no OwnerAccount."
    );
}

// -----------------------------------------------------------------------------
// Fee Parity Tests
// -----------------------------------------------------------------------------

// -----------------------------------------------------------------------------
// Bandwidth / AEXT Tracking Tests
// -----------------------------------------------------------------------------

/// Tests for ResourceTracker::increase() Java parity.
/// Java reference values computed from ResourceProcessor.increase() with
/// PRECISION=1_000_000 and DEFAULT_WINDOW_SIZE=28800.
mod bandwidth_tests {
    use tron_backend_execution::{AccountAext, BandwidthParams, BandwidthPath, ResourceTracker};

    const WINDOW: i64 = 28800;

    #[test]
    fn test_increase_no_prior_usage() {
        // First usage: last_usage=0, last_time=0, usage=100
        let result = ResourceTracker::increase(0, 100, 0, 1000, WINDOW);
        assert_eq!(
            result, 100,
            "First usage with no history should return usage"
        );
    }

    #[test]
    fn test_increase_same_slot_accumulates() {
        // Same slot: last_time == now, should accumulate without decay
        let result = ResourceTracker::increase(200, 50, 1000, 1000, WINDOW);
        assert_eq!(result, 250, "Same-slot usage should simply accumulate");
    }

    #[test]
    fn test_increase_full_window_expired() {
        // Time delta >= window: all previous usage fully expired
        let result = ResourceTracker::increase(1000, 50, 0, WINDOW + 1, WINDOW);
        assert_eq!(
            result, 50,
            "After full window expiry, only new usage remains"
        );
    }

    #[test]
    fn test_increase_partial_decay() {
        // Partial decay: Java-parity test case
        // last_usage=1000, usage=0, last_time=0, now=14400 (half window), window=28800
        // avgLast = divideCeil(1000 * 1000000, 28800) = divideCeil(1000000000, 28800) = 34723
        // decay = (28800 - 14400) / 28800 = 0.5
        // decayed = Math.round(34723 * 0.5) = Math.round(17361.5) = 17362
        // total = 17362, getUsage = 17362 * 28800 / 1000000 = 500025600 / 1000000 = 500
        let result = ResourceTracker::increase(1000, 0, 0, 14400, WINDOW);
        assert_eq!(
            result, 500,
            "Half-window decay should match Java ResourceProcessor"
        );
    }

    #[test]
    fn test_increase_quarter_decay() {
        // 3/4 window elapsed: decay = 0.25
        // last_usage=1000, now=21600 (3/4 window)
        // avgLast = divideCeil(1000 * 1000000, 28800) = 34723
        // decay = (28800 - 21600) / 28800 = 7200/28800 = 0.25
        // decayed = Math.round(34723 * 0.25) = Math.round(8680.75) = 8681
        // getUsage = 8681 * 28800 / 1000000 = 250012800 / 1000000 = 250
        let result = ResourceTracker::increase(1000, 0, 0, 21600, WINDOW);
        assert_eq!(result, 250, "Quarter-remaining decay should match Java");
    }

    #[test]
    fn test_increase_with_new_usage_after_decay() {
        // Partial decay + new usage
        // last_usage=1000, usage=200, last_time=0, now=14400, window=28800
        // avgLast decayed = 17362 (from partial_decay test)
        // avgNew = divideCeil(200 * 1000000, 28800) = divideCeil(200000000, 28800) = 6945
        // total = 17362 + 6945 = 24307
        // getUsage = 24307 * 28800 / 1000000 = 700041600 / 1000000 = 700
        let result = ResourceTracker::increase(1000, 200, 0, 14400, WINDOW);
        assert_eq!(result, 700, "Decay + new usage should match Java");
    }

    #[test]
    fn test_increase_window_zero_returns_usage() {
        let result = ResourceTracker::increase(500, 100, 0, 1000, 0);
        assert_eq!(result, 100, "Zero window should return just the new usage");
    }

    #[test]
    fn test_track_bandwidth_v2_account_net_path() {
        // Owner has frozen bandwidth, should use ACCOUNT_NET path
        let aext = AccountAext::with_defaults();
        let params = BandwidthParams {
            bytes_used: 100,
            now: 1000,
            current_aext: aext,
            account_net_limit: 5000, // Enough frozen bandwidth
            free_net_limit: 5000,
            public_net_limit: 14_400_000_000,
            public_net_usage: 0,
            public_net_time: 0,
            creates_new_account: false,
            create_account_bandwidth_rate: 1,
            transaction_fee: 10,
        };

        let result = ResourceTracker::track_bandwidth_v2(&params).unwrap();
        assert_eq!(result.path, BandwidthPath::AccountNet);
        assert_eq!(result.after_aext.net_usage, 100);
        assert_eq!(result.after_aext.latest_consume_time, 1000);
        assert!(result.new_public_net_usage.is_none());
        assert_eq!(result.fee_amount, 0);
    }

    #[test]
    fn test_track_bandwidth_v2_free_net_path() {
        // No frozen bandwidth, should use FREE_NET path
        let aext = AccountAext::with_defaults();
        let params = BandwidthParams {
            bytes_used: 100,
            now: 1000,
            current_aext: aext,
            account_net_limit: 0, // No frozen bandwidth
            free_net_limit: 5000,
            public_net_limit: 14_400_000_000,
            public_net_usage: 0,
            public_net_time: 0,
            creates_new_account: false,
            create_account_bandwidth_rate: 1,
            transaction_fee: 10,
        };

        let result = ResourceTracker::track_bandwidth_v2(&params).unwrap();
        assert_eq!(result.path, BandwidthPath::FreeNet);
        assert_eq!(result.after_aext.free_net_usage, 100);
        assert_eq!(result.after_aext.latest_consume_free_time, 1000);
        assert!(result.new_public_net_usage.is_some());
        assert_eq!(result.fee_amount, 0);
    }

    #[test]
    fn test_track_bandwidth_v2_free_net_blocked_by_global_limit() {
        // Account has free bandwidth, but global PUBLIC_NET is exhausted
        let aext = AccountAext::with_defaults();
        let params = BandwidthParams {
            bytes_used: 100,
            now: 1000,
            current_aext: aext,
            account_net_limit: 0,
            free_net_limit: 5000,
            public_net_limit: 50, // Global limit too low
            public_net_usage: 0,
            public_net_time: 0,
            creates_new_account: false,
            create_account_bandwidth_rate: 1,
            transaction_fee: 10,
        };

        let result = ResourceTracker::track_bandwidth_v2(&params).unwrap();
        assert_eq!(
            result.path,
            BandwidthPath::Fee,
            "Should fall back to FEE when global limit exceeded"
        );
        assert_eq!(result.fee_amount, 1000, "Fee = 100 bytes * 10 SUN/byte");
    }

    #[test]
    fn test_track_bandwidth_v2_fee_path() {
        // No frozen bandwidth and free net exhausted → FEE path
        let mut aext = AccountAext::with_defaults();
        aext.free_net_usage = 5000; // Already used up
        aext.latest_consume_free_time = 1000; // Same slot so no decay

        let params = BandwidthParams {
            bytes_used: 100,
            now: 1000,
            current_aext: aext,
            account_net_limit: 0,
            free_net_limit: 5000,
            public_net_limit: 14_400_000_000,
            public_net_usage: 0,
            public_net_time: 0,
            creates_new_account: false,
            create_account_bandwidth_rate: 1,
            transaction_fee: 10,
        };

        let result = ResourceTracker::track_bandwidth_v2(&params).unwrap();
        assert_eq!(result.path, BandwidthPath::Fee);
        assert_eq!(result.fee_amount, 1000, "Fee = 100 bytes * 10 SUN/byte");
    }

    #[test]
    fn test_track_bandwidth_v2_create_account_path() {
        // Creates new account: netCost = bytes * rate
        let aext = AccountAext::with_defaults();
        let params = BandwidthParams {
            bytes_used: 100,
            now: 1000,
            current_aext: aext,
            account_net_limit: 5000, // Enough frozen bandwidth for netCost
            free_net_limit: 5000,
            public_net_limit: 14_400_000_000,
            public_net_usage: 0,
            public_net_time: 0,
            creates_new_account: true,
            create_account_bandwidth_rate: 2, // 100 bytes * 2 = 200 netCost
            transaction_fee: 10,
        };

        let result = ResourceTracker::track_bandwidth_v2(&params).unwrap();
        assert_eq!(result.path, BandwidthPath::CreateAccount);
        // netCost = 100 * 2 = 200, should be recorded in net_usage
        assert_eq!(result.after_aext.net_usage, 200);
        assert_eq!(result.fee_amount, 0);
    }

    #[test]
    fn test_head_slot_computation() {
        // Verify headSlot = (block_timestamp - genesis_timestamp) / 3000
        let genesis_ts: i64 = 1529891469000; // mainnet genesis
        let block_ts: u64 = 1529891469000 + 3000 * 100; // 100 slots after genesis

        let head_slot = (block_ts as i64 - genesis_ts) / 3000;
        assert_eq!(head_slot, 100);

        // Verify it differs from the old incorrect formula
        let old_slot = block_ts / 3000; // Without genesis offset
        assert_ne!(
            head_slot, old_slot as i64,
            "Genesis-offset headSlot differs from raw division"
        );
    }
}

#[test]
fn test_transfer_no_extra_flat_fee_charged() {
    // Strict parity: Java's TRANSFER_FEE = 0. No extra fee beyond create-account-fee.
    // This test verifies that even if fee_config has non_vm_blackhole_credit_flat set,
    // TransferContract does NOT use it.
    let (mut storage_adapter, owner_addr) = setup_storage_with_owner(10_000_000_000);
    let service = new_test_service_with_system_enabled();

    let to_addr = Address::from([0x22u8; 20]);
    let owner_raw = make_from_raw(&owner_addr);
    let to_raw = make_tron_address_21(0x41, [0x22u8; 20]);

    // Create recipient so no create-account-fee applies
    storage_adapter
        .set_account(
            to_addr,
            AccountInfo {
                balance: U256::from(1_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();

    let amount = 500_000i64;
    let tx = build_transfer_tx(
        owner_addr,
        Some(owner_raw),
        Some(to_raw),
        Some(to_addr),
        amount,
    );

    let result = service.execute_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(
        result.is_ok(),
        "Transfer should succeed: {:?}",
        result.err()
    );

    // Verify sender was only debited by `amount` (no additional fee)
    let sender_account = storage_adapter.get_account(&owner_addr).unwrap().unwrap();
    assert_eq!(
        sender_account.balance,
        U256::from(10_000_000_000u64 - amount as u64),
        "Sender should only be debited by amount, no extra flat fee"
    );
}
