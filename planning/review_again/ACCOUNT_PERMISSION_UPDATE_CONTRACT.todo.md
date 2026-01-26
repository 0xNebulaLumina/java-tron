# TODO / Fix Plan: `ACCOUNT_PERMISSION_UPDATE_CONTRACT` parity gaps

This checklist targets the gaps identified in `planning/review_again/ACCOUNT_PERMISSION_UPDATE_CONTRACT.planning.md`.

## 0) Decide parity target (do this first)

- [x] Confirm desired scope:
  - [x] **Actuator-only parity** (match `AccountPermissionUpdateActuator` + `AccountCapsule.updatePermissions`)
  - [x] **End-to-end parity** (also match java-tron's transactional rollback expectations, receipt fields, and dynamic-property side effects)
- [x] Confirm write model expectations in remote mode:
  - [x] Rust is authoritative and persists (then rollback/atomicity must be correct)
  - [ ] Rust computes and returns deltas, Java applies (then handlers should not directly write, and must return complete `state_changes`/sidecars)

## 1) Fix burn semantics under blackhole optimization

Goal: match java-tron `supportBlackHoleOptimization() ? burnTrx(fee) : credit blackhole`.

- [x] Update `execute_account_permission_update_contract()` in `rust-backend/crates/core/src/service/mod.rs`:
  - [x] When `support_blackhole_optimization == true`:
    - [x] Call `storage_adapter.burn_trx(fee as u64)` (increment `BURN_TRX_AMOUNT`)
    - [x] Keep "no blackhole credit" behavior
  - [x] When `support_blackhole_optimization == false`:
    - [x] Credit blackhole using `storage_adapter.get_blackhole_address()` (not hardcoded `get_blackhole_address_evm()`), consistent with other handlers
- [x] Add Rust tests covering burn behavior (`rust-backend/crates/core/src/service/tests/contracts.rs`):
  - [x] `test_account_permission_update_burn_trx_when_blackhole_optimization_enabled`: Seeds `ALLOW_BLACKHOLE_OPTIMIZATION = 1` + `BURN_TRX_AMOUNT = 0`; executes contract; asserts `BURN_TRX_AMOUNT == fee`
  - [x] `test_account_permission_update_credit_blackhole_when_optimization_disabled`: Seeds `ALLOW_BLACKHOLE_OPTIMIZATION = 0`; executes contract; asserts blackhole account balance increased by `fee`

## 2) Fix atomicity / rollback behavior on execution failures

Problem: Rust currently writes permission updates before verifying fee payment; in direct-write mode this can leave persisted permission changes on failure.

**Decision: Option A (systemic buffered writes) has been implemented and is the active approach.**

### Option A (systemic): always buffer writes and commit on success ✅ IMPLEMENTED

- [x] In gRPC execution path (`rust-backend/crates/core/src/service/grpc/mod.rs`):
  - [x] Create the storage adapter with a write buffer for all non-VM system contracts (line 1059: `matches!(tx_kind, crate::backend::TxKind::NonVm)`)
  - [x] Commit the buffer only when execution succeeds; otherwise drop without commit (lines 1226-1260)
  - [x] Ensure `touched_keys`/`write_mode` semantics remain consistent
- [x] `ExecutionWriteBuffer` implementation in `rust-backend/crates/execution/src/storage_adapter/write_buffer.rs`:
  - [x] Accumulates all puts/deletes in memory during execution
  - [x] `commit()` method writes all operations atomically per database
  - [x] Dropping buffer without commit discards all pending writes
- [x] Conformance runner uses buffered writes (`rust-backend/crates/core/src/conformance/runner.rs:665`)
- [x] Unit tests for write buffer atomicity:
  - [x] `test_write_buffer_not_committed_on_failure` (runner.rs:1348)
  - [x] `test_touched_keys_tracking` (runner.rs:1373)
  - [x] `test_write_buffer_overwrites` (runner.rs:1404)
  - [x] `test_write_buffer_put_then_delete` (runner.rs:1429)
- [x] Add tests for atomicity behavior (`rust-backend/crates/core/src/service/tests/contracts.rs`):
  - [x] `test_account_permission_update_insufficient_balance_error_message`: Verifies error message format matches Java's BalanceInsufficientException
  - [x] `test_account_permission_update_atomicity_with_write_buffer`: Demonstrates ExecutionWriteBuffer mechanism that provides atomicity in gRPC path
  - Note: Full integration test requires gRPC buffered path; unit tests document the mechanism

### Option B (contract-local): reorder writes to avoid partial persistence *(not implemented - unnecessary due to Option A)*

> **Note:** Option B was NOT implemented. The current code in `execute_account_permission_update_contract()` still writes permissions (line 3715) BEFORE the balance check (line 3721). However, this is safe because Option A's systemic buffer approach guarantees atomicity - all writes go to the buffer, and when the balance check fails with `Err(...)`, the gRPC handler drops the buffer without committing.

- [ ] ~~In `execute_account_permission_update_contract()`:~~
  - [ ] ~~Load fee and validate `fee >= 0` and `balance >= fee` **before** any `put_account_proto(...)`~~
  - [x] Apply permission updates and fee deduction in-memory *(partially - permissions updated in-memory before put)*
  - [ ] ~~Persist the account once (or at least ensure no persistence occurs before the balance check)~~
  - [x] Apply fee routing (burn/blackhole) only after fee has been deducted
- [x] Add unit test for error message parity:
  - [x] `test_account_permission_update_insufficient_balance_error_message`: Sets balance `< fee`; executes; asserts:
    - [x] Returned error string matches Java format (`"<ownerHex> insufficient balance, balance: ..., amount: ..."`)
    - Note: Stored state unchanged is guaranteed by Option A's systemic buffer in gRPC path

## 3) Align `AVAILABLE_CONTRACT_TYPE` handling with Java

Goal: Java always reads `AVAILABLE_CONTRACT_TYPE` and fails hard if missing; Rust currently treats missing/short as "allow all".

- [x] Decide desired behavior:
  - [x] strict parity: treat missing/short as an error (preferred for correctness/conformance)
  - [ ] pragmatic fallback: keep allow-all (documented divergence)
- [x] If strict parity:
  - [x] Update `check_account_permission_update_permission()` in `rust-backend/crates/core/src/service/mod.rs`:
    - [x] Remove `allow_all` fallback
    - [x] Require `AVAILABLE_CONTRACT_TYPE` exists and is `>= 32` bytes
    - [x] Return a clear error if missing (consider matching Java's `"not found AVAILABLE_CONTRACT_TYPE"` behavior if that's observable)
- [x] Add unit tests (`rust-backend/crates/core/src/service/tests/contracts.rs`):
  - [x] `test_account_permission_update_invalid_contract_type_in_operations`: Configures `AVAILABLE_CONTRACT_TYPE` with bit 0 unset; sets bit 0 in active permission `operations`; asserts error `"0 isn't a validate ContractType"`
  - [x] `test_account_permission_update_missing_available_contract_type`: Tests error when `AVAILABLE_CONTRACT_TYPE` is missing from dynamic properties
  - [x] `test_account_permission_update_available_contract_type_too_short`: Tests error when `AVAILABLE_CONTRACT_TYPE` is < 32 bytes

## 4) Align dynamic property semantics (optional but closer parity)

Java uses strict `== 1` gates and throws when keys are missing.

- [x] Boolean dynamic properties:
  - [x] Update `EngineBackedEvmStateStore::get_allow_multi_sign()` to interpret `val == 1` (not `val != 0`)
  - [x] Update `EngineBackedEvmStateStore::support_black_hole_optimization()` similarly (`== 1`)
- [x] Missing-key behavior:
  - [x] Decide which keys must exist for this contract:
    - [x] `ALLOW_MULTI_SIGN` - must exist (Java throws `IllegalArgumentException`)
    - [x] `TOTAL_SIGN_NUM` - must exist (Java throws `IllegalArgumentException`)
    - [x] `AVAILABLE_CONTRACT_TYPE` - must exist (Java throws `IllegalArgumentException`)
    - [x] `UPDATE_ACCOUNT_PERMISSION_FEE` - must exist (Java throws `IllegalArgumentException`)
  - [x] If strict parity:
    - [x] Change corresponding getters in `rust-backend/crates/execution/src/storage_adapter/engine.rs` to return errors when missing (instead of defaults)
    - [x] Add tests for missing-key cases and confirm errors propagate up cleanly:
      - [x] `test_account_permission_update_missing_allow_multi_sign`
      - [x] `test_account_permission_update_missing_total_sign_num`
      - [x] `test_account_permission_update_missing_update_account_permission_fee`
      - [x] `test_account_permission_update_missing_available_contract_type` (already existed)

## 5) Receipt parity (only if required)

Goal: ensure remote execution can reproduce `ret.setStatus(fee, SUCESS)`-equivalent receipt fields.

- [x] Confirm whether `tron_transaction_result` is required/consumed by Java for this contract.
- [x] If yes, add/verify:
  - [x] `tron_transaction_result` contains the fee and success code fields in the expected encoding
  - [ ] tests verifying Java receipt equivalence (integration-level)

## 6) Verification

- [x] Rust:
  - [x] `cd rust-backend && cargo test`
  - [ ] Run any existing conformance runner cases that cover contract type 46 (if available)
- [ ] Java (if behavior changes affect node integration):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.AccountPermissionUpdateActuatorTest"`
  - [ ] Dual-mode integration (if applicable): `./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"`

