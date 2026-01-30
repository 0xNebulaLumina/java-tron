# TODO / Fix Plan: `CANCEL_ALL_UNFREEZE_V2_CONTRACT` parity gaps

This checklist assumes we want Rust `CANCEL_ALL_UNFREEZE_V2_CONTRACT` to match java-tron's actuator behavior **including receipt encoding**.

## 0) Decide the parity target (do this first)

- [x] Confirm target:
  - [x] **State-only parity** (balances/weights/unfrozenV2 mutations match)
  - [x] **State + receipt parity** (match `Transaction.Result` fields/encoding used by java-tron)
  - [x] **Strict oracle parity** (also match conformance `expected/result.pb` bytes where feasible)
- [x] Confirm whether `expected/result.pb` will be enforced by the conformance runner (currently it is not compared).

## 1) Fix receipt encoding parity (highest impact)

Goal: match java-tron's `Transaction.Result` behavior:

- field 27 `withdraw_expire_amount` should be emitted only when non-zero
- field 28 map `cancel_unfreezeV2_amount` should contain **all 3 keys**:
  - `"BANDWIDTH"`, `"ENERGY"`, `"TRON_POWER"`, even when the value is `0`

Checklist:

- [x] Update `TransactionResultBuilder::with_cancel_unfreeze_v2_amounts` in `rust-backend/crates/core/src/service/contracts/proto.rs`:
  - [x] Always include entries for all three keys (remove the `> 0` filters).
  - [x] Consider matching Java fixture serialization order:
    - observed in `conformance/fixtures/cancel_all_unfreeze_v2_contract/*/expected/result.pb` as:
      - `ENERGY`, then `TRON_POWER`, then `BANDWIDTH`
    - (this appears consistent with Java `HashMap` bucket iteration order for these strings).
- [x] Update `execute_cancel_all_unfreeze_v2_contract()` in `rust-backend/crates/core/src/service/mod.rs`:
  - [x] Only call `.with_withdraw_expire_amount(x)` when `x > 0`, or change the builder to treat `0` as "unset".
- [x] Add Rust tests (receipt-focused):
  - [x] Build a receipt with `withdraw_expire_amount = 0` and confirm field 27 is absent after decode.
  - [x] Build a receipt with cancel amounts where only one resource is non-zero; confirm decode map contains all 3 keys.
  - [x] (Optional strict) Ensure encoded bytes match fixture `expected/result.pb` for at least one case.

## 2) Align owner-address parsing with Java (correctness/robustness)

Goal: Java uses `owner_address` from the protobuf contract; Rust should, too.

- [x] Parse `owner_address` from `transaction.metadata.contract_parameter.value` (CancelAllUnfreezeV2Contract field 1):
  - [x] Use the same lightweight protobuf field parsing approach used by `execute_withdraw_expire_unfreeze_contract()`.
  - [x] Validate address validity using the decoded owner bytes.
  - [x] Use the decoded owner for account lookups/state updates.
- [x] Decide whether to enforce `decoded_owner == transaction.metadata.from_raw` (if both are present):
  - Note: Not enforced - parsing from protobuf is the source of truth like Java.
- [x] Add tests/fixtures:
  - [x] Edge-case tests for invalid owner_address (12 tests added in `cancel_all_unfreeze_v2.rs`):
    - `test_cancel_all_unfreeze_v2_rejects_missing_contract_parameter`
    - `test_cancel_all_unfreeze_v2_rejects_wrong_type_url`
    - `test_cancel_all_unfreeze_v2_rejects_empty_owner_address`
    - `test_cancel_all_unfreeze_v2_rejects_20_byte_owner_address`
    - `test_cancel_all_unfreeze_v2_rejects_wrong_prefix`
    - `test_cancel_all_unfreeze_v2_rejects_22_byte_owner_address`
    - `test_cancel_all_unfreeze_v2_rejects_malformed_protobuf`
    - `test_cancel_all_unfreeze_v2_rejects_nonexistent_account`
    - `test_cancel_all_unfreeze_v2_rejects_empty_unfrozen_list`
    - `test_cancel_all_unfreeze_v2_rejects_when_feature_disabled`
    - `test_cancel_all_unfreeze_v2_happy_path_with_valid_proto`
    - `test_cancel_all_unfreeze_v2_proto_owner_takes_precedence`

## 3) Verification

- [x] Rust:
  - [x] `cd rust-backend && cargo test -p tron-backend-core cancel_unfreeze` (5 passed - receipt encoding)
  - [x] `cd rust-backend && cargo test -p tron-backend-core proto` (20 passed)
  - [x] `cd rust-backend && cargo test -p tron-backend-core cancel_all_unfreeze_v2` (12 passed - edge cases)
  - [ ] (Optional) run conformance fixtures for CancelAllUnfreezeV2 and ensure DB diffs remain clean:
    - `cd rust-backend && cargo test -- --ignored test_run_real_fixtures` (or use the repo's fixture runner if available)
- [ ] Java (optional reference):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.CancelAllUnfreezeV2ActuatorTest"`

