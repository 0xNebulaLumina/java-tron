think harder.

No need to start implementing, just read through and understand the codebase and give me a detailed plan on:

rust-backend/crates/execution/src/storage_adapter.rs is too long, re-org the codes to reduce the file size.

+ keep the original logic/behavior/const/..., don't introduce logic/behavior changes
+ don't delete/remove tests
+ don't modify test logic

---

• Here’s a concrete, no‑behavior‑change re‑org plan to split storage_adapter.rs into coherent submodules while keeping the public API and tests intact.

Overview

- Goal: split storage_adapter.rs into smaller focused modules under storage_adapter/, reduce duplication, and keep public exports and behavior unchanged.
- Keep lib.rs’s pub use storage_adapter::{...} stable via re‑exports in storage_adapter/mod.rs.
- Do not modify test logic; only relocate tests alongside the code they cover.

Current Layout (Key Sections)

- Trait and in‑memory impl:
    - EvmStateStore trait: rust-backend/crates/execution/src/storage_adapter.rs:610
    - InMemoryEvmStateStore: rust-backend/crates/execution/src/storage_adapter.rs:636
- Engine‑backed store + persistence helpers:
    - EngineBackedEvmStateStore: rust-backend/crates/execution/src/storage_adapter.rs:812
    - Account name, witness, votes, freeze, dynamic props, totals: in the same file
- Database wrapper (duplicated):
    - EvmStateDatabase + Database/DatabaseCommit: rust-backend/crates/execution/src/storage_adapter.rs:1887 and rust-backend/crates/execution/src/storage_adapter/database.rs
- Types and helpers:
    - WitnessInfo, FreezeRecord, Vote, VotesRecord, AccountAext: top of file
    - StateChangeRecord: rust-backend/crates/execution/src/storage_adapter.rs:1887
    - ResourceTracker, BandwidthPath: rust-backend/crates/execution/src/storage_adapter.rs:3047
    - keccak256, to_tron_address, test‑only from_tron_address: rust-backend/crates/execution/src/storage_adapter.rs:2285, rust-backend/crates/execution/src/storage_adapter.rs:2293
- Tests: many #[test] near file end (e.g., address conversion, snapshots, account name, witness, freeze, resource tracker).

Target Layout

- storage_adapter/mod.rs (new): central module and re‑exports.
- storage_adapter/traits.rs (new): EvmStateStore, SnapshotHook (if needed by cross‑module).
- storage_adapter/types.rs (new): WitnessInfo, FreezeRecord, Vote, VotesRecord, AccountAext, StateChangeRecord.
- storage_adapter/in_memory.rs (new): InMemoryEvmStateStore and its AEXT/freeze helpers.
- storage_adapter/engine.rs (new): EngineBackedEvmStateStore, DB name helpers, (de)serialization, account name, witness, votes, freeze, dynamic props, totals, engine AEXT helpers.
- storage_adapter/database.rs (exists): keep sole EvmStateDatabase + Database/DatabaseCommit; remove duplicate from old file.
- storage_adapter/resource.rs (new): ResourceTracker, BandwidthPath.
- storage_adapter/utils.rs (new): keccak256, to_tron_address, test‑only from_tron_address, shared varint read/write if we centralize them.

Public API Compatibility

- In storage_adapter/mod.rs, re‑export exactly what lib.rs expects:
    - pub use traits::EvmStateStore;
    - pub use in_memory::InMemoryEvmStateStore;
    - pub use engine::EngineBackedEvmStateStore;
    - pub use database::EvmStateDatabase;
    - pub use types::{StateChangeRecord, WitnessInfo, FreezeRecord, VotesRecord, Vote, AccountAext};
    - pub use resource::{ResourceTracker, BandwidthPath};
- Keep method names intact (e.g., put_freeze_record, compute_tron_power_in_sun aliases). No signature changes.
- Keep constants and serialization formats exactly as‑is.

Move Map

- Traits:
    - Move EvmStateStore to storage_adapter/traits.rs (rust-backend/crates/execution/src/storage_adapter.rs:610).
- In‑memory:
    - Move InMemoryEvmStateStore + AEXT/freeze/Tron power helpers to storage_adapter/in_memory.rs (rust-backend/crates/execution/src/storage_adapter.rs:636, 667–756, 729–747).
- Engine‑backed:
    - Move EngineBackedEvmStateStore + DB name helpers + (de)serialization + dynamic props + totals + witness/votes + freeze + account name + engine AEXT helpers to storage_adapter/engine.rs (rust-backend/
    crates/execution/src/storage_adapter.rs:812, 842–1460, 1626–1764).
- Database:
    - Keep storage_adapter/database.rs as canonical; delete the duplicate EvmStateDatabase/impls from the old monolith (rust-backend/crates/execution/src/storage_adapter.rs:1887–end of those sections), and
    point imports to the submodule.
- Types:
    - Move WitnessInfo, FreezeRecord, Vote, VotesRecord, AccountAext, StateChangeRecord to storage_adapter/types.rs (top of file and rust-backend/crates/execution/src/storage_adapter.rs:1887).
- Resource:
    - Move ResourceTracker, BandwidthPath to storage_adapter/resource.rs (rust-backend/crates/execution/src/storage_adapter.rs:3047, 3050, 3056).
- Utils:
    - Move keccak256, to_tron_address, #[cfg(test)] from_tron_address to storage_adapter/utils.rs (rust-backend/crates/execution/src/storage_adapter.rs:2285, 2293).

Tests

- Do not change test logic; relocate tests to the module that owns the code they exercise:
    - Database tests (snapshot hooks, modified accounts, revert): into storage_adapter/database.rs under #[cfg(test)] mod tests.
    - Address encoding/decoding tests: into storage_adapter/utils.rs tests, reusing the test‑only from_tron_address.
    - Account name tests and dynamic prop/totals/freeze ledger/witness/votes: into storage_adapter/engine.rs tests.
    - AEXT serialization tests: into storage_adapter/types.rs tests.
    - Resource tracker tests: into storage_adapter/resource.rs tests.
- If a test used super::*, adapt imports for the new module root but keep test bodies the same. Test assertions must remain identical.

Step‑By‑Step Plan

- Create storage_adapter/mod.rs with submodule declarations and re‑exports.
- Extract traits.rs, move EvmStateStore; update uses to crate::storage_adapter::traits::EvmStateStore.
- Extract types.rs, move domain structs/enums; leave serialization logic unchanged.
- Extract in_memory.rs, move InMemoryEvmStateStore and related helpers; implement EvmStateStore there.
- Extract engine.rs, move EngineBackedEvmStateStore + all persistence helpers; implement EvmStateStore there.
- Consolidate database into storage_adapter/database.rs; remove duplicate from monolith; import types::StateChangeRecord, utils::{keccak256, to_tron_address}, traits::EvmStateStore.
- Extract resource.rs with ResourceTracker + BandwidthPath unchanged.
- Extract utils.rs with keccak256, to_tron_address, test‑only from_tron_address; optionally centralize varint helpers used by Vote/VotesRecord by copying exact logic into utils to avoid behavior drift.
- Move tests into their owning files (#[cfg(test)] modules) without changing assertions or logic.
- Delete old storage_adapter.rs after all moves; keep mod storage_adapter; in lib.rs unchanged.

Validation

- Build: cd rust-backend && cargo build --release.
- Unit tests: cd rust-backend && cargo test -p execution.
- Sanity: run rg -n "EvmStateDatabase" rust-backend/crates/execution/src to confirm single definition; check pub use in lib.rs still compiles unchanged.
- Optional: rg -n "to_tron_address|keccak256|ResourceTracker|EvmStateStore" to verify imports resolved to submodules.

Risks and Mitigations

- Duplicate EvmStateDatabase definitions: ensure only the submodule remains; adjust imports accordingly.
- Cross‑module cyclic deps: keep traits and types leaf‑like; database depends on them; engine depends on utils/types/traits.
- Test visibility: if tests need internal items, keep them in the same file as #[cfg(test)] mod tests or expose minimally via pub(crate) without altering behavior.
- Address/varint helpers: if centralizing varints increases risk, keep the exact functions local to the type modules.

If you want, I can implement this refactor next and run the execution crate’s tests to verify nothing changed behaviorally.

