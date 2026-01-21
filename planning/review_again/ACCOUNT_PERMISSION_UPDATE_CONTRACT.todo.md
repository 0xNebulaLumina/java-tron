# TODO / Fix Plan: `ACCOUNT_PERMISSION_UPDATE_CONTRACT` parity gaps

This checklist targets the gaps identified in `planning/review_again/ACCOUNT_PERMISSION_UPDATE_CONTRACT.planning.md`.

## 0) Decide parity target (do this first)

- [ ] Confirm desired scope:
  - [ ] **Actuator-only parity** (match `AccountPermissionUpdateActuator` + `AccountCapsule.updatePermissions`)
  - [ ] **End-to-end parity** (also match java-tron’s transactional rollback expectations, receipt fields, and dynamic-property side effects)
- [ ] Confirm write model expectations in remote mode:
  - [ ] Rust is authoritative and persists (then rollback/atomicity must be correct)
  - [ ] Rust computes and returns deltas, Java applies (then handlers should not directly write, and must return complete `state_changes`/sidecars)

## 1) Fix burn semantics under blackhole optimization

Goal: match java-tron `supportBlackHoleOptimization() ? burnTrx(fee) : credit blackhole`.

- [ ] Update `execute_account_permission_update_contract()` in `rust-backend/crates/core/src/service/mod.rs`:
  - [ ] When `support_blackhole_optimization == true`:
    - [ ] Call `storage_adapter.burn_trx(fee as u64)` (increment `BURN_TRX_AMOUNT`)
    - [ ] Keep “no blackhole credit” behavior
  - [ ] When `support_blackhole_optimization == false`:
    - [ ] Credit blackhole using `storage_adapter.get_blackhole_address()` (not hardcoded `get_blackhole_address_evm()`), consistent with other handlers
- [ ] Add Rust tests covering burn behavior:
  - [ ] Seed `ALLOW_BLACKHOLE_OPTIMIZATION = 1` + `BURN_TRX_AMOUNT = 0`; execute contract; assert `BURN_TRX_AMOUNT == fee`
  - [ ] Seed `ALLOW_BLACKHOLE_OPTIMIZATION = 0`; execute contract; assert blackhole account balance increased by `fee`

## 2) Fix atomicity / rollback behavior on execution failures

Problem: Rust currently writes permission updates before verifying fee payment; in direct-write mode this can leave persisted permission changes on failure.

Pick one approach (or do both; (A) is systemic, (B) is contract-local hardening):

### Option A (systemic): always buffer writes and commit on success

- [ ] In gRPC execution path (`rust-backend/crates/core/src/service/grpc/mod.rs`):
  - [ ] Create the storage adapter with a write buffer for contract execution unconditionally (or for all non-VM system contracts)
  - [ ] Commit the buffer only when execution succeeds; otherwise drop without commit
  - [ ] Ensure `touched_keys`/`write_mode` semantics remain consistent
- [ ] Add test(s) to confirm “no writes on failure” for AccountPermissionUpdate:
  - [ ] Build tx that fails due to insufficient balance
  - [ ] Assert account permissions remain unchanged in storage post-execution

### Option B (contract-local): reorder writes to avoid partial persistence

- [ ] In `execute_account_permission_update_contract()`:
  - [ ] Load fee and validate `fee >= 0` and `balance >= fee` **before** any `put_account_proto(...)`
  - [ ] Apply permission updates and fee deduction in-memory
  - [ ] Persist the account once (or at least ensure no persistence occurs before the balance check)
  - [ ] Apply fee routing (burn/blackhole) only after fee has been deducted
- [ ] Add unit test:
  - [ ] Set balance `< fee`; execute; assert:
    - [ ] returned error string matches Java (`"<ownerHex> insufficient balance, balance: ..., amount: ..."`),
    - [ ] stored permissions unchanged
    - [ ] stored balance unchanged

## 3) Align `AVAILABLE_CONTRACT_TYPE` handling with Java

Goal: Java always reads `AVAILABLE_CONTRACT_TYPE` and fails hard if missing; Rust currently treats missing/short as “allow all”.

- [ ] Decide desired behavior:
  - [ ] strict parity: treat missing/short as an error (preferred for correctness/conformance)
  - [ ] pragmatic fallback: keep allow-all (documented divergence)
- [ ] If strict parity:
  - [ ] Update `check_account_permission_update_permission()` in `rust-backend/crates/core/src/service/mod.rs`:
    - [ ] Remove `allow_all` fallback
    - [ ] Require `AVAILABLE_CONTRACT_TYPE` exists and is `>= 32` bytes
    - [ ] Return a clear error if missing (consider matching Java’s `"not found AVAILABLE_CONTRACT_TYPE"` behavior if that’s observable)
- [ ] Add unit test:
  - [ ] Configure `AVAILABLE_CONTRACT_TYPE` with a bit unset; set same bit in active permission `operations`; assert error `"<i> isn't a validate ContractType"`

## 4) Align dynamic property semantics (optional but closer parity)

Java uses strict `== 1` gates and throws when keys are missing.

- [ ] Boolean dynamic properties:
  - [ ] Update `EngineBackedEvmStateStore::get_allow_multi_sign()` to interpret `val == 1` (not `val != 0`)
  - [ ] Update `EngineBackedEvmStateStore::support_black_hole_optimization()` similarly (`== 1`)
- [ ] Missing-key behavior:
  - [ ] Decide which keys must exist for this contract:
    - [ ] `ALLOW_MULTI_SIGN`
    - [ ] `TOTAL_SIGN_NUM`
    - [ ] `AVAILABLE_CONTRACT_TYPE`
    - [ ] `UPDATE_ACCOUNT_PERMISSION_FEE`
  - [ ] If strict parity:
    - [ ] Change corresponding getters in `rust-backend/crates/execution/src/storage_adapter/engine.rs` to return errors when missing (instead of defaults)
    - [ ] Add tests for missing-key cases and confirm errors propagate up cleanly

## 5) Receipt parity (only if required)

Goal: ensure remote execution can reproduce `ret.setStatus(fee, SUCESS)`-equivalent receipt fields.

- [ ] Confirm whether `tron_transaction_result` is required/consumed by Java for this contract.
- [ ] If yes, add/verify:
  - [ ] `tron_transaction_result` contains the fee and success code fields in the expected encoding
  - [ ] tests verifying Java receipt equivalence (integration-level)

## 6) Verification

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] Run any existing conformance runner cases that cover contract type 46 (if available)
- [ ] Java (if behavior changes affect node integration):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.AccountPermissionUpdateActuatorTest"`
  - [ ] Dual-mode integration (if applicable): `./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"`

