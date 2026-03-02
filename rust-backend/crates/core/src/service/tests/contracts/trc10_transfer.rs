//! TRC-10 TransferAssetContract tests.

use super::super::super::*;
use super::common::{new_test_context, seed_dynamic_properties};
use tron_backend_execution::{EngineBackedEvmStateStore, TronTransaction, TxMetadata};
use revm_primitives::{Address, Bytes, U256, AccountInfo};
use tron_backend_common::{ModuleManager, ExecutionConfig, RemoteExecutionConfig};

fn new_test_service_with_trc10_enabled() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            trc10_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

#[test]
fn test_trc10_transfer_emits_recipient_account_creation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path()).unwrap();
    // Match early-chain behavior for this test: no TRX fee for creating recipient accounts.
    storage_engine
        .put(
            "properties",
            b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT",
            &0u64.to_be_bytes(),
        )
        .unwrap();
    // Seed ALLOW_MULTI_SIGN=1 so recipient account creation includes default permissions.
    storage_engine
        .put(
            "properties",
            b"ALLOW_MULTI_SIGN",
            &1i64.to_be_bytes(),
        )
        .unwrap();

    let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine);
    let service = new_test_service_with_trc10_enabled();

    // Seed an issued TRC-10 so get_asset_issue succeeds (legacy allowSameTokenName=0).
    let asset_id = b"TEST".to_vec();
    let mut asset_issue = tron_backend_execution::protocol::AssetIssueContractData::default();
    asset_issue.id = "1000009".to_string();
    storage_adapter
        .put_asset_issue(&asset_id, &asset_issue, false)
        .unwrap();

    // Owner must exist and have sufficient TRC-10 balance.
    let owner_address = Address::from([0x11u8; 20]);
    storage_adapter
        .set_account(
            owner_address,
            AccountInfo {
                balance: U256::from(10_000_000u64),
                nonce: 0,
                code_hash: revm::primitives::B256::ZERO,
                code: None,
            },
        )
        .unwrap();
    let mut owner_proto = storage_adapter.get_account_proto(&owner_address).unwrap().unwrap();
    owner_proto
        .asset
        .insert("TEST".to_string(), 1_000 /* units */);
    owner_proto
        .asset_v2
        .insert("1000009".to_string(), 1_000 /* units */);
    storage_adapter
        .put_account_proto(&owner_address, &owner_proto)
        .unwrap();

    // Recipient does not exist pre-exec.
    let recipient_address = Address::from([0x22u8; 20]);

    // Build 21-byte TRON addresses (0x41 prefix + 20-byte address) for validation parity
    let mut owner_tron_21 = vec![0x41u8];
    owner_tron_21.extend_from_slice(owner_address.as_slice());
    let mut recipient_tron_21 = vec![0x41u8];
    recipient_tron_21.extend_from_slice(recipient_address.as_slice());

    let tx = TronTransaction {
        from: owner_address,
        to: Some(recipient_address),
        value: U256::from(100u64),
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(tron_backend_execution::TronContractType::TransferAssetContract),
            asset_id: Some(asset_id),
            from_raw: Some(owner_tron_21),
            to_raw: Some(recipient_tron_21),
            ..Default::default()
        },
    };

    let result = service.execute_trc10_transfer_contract(&mut storage_adapter, &tx, &new_test_context());
    assert!(result.is_ok(), "TRC-10 transfer should succeed: {:?}", result.err());
    let exec_result = result.unwrap();

    let recipient_change = exec_result.state_changes.iter().find_map(|sc| match sc {
        tron_backend_execution::TronStateChange::AccountChange {
            address,
            old_account,
            new_account,
        } if *address == recipient_address => Some((old_account, new_account)),
        _ => None,
    });

    assert!(recipient_change.is_some(), "Expected recipient AccountChange in state_changes");
    let (old_account, new_account) = recipient_change.unwrap();
    assert!(old_account.is_none(), "Recipient should be emitted as account creation (old_account=None)");
    assert!(new_account.is_some(), "Recipient should have a post-state AccountInfo (new_account=Some)");

    // Verify owner balance is unchanged (create_account_fee=0, no TRX transferred in TRC-10 transfer)
    let owner_change = exec_result.state_changes.iter().find_map(|sc| match sc {
        tron_backend_execution::TronStateChange::AccountChange {
            address,
            old_account,
            new_account,
        } if *address == owner_address => Some((old_account.clone(), new_account.clone())),
        _ => None,
    });
    assert!(owner_change.is_some(), "Expected owner AccountChange in state_changes");
    let (old_owner, new_owner) = owner_change.unwrap();
    assert!(old_owner.is_some(), "Owner should have pre-state");
    assert!(new_owner.is_some(), "Owner should have post-state");
    // With create_account_fee=0, owner TRX balance should be unchanged
    assert_eq!(old_owner.unwrap().balance, new_owner.unwrap().balance, "Owner TRX balance should be unchanged when create_account_fee=0");

    // Verify TRC-10 change is emitted with correct data
    assert!(!exec_result.trc10_changes.is_empty(), "Expected at least one TRC-10 change");
    let asset_transfer = exec_result.trc10_changes.iter().find(|c| {
        matches!(c, tron_backend_execution::Trc10Change::AssetTransferred(t) if t.owner_address == owner_address && t.to_address == recipient_address)
    });
    assert!(asset_transfer.is_some(), "Expected Trc10Change::AssetTransferred for owner→recipient");
    if let Some(tron_backend_execution::Trc10Change::AssetTransferred(t)) = asset_transfer {
        assert_eq!(t.amount, 100, "Transfer amount should be 100");
        assert_eq!(t.asset_name, b"TEST".to_vec(), "Asset name should be TEST");
    }

    // Verify post-execution asset balances in storage
    let owner_proto_after = storage_adapter.get_account_proto(&owner_address).unwrap().unwrap();
    let recipient_proto_after = storage_adapter.get_account_proto(&recipient_address).unwrap().unwrap();

    // Owner should have 900 units remaining (started with 1000, transferred 100)
    assert_eq!(*owner_proto_after.asset.get("TEST").unwrap_or(&0), 900,
        "Owner asset balance should decrease by transfer amount");
    assert_eq!(*owner_proto_after.asset_v2.get("1000009").unwrap_or(&0), 900,
        "Owner asset_v2 balance should decrease by transfer amount");

    // Recipient should have 100 units
    assert_eq!(*recipient_proto_after.asset.get("TEST").unwrap_or(&0), 100,
        "Recipient asset balance should equal transfer amount");
    assert_eq!(*recipient_proto_after.asset_v2.get("1000009").unwrap_or(&0), 100,
        "Recipient asset_v2 balance should equal transfer amount");
}
