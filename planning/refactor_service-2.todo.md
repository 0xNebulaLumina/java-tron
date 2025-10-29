Refactor rust-backend/crates/core/src/service.rs (Phase 2: Quick Wins)

Context
- Current file: rust-backend/crates/core/src/service.rs (~5.3k LOC)
- Targets for quick impact:
  - gRPC handlers (tonic Backend impl + helpers)
  - Freeze/Unfreeze contracts (V1 + V2) and their protobuf parsers

Goals
- Reduce service.rs size significantly without changing behavior.
- Keep public API and crate exports stable.
- Do NOT delete or change tests; relocation only when needed.
- Preserve all logging, error messages, and result shapes.

Non‑Goals
- No functional changes, no policy changes, no feature additions.
- No refactors to other contracts (transfer, witness, TRC10, etc.) beyond wiring needs.

Guardrails (Behavior Preservation)
- Keep method signatures and visibility of BackendService and moved methods intact.
- Do not change strings, logging levels, or order of state_changes/logs in results.
- Keep module path for generated proto `crate::backend` unchanged.
- Ensure fee handling, non‑VM heuristic, and bandwidth/energy calculations remain identical.

Current Hotspots (anchors are approximate)
- Freeze/Unfreeze V1+V2 handlers and parsers: ~1279–2140, 2140–2620
- gRPC trait impl: starts ~3542 to end of impl block
- Helpers used by gRPC: address/AEXT/conversions near 4760+
- Unit tests inside file: ~2325+ (several #[test] and #[tokio::test])

Target Layout (directory module `service/`)
- service/mod.rs
  - struct BackendService and ctor new()
  - Accessors: get_storage_module, get_execution_module, get_storage_engine, get_execution_config
  - Router(s) that other code depends on (e.g., execute_non_vm_contract)
  - Keep small cross‑cutting helpers here initially (calculate_bandwidth_usage, is_likely_non_vm_transaction, apply_fee_post_processing) to minimize churn
- service/grpc/
  - mod.rs: move entire `impl crate::backend::backend_server::Backend for BackendService` verbatim
  - conversion.rs: convert_protobuf_transaction, convert_protobuf_context, convert_execution_result_to_protobuf
  - address.rs: strip_tron_address_prefix, add_tron_address_prefix
  - aext.rs: parse_pre_execution_aext
- service/contracts/
  - freeze.rs: execute_freeze_balance_contract, execute_unfreeze_balance_contract, execute_freeze_balance_v2_contract, execute_unfreeze_balance_v2_contract; parsers (parse_*), FreezeResource + Freeze/Unfreeze param structs
  - proto.rs: read_varint (shared by parsers)

Exports/Visibility
- Re‑export nothing new publicly; keep `pub use service::BackendService;` in lib.rs intact.
- Use `pub(super)` for helpers only consumed inside `service`.
- All moved `impl BackendService { ... }` blocks remain methods on the same type.

Phased Plan
1) Scaffold modules (no behavior changes)
   - Turn `service.rs` into `service/mod.rs` (same contents initially)
   - Create folders: `service/grpc/`, `service/contracts/`
   - Create empty files with module declarations: grpc/mod.rs, grpc/conversion.rs, grpc/address.rs, grpc/aext.rs, contracts/freeze.rs, contracts/proto.rs

2) Extract gRPC (tonic) implementation
   - Move the entire `impl crate::backend::backend_server::Backend for BackendService` into `service/grpc/mod.rs` unchanged
   - Add `use super::BackendService;` and bring in needed items via `use super::*` or explicit imports
   - In `grpc/mod.rs`, call helpers via `super::grpc::{conversion::..., address::..., aext::...}` or `super::{...}`
   - Move helper functions into:
     - `grpc/conversion.rs`: convert_protobuf_transaction, convert_protobuf_context, convert_execution_result_to_protobuf
     - `grpc/address.rs`: strip_tron_address_prefix, add_tron_address_prefix
     - `grpc/aext.rs`: parse_pre_execution_aext
   - Wire `mod conversion; mod address; mod aext;` inside grpc/mod.rs
   - Ensure no changes to function bodies (copy verbatim), adjust only paths/uses

3) Extract Freeze/Unfreeze (V1 + V2)
   - Move to `service/contracts/freeze.rs`:
     - Types: FreezeResource, FreezeParams, UnfreezeParams, FreezeV2Params, UnfreezeV2Params
     - Parsers: parse_freeze_balance_params, parse_unfreeze_balance_params, parse_freeze_balance_v2_params, parse_unfreeze_balance_v2_params
     - Handlers: execute_freeze_balance_contract, execute_unfreeze_balance_contract, execute_freeze_balance_v2_contract, execute_unfreeze_balance_v2_contract
   - Move `read_varint` to `service/contracts/proto.rs` and import it in freeze.rs
   - Keep logging strings and error messages unchanged
   - Keep result and emission logic identical (freeze_changes/global_resource_changes)
   - In `service/mod.rs`, import and use these methods transparently (existing call sites unchanged)

4) Tests (relocate only, no logic change)
   - Move unit tests from service.rs into dedicated files under `service/tests/`:
     - `bandwidth_tests.rs` for test_calculate_bandwidth_usage and tx_kind conversion tests
     - `freeze_v1_tests.rs`, `freeze_v2_tests.rs`, `unfreeze_v1_tests.rs`, `unfreeze_v2_tests.rs`
     - Any async tests remain with the same annotations
   - Update imports (paths) only; do not change assertions or logic
   - Ensure tests compile against new module paths

5) Compile and verify
   - cargo build -p core
   - cargo test -p core
   - (Optional) Gradle Java tests for dual-mode integration if environment available:
     - ./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"

6) Follow‑ups (optional after quick wins)
   - Split grpc/mod.rs further by domain (storage.rs, snapshot.rs, txn.rs, db.rs, metrics.rs)
   - Consider moving `apply_fee_post_processing` and `is_likely_non_vm_transaction` to a `service/fees.rs`
   - Consider `service/utils.rs` for `calculate_bandwidth_usage`

Acceptance Criteria
- service.rs is replaced by service/ with module files; main file shrinks by >50%
- All tests pass with zero assertion changes
- gRPC server binary runs unchanged (no API or behavior drift)
- Freeze/Unfreeze CSV/state-change outputs match pre‑refactor outputs

Risk Register + Mitigations
- Missing imports or visibility issues: mitigate by incremental compiles per phase
- Accidentally changed log strings/order: copy bodies verbatim; review diffs carefully
- Test path issues after move: only adjust `use` paths; no code edits

Rollback Plan
- Single revert of the refactor commit restores previous monolithic file

Mapping (moved items → new files)
- gRPC impl: service/grpc/mod.rs
- strip_tron_address_prefix / add_tron_address_prefix: service/grpc/address.rs
- parse_pre_execution_aext: service/grpc/aext.rs
- convert_protobuf_transaction / convert_protobuf_context / convert_execution_result_to_protobuf: service/grpc/conversion.rs
- Freeze/Unfreeze handlers and parsers + FreezeResource/params: service/contracts/freeze.rs
- read_varint: service/contracts/proto.rs

Detailed TODO Checklist

[ ] Phase 1: Module scaffolding
  [ ] Rename rust-backend/crates/core/src/service.rs → service/mod.rs
  [ ] Create directories: service/grpc, service/contracts, service/tests
  [ ] Add empty modules: grpc/{mod,conversion,address,aext}.rs, contracts/{freeze,proto}.rs
  [ ] Ensure lib.rs still has `pub mod service; pub use service::BackendService;`

[ ] Phase 2: Move gRPC implementation
  [ ] Cut the entire tonic impl (`impl backend_server::Backend for BackendService`) into grpc/mod.rs
  [ ] Add `mod conversion; mod address; mod aext;` into grpc/mod.rs
  [ ] Move helper fns into appropriate grpc/* files (copy bodies verbatim)
  [ ] Fix `use` statements; build to verify

[ ] Phase 3: Extract Freeze/Unfreeze (V1+V2)
  [ ] Create contracts/proto.rs and move read_varint
  [ ] Create contracts/freeze.rs and move:
      - FreezeResource, FreezeParams, UnfreezeParams, FreezeV2Params, UnfreezeV2Params
      - parse_freeze_balance_params, parse_unfreeze_balance_params
      - parse_freeze_balance_v2_params, parse_unfreeze_balance_v2_params
      - execute_freeze_balance_contract, execute_unfreeze_balance_contract
      - execute_freeze_balance_v2_contract, execute_unfreeze_balance_v2_contract
  [ ] Update service/mod.rs to `use` contracts::freeze::* if needed
  [ ] Build to verify

[ ] Phase 4: Tests relocation (no logic changes)
  [ ] Move test_calculate_bandwidth_usage and tx_kind_conversion → service/tests/bandwidth_tests.rs
  [ ] Move freeze/unfreeze tests into service/tests/{freeze_v1,unfreeze_v1,freeze_v2,unfreeze_v2}_tests.rs
  [ ] Adjust imports only; keep test bodies identical
  [ ] cargo test -p core

[ ] Phase 5: Sanity validations
  [ ] Compare log outputs in key paths (spot-check) for equivalence
  [ ] Verify CSV/state-change parity for freeze contracts (existing tests)
  [ ] Optional: run Gradle dual‑mode tests if available

Notes
- Keep calculate_bandwidth_usage, execute_non_vm_contract, apply_fee_post_processing, is_likely_non_vm_transaction in service/mod.rs for now to minimize cross‑module churn.
- Consider a separate PR to further split grpc/mod.rs by RPC domain once this lands cleanly.

