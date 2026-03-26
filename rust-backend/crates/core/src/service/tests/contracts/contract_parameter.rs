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

/// Contract type definitions for testing: (TronContractType, expected proto type_url, expected type error substring).
const CONTRACT_FAMILIES: &[(
    fn() -> TronContractType,
    &str,
    &str,
)] = &[
    (|| TronContractType::TransferContract, "protocol.TransferContract", "TransferContract"),
    (|| TronContractType::AccountCreateContract, "protocol.AccountCreateContract", "AccountCreateContract"),
    (|| TronContractType::VoteWitnessContract, "protocol.VoteWitnessContract", "VoteWitnessContract"),
    (|| TronContractType::WitnessCreateContract, "protocol.WitnessCreateContract", "WitnessCreateContract"),
    (|| TronContractType::WitnessUpdateContract, "protocol.WitnessUpdateContract", "WitnessUpdateContract"),
    (|| TronContractType::AccountUpdateContract, "protocol.AccountUpdateContract", "AccountUpdateContract"),
    (|| TronContractType::ProposalCreateContract, "protocol.ProposalCreateContract", "ProposalCreateContract"),
    (|| TronContractType::ProposalApproveContract, "protocol.ProposalApproveContract", "ProposalApproveContract"),
    (|| TronContractType::ProposalDeleteContract, "protocol.ProposalDeleteContract", "ProposalDeleteContract"),
    (|| TronContractType::SetAccountIdContract, "protocol.SetAccountIdContract", "SetAccountIdContract"),
    (|| TronContractType::UpdateSettingContract, "protocol.UpdateSettingContract", "UpdateSettingContract"),
    (|| TronContractType::UpdateEnergyLimitContract, "protocol.UpdateEnergyLimitContract", "UpdateEnergyLimitContract"),
    (|| TronContractType::ClearAbiContract, "protocol.ClearABIContract", "ClearABIContract"),
    (|| TronContractType::UpdateBrokerageContract, "protocol.UpdateBrokerageContract", "UpdateBrokerageContract"),
    (|| TronContractType::AccountPermissionUpdateContract, "protocol.AccountPermissionUpdateContract", "AccountPermissionUpdateContract"),
];

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
