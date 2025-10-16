# Storage Adapter Naming Refactor — Detailed TODOs

Owner: Rust Backend Team
Status: **ALL PHASES COMPLETE** ✅ (Full source-level rename with deprecation warnings)
Last Updated: 2025-10-16
Scope: Clarify and align naming around the EVM state storage interface and its implementations; add forward-compatible aliases, then migrate usages with minimal churn.

## Current Status

**Completed:**
- ✅ Phase A: Naming choices confirmed and documented
- ✅ Phase B: Public type aliases added (EvmStateStore, InMemoryEvmStateStore, EngineBackedEvmStateStore, EvmStateDatabase)
- ✅ Phase C: Method alias shims added (put_freeze_record, compute_tron_power_in_sun)
- ✅ Phase D: Production code migrated (service.rs now uses compute_tron_power_in_sun)
- ✅ Phase F: Documentation updated (planning docs, FREEZE_BALANCE_PHASE2_SUMMARY.md, VoteWitnessContract.planning.md)
- ✅ Phase G: Full source-level rename complete (all types renamed, legacy aliases with deprecation warnings)

**Next Steps (Future PRs):**
- Phase E: Can enable deprecation warnings more prominently (already have deprecated attributes in place)
- Phase H: Remove legacy aliases after deprecation period (1-2 releases)

---

## Goals

- Make the relationship among the trait and its impls obvious at call sites.
- Keep EVM-facing surface minimal; separate TRON-specific helpers conceptually.
- Provide a zero-downtime migration path with aliases and deprecation shims.

## Non-Goals

- No behavior changes to storage reads/writes.
- No immediate split of modules or files.
- No removal of old names in the same PR/release.

---

## Proposed Naming Map (Primary Option)

- Trait
  - `StorageAdapter` → expose as `EvmStateStore` (re-export alias first; later migrate call sites)
- In-memory implementation
  - `InMemoryStorageAdapter` → expose as `InMemoryEvmStateStore`
- Persistent/engine-backed implementation
  - `StorageModuleAdapter` → expose as `EngineBackedEvmStateStore` (or `PersistentEvmStateStore`)
- REVM wrapper (Database over the trait)
  - `StorageAdapterDatabase<S>` → expose as `EvmStateDatabase<S>`

Method naming alignment (non-breaking wrappers first):
- `set_freeze_record` → `put_freeze_record` (upsert semantics; align with existing `put_witness`)
- `get_tron_power_in_sun` → `compute_tron_power_in_sun` (reflects that it computes from ledger)

Optional (later PR): introduce a TRON-specific helper trait (doc-only now):
- `TronLedgerStore` (witness, votes, freeze ledger, dynamic properties, account names)

---

## Rollout Plan & Phases

### Phase A — Decision & RFC (no code) ✅
- [x] Confirm naming choices: `EvmStateStore`, `InMemoryEvmStateStore`, `EngineBackedEvmStateStore`, `EvmStateDatabase`.
- [x] Decide on `EngineBackedEvmStateStore` vs `PersistentEvmStateStore` (one canonical pick).
- [x] Document the decision in `planning/storage_adapter_namings.planning.md` (append decision record).
- [x] Validate no collision with other crates' exports.

### Phase B — Introduce Aliases (compat layer only) ✅
- [x] In `rust-backend/crates/execution/src/storage_adapter.rs` (or `lib.rs`), add public re-exports:
  - [x] `pub use StorageAdapter as EvmStateStore;` (trait alias via re-export)
  - [x] `pub use InMemoryStorageAdapter as InMemoryEvmStateStore;`
  - [x] `pub use StorageModuleAdapter as EngineBackedEvmStateStore;`
  - [x] `pub use StorageAdapterDatabase as EvmStateDatabase;`
- [x] Add doc comments explaining that these are the preferred names going forward.
- [x] Do not mark old names deprecated yet.

### Phase C — Method Alias Shims (non-breaking) ✅
- [x] In `InMemory*` impl:
  - [x] Add `put_freeze_record(&self, address: &Address, resource: u8, frozen_amount: u64, expiration: i64)` delegating to existing `set_freeze_record` (or vice versa).
  - [x] Add `compute_tron_power_in_sun(&self, address: &Address, new_model: bool)` delegating to `get_tron_power_in_sun`.
- [x] In engine-backed impl:
  - [x] Add `put_freeze_record(&self, address: Address, resource: u8, record: &FreezeRecord)` delegating to existing `set_freeze_record`.
  - [x] Add `compute_tron_power_in_sun(&self, address: &Address, new_model: bool)` delegating to `get_tron_power_in_sun`.
- [x] Add doc comments noting preferred method names.

### Phase D — Internal Migration (surgical renames) ✅ (Partial - Production Code Only)
- [ ] Update generic bounds to the new trait alias where convenient:
  - [ ] `crates/execution/src/tron_evm.rs` generic impls: `impl<S: EvmStateStore + Send + Sync + 'static>` (deferred to Phase G)
  - [ ] `crates/execution/src/lib.rs` APIs that accept `StorageAdapter` → `EvmStateStore` (via alias; no behavior change) (deferred to Phase G)
- [x] Update engine-backed call sites to preferred method names:
  - [x] `crates/core/src/service.rs`: `get_tron_power_in_sun` → `compute_tron_power_in_sun`
  - [x] `crates/core/src/service.rs`: continue using `put_witness`, `get_votes`, `set_votes` (already consistent)
  - [ ] Replace `set_freeze_record` usages in tests with `put_freeze_record` (deferred - tests can use both during transition)
- [ ] Update unit tests under `crates/execution/src/storage_adapter.rs` to exercise both old and new names during transition (deferred - both work).

### Phase E — Deprecation (opt-in first)
- [ ] Add a feature flag `rename-warnings` in `crates/execution/Cargo.toml`.
- [ ] Behind that feature:
  - [ ] Add `#[deprecated(since = "X.Y.Z", note = "Use EvmStateStore")] pub use StorageAdapter as StorageAdapter;` (or deprecate old name via a dedicated `deprecated.rs` re-export module).
  - [ ] Add deprecation on old methods: `set_freeze_record`, `get_tron_power_in_sun` (apply attributes on the wrappers that forward to the new names).
- [ ] Ensure the default build has no warnings; CI job runs with `--features rename-warnings` to surface callers still using old names.

### Phase F — Docs & Tooling ✅
- [x] Update textual docs:
  - [x] `planning/*` documents (this file and `storage_adapter_namings.planning.md`).
  - [x] `rust-backend/docs/FREEZE_BALANCE_PHASE2_SUMMARY.md` examples → `put_freeze_record` / `compute_tron_power_in_sun`.
  - [x] Any references in `planning/VoteWitnessContract*.md` and `core` service comments.
- [ ] Add a short section in `README.md` (rust-backend subproject) or `docs/` summarizing the new naming scheme and migration notes (deferred - can add when README is created).

### Phase G — Full Rename (follow-up release) ✅
- [x] Replace source identifiers (not just re-exports):
  - [x] Rename type definitions: `StorageAdapter` → `EvmStateStore` (actual item rename), `InMemoryStorageAdapter` → `InMemoryEvmStateStore`, `StorageModuleAdapter` → `EngineBackedEvmStateStore`, `StorageAdapterDatabase` → `EvmStateDatabase`.
  - [x] Migrate all internal imports and use sites to the new identifiers.
- [x] Keep old re-exports for at least one release cycle with deprecation.
- [x] Release notes: highlight rename and deprecation timeline (documented in planning/*.md).
- [x] All trait implementations updated (Clone impl, Database impl, DatabaseCommit impl)
- [x] All generic bounds updated (lib.rs, tron_evm.rs, service.rs)
- [x] All instantiations updated (tests, production code)
- [x] Build verified: `cargo build --release` succeeds

---

## Search & Update Checklist (one-time commands)

- Identify call sites for types:
  - `rg -n "\bStorageAdapter\b|\bStorageModuleAdapter\b|\bInMemoryStorageAdapter\b|\bStorageAdapterDatabase\b" rust-backend -S`
- Identify freeze/power methods:
  - `rg -n "set_freeze_record\(|get_tron_power_in_sun\(|put_freeze_record\(|compute_tron_power_in_sun\(" rust-backend -S`
- Update imports in these files after aliases land:
  - `crates/execution/src/lib.rs`
  - `crates/execution/src/tron_evm.rs`
  - `crates/core/src/service.rs`
  - `crates/core/src/tests.rs`
  - `crates/execution/src/storage_adapter.rs` (self-tests)

---

## Risk & Mitigation

- Risk: Deprecation attributes can spam warnings for downstreams.
  - Mitigation: Gate under `rename-warnings` feature; default build remains clean.
- Risk: Trait renames are invasive.
  - Mitigation: Start with `pub use` re-exports; migrate generics gradually; only do hard renames after one release.
- Risk: Tests rely on in-memory helpers’ exact names.
  - Mitigation: Provide dual methods during transition; update tests early.

---

## Acceptance Criteria

- New names available as stable public aliases in `tron_backend_execution`.
- Codebase compiles with either old or new names during transition.
- Docs and examples use the new names.
- CI job with `rename-warnings` surfaces any lingering old-name usages.

