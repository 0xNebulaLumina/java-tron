# TODO / Fix Plan: `WITHDRAW_EXPIRE_UNFREEZE_CONTRACT` parity gaps

This checklist assumes we want remote execution to match java-tron's `WithdrawExpireUnfreezeActuator` behavior, including fixture cases with malformed owner addresses.

## 0) Decide the parity target (do this first)

- [x] Confirm target:
  - [x] **Execution parity**: match actuator validation + state transitions + receipt fields
  - [x] **Strict fixture parity**: ensure conformance `validate_fail_*` fixtures fail with the same error strings as Java (even when tx fields are malformed)

## 1) Fix the conversion-layer mismatch (highest impact)

Goal: ensure malformed/empty `TronTransaction.from` bytes do **not** prevent contract-level validation from producing Java-equivalent errors for `WITHDRAW_EXPIRE_UNFREEZE_CONTRACT`.

Why: `execute_withdraw_expire_unfreeze_contract()` already parses `owner_address` from `contract_parameter.value`; it does not need `tx.from` to be parseable.

Checklist:

- [x] Update `rust-backend/crates/core/src/service/grpc/conversion.rs`:
  - [x] Add `Some(tron_backend_execution::TronContractType::WithdrawExpireUnfreezeContract)` to the `allow_malformed_from` match list in `convert_protobuf_transaction()`.
  - [x] (Recommended) Also added the other resource/delegation and freeze contracts:
    - `FreezeBalanceContract` (11), `UnfreezeBalanceContract` (12)
    - `FreezeBalanceV2Contract` (54), `UnfreezeBalanceV2Contract` (55)
    - `DelegateResourceContract` (57), `UndelegateResourceContract` (58)
  - [x] Ensure behavior is: malformed `from` → `from = Address::ZERO`, but `metadata.from_raw = Some(tx.from.clone())` is preserved.

## 2) Add/extend tests to lock parity in

- [x] Rust unit test (conversion-focused):
  - [x] `test_convert_protobuf_transaction_allows_empty_from_for_withdraw_expire_unfreeze`:
    - `tx_kind = NON_VM`
    - `contract_type = WITHDRAW_EXPIRE_UNFREEZE_CONTRACT`
    - `from = []` (empty)
    - `contract_parameter.type_url = "type.googleapis.com/protocol.WithdrawExpireUnfreezeContract"`
    - `contract_parameter.value = []` (matches proto3 default for empty bytes field)
  - [x] Assert `convert_protobuf_transaction()` succeeds and does not return the address-length error.
  - [x] Assert `contract_parameter` is preserved for contract-level validation.
- [x] `test_convert_protobuf_transaction_allows_empty_from_for_freeze_v2_family`:
  - [x] Tests all 6 freeze/resource contracts allow empty from addresses.
- [x] Rust integration/conformance check (behavior-focused):
  - [x] Ran conformance fixtures; `validate_fail_owner_address_invalid_empty` passes with error `"Invalid address"` (not a conversion error).

## 3) Verification

- [x] Rust:
  - [x] `cd rust-backend && cargo test --workspace` — 422 passed, 3 failed (pre-existing vote_witness failures), 3 ignored
  - [x] All 10 WITHDRAW_EXPIRE_UNFREEZE_CONTRACT conformance fixtures pass
  - [x] All conformance fixtures pass (`./scripts/ci/run_fixture_conformance.sh --rust-only`)
- [ ] Java oracle (optional, not in scope for Rust-side changes):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.WithdrawExpireUnfreezeActuatorTest"`
