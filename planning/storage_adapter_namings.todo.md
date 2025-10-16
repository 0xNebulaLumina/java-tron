# Storage Adapter Naming Refactor — Detailed TODOs

Owner: Rust Backend Team
Status: Draft (Plan Only; no code changes yet)
Scope: Clarify and align naming around the EVM state storage interface and its implementations; add forward-compatible aliases, then migrate usages with minimal churn.

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

### Phase A — Decision & RFC (no code)
- [ ] Confirm naming choices: `EvmStateStore`, `InMemoryEvmStateStore`, `EngineBackedEvmStateStore`, `EvmStateDatabase`.
- [ ] Decide on `EngineBackedEvmStateStore` vs `PersistentEvmStateStore` (one canonical pick).
- [ ] Document the decision in `planning/storage_adapter_namings.planning.md` (append decision record).
- [ ] Validate no collision with other crates’ exports.

### Phase B — Introduce Aliases (compat layer only)
- [ ] In `rust-backend/crates/execution/src/storage_adapter.rs` (or `lib.rs`), add public re-exports:
  - [ ] `pub use StorageAdapter as EvmStateStore;` (trait alias via re-export)
  - [ ] `pub use InMemoryStorageAdapter as InMemoryEvmStateStore;`
  - [ ] `pub use StorageModuleAdapter as EngineBackedEvmStateStore;`
  - [ ] `pub use StorageAdapterDatabase as EvmStateDatabase;`
- [ ] Add doc comments explaining that these are the preferred names going forward.
- [ ] Do not mark old names deprecated yet.

### Phase C — Method Alias Shims (non-breaking)
- [ ] In `InMemory*` impl:
  - [ ] Add `put_freeze_record(&self, address: &Address, resource: u8, frozen_amount: u64, expiration: i64)` delegating to existing `set_freeze_record` (or vice versa).
  - [ ] Add `compute_tron_power_in_sun(&self, address: &Address, new_model: bool)` delegating to `get_tron_power_in_sun`.
- [ ] In engine-backed impl:
  - [ ] Add `put_freeze_record(&self, address: Address, resource: u8, record: &FreezeRecord)` delegating to existing `set_freeze_record`.
  - [ ] Add `compute_tron_power_in_sun(&self, address: &Address, new_model: bool)` delegating to `get_tron_power_in_sun`.
- [ ] Add doc comments noting preferred method names.

### Phase D — Internal Migration (surgical renames)
- [ ] Update generic bounds to the new trait alias where convenient:
  - [ ] `crates/execution/src/tron_evm.rs` generic impls: `impl<S: EvmStateStore + Send + Sync + 'static>`
  - [ ] `crates/execution/src/lib.rs` APIs that accept `StorageAdapter` → `EvmStateStore` (via alias; no behavior change)
- [ ] Update engine-backed call sites to preferred method names:
  - [ ] `crates/core/src/service.rs`: `get_tron_power_in_sun` → `compute_tron_power_in_sun`
  - [ ] `crates/core/src/service.rs`: continue using `put_witness`, `get_votes`, `set_votes` (already consistent)
  - [ ] Replace `set_freeze_record` usages in tests with `put_freeze_record` (after shims exist)
- [ ] Update unit tests under `crates/execution/src/storage_adapter.rs` to exercise both old and new names during transition.

### Phase E — Deprecation (opt-in first)
- [ ] Add a feature flag `rename-warnings` in `crates/execution/Cargo.toml`.
- [ ] Behind that feature:
  - [ ] Add `#[deprecated(since = "X.Y.Z", note = "Use EvmStateStore")] pub use StorageAdapter as StorageAdapter;` (or deprecate old name via a dedicated `deprecated.rs` re-export module).
  - [ ] Add deprecation on old methods: `set_freeze_record`, `get_tron_power_in_sun` (apply attributes on the wrappers that forward to the new names).
- [ ] Ensure the default build has no warnings; CI job runs with `--features rename-warnings` to surface callers still using old names.

### Phase F — Docs & Tooling
- [ ] Update textual docs:
  - [ ] `planning/*` documents (this file and `storage_adapter_namings.planning.md`).
  - [ ] `rust-backend/docs/FREEZE_BALANCE_PHASE2_SUMMARY.md` examples → `put_freeze_record` / `compute_tron_power_in_sun`.
  - [ ] Any references in `planning/VoteWitnessContract*.md` and `core` service comments.
- [ ] Add a short section in `README.md` (rust-backend subproject) or `docs/` summarizing the new naming scheme and migration notes.

### Phase G — Full Rename (follow-up release)
- [ ] Replace source identifiers (not just re-exports):
  - [ ] Rename type definitions: `StorageAdapter` → `EvmStateStore` (actual item rename), `InMemoryStorageAdapter` → `InMemoryEvmStateStore`, `StorageModuleAdapter` → `EngineBackedEvmStateStore`, `StorageAdapterDatabase` → `EvmStateDatabase`.
  - [ ] Migrate all internal imports and use sites to the new identifiers.
- [ ] Keep old re-exports for at least one release cycle with deprecation.
- [ ] Release notes: highlight rename and deprecation timeline.

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

