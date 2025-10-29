# Refactor Plan: Split `storage_adapter.rs` Into Focused Modules

Status: draft
Owner: execution team
Scope: rust-backend/crates/execution/src/storage_adapter/* and references in rust-backend/crates/execution/src/lib.rs

## Objectives
- Reduce `storage_adapter.rs` size and complexity by splitting into focused submodules.
- Preserve logic, behavior, public API, constants, and serialization formats exactly.
- Do not delete or change test logic (relocate only, if needed).
- Eliminate duplicate `EvmStateDatabase` definitions by keeping a single canonical implementation.

## Non‑Goals
- No feature changes or behavior tweaks.
- No external API changes visible to the rest of the crate (keep `lib.rs` re-exports stable).
- No performance tuning beyond mechanical relocation/organization.

## Target Module Layout
Create a cohesive `storage_adapter/` module with clear boundaries:

```
rust-backend/crates/execution/src/storage_adapter/
  mod.rs                    # submodule declarations + public re-exports (compat with lib.rs)
  traits.rs                 # EVM-facing store trait(s) + hooks
  types.rs                  # domain types: WitnessInfo, FreezeRecord, Vote, VotesRecord, AccountAext, StateChangeRecord
  in_memory.rs              # InMemoryEvmStateStore + its helpers (AEXT, freeze, tron power)
  engine.rs                 # EngineBackedEvmStateStore + persistence, dynamic props, witness/votes, freeze, aext
  database.rs               # EvmStateDatabase + Database/DatabaseCommit impls (already exists)
  resource.rs               # ResourceTracker, BandwidthPath
  utils.rs                  # keccak256, to_tron_address, (test-only) from_tron_address, varint helpers (if centralized)
```

Compatibility requirement: `lib.rs` expects
`pub use storage_adapter::{EvmStateStore, InMemoryEvmStateStore, EngineBackedEvmStateStore, EvmStateDatabase, StateChangeRecord, WitnessInfo, FreezeRecord, VotesRecord, Vote, AccountAext, ResourceTracker, BandwidthPath};`
We will satisfy this with re-exports in `storage_adapter/mod.rs`.

## Invariants and Constraints
- No functional or serialization behavior changes.
- Keep exact method names and signatures for all public items.
- Keep constants, magic numbers, and protobuf wire formats identical.
- Do not remove or alter tests; only relocate. Test assertions must remain unchanged.
- Maintain `prost` usage for `WitnessInfo`. For `Vote` and `VotesRecord`, preserve current manual varint encoding/decoding.
- Ensure a single, canonical `EvmStateDatabase` implementation resides in `storage_adapter/database.rs`.

## Symbol Mapping (Before → After)
- Trait: `EvmStateStore` → `storage_adapter/traits.rs`
- In-memory store: `InMemoryEvmStateStore` (+ AEXT/freeze helpers) → `storage_adapter/in_memory.rs`
- Engine-backed store: `EngineBackedEvmStateStore` (+ all DB helpers: account/code/storage keys, contract storage composition, account (de)serialization, dynamic properties, witness/votes, freeze, account-name, totals, engine AEXT helpers) → `storage_adapter/engine.rs`
- Database wrapper: `EvmStateDatabase` (+ Database/Commit impl) → keep only in `storage_adapter/database.rs` (delete duplicate from monolith)
- Types: `WitnessInfo`, `FreezeRecord`, `Vote`, `VotesRecord`, `AccountAext`, `StateChangeRecord` → `storage_adapter/types.rs`
- Resource accounting: `ResourceTracker`, `BandwidthPath` → `storage_adapter/resource.rs`
- Utilities: `keccak256`, `to_tron_address`, `#[cfg(test)] from_tron_address` → `storage_adapter/utils.rs`
- Varint helpers used by `Vote` and `VotesRecord`: keep inline for zero-risk; optionally centralize in `utils.rs` with byte-for-byte identical code guarded by tests (see Optional Step O2).

## Detailed TODOs

Phase 0 – Baseline and Guards
- [ ] Verify current build and tests pass:
  - `cd rust-backend && cargo build --release`
  - `cd rust-backend && cargo test -p execution`
- [ ] Grep for public exports used outside storage_adapter to ensure compatibility surface:
  - `rg -n "EvmStateStore|InMemoryEvmStateStore|EngineBackedEvmStateStore|EvmStateDatabase|StateChangeRecord|WitnessInfo|FreezeRecord|VotesRecord|Vote|AccountAext|ResourceTracker|BandwidthPath" rust-backend`.

Phase 1 – Module Shell and Re‑exports
- [ ] Add `storage_adapter/mod.rs` declaring the submodules and re-exporting public types to match `lib.rs` expectations.
- [ ] Ensure `lib.rs` continues to `mod storage_adapter;` without changes.

Phase 2 – Unify Database Wrapper (remove duplication)
- [ ] Confirm canonical implementation in `storage_adapter/database.rs` contains all behavior:
  - Account creation/modification tracking, state_change_records, snapshots, hooks.
  - Persist code and storage (includes “critical fix” persistence of storage changes).
  - Logging parity with the monolithic duplicate.
- [ ] Diff monolithic `EvmStateDatabase` vs `storage_adapter/database.rs`; port any missing logs or behaviors into the canonical file without logic change.
- [ ] Remove duplicate `EvmStateDatabase` from monolith (will be done in Phase 8 when deleting the file).
- [ ] Adjust imports in `database.rs` to use new module paths after we create `traits.rs`, `types.rs`, `utils.rs`.

Phase 3 – Extract Types
- [ ] Create `storage_adapter/types.rs` and move:
  - `WitnessInfo` (+ prost serialization/deserialization via `crate::protocol::Witness`).
  - `FreezeRecord` (+ byte-level serialization/deserialization).
  - `Vote`, `VotesRecord` (+ current manual varint encode/decode, keep as-is initially).
  - `AccountAext` (+ 66-byte serialization/deserialization, `with_defaults`).
  - `StateChangeRecord` enum.
- [ ] Update all references to point to `crate::storage_adapter::types::*` or re-exports from `mod.rs`.

Phase 4 – Extract Traits
- [ ] Create `storage_adapter/traits.rs` and move `EvmStateStore` (+ doc comments) and `SnapshotHook` type alias if shared.
- [ ] Update `database.rs`, `in_memory.rs`, `engine.rs` to import trait from `traits` (or via `mod.rs` re-exports).

Phase 5 – Extract Utils
- [ ] Create `storage_adapter/utils.rs` and move: `keccak256`, `to_tron_address`, and `#[cfg(test)] from_tron_address`.
- [ ] Update all uses in `database.rs`, `engine.rs`, and tests to use `crate::storage_adapter::utils::*`.
- [ ] Keep byte-for-byte identical logic for address conversions and hashing.

Phase 6 – Extract In‑Memory Store
- [ ] Create `storage_adapter/in_memory.rs` and move `InMemoryEvmStateStore` and its helper methods:
  - Freeze ledger: `get_freeze_record`, `set_freeze_record`, `put_freeze_record`, `get_tron_power_in_sun`.
  - AEXT: `get_account_aext`, `set_account_aext`, `get_or_init_account_aext`.
  - Implement `EvmStateStore` trait (account/code/storage ops) unchanged.
- [ ] Ensure all method signatures and logging stay identical.

Phase 7 – Extract Engine‑Backed Store
- [ ] Create `storage_adapter/engine.rs` and move `EngineBackedEvmStateStore` and all associated helpers:
  - DB name resolvers: `account_database`, `code_database`, `contract_state_database`, `contract_database`, `dynamic_properties_database`, `witness_database`, `votes_database`, `freeze_records_database`, `account_name_database`.
  - Key composition: `account_key`, `code_key`, `witness_key`, `votes_key`, `freeze_record_key`, `contract_storage_key`.
  - Account (de)serialization, including `serialize_account`, `deserialize_account`, `extract_balance_from_protobuf`, and local varint helpers used therein.
  - Witness and votes: `get_witness`, `put_witness`, `remove_witness`, `get_votes`, `put_votes`, `remove_votes`.
  - Freeze ledger: `get_freeze_record`, `set_freeze_record`, `put_freeze_record`, `remove_freeze_record`, `get_tron_power_in_sun`.
  - Dynamic properties and totals: `get_public_net_usage`, `set_public_net_usage`, `get_public_net_time`, `set_public_net_time`, `get_total_net_weight`, `get_total_net_limit`, `compute_total_net_weight`, `compute_total_energy_weight`.
  - Account name: `get_account_name`, `set_account_name` (+ validations) with unchanged behavior.
  - AEXT: `get_account_aext`, `set_account_aext`, `get_or_init_account_aext`.
  - Implement `EvmStateStore` trait unchanged.
- [ ] Ensure all storage_engine interactions and logging remain identical.

Phase 8 – Extract Resource Accounting
- [ ] Create `storage_adapter/resource.rs` and move `ResourceTracker` and `BandwidthPath` as-is.
- [ ] Keep formulas and defaults exactly (window=28800 etc.).

Phase 9 – Tests Relocation (no logic changes)
- [ ] Move tests from the monolith into the owning files under `#[cfg(test)]` modules:
  - Database-related tests (snapshot hooks, modified accounts, revert) → `storage_adapter/database.rs`.
  - Address conversion tests (including provided sample) → `storage_adapter/utils.rs`.
  - Account name tests → `storage_adapter/engine.rs`.
  - Witness/Votes protobuf tests → `storage_adapter/types.rs` (for Witness), and keep Votes tests alongside `types.rs` if present.
  - Freeze ledger and tron power tests (engine/in-memory) → respective files.
  - AEXT serialization roundtrip/with_defaults tests → `storage_adapter/types.rs`.
  - ResourceTracker tests → `storage_adapter/resource.rs`.
- [ ] Do not change assertions or expected values; update imports only.

Phase 10 – Delete Monolith and Wire Up
- [ ] Remove `storage_adapter.rs` after all symbols and tests are relocated.
- [ ] Ensure `lib.rs` compiles using `mod storage_adapter;` and re-exports from `storage_adapter/mod.rs` keep the API stable.

Phase 11 – Build and Validate
- [ ] `cd rust-backend && cargo build --release`
- [ ] `cd rust-backend && cargo test -p execution`
- [ ] `rg -n "EvmStateDatabase" rust-backend/crates/execution/src` → confirm single definition in `storage_adapter/database.rs`.
- [ ] Smoke compile dependent crates if any; ensure no downstream breakages.

Phase 12 – Documentation and Comments
- [ ] Add module-level doc comments to each new file explaining purpose and boundaries.
- [ ] Add a high-level note in `storage_adapter/mod.rs` about public re-exports and compatibility with `lib.rs`.

## Optional Steps (Only if time permits and zero behavior risk)
O1 – Reduce Varint Helper Duplication
- [ ] Lift identical varint read/write used by Vote/VotesRecord/account decode into `utils.rs` as `write_varint_u64` and `read_varint_u64`, preserving exact semantics.
- [ ] Update call sites; keep tests validating serialization round-trips exactly.

O2 – Narrow Internal Visibility
- [ ] Use `pub(crate)` for items that do not need crate‑external visibility, while preserving public API via `mod.rs` re-exports (no external breakage).

## Risk Management
- Cyclic dependencies: keep `traits` and `types` leaf-like; `database` depends on `traits`/`types`/`utils`; `engine` depends on `traits`/`types`/`utils`.
- Behavior drift: avoid refactors beyond moves; copy byte‑exact logic for serialization and address funcs.
- Tests: relocation only; no assertion changes. If a test imports via `super::*`, ensure module paths are updated accordingly within the new file.

## Rollback Plan
- The refactor proceeds in small phases with builds after each extraction. If a step fails, revert only the last moved unit (file) and re-validate. Keep the monolithic file until Phase 10 to simplify rollback.

## Acceptance Criteria
- All execution crate tests pass unchanged.
- `lib.rs` public exports remain identical (no changes required to callers).
- `storage_adapter.rs` removed; new files in place; codebase compiles cleanly.
- No functional diffs in serialization, DB interactions, or resource accounting.

## Quick Implementation Order (Minimal Back-and-Forth)
1) mod.rs + re-exports
2) types.rs
3) traits.rs
4) utils.rs
5) in_memory.rs
6) engine.rs
7) move tests
8) remove monolith
9) validate build/tests

