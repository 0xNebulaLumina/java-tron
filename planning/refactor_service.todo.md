# Refactor Plan: Split `rust-backend/crates/core/src/service.rs`

This document describes a no‑behavior‑change refactor to reorganize `service.rs` into smaller, focused modules. It includes explicit constraints, the target module layout, detailed step‑by‑step TODOs, function‑to‑module mapping, verification steps, and risk mitigations.

## Ground Rules (Must Not Change)
- No logic/behavior changes. All code moves are mechanical; messages, ordering, and data remain identical.
- Do not delete or modify existing tests’ logic. Service‑internal tests are moved verbatim; framework tests stay untouched.
- Public API of the crate remains the same: `pub use service::BackendService;` and how the binary imports it.
- Tonic gRPC interface, protobuf types, and wire behavior remain unchanged.
- Keep constants and enums values identical.
- Preserve logging strings (content and levels) to avoid golden regressions.

## Current File Responsibilities (Summary)
- Service wiring and module getters (`get_storage_module/engine`, `get_execution_module`, uptime/metadata).
- Tonic service impl for storage, admin, metrics, and execution RPCs.
- Conversion helpers between protobuf and execution types.
- Address/AEXT/bandwidth helpers and varint reader.
- Fee post‑processing (`blackhole`/`burn`).
- Non‑VM contract dispatch and handlers (transfer, account update, witness create/update, vote witness, freeze/unfreeze v1/v2) including protobuf parsing for params.
- Embedded tests for helpers and handlers.

## Target Module Structure

Convert `service.rs` into a directory module with thin wrappers to preserve method names and test visibility.

- `crates/core/src/service/`
  - `mod.rs` (keeps `BackendService`, module getters, tonic trait impl delegating to `rpc::*`, and wrapper inherent methods)
  - `types.rs` (Freeze/Unfreeze params and enums; constants `MAX_VOTE_NUMBER`, `TRX_PRECISION`)
  - `fees.rs` (`apply_fee_post_processing`)
  - `convert.rs` (`convert_protobuf_transaction`, `convert_protobuf_context`, `convert_execution_result`)
  - `utils/`
    - `address.rs` (`strip_tron_address_prefix`, `add_tron_address_prefix`)
    - `varint.rs` (`read_varint`)
    - `bandwidth.rs` (`calculate_bandwidth_usage`)
    - `aext.rs` (`parse_pre_execution_aext`)
  - `handlers/`
    - `mod.rs` (re‑exports, shared glue)
    - `non_vm.rs` (`execute_non_vm_contract` dispatch)
    - `transfer.rs` (`execute_transfer_contract`)
    - `account_update.rs` (`execute_account_update_contract`)
    - `witness.rs` (`execute_witness_create_contract`, `execute_witness_update_contract`)
    - `vote.rs` (`parse_vote_witness_contract`, `parse_vote`, `execute_vote_witness_contract`)
    - `freeze_v1.rs` (`parse_freeze_balance_params`, `parse_unfreeze_balance_params`, `execute_freeze_balance_contract`, `execute_unfreeze_balance_contract`)
    - `freeze_v2.rs` (`parse_freeze_balance_v2_params`, `parse_unfreeze_balance_v2_params`, `execute_freeze_balance_v2_contract`, `execute_unfreeze_balance_v2_contract`)
  - `rpc/`
    - `health.rs` (`health`, `get_metadata`)
    - `storage.rs` (`get`, `put`, `delete`, `has`, `batch_get`, `batch_write`, `iterator`, `get_keys_next`, `get_values_next`)
    - `storage_admin.rs` (`list_databases`, `get_stats`, `stream_metrics`, `init_db`, `close_db`, `reset_db`, `size`, `is_empty`, `is_alive`)
    - `execution.rs` (`execute_transaction`, `call_contract`, `estimate_energy`, `get_code`, `get_storage_at`, `get_nonce`, `get_balance`, `create_evm_snapshot`, `revert_to_evm_snapshot`)
  - `tests.rs` (moved from embedded `mod tests` verbatim)
  - `integration_tests.rs` (moved from embedded `mod integration_tests` verbatim)

Notes:
- `mod.rs` retains method names and private visibility used by tests, but bodies call into the new modules.
- Use `pub(super)` for submodule functions where appropriate, and keep all external calls going through `BackendService` wrappers for a stable surface.

## Function‑to‑Module Mapping (Authoritative)

Helpers and Types
- `types.rs`
  - `FreezeParams`, `UnfreezeParams`, `FreezeV2Params`, `UnfreezeV2Params`, `FreezeResource`
  - `const MAX_VOTE_NUMBER`, `const TRX_PRECISION`
- `utils/address.rs`
  - `fn strip_tron_address_prefix(&[u8]) -> Result<&[u8], String>`
  - `fn add_tron_address_prefix(&Address) -> Vec<u8>`
- `utils/varint.rs`
  - `fn read_varint(&[u8]) -> Result<(u64, usize), String>`
- `utils/bandwidth.rs`
  - `fn calculate_bandwidth_usage(&TronTransaction) -> u64`
- `utils/aext.rs`
  - `fn parse_pre_execution_aext(&self, &[AccountAextSnapshot]) -> HashMap<Address, AccountAext>`
- `fees.rs`
  - `fn apply_fee_post_processing(&self, result, tx, context, is_non_vm) -> Result<(), String>`
- `convert.rs`
  - `fn convert_protobuf_transaction(&self, tx: Option<&crate::backend::TronTransaction>) -> Result<(TronTransaction, crate::backend::TxKind), String>`
  - `fn convert_protobuf_context(&self, ctx: Option<&crate::backend::ExecutionContext>) -> Result<TronExecutionContext, String>`
  - `fn convert_execution_result(&self, result: &TronExecutionResult, status: crate::backend::execution_result::Status) -> ExecuteTransactionResponse`

Handlers
- `handlers/non_vm.rs`
  - `fn execute_non_vm_contract(&self, &mut EngineBackedEvmStateStore, &TronTransaction, &TronExecutionContext) -> Result<TronExecutionResult, String>`
  - `fn is_likely_non_vm_transaction(&self, &TronTransaction, &EngineBackedEvmStateStore) -> bool` (keep adjacent to dispatch)
- `handlers/transfer.rs`
  - `fn execute_transfer_contract(&self, &mut EngineBackedEvmStateStore, &TronTransaction, &TronExecutionContext) -> Result<TronExecutionResult, String>`
- `handlers/account_update.rs`
  - `fn execute_account_update_contract(&self, ..) -> Result<TronExecutionResult, String>`
- `handlers/witness.rs`
  - `fn execute_witness_create_contract(&self, ..) -> Result<TronExecutionResult, String>`
  - `fn execute_witness_update_contract(&self, ..) -> Result<TronExecutionResult, String>`
- `handlers/vote.rs`
  - `fn parse_vote_witness_contract(&[u8]) -> Result<Vec<(Address, u64)>, String>`
  - `fn parse_vote(&[u8]) -> Result<(Address, u64), String>`
  - `fn execute_vote_witness_contract(&self, ..) -> Result<TronExecutionResult, String>`
- `handlers/freeze_v1.rs`
  - `fn parse_freeze_balance_params(&Bytes) -> Result<FreezeParams, String>`
  - `fn parse_unfreeze_balance_params(&Bytes) -> Result<UnfreezeParams, String>`
  - `fn execute_freeze_balance_contract(&self, ..) -> Result<TronExecutionResult, String>`
  - `fn execute_unfreeze_balance_contract(&self, ..) -> Result<TronExecutionResult, String>`
- `handlers/freeze_v2.rs`
  - `fn parse_freeze_balance_v2_params(&Bytes) -> Result<FreezeV2Params, String>`
  - `fn parse_unfreeze_balance_v2_params(&Bytes) -> Result<UnfreezeV2Params, String>`
  - `fn execute_freeze_balance_v2_contract(&self, ..) -> Result<TronExecutionResult, String>`
  - `fn execute_unfreeze_balance_v2_contract(&self, ..) -> Result<TronExecutionResult, String>`

RPC
- `rpc/health.rs`: `health`, `get_metadata`
- `rpc/storage.rs`: `get`, `put`, `delete`, `has`, `batch_write`, `batch_get`, `iterator`, `get_keys_next`, `get_values_next`
- `rpc/storage_admin.rs`: `list_databases`, `get_stats`, `stream_metrics`, `init_db`, `close_db`, `reset_db`, `size`, `is_empty`, `is_alive`
- `rpc/execution.rs`: `execute_transaction`, `call_contract`, `estimate_energy`, `get_code`, `get_storage_at`, `get_nonce`, `get_balance`, `create_evm_snapshot`, `revert_to_evm_snapshot`

## Wrapper Pattern (Preserve Signatures and Visibility)
- For each moved function, leave an inherent method on `BackendService` in `mod.rs` with the same signature calling the submodule.
- Keep `impl crate::backend::backend_server::Backend for BackendService` in `mod.rs`, methods delegate to `rpc::*` implementations.
- Use `pub(super)` for submodule functions to avoid leaking internals while permitting wrappers to call them.

## Detailed TODOs (Step‑by‑Step)

Phase 0 — Baseline and Safety
- [ ] Build and test baseline:
  - [ ] `cargo build -p tron-backend-core -p tron-backend`
  - [ ] `cargo test -p tron-backend-core` (expect current tests to pass)
- [ ] Note file size and key symbol list for later diff:
  - [ ] `wc -c crates/core/src/service.rs`
  - [ ] `rg -n "^\s*(fn|impl|mod)" crates/core/src/service.rs`

Phase 1 — Scaffold Module Structure
- [ ] Move `service.rs` → `service/mod.rs` (git move)
- [ ] Add child modules in `mod.rs`:
  - [ ] `mod types; mod fees; mod convert;`
  - [ ] `mod utils { pub mod address; pub mod varint; pub mod bandwidth; pub mod aext; }`
  - [ ] `mod handlers { pub mod non_vm; pub mod transfer; pub mod account_update; pub mod witness; pub mod vote; pub mod freeze_v1; pub mod freeze_v2; }`
  - [ ] `mod rpc { pub mod health; pub mod storage; pub mod storage_admin; pub mod execution; }`
- [ ] Ensure `pub use` external surface unchanged in `crates/core/src/lib.rs`.
- [ ] Build to catch path typos (no functional changes yet).

Phase 2 — Extract Types and Utils (Pure Moves)
- [ ] Create `types.rs` and move: `Freeze*Params`, `FreezeResource`, `MAX_VOTE_NUMBER`, `TRX_PRECISION`.
- [ ] Create `utils/address.rs` and move: `strip_tron_address_prefix`, `add_tron_address_prefix`.
- [ ] Create `utils/varint.rs` and move: `read_varint`.
- [ ] Create `utils/bandwidth.rs` and move: `calculate_bandwidth_usage`.
- [ ] Create `utils/aext.rs` and move: `parse_pre_execution_aext`.
- [ ] In `mod.rs`, add wrappers calling into these utils to preserve existing call sites and test access.
- [ ] Build and run unit tests.

Phase 3 — Extract Conversion and Fees (Pure Moves)
- [ ] Create `convert.rs` and move: `convert_protobuf_transaction`, `convert_protobuf_context`, `convert_execution_result`.
- [ ] Create `fees.rs` and move: `apply_fee_post_processing`.
- [ ] Keep wrappers in `mod.rs` calling into these functions.
- [ ] Build and run unit tests.

Phase 4 — Extract Contract Handlers (Pure Moves)
- [ ] `handlers/non_vm.rs`: move `execute_non_vm_contract`, `is_likely_non_vm_transaction`.
- [ ] `handlers/transfer.rs`: move `execute_transfer_contract`.
- [ ] `handlers/account_update.rs`: move `execute_account_update_contract`.
- [ ] `handlers/witness.rs`: move `execute_witness_create_contract`, `execute_witness_update_contract`.
- [ ] `handlers/vote.rs`: move `parse_vote_witness_contract`, `parse_vote`, `execute_vote_witness_contract`.
- [ ] `handlers/freeze_v1.rs`: move `parse_freeze_balance_params`, `parse_unfreeze_balance_params`, `execute_freeze_balance_contract`, `execute_unfreeze_balance_contract`.
- [ ] `handlers/freeze_v2.rs`: move `parse_freeze_balance_v2_params`, `parse_unfreeze_balance_v2_params`, `execute_freeze_balance_v2_contract`, `execute_unfreeze_balance_v2_contract`.
- [ ] Maintain all logging strings and error messages exactly.
- [ ] Keep wrappers in `mod.rs` with the original names.
- [ ] Build and run unit tests.

Phase 5 — Extract RPC Method Bodies
- [ ] `rpc/health.rs`: move bodies of `health`, `get_metadata` to free functions: `pub(super) async fn health(svc: &BackendService, req: Request<...>) -> Result<Response<...>, Status>`.
- [ ] `rpc/storage.rs`: move storage RPC bodies similarly.
- [ ] `rpc/storage_admin.rs`: move admin/stats/metrics RPC bodies similarly.
- [ ] `rpc/execution.rs`: move execution RPC bodies similarly.
- [ ] In tonic trait impl (left in `mod.rs`), delegate to the above functions.
- [ ] Build and run unit tests.

Phase 6 — Move Embedded Tests (Verbatim)
- [ ] Create `service/tests.rs` and move the entire embedded `#[cfg(test)] mod tests` block verbatim.
- [ ] Create `service/integration_tests.rs` and move the `#[cfg(test)] mod integration_tests` block verbatim.
- [ ] Ensure they are declared in `service/mod.rs` with `#[cfg(test)] mod tests; #[cfg(test)] mod integration_tests;`.
- [ ] Do not change any test code or expectations.
- [ ] Run `cargo test -p tron-backend-core`.

Phase 7 — Final Polish and Verification
- [ ] Grep for any remaining references to `service.rs` in docs; update paths as needed (without content changes):
  - [ ] `rg -n "service.rs" rust-backend/docs`
- [ ] Confirm no public API leaks from submodules (only wrappers are visible externally).
- [ ] Re‑run full build and tests.
- [ ] Optional: re‑run a minimal runtime sanity (health/metadata) if environment permits.

## Imports and Visibility Guidelines
- Submodules import protobuf types with `use crate::backend;` or selective types as needed.
- Access to `ExecutionModule` and `StorageEngine` stays via helper getters in `mod.rs`.
- Submodules that need config use `svc.get_execution_config()` via the wrapper on `BackendService`.
- Mark submodule functions `pub(super)`; only `mod.rs` exposes wrappers.
- Avoid `pub(crate)` unless necessary for test module access; prefer `pub(super)` for tight scope.

## Acceptance Criteria
- Build succeeds: `cargo build -p tron-backend-core -p tron-backend`.
- All tests pass unchanged: `cargo test -p tron-backend-core`.
- `rust-backend/src/main.rs` still compiles without modification.
- gRPC interface signature and behavior unchanged (compile‑time verified by tonic impl staying put).
- Logging and error strings unchanged (grep diff reveals only moved file paths, not content).
- Deterministic ordering of state changes preserved.

## Risk & Mitigations
- Test visibility of private methods: mitigate by keeping wrappers in `mod.rs` and moving tests into `service/` as child modules.
- Import churn errors: iterate with frequent builds per phase; move small, compile often.
- Hidden behavior drift: avoid refactors like renames; move code verbatim; run tests after each phase.
- Doc references to line numbers: track and update doc paths only (no content changes).

## Verification Checklist (Per Phase)
- [ ] Compiles with zero warnings introduced by the move.
- [ ] No changes to function signatures visible outside `service/`.
- [ ] Grep important log/error messages to ensure they still exist:
  - [ ] `rg -n "WitnessCreate completed" crates/core/src/service`
  - [ ] `rg -n "FreezeBalance completed" crates/core/src/service`
  - [ ] `rg -n "Non-VM transaction" crates/core/src/service`
- [ ] Check that state change sorting logic is intact and in the same handler modules.

## Out of Scope (Follow‑ups Only)
- Deduplication of repeated proto mapping code (could be centralized in `convert.rs` later).
- Any behavior changes, optimizations, or new features.
- Additional tests beyond moving the embedded ones.

## Expected Post‑Refactor Tree (Service)
```
crates/core/src/service/
  mod.rs
  types.rs
  fees.rs
  convert.rs
  utils/
    address.rs
    varint.rs
    bandwidth.rs
    aext.rs
  handlers/
    mod.rs
    non_vm.rs
    transfer.rs
    account_update.rs
    witness.rs
    vote.rs
    freeze_v1.rs
    freeze_v2.rs
  rpc/
    health.rs
    storage.rs
    storage_admin.rs
    execution.rs
  tests.rs
  integration_tests.rs
```

## Operational Notes
- Keep commit granularity per phase for easy review (if committing).
- Use `git mv` to preserve history for the main file and tests.
- Avoid reformatting to keep diffs small; mechanical moves only.

---
Last updated: Planned refactor only; no code moved yet.
