think harder.

No need to start implementing, just read through and understand the codebase and give me a detailed plan on:

rust-backend/crates/core/src/service.rs is too long, re-org the codes to reduce the file size.

+ keep the original logic/behavior/const/..., don't introduce logic/behavior changes
+ don't delete/remove tests
+ don't modify test logic

---

Here’s a concrete, low‑risk reorg plan to shrink rust-backend/crates/core/src/service.rs without changing behavior or tests.

Overview

- Goal: split a 232 KB monolith into focused modules with zero behavior changes.
- Non-goals: altering logic, changing public APIs, deleting or changing tests.
- Strategy: turn service.rs into a module directory, extract cohesive submodules, and keep thin delegating wrappers on BackendService so all method names and call sites remain intact.

Current Layout (Key Responsibilities)

- Service wiring: module accessors, health/metadata, tonic service impl.
- gRPC storage API: get/put/delete/batch, iterators, DB admin, stats/metrics streaming.
- gRPC execution API: execute_transaction, call_contract, estimate_energy, state reads, snapshots.
- Execution helpers: conversions between protobuf and execution types, address helpers, AEXT parsing, bandwidth calc, fee post-processing.
- Contract handlers: non-VM dispatch, transfer, account_update, witness_create/update, vote_witness, freeze/unfreeze v1/v2 parsing and execution.
- Tests embedded in the module exercising helpers and system contract handlers.

Target Layout

- Keep crate external surface identical: pub mod service; in lib.rs and use tron_backend_core::BackendService; in rust-backend/src/main.rs remain unchanged.
- Convert service.rs → directory module with thin wrappers:
    - crates/core/src/service/mod.rs:
        - Define BackendService type and “module getter” helpers.
        - Implement the tonic Backend trait, delegating to rpc::* modules.
        - Keep inherent methods as thin wrappers that delegate to extracted modules, preserving method names and visibility for tests.
        - Keep #[cfg(test)] mod tests; and #[cfg(test)] mod integration_tests; as child test modules so private methods remain callable from tests.
    - crates/core/src/service/types.rs:
        - FreezeParams, UnfreezeParams, FreezeV2Params, UnfreezeV2Params, FreezeResource, constants (e.g., MAX_VOTE_NUMBER, TRX_PRECISION).
    - crates/core/src/service/utils/:
        - address.rs: strip_tron_address_prefix, add_tron_address_prefix.
        - varint.rs: read_varint.
        - bandwidth.rs: calculate_bandwidth_usage.
        - aext.rs: parse_pre_execution_aext.
    - crates/core/src/service/fees.rs:
        - apply_fee_post_processing.
    - crates/core/src/service/convert.rs:
        - convert_protobuf_transaction, convert_protobuf_context, convert_execution_result.
    - crates/core/src/service/handlers/:
        - mod.rs: common exports and dispatch glue.
        - non_vm.rs: execute_non_vm_contract dispatch.
        - transfer.rs: execute_transfer_contract.
        - account_update.rs: execute_account_update_contract.
        - witness.rs: execute_witness_create_contract, execute_witness_update_contract.
        - vote.rs: parsing helpers (parse_vote_witness_contract, parse_vote), execute_vote_witness_contract.
        - freeze_v1.rs: parse_freeze_balance_params, parse_unfreeze_balance_params, execute_freeze_balance_contract, execute_unfreeze_balance_contract.
        - freeze_v2.rs: parse_freeze_balance_v2_params, parse_unfreeze_balance_v2_params, execute_freeze_balance_v2_contract, execute_unfreeze_balance_v2_contract.
    - crates/core/src/service/rpc/:
        - health.rs: health, get_metadata.
        - storage.rs: get, put, delete, has, batch_*, iterator, get_*_next.
        - storage_admin.rs: init_db, close_db, reset_db, size, is_empty, is_alive, get_stats, stream_metrics.
        - execution.rs: execute_transaction, call_contract, estimate_energy, get_code, get_storage_at, get_nonce, get_balance, create_evm_snapshot, revert_to_evm_snapshot.

Refactor Plan (Phases)

- Phase 1 — Module scaffolding
    - Rename file to directory module: service.rs → service/mod.rs (no public API changes).
    - Add mod types; mod utils::{address, varint, bandwidth, aext}; mod fees; mod convert; mod handlers::{…}; mod rpc::{…}; in service/mod.rs.
- Phase 2 — Pure-extract helpers and types
    - Move structs/constants to types.rs.
    - Move helper fns to utils/*.
    - Move fee logic to fees.rs.
    - Move conversion logic to convert.rs.
    - In mod.rs, keep inherent methods on BackendService as thin wrappers:
        - Example: fn calculate_bandwidth_usage(..) -> u64 { utils::bandwidth::calculate_bandwidth_usage(..) }
- Phase 3 — Extract handlers
    - Create handler files and move the bodies of:
        - execute_non_vm_contract, execute_transfer_contract, execute_account_update_contract, execute_witness_*, execute_vote_witness_contract, execute_freeze_*, execute_unfreeze_*, and related parse_*
        helpers.
    - In mod.rs keep inherent methods with the original signatures calling handlers::* functions. This preserves test visibility to private methods.
- Phase 4 — Extract gRPC endpoints
    - Move each tonic RPC method body to rpc::* with signatures:
        - pub(super) async fn health(svc: &BackendService, req: Request<...>) -> Result<Response<...>, Status>
    - Keep the tonic trait impl in mod.rs delegating to rpc::*—unchanged wire API.
- Phase 5 — Tests preserved without logic changes
    - Keep #[cfg(test)] mod tests; and #[cfg(test)] mod integration_tests; as child modules of service:
        - Move their contents verbatim into service/tests.rs and service/integration_tests.rs.
        - Because they remain child modules of service, they still see private inherent methods defined in mod.rs (wrappers), preserving access and behavior.
    - Do not touch crates/core/src/tests.rs (it targets ExecutionModule, not the service).
- Phase 6 — Build and verify
    - Build rust-backend and run cargo test -p tron-backend-core.
    - If needed, fix only module import paths inside the service module (no logic changes).
    - Optional: update doc references in rust-backend/docs/ that point to service.rs:LINES to the new paths.

Compatibility Details

- Public API unchanged:
    - pub use service::BackendService; in lib.rs remains valid.
    - rust-backend/src/main.rs keeps use tron_backend_core::BackendService;.
    - All tonic gRPC service paths remain stable; only their implementations move behind delegation.
- Private method access for tests:
    - Private methods remain defined on BackendService in service/mod.rs as wrapper methods calling into submodules, so child test modules continue to call them without changes.
- Behavior parity:
    - All function bodies are moved verbatim; no renames, no logic edits.
    - Logging, error messages, sort orders, and field population remain the same.
    - Any duplicated snippets stay duplicated initially to avoid risk; optional dedupe can follow later.

Function-to-Module Mapping (examples)

- Address helpers: service/utils/address.rs — strip_tron_address_prefix, add_tron_address_prefix.
- Varint: service/utils/varint.rs — read_varint.
- Bandwidth: service/utils/bandwidth.rs — calculate_bandwidth_usage.
- AEXT parse: service/utils/aext.rs — parse_pre_execution_aext.
- Fees: service/fees.rs — apply_fee_post_processing.
- Conversions: service/convert.rs — convert_protobuf_transaction, convert_protobuf_context, convert_execution_result.
- Dispatch: service/handlers/non_vm.rs — execute_non_vm_contract.
- Contracts:
    - service/handlers/transfer.rs — execute_transfer_contract.
    - service/handlers/account_update.rs — execute_account_update_contract.
    - service/handlers/witness.rs — execute_witness_create_contract, execute_witness_update_contract.
    - service/handlers/vote.rs — parse_vote_witness_contract, parse_vote, execute_vote_witness_contract.
    - service/handlers/freeze_v1.rs — parse_freeze_balance_params, parse_unfreeze_balance_params, execute_freeze_balance_contract, execute_unfreeze_balance_contract.
    - service/handlers/freeze_v2.rs — parse_freeze_balance_v2_params, parse_unfreeze_balance_v2_params, execute_freeze_balance_v2_contract, execute_unfreeze_balance_v2_contract.
- gRPC:
    - service/rpc/health.rs — health, get_metadata.
    - service/rpc/storage.rs — core KV and iterators.
    - service/rpc/storage_admin.rs — admin ops, stats/metrics.
    - service/rpc/execution.rs — execute/call/estimate/state/snapshot.

Wrapper Pattern (no behavior change)

- Keep original method names and signatures in service/mod.rs:
    - fn execute_account_update_contract(&self, store, tx, ctx) -> Result<...> { handlers::account_update::execute(self, store, tx, ctx) }
    - fn convert_protobuf_transaction(&self, tx) -> Result<...> { convert::convert_protobuf_transaction(self, tx) }
    - fn calculate_bandwidth_usage(tx) -> u64 { utils::bandwidth::calculate_bandwidth_usage(tx) }
- Tonic impl delegates:
    - async fn get(...) { rpc::storage::get(self, request).await }

Risks and Mitigations

- Private visibility and tests:
    - Tests rely on calling private methods; wrappers defined in the parent module preserve access.
- Cross-module imports:
    - Use pub(super) for functions in submodules and keep all external references via BackendService wrappers to avoid leaking internals.
- Docs pointing to service.rs:LINES:
    - Note potential stale doc references (e.g., rust-backend/docs/FREEZE_BALANCE_PHASE2_SUMMARY.md) and update after refactor.

Validation

- Compile core crate and backend binary.
- Run unit tests in crates/core/src/tests.rs and the service child tests (moved verbatim).
- Sanity run: execute a minimal gRPC health/metadata call path (optional, if test harness allows running the server).

Optional Follow‑ups (separate PR)

- Deduplicate repeated state change → proto mapping (currently duplicated) into a single function in convert.rs to reduce code size further, keeping exact output unchanged.
- Add lightweight module-level docs/comments to guide future contributors (no logic changes).

If you’re good with this approach, I can scaffold the module structure and perform a mechanical move with wrappers, then build and run tests to confirm nothing changed in behavior.

---

