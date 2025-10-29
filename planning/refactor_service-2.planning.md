No need to start implementing, just read through and understand the codebase and give me a detailed plan on:

rust-backend/crates/core/src/service.rs is too long, re-org the codes to reduce the file size.

+ keep the original logic/behavior/const/..., don't introduce logic/behavior changes
+ don't delete/remove tests
+ don't modify test logic


BTW I thunk we can focus on high impact items for quick wins only.
e.g., the largest components
+ Freeze/Unfreeze Contracts
+ gRPC Handlers

---


• Here’s a precise, low-risk reorg plan to shrink service.rs by extracting only the highest-impact blocks first, preserving behavior and tests.

Scope And Goals

- Reduce service.rs size with minimal churn and no behavior changes.
- Focus on the two heaviest areas:
    - Freeze/Unfreeze contracts: parsing + handlers.
    - gRPC handlers: the Backend trait impl and conversion helpers.
- Do not delete or change test logic; relocation is allowed.

Hotspots To Target

- Freeze/Unfreeze V1/V2 handlers start around rust-backend/crates/core/src/service.rs:1279, plus parsers around :1617, :1868, and :2140.
- gRPC trait impl impl backend_server::Backend for BackendService starts at rust-backend/crates/core/src/service.rs:3542.
- Type conversion and gRPC helpers live below :4760.
- Tests inside this file start around rust-backend/crates/core/src/service.rs:2325.
- Current file size: rust-backend/crates/core/src/service.rs:5342.

Proposed Module Split

- Keep the module path as service so public API remains stable (pub mod service; pub use service::BackendService; stays valid).
- Convert to a directory module: rust-backend/crates/core/src/service/.
    - service/mod.rs:
        - Define BackendService struct, ctor, and core getters: get_storage_module, get_execution_module, get_storage_engine, get_execution_config.
        - Keep thin, behavior-free helpers that are widely used or referenced in tests (e.g., calculate_bandwidth_usage, unless moved cleanly).
        - Optionally include constants if used across submodules, or move to service/constants.rs.
    - service/grpc/:
        - mod.rs: entire #[tonic::async_trait] impl backend_server::Backend for BackendService (moved verbatim).
        - conversion.rs: convert_protobuf_transaction, convert_protobuf_context, convert_execution_result_to_protobuf.
        - address.rs: strip_tron_address_prefix, add_tron_address_prefix.
        - aext.rs: parse_pre_execution_aext.
        - If preferred, group smaller RPCs by domain for readability: storage.rs (get/put/has/batch/iterator), snapshots.rs, db_manage.rs, metrics.rs, execution.rs (just the execute_transaction method
        delegating to conversion helpers and BackendService execution).
    - service/contracts/:
        - freeze.rs: all freeze/unfreeze handlers + param parsers + FreezeResource and param structs:
            - execute_freeze_balance_contract
            - execute_unfreeze_balance_contract
            - execute_freeze_balance_v2_contract
            - execute_unfreeze_balance_v2_contract
            - parse_freeze_balance_params
            - parse_unfreeze_balance_params
            - parse_freeze_balance_v2_params
            - parse_unfreeze_balance_v2_params
            - FreezeResource, FreezeParams, UnfreezeParams, FreezeV2Params, UnfreezeV2Params
        - proto.rs: shared read_varint used by parsers (to avoid duplication, single source of truth).
        - fees.rs (optional quick-win): apply_fee_post_processing, is_likely_non_vm_transaction if you want to lift these out of the gRPC path.
    - service/constants.rs:
        - MAX_VOTE_NUMBER, TRX_PRECISION, or keep near their usage if they’re truly local to witness/vote.

Visibility guidelines:

- Use pub(super) for items only needed within service.
- Keep method signatures on BackendService unchanged; move impl blocks into submodules to preserve call sites.
- Re-export selectively from service/mod.rs as needed for existing tests.

Step-By-Step Refactor Plan

- Phase 1: Module shell and file moves (no logic edits).
    - Rename service.rs → service/mod.rs, adjust mod structure; keep pub use service::BackendService; unchanged.
    - Move the entire gRPC impl (starting service.rs:3542) into service/grpc/mod.rs intact.
    - Move the conversion/address/AEXT helper fns into service/grpc/{conversion.rs,address.rs,aext.rs} and import them inside grpc/mod.rs.
    - Compile locally to ensure no missing imports/visibilities.
- Phase 2: Freeze/Unfreeze split (high impact).
    - Extract freeze/unfreeze handlers and parsers into service/contracts/freeze.rs.
    - Extract read_varint into service/contracts/proto.rs and use it from freeze parsers.
    - Ensure BackendService impls for those methods are preserved by placing impl blocks in freeze.rs.
    - Compile to validate visibility and imports.
- Phase 3: Tests relocation (no logic changes).
    - Move unit tests currently in service.rs:2325+ into service/tests/ mirrored by topic:
        - service/tests/bandwidth_tests.rs (for calculate_bandwidth_usage).
        - service/tests/freeze_tests.rs, service/tests/unfreeze_tests.rs (copy-paste without changes).
        - Keep the test bodies identical; at most update module paths/imports.
    - If any test referenced private items, set pub(super) on those items rather than changing assertions.
    - Run cargo test -p core to confirm parity.
- Phase 4: Small glue and polish.
    - Ensure all use statements are module-local and minimal.
    - Keep all logging statements, error strings, and return types intact to avoid golden result drift.
    - No renames of public types, no signature changes.

Behavioral Safeguards

- Do not alter:
    - Error messages, logging content/levels, or order of state changes.
    - Fee processing logic or the non-VM heuristic behavior.
    - Bandwidth/energy calculations or config gating for system contracts.
- Keep BackendService API stable; all movement is internal to module layout.

Test Strategy

- No test deletion; move tests only. Keep logic and assertions identical.
- Validate with:
    - cargo test -p core for Rust unit/integration tests.
    - If desired, run dual-mode Java tests unchanged to ensure gRPC shape parity:
        - ./gradlew :framework:test --tests \"org.tron.core.storage.spi.DualStorageModeIntegrationTest\"
- Optional: run make dual-mode-test.

Quick Wins And Expected Impact

- Extracting the gRPC impl reduces service.rs by ~2,000+ lines immediately.
    - Anchor: rust-backend/crates/core/src/service.rs:3542.
- Extracting Freeze/Unfreeze handlers and parsers reduces another ~1,000+ lines.
    - Anchors: rust-backend/crates/core/src/service.rs:1279, :1617, :1868, :2140.
- Moving unit tests out will drop a few hundred lines from the main module without changing behavior.
    - Anchors: rust-backend/crates/core/src/service.rs:2325, :3477, :3532.

Optional Next Steps (Future)

- Group remaining non-VM handlers into service/contracts/ submodules (transfer, TRC10, witness).
- Centralize common result-building patterns and resource emissions into small utilities to reduce duplication.
- Consider a thin routing layer for non-VM contract execution keyed by TronContractType for even flatter code.

If you want, I can draft the exact file skeletons and mod/use statements to make this a no-diff move in semantics and get it compiling green on first pass.