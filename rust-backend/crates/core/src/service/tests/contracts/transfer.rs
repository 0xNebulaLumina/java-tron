//! TransferContract tests: validation edge cases for Java parity.

use super::super::super::*;
use super::common::{make_from_raw, new_test_context, new_test_service_with_system_enabled, seed_dynamic_properties};
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TxMetadata};
use revm_primitives::{Address, Bytes, U256, AccountInfo};
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
    assert!(result.is_ok(), "Normal transfer should succeed: {:?}", result.err());

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
    assert!(result.is_ok(), "Transfer should succeed: {:?}", result.err());

    // Verify sender was only debited by `amount` (no additional fee)
    let sender_account = storage_adapter.get_account(&owner_addr).unwrap().unwrap();
    assert_eq!(
        sender_account.balance,
        U256::from(10_000_000_000u64 - amount as u64),
        "Sender should only be debited by amount, no extra flat fee"
    );
}
