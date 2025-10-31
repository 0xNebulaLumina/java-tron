Pre‑State‑2 Plan: Remote Read Parity For TRC‑10 Across Blocks

Context
- Symptom: Remote (execution+storage) CSV first mismatch at block 3189, tx f0b6b6afc8bb55f4b1d8b4084f33095eabb2a5f6761de21e7609907c672c1a9a (WitnessCreate). The blackhole account oldValue in remote CSV lags by 1,024,000,000 SUN compared to embedded.
- Preceding tx (block 3188, AssetIssue) credits blackhole by 1,024,000,000 SUN. Embedded WitnessCreate reads the post‑AssetIssue balance; remote reads the pre‑AssetIssue balance.
- Root cause: The Rust overlay applies TRC‑10 deltas only within the block (shadow), while Java applies TRC‑10 to storage after execution. The next block’s first remote read can occur before Java’s TRC‑10 mutations are flushed/visible in the remote storage snapshot used by the backend.

Goal
- Ensure remote execution reads a storage state that reflects Java‑applied TRC‑10 updates before executing the next block, producing the same oldValue/newValue and state digests as embedded.

Guiding Principles
- Preserve current execution semantics; fix visibility/ordering of TRC‑10 persistence.
- Prefer targeted changes with explicit toggles and rich logging to aid rollback and triage.
- Keep CSV builder logic (LedgerCsvSynthesizer) intact; address root cause (stale storage reads across blocks).

Scope
- Java framework/chainbase: mark TRC‑10 account/dynamic mutations as “dirty” and flush to remote storage in‑band after application; add a guarded post‑exec flush barrier in RuntimeSpiImpl.
- Rust backend: maintain per‑block overlay, add a guarded overlay seeding fallback on block boundary (only when storage hasn’t caught up), and consider a storage snapshot barrier for new blocks.

Non‑Goals
- Do not change state change formats or CSV schema.
- Do not alter fee/burning logic semantics.

References (for implementers)
- Java TRC‑10 apply: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- Account store (remote path): `chainbase/src/main/java/org/tron/core/store/AccountStore.java`
- Revoking/DB adapters: `chainbase/src/main/java/org/tron/core/db/TronStoreWithRevoking.java`, `chainbase/src/main/java/org/tron/core/db/TronDatabase.java`
- Storage backend wiring: `framework/src/main/java/org/tron/core/storage/spi/StorageBackendFactoryImpl.java`, `framework/src/main/java/org/tron/core/storage/spi/StorageSpiBackendAdapter.java`, `framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java`
- Resource sync service: `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java`, `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncContext.java`
- Rust overlay + service: `rust-backend/crates/core/src/service/mod.rs`, `rust-backend/crates/core/src/service/overlay.rs`, `rust-backend/crates/core/src/service/grpc/mod.rs`

---

Java Side — Detailed Plan & TODOs

Objectives
- After `applyTrc10LedgerChanges(...)` (AssetIssue / Participate), ensure the modified accounts and relevant dynamic keys are flushed to remote storage before the next tx/block.
- Add a post‑exec flush barrier in `RuntimeSpiImpl.execute(...)` after TRC‑10 application.

Design Notes
- In remote mode, chainbase stores use `StorageBackendDB` (revoking) over `StorageBackendDbSource`, backed by `StorageSpiBackendAdapter` calling `RemoteStorageSPI`. All `put/batchWrite` calls are synchronous (`.get()`), but revoking snapshots can delay persistence; ResourceSyncService batches and commits through `StorageSPI.batchWrite(...)` directly.

Implementation TODOs
- Toggle and config
  - [x] Introduce JVM property `remote.resource.sync.postexec` (default true in REMOTE mode) to enable post‑exec flushing after TRC‑10 application.
  - [x] Document existing toggles used by the path:
        - `remote.resource.sync.enabled` (default true in REMOTE)
        - `remote.resource.sync.debug` (default false)
        - `remote.resource.sync.confirm` (optional read‑after‑write verification)

- RuntimeSpiImpl post‑exec flush
  - [x] In `RuntimeSpiImpl.execute(...)`, immediately after successful `applyTrc10LedgerChanges(...)` logging (`Successfully applied TRC-10 ledger changes`), call a guarded post‑exec flush:
        - If `StorageSpiFactory.determineStorageMode() == REMOTE` and `remote.resource.sync.postexec=true`, call `ResourceSyncContext.flushPreExec()` to flush dirty sets that were recorded during TRC‑10 apply.
        - Ensure `ResourceSyncContext.finish()` still runs at the end to clear the context.
  - [x] Log an INFO line with txId and flushed counts on post‑exec flush (reuse `ResourceSyncService` debug where possible).

- Mark TRC‑10 mutations as dirty during apply
  - [x] In `applyTrc10AssetIssue(...)`:
        - After debiting owner and crediting blackhole (or burn), record dirty keys:
          - `ResourceSyncContext.recordAccountDirty(ownerAddress)`
          - If blackhole credit (not burn), `recordAccountDirty(blackholeAddress)`
          - If burn, `recordDynamicKeyDirty("BURN_TRX_AMOUNT".getBytes())` (or whatever dynamic key is actually updated during burn)
  - [x] In `applyTrc10AssetParticipate(...)`:
        - After paying owner and crediting issuer TRX balances, mark both addresses dirty via `recordAccountDirty(...)`.
  - [x] Maintain current actuator parity logic for remainSupply and V1/V2 stores; only add dirty‑marking and rely on post‑exec flush to persist.

- Observability
  - [x] Add INFO in `applyTrc10LedgerChanges(...)` summarizing number of dirty accounts/dynamic keys and whether a post‑exec flush will occur.
  - [x] Optionally enable `remote.resource.sync.confirm=true` to perform read‑back confirmation for just‑flushed keys (omit on hot paths in prod if too costly).

- Testing
  - Unit
    - [ ] Add unit tests around `ResourceSyncService` to verify ordering (asset V1 → asset V2 → accounts → dynamic) and that batchWrite is invoked for marked keys.
  - Integration (lightweight)
    - [ ] Drive an AssetIssue then WitnessCreate across block boundary with REMOTE storage, assert blackhole oldValue matches embedded in CSV.
    - [ ] Repeat for ParticipateAssetIssue across boundary (owner debit, issuer credit appear in CSV old/new parity with embedded).
    - [ ] Verify both burn and blackhole modes (dynamic property `supportBlackHoleOptimization`).

- Risks & Mitigation
  - Flush latency: post‑exec flush adds synchronous work; mitigated by batching and single roundtrip.
  - Failure to flush: service already logs errors and uses a circuit breaker; if flush fails, execution proceeds but stale read may persist—ensure logs flag this clearly for triage.

---

Rust Side — Detailed Plan & TODOs

Objectives
- Keep per‑block overlay as the intra‑block truth, but ensure first reads of the next block see the Java‑applied TRC‑10 updates even if the DB propagation is delayed.
- Provide a guarded fallback: seed the next block’s overlay with prior block’s shadow TRC‑10 deltas for specific addresses when DB hasn’t caught up.

Implementation TODOs
- Config toggles
  - [x] Add `remote.overlay.seed_shadow_trc10` (default false) to enable overlay seeding on block boundary.
  - [x] Add `remote.storage.block_barrier` (default false) to optionally force a storage engine snapshot/barrier at block boundary (see below).

- Track shadow TRC‑10 touched addresses
  - [x] Add `last_block_trc10_touched: Arc<RwLock<HashSet<Address>>>` field to `BackendService` struct.
  - [x] In `execute_asset_issue_contract(...)`, after applying overlay deltas, insert owner and blackhole addresses into `last_block_trc10_touched`.
  - [x] In `execute_participate_asset_issue_contract(...)`, insert owner and issuer addresses into the same set when overlay deltas are applied.
  - [x] Clear the set when a new block overlay is created.

- Seed next‑block overlay (guarded)
  - [x] In `get_or_create_overlay(...)` when a new block key is detected, if `remote.overlay.seed_shadow_trc10=true`, attempt to preload the overlay with accounts from `last_block_trc10_touched`:
        - For each address A: read account from storage; if read value appears to lag the prior overlay's value (optional heuristic), write the prior overlay's AccountInfo into the new overlay for A.
        - Add DEBUG/INFO logs summarizing how many addresses were seeded and for which ops.
  - [x] Ensure this does not double‑apply deltas (only writes to overlay, not DB; and only seeds if storage is behind or always seed overlay state as a cache source while DB is authoritative for persistence).
  - [x] Add `read_account_from_storage` helper method to read accounts from storage engine for seeding.

- Optional storage barrier at block boundary
  - [ ] Provide a storage engine “refresh/snapshot” API that is invoked on new block creation if `remote.storage.block_barrier=true` (forces visibility of any prior writes committed via gRPC before the block starts execution).
  - [ ] Wire this call early in the gRPC path before first tx of a block.

- Diagnostics
  - [x] Add INFO logs on WitnessCreate/AssetIssue/Participate showing pre‑read balances for key accounts (owner/blackhole/issuer) and whether overlay or storage was used (HIT/MISS).
  - [x] Add DEBUG logs for TRC-10 touched address tracking and overlay seeding operations.
  - [x] Add INFO logs for block boundary detection and overlay creation/seeding.
  - [ ] Add metrics counters for overlay seed operations and storage barrier invocations (future enhancement).

- Testing
  - [ ] With `remote.overlay.seed_shadow_trc10=true`, reproduce 3188→3189 sequence and assert parity.
  - [ ] With `remote.storage.block_barrier=true`, verify parity without seeding; use this as a stricter read‑your‑writes mode when available.

- Rollback Plan
  - Keep both toggles off by default. Enable on test nets first, then on canaries. Full disable path restored by flipping toggles.

---

End‑to‑End Validation Plan
- Reproduce the specific mismatch:
  - [ ] Enable post‑exec Java flush path. Run AssetIssue at block N, then WitnessCreate at block N+1. Confirm blackhole oldValue in remote CSV equals embedded (0x8000000be7406f80 in the reported case).
  - [ ] Validate state digest (`state_digest_sha256`) matches embedded for these rows.
  - [ ] Inspect `remote-java.*.log` to see “Applying TRC‑10 ledger changes …” followed by post‑exec flush INFO before the next block begins.
- Broader checks:
  - [ ] ParticipateAssetIssue across block boundary (owner debit/issuer credit) and both burn/blackhole modes.
  - [ ] Ensure no regressions for intra‑block parity (overlay remains correct inside a block).

---

Risk Analysis
- Added synchronous work (flush) increases tx processing time marginally; mitigated by batching.
- Overlay seeding risk: if misapplied, could surface transient values when DB already has the latest state. Guard with heuristics and config; keep read precedence: overlay over storage.
- Storage barrier feasibility depends on storage engine capabilities; keep behind a flag.

Operational Considerations
- Expose toggles via JVM properties and Rust TOML config; document defaults.
- Add a short “How to enable/disable” with example flags:
  - Java: `-Dremote.resource.sync.enabled=true -Dremote.resource.sync.postexec=true -Dremote.resource.sync.confirm=false`
  - Rust (config.toml):
    - `overlay_enabled = true`
    - `overlay_shadow_trc10 = true`
    - `overlay_seed_shadow_trc10 = true` (new)
    - `storage_block_barrier = false` (new)

Acceptance Criteria
- For the known first mismatch (block 3188→3189), remote and embedded CSV agree on:
  - state_change_count
  - state_changes_json (addresses and old/new byte payloads)
  - state_digest_sha256
- No regressions on earlier rows; performance impact within acceptable bounds.

Appendix — File Touch List (planned)
- Java
  - `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` (post‑exec flush site, dirty marking)
  - `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java` (optional helper/logging)
  - `framework/src/main/java/org/tron/core/storage/spi/StorageSpiFactory.java` (no code change, reference only)
  - `chainbase/src/main/java/org/tron/core/store/AccountStore.java` (reference; no functional change)
- Rust
  - `rust-backend/crates/core/src/service/mod.rs` (track touched addresses; seed overlay; optional barrier)
  - `rust-backend/crates/core/src/service/overlay.rs` (no change; reference)
  - `rust-backend/crates/core/src/service/grpc/mod.rs` (invoke overlay seeding + barrier)

