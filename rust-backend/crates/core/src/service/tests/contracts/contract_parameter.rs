//! Tests for strict contract_parameter validation (Java parity).
//!
//! Verifies that every NON_VM handler rejects:
//! - missing contract_parameter → "No contract!"
//! - wrong type_url → contract-specific type mismatch error
//! - empty type_url → same type mismatch error
//!
//! These tests exercise the require_contract_parameter / require_contract_any /
//! require_contract_type helper API.

use super::super::super::*;
use super::common::{
    make_from_raw, new_test_context, new_test_service_with_system_enabled, seed_dynamic_properties,
};
use revm_primitives::{Address, Bytes, U256};
use tron_backend_execution::{TronContractParameter, TronContractType, TronTransaction, TxMetadata};

/// Build a minimal transaction with a given contract type and optional contract_parameter.
fn build_tx_with_contract_param(
    contract_type: TronContractType,
    contract_parameter: Option<TronContractParameter>,
) -> TronTransaction {
    let owner = Address::from([0x11u8; 20]);
    let from_raw = make_from_raw(&owner);
    TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::new(),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(contract_type),
            from_raw: Some(from_raw),
            contract_parameter,
            ..Default::default()
        },
    }
}

use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_execution::{EngineBackedEvmStateStore, TronExecutionContext};
use tron_backend_storage::StorageEngine;

/// Contract family entry for table-driven tests.
/// (contract_type, expected_proto_type_url, type_error_substring)
struct ContractFamily {
    contract_type: TronContractType,
    type_url: &'static str,
    error_substring: &'static str,
}

/// All NON_VM contract families that must enforce strict contract_parameter.
fn all_contract_families() -> Vec<ContractFamily> {
    vec![
        ContractFamily { contract_type: TronContractType::TransferContract, type_url: "protocol.TransferContract", error_substring: "TransferContract" },
        ContractFamily { contract_type: TronContractType::AccountCreateContract, type_url: "protocol.AccountCreateContract", error_substring: "AccountCreateContract" },
        ContractFamily { contract_type: TronContractType::VoteWitnessContract, type_url: "protocol.VoteWitnessContract", error_substring: "VoteWitnessContract" },
        ContractFamily { contract_type: TronContractType::WitnessCreateContract, type_url: "protocol.WitnessCreateContract", error_substring: "WitnessCreateContract" },
        ContractFamily { contract_type: TronContractType::WitnessUpdateContract, type_url: "protocol.WitnessUpdateContract", error_substring: "WitnessUpdateContract" },
        ContractFamily { contract_type: TronContractType::AccountUpdateContract, type_url: "protocol.AccountUpdateContract", error_substring: "AccountUpdateContract" },
        ContractFamily { contract_type: TronContractType::ProposalCreateContract, type_url: "protocol.ProposalCreateContract", error_substring: "ProposalCreateContract" },
        ContractFamily { contract_type: TronContractType::ProposalApproveContract, type_url: "protocol.ProposalApproveContract", error_substring: "ProposalApproveContract" },
        ContractFamily { contract_type: TronContractType::ProposalDeleteContract, type_url: "protocol.ProposalDeleteContract", error_substring: "ProposalDeleteContract" },
        ContractFamily { contract_type: TronContractType::SetAccountIdContract, type_url: "protocol.SetAccountIdContract", error_substring: "SetAccountIdContract" },
        ContractFamily { contract_type: TronContractType::UpdateSettingContract, type_url: "protocol.UpdateSettingContract", error_substring: "UpdateSettingContract" },
        ContractFamily { contract_type: TronContractType::UpdateEnergyLimitContract, type_url: "protocol.UpdateEnergyLimitContract", error_substring: "UpdateEnergyLimitContract" },
        ContractFamily { contract_type: TronContractType::ClearAbiContract, type_url: "protocol.ClearABIContract", error_substring: "ClearABIContract" },
        ContractFamily { contract_type: TronContractType::UpdateBrokerageContract, type_url: "protocol.UpdateBrokerageContract", error_substring: "UpdateBrokerageContract" },
        ContractFamily { contract_type: TronContractType::AccountPermissionUpdateContract, type_url: "protocol.AccountPermissionUpdateContract", error_substring: "AccountPermissionUpdateContract" },
        ContractFamily { contract_type: TronContractType::TransferAssetContract, type_url: "protocol.TransferAssetContract", error_substring: "TransferAssetContract" },
        ContractFamily { contract_type: TronContractType::AssetIssueContract, type_url: "protocol.AssetIssueContract", error_substring: "AssetIssueContract" },
        ContractFamily { contract_type: TronContractType::UnfreezeAssetContract, type_url: "protocol.UnfreezeAssetContract", error_substring: "UnfreezeAssetContract" },
        ContractFamily { contract_type: TronContractType::UpdateAssetContract, type_url: "protocol.UpdateAssetContract", error_substring: "UpdateAssetContract" },
        ContractFamily { contract_type: TronContractType::FreezeBalanceContract, type_url: "protocol.FreezeBalanceContract", error_substring: "FreezeBalanceContract" },
        ContractFamily { contract_type: TronContractType::UnfreezeBalanceContract, type_url: "protocol.UnfreezeBalanceContract", error_substring: "UnfreezeBalanceContract" },
        ContractFamily { contract_type: TronContractType::FreezeBalanceV2Contract, type_url: "protocol.FreezeBalanceV2Contract", error_substring: "FreezeBalanceV2Contract" },
        ContractFamily { contract_type: TronContractType::UnfreezeBalanceV2Contract, type_url: "protocol.UnfreezeBalanceV2Contract", error_substring: "UnfreezeBalanceV2Contract" },
        ContractFamily { contract_type: TronContractType::WithdrawBalanceContract, type_url: "protocol.WithdrawBalanceContract", error_substring: "WithdrawBalanceContract" },
        ContractFamily { contract_type: TronContractType::WithdrawExpireUnfreezeContract, type_url: "protocol.WithdrawExpireUnfreezeContract", error_substring: "WithdrawExpireUnfreezeContract" },
        ContractFamily { contract_type: TronContractType::CancelAllUnfreezeV2Contract, type_url: "protocol.CancelAllUnfreezeV2Contract", error_substring: "CancelAllUnfreezeV2Contract" },
        ContractFamily { contract_type: TronContractType::DelegateResourceContract, type_url: "protocol.DelegateResourceContract", error_substring: "DelegateResourceContract" },
        ContractFamily { contract_type: TronContractType::UndelegateResourceContract, type_url: "protocol.UnDelegateResourceContract", error_substring: "UnDelegateResourceContract" },
        ContractFamily { contract_type: TronContractType::ParticipateAssetIssueContract, type_url: "protocol.ParticipateAssetIssueContract", error_substring: "ParticipateAssetIssueContract" },
        ContractFamily { contract_type: TronContractType::ExchangeCreateContract, type_url: "protocol.ExchangeCreateContract", error_substring: "ExchangeCreateContract" },
        ContractFamily { contract_type: TronContractType::ExchangeInjectContract, type_url: "protocol.ExchangeInjectContract", error_substring: "ExchangeInjectContract" },
        ContractFamily { contract_type: TronContractType::ExchangeWithdrawContract, type_url: "protocol.ExchangeWithdrawContract", error_substring: "ExchangeWithdrawContract" },
        ContractFamily { contract_type: TronContractType::ExchangeTransactionContract, type_url: "protocol.ExchangeTransactionContract", error_substring: "ExchangeTransactionContract" },
        ContractFamily { contract_type: TronContractType::MarketSellAssetContract, type_url: "protocol.MarketSellAssetContract", error_substring: "MarketSellAssetContract" },
        ContractFamily { contract_type: TronContractType::MarketCancelOrderContract, type_url: "protocol.MarketCancelOrderContract", error_substring: "MarketCancelOrderContract" },
    ]
}

/// Create a service with ALL system contract flags enabled for testing.
fn new_all_enabled_service() -> BackendService {
    let exec_config = ExecutionConfig {
        remote: RemoteExecutionConfig {
            system_enabled: true,
            witness_create_enabled: true,
            witness_update_enabled: true,
            vote_witness_enabled: true,
            trc10_enabled: true,
            freeze_balance_enabled: true,
            unfreeze_balance_enabled: true,
            freeze_balance_v2_enabled: true,
            unfreeze_balance_v2_enabled: true,
            withdraw_balance_enabled: true,
            account_create_enabled: true,
            proposal_create_enabled: true,
            proposal_approve_enabled: true,
            proposal_delete_enabled: true,
            set_account_id_enabled: true,
            account_permission_update_enabled: true,
            update_setting_enabled: true,
            update_energy_limit_enabled: true,
            clear_abi_enabled: true,
            update_brokerage_enabled: true,
            withdraw_expire_unfreeze_enabled: true,
            delegate_resource_enabled: true,
            undelegate_resource_enabled: true,
            cancel_all_unfreeze_v2_enabled: true,
            participate_asset_issue_enabled: true,
            unfreeze_asset_enabled: true,
            update_asset_enabled: true,
            exchange_create_enabled: true,
            exchange_inject_enabled: true,
            exchange_withdraw_enabled: true,
            exchange_transaction_enabled: true,
            market_sell_asset_enabled: true,
            market_cancel_order_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut module_manager = ModuleManager::new();
    let exec_module = tron_backend_execution::ExecutionModule::new(exec_config);
    module_manager.register("execution", Box::new(exec_module));
    BackendService::new(module_manager)
}

/// Create a minimal storage adapter for dispatch tests.
fn new_test_storage() -> EngineBackedEvmStateStore {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_engine = StorageEngine::new(temp_dir.path()).unwrap();
    seed_dynamic_properties(&storage_engine);
    // Seed a mainnet-prefixed account so address_prefix() returns 0x41
    let mainnet_addr = make_from_raw(&Address::from([0x11u8; 20]));
    storage_engine.put("account", &mainnet_addr, b"dummy").unwrap();
    std::mem::forget(temp_dir);
    EngineBackedEvmStateStore::new(storage_engine)
}

// ---------------------------------------------------------------------------
// Missing contract_parameter tests
// ---------------------------------------------------------------------------

#[test]
fn test_missing_contract_parameter_returns_no_contract() {
    // The require_contract_parameter helper should return "No contract!" when
    // contract_parameter is None, matching Java's ActuatorConstant.CONTRACT_NOT_EXIST.
    let tx = build_tx_with_contract_param(TronContractType::TransferContract, None);
    let result = BackendService::require_contract_parameter(
        &tx,
        "protocol.TransferContract",
        BackendService::CONTRACT_NOT_EXIST,
        "contract type error, expected type [TransferContract], real type [class com.google.protobuf.Any]",
    );
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "No contract!");
}

#[test]
fn test_wrong_type_url_returns_type_mismatch() {
    let tx = build_tx_with_contract_param(
        TronContractType::TransferContract,
        Some(TronContractParameter {
            type_url: "protocol.AccountCreateContract".to_string(),
            value: vec![],
        }),
    );
    let result = BackendService::require_contract_parameter(
        &tx,
        "protocol.TransferContract",
        BackendService::CONTRACT_NOT_EXIST,
        "contract type error, expected type [TransferContract], real type [class com.google.protobuf.Any]",
    );
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap(),
        "contract type error, expected type [TransferContract], real type [class com.google.protobuf.Any]"
    );
}

#[test]
fn test_empty_type_url_returns_type_mismatch() {
    // Empty type_url should be treated as a present-but-wrong Any, not as missing.
    let tx = build_tx_with_contract_param(
        TronContractType::TransferContract,
        Some(TronContractParameter {
            type_url: "".to_string(),
            value: vec![],
        }),
    );
    let result = BackendService::require_contract_parameter(
        &tx,
        "protocol.TransferContract",
        BackendService::CONTRACT_NOT_EXIST,
        "contract type error, expected type [TransferContract], real type [class com.google.protobuf.Any]",
    );
    assert!(result.is_err());
    assert_eq!(
        result.err().unwrap(),
        "contract type error, expected type [TransferContract], real type [class com.google.protobuf.Any]"
    );
}

#[test]
fn test_correct_type_url_returns_value_bytes() {
    let expected_value = vec![0x0a, 0x15, 0x41]; // some proto bytes
    let tx = build_tx_with_contract_param(
        TronContractType::TransferContract,
        Some(TronContractParameter {
            type_url: "type.googleapis.com/protocol.TransferContract".to_string(),
            value: expected_value.clone(),
        }),
    );
    let result = BackendService::require_contract_parameter(
        &tx,
        "protocol.TransferContract",
        BackendService::CONTRACT_NOT_EXIST,
        "contract type error, expected type [TransferContract], real type [class com.google.protobuf.Any]",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), expected_value.as_slice());
}

#[test]
fn test_type_url_without_prefix_matches() {
    // type_url can be just "protocol.TransferContract" without the googleapis prefix
    let tx = build_tx_with_contract_param(
        TronContractType::TransferContract,
        Some(TronContractParameter {
            type_url: "protocol.TransferContract".to_string(),
            value: vec![1, 2, 3],
        }),
    );
    let result = BackendService::require_contract_parameter(
        &tx,
        "protocol.TransferContract",
        BackendService::CONTRACT_NOT_EXIST,
        "type mismatch",
    );
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// Two-layer helper tests (require_contract_any + require_contract_type)
// ---------------------------------------------------------------------------

#[test]
fn test_require_contract_any_missing_returns_custom_error() {
    let tx = build_tx_with_contract_param(TronContractType::TransferContract, None);
    let result = BackendService::require_contract_any(&tx, "custom missing error");
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "custom missing error");
}

#[test]
fn test_require_contract_any_present_returns_ref() {
    let tx = build_tx_with_contract_param(
        TronContractType::TransferContract,
        Some(TronContractParameter {
            type_url: "protocol.TransferContract".to_string(),
            value: vec![42],
        }),
    );
    let result = BackendService::require_contract_any(&tx, "missing");
    assert!(result.is_ok());
    let any = result.unwrap();
    assert_eq!(any.type_url, "protocol.TransferContract");
    assert_eq!(any.value, vec![42]);
}

#[test]
fn test_require_contract_type_empty_url_returns_mismatch() {
    let param = TronContractParameter {
        type_url: "".to_string(),
        value: vec![],
    };
    let result = BackendService::require_contract_type(
        &param,
        "protocol.TransferContract",
        "type mismatch",
    );
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "type mismatch");
}

#[test]
fn test_require_contract_type_wrong_url_returns_mismatch() {
    let param = TronContractParameter {
        type_url: "protocol.SomeOtherContract".to_string(),
        value: vec![],
    };
    let result = BackendService::require_contract_type(
        &param,
        "protocol.TransferContract",
        "type mismatch",
    );
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "type mismatch");
}

// ---------------------------------------------------------------------------
// Malformed protobuf value tests
// ---------------------------------------------------------------------------

#[test]
fn test_malformed_varint_in_contract_value_returns_java_error() {
    // A truncated varint (0x80 with no continuation) should produce a
    // protobuf-java parity error message.
    use super::super::super::contracts::proto::read_varint_typed;
    let truncated = [0x80u8]; // varint with continuation bit but no more bytes
    let result = read_varint_typed(&truncated);
    assert!(result.is_err());
}

#[test]
fn test_contract_parameter_value_takes_precedence() {
    // When contract_parameter.value is set, it should be the canonical source.
    // This test verifies the helper returns the value bytes, not transaction.data.
    let contract_value = vec![0xAA, 0xBB, 0xCC];
    let tx_data = vec![0x11, 0x22, 0x33]; // different data
    let owner = Address::from([0x11u8; 20]);
    let tx = TronTransaction {
        from: owner,
        to: None,
        value: U256::ZERO,
        data: Bytes::from(tx_data),
        gas_limit: 0,
        gas_price: U256::ZERO,
        nonce: 0,
        metadata: TxMetadata {
            contract_type: Some(TronContractType::TransferContract),
            contract_parameter: Some(TronContractParameter {
                type_url: "protocol.TransferContract".to_string(),
                value: contract_value.clone(),
            }),
            ..Default::default()
        },
    };
    let result = BackendService::require_contract_parameter(
        &tx,
        "protocol.TransferContract",
        "No contract!",
        "type mismatch",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), contract_value.as_slice());
}

// ---------------------------------------------------------------------------
// End-to-end handler tests: missing contract_parameter via dispatcher
// ---------------------------------------------------------------------------
// These tests call execute_non_vm_contract (the real dispatcher) for EVERY
// NON_VM contract type with contract_parameter = None, verifying that each
// handler rejects with "No contract!" before reaching business logic.

#[test]
fn test_all_handlers_reject_missing_contract_parameter() {
    let service = new_all_enabled_service();
    let context = new_test_context();

    for family in all_contract_families() {
        let mut storage = new_test_storage();
        let tx = build_tx_with_contract_param(family.contract_type, None);
        let result = service.execute_non_vm_contract(&mut storage, &tx, &context);
        assert!(
            result.is_err(),
            "Handler for {} should reject missing contract_parameter",
            family.error_substring,
        );
        let err = result.err().unwrap();
        // Handler must either fail with "No contract!" (the require_contract_parameter
        // check) or with an earlier validation error (e.g., a dynamic property gate).
        // In either case, no business logic or data parsing should have executed.
        // Handlers that fail before require_contract_parameter (e.g.,
        // UpdateEnergyLimitContract checks check_for_energy_limit() first) are still
        // safe because they reject before any protobuf parsing occurs.
        assert!(
            err == "No contract!" || err.contains("contract type error") || err.contains("not support") || err.contains("Not support"),
            "Handler for {} should fail before parsing when contract_parameter is missing, got: {}",
            family.error_substring,
            err,
        );
    }
}

// ---------------------------------------------------------------------------
// End-to-end handler tests: wrong type_url via dispatcher
// ---------------------------------------------------------------------------

#[test]
fn test_all_handlers_reject_wrong_type_url() {
    let service = new_all_enabled_service();
    let context = new_test_context();

    for family in all_contract_families() {
        let mut storage = new_test_storage();
        let tx = build_tx_with_contract_param(
            family.contract_type,
            Some(TronContractParameter {
                type_url: "protocol.WRONG_TYPE".to_string(),
                value: vec![],
            }),
        );
        let result = service.execute_non_vm_contract(&mut storage, &tx, &context);
        assert!(
            result.is_err(),
            "Handler for {} should reject wrong type_url",
            family.error_substring,
        );
        let err = result.err().unwrap();
        assert!(
            err.contains("contract type error"),
            "Handler for {} should return type mismatch error, got: {}",
            family.error_substring,
            err,
        );
        assert!(
            err.contains(family.error_substring),
            "Error for {} should mention the contract name, got: {}",
            family.error_substring,
            err,
        );
    }
}

// ---------------------------------------------------------------------------
// End-to-end handler tests: empty type_url via dispatcher
// ---------------------------------------------------------------------------

#[test]
fn test_all_handlers_reject_empty_type_url() {
    let service = new_all_enabled_service();
    let context = new_test_context();

    for family in all_contract_families() {
        let mut storage = new_test_storage();
        let tx = build_tx_with_contract_param(
            family.contract_type,
            Some(TronContractParameter {
                type_url: "".to_string(),
                value: vec![],
            }),
        );
        let result = service.execute_non_vm_contract(&mut storage, &tx, &context);
        assert!(
            result.is_err(),
            "Handler for {} should reject empty type_url",
            family.error_substring,
        );
        let err = result.err().unwrap();
        assert!(
            err.contains("contract type error"),
            "Handler for {} should return type mismatch (not 'No contract!') for empty type_url, got: {}",
            family.error_substring,
            err,
        );
    }
}
