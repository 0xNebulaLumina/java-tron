Pre‑State‑2 Review: Detailed Plan & TODOs

Context
- Symptom: First remote vs embedded CSV mismatch at block 3189, tx f0b6…a9a (WitnessCreate). Remote oldValue for blackhole lags by 1,024,000,000 SUN (post‑AssetIssue fee credit from block 3188).
- Root cause: Remote next‑block read didn’t observe Java‑applied TRC‑10 persistence yet. Overlay shadowing worked intra‑block, but the new block’s view came from DB without seeing the Java flush in time.

Current Status (from 14ec601… and 4f0e147…)
- Java (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java)
  - Added post‑exec flush hook `flushPostExecTrc10Mutations(...)` after `applyTrc10LedgerChanges(...)`, guarded by `-Dremote.resource.sync.postexec` (default true in REMOTE).
  - Added dirty‑marking for accounts, dynamic keys, and asset issue V1/V2 keys during TRC‑10 apply.
  - ResourceSync pipeline exists: Manager begins/flushes pre‑exec; Runtime adds post‑exec flush.
- Rust (rust-backend)
  - Added config flags: `overlay_seed_shadow_trc10` (default false), `storage_block_barrier` (default false).
  - Tracks TRC‑10 touched addresses in `last_block_trc10_touched`.
  - Seeds new overlay from storage for touched addresses (not from prior overlay). Clears touched set on block boundary.

Primary Gaps to Fix
1) Overlay seeding source: Should seed from prior overlay (then fallback to storage), not from storage only; otherwise it re‑introduces stale pre‑TRC‑10 values and won’t fix the 3188→3189 mismatch.
2) Seeding heuristic and precedence: When both prior overlay and storage have entries, prefer prior overlay if it includes shadow TRC‑10 deltas; DB remains authoritative for persistence. Overlay is a read‑cache only.
3) Compile/runtime correctness: Replace non‑existent `StorageModule::get_engine()` usage; align helper signatures (use `&StorageEngine` or storage adapter) and error handling.
4) Observability: Add INFO‑level summary of dirty counts/flush decision during TRC‑10 apply; add counters for overlay seeding and optional storage barrier.
5) Optional barrier: Keep `storage_block_barrier` design; wire later if needed (behind flag) for stricter read‑your‑writes.

Goals
- Remote reads for the first tx of block N+1 must see TRC‑10 effects committed by Java after block N, matching embedded oldValue/newValue and state digests without changing semantics of fee logic or CSV schema.
- Provide safe, reversible toggles; rich logging for triage.

Design Overview
- Java: Keep post‑exec flush default‑on and deterministic; ensure complete dirty‑marking; provide INFO logs with counts and tx id.
- Rust: On block boundary, create new overlay seeded with prior overlay account states for addresses touched by shadow TRC‑10 in the previous block; fallback to storage when not available; guard with `overlay_seed_shadow_trc10`. Preserve overlay‑over‑storage read precedence.
- Optional: Storage barrier at block boundary to force storage snapshot refresh, guarded by `storage_block_barrier`.

Rust — Overlay Seeding Plan
- Where: `rust-backend/crates/core/src/service/mod.rs` in `get_or_create_overlay(...)`.
- Steps:
  - Before tearing down the previous overlay, snapshot it under a read lock.
  - Capture `seed_addresses` from `last_block_trc10_touched` (under read lock).
  - For each address A in `seed_addresses`:
    - If previous overlay has AccountInfo for A, write it into the newly created overlay (always safe; read‑cache only).
    - Else, optionally read from storage (via storage adapter or engine), and seed if present.
  - Clear `last_block_trc10_touched` after new overlay creation.
- Pseudocode outline:
  - Acquire write lock on `overlay`.
  - Determine if block changed; if no, return.
  - Read `seed_addresses` and (optional) previous overlay snapshot under read lock.
  - Create `new_overlay` with new block key.
  - If `overlay_seed_shadow_trc10`:
    - For each addr in `seed_addresses`:
      - if prev_overlay.get_account(addr) -> Some(acc) => `new_overlay.put_account(addr, acc)`.
      - else if storage has acc => `new_overlay.put_account(addr, acc)`.
  - Replace `overlay` with `Some(new_overlay)`.
  - Clear `last_block_trc10_touched` (write lock).

Rust — Seeding Heuristics (Optional)
- If both prior overlay and storage have AccountInfo, choose prior overlay when it reflects the latest shadow TRC‑10 deltas (e.g., overlay.balance == storage.balance +/- expected TRC‑10 delta). Default: always prefer prior overlay to ensure cross‑block continuity.
- Add DEBUG logs indicating which source was chosen (OVERLAY vs STORAGE) per address; add INFO summary with counts.

Rust — Storage Access & Helper Fixes
- Replace incorrect `get_engine()` usage with existing `engine()` accessor: `let engine = storage_module_any.engine()?;`.
- Update `read_account_from_storage` signature to accept `&StorageEngine` (not `&Arc<...>`), or reuse a storage adapter (`EngineBackedEvmStateStore`) for account reads to avoid duplicating deserialization logic.
- Ensure helper deserialization matches the real on‑disk layout; prefer the storage adapter’s `get_account(...)` if available.

Java — Post‑Exec Flush & Dirty‑Marking
- Keep `remote.resource.sync.postexec=true` default in REMOTE mode.
- Ensure INFO‑level log after TRC‑10 apply summarizing:
  - txId, counts for: dirty accounts, dirty dynamic keys, dirty asset V1, dirty asset V2.
  - whether post‑exec flush will run (based on mode/flag and `hasTrc10Changes`).
- Confirm all TRC‑10 mutation points mark dirty keys:
  - AssetIssue: owner, blackhole or BURN_TRX_AMOUNT, TOKEN_ID_NUM, asset V1/V2 keys.
  - Participate: owner, issuer; asset balances via V2 methods.

Optional Barrier (Future)
- Keep `remote.storage.block_barrier` flag.
- Provide a storage engine refresh/snapshot method; call at block boundary before first tx when enabled.
- Measure performance impact; leave disabled by default.

Observability & Metrics
- Logs
  - INFO: Block boundary detection and overlay creation.
  - INFO: Overlay seeding summary — addresses attempted, seeded from OVERLAY, seeded from STORAGE.
  - DEBUG: Per‑address seeding source and balances.
  - INFO (Java): Post‑exec flush summary (dirty counts, duration, txId).
- Metrics (future enhancement)
  - Counters: overlay_seeding_total, overlay_seeding_from_overlay, overlay_seeding_from_storage.
  - Counter: storage_block_barrier_invocations.
  - Timer: resource_sync_flush_ms.

Validation Plan
- Reproduce 3188→3189 sequence with `overlay_seed_shadow_trc10=true` and `remote.resource.sync.postexec=true`:
  - Assert remote CSV oldValue/newValue for blackhole at 3189 WitnessCreate equals embedded.
  - Verify `state_digest_sha256` parity for those rows.
  - Logs show post‑exec flush INFO after AssetIssue and before 3189 execution.
- Additional checks:
  - ParticipateAssetIssue across block boundary (owner debit, issuer credit) in both blackhole and burn modes.
  - Verify intra‑block parity remains unchanged.
  - Disable overlay seeding (flag=false) to isolate impact of post‑exec flush only.

Rollout Strategy
- Phase A: Keep `remote.resource.sync.postexec=true` (default). Observe parity improvements; measure latency.
- Phase B: Enable `overlay_seed_shadow_trc10=true` in non‑prod, validate cross‑block parity; then selectively enable in prod if needed.
- Phase C (optional): Experiment with `storage_block_barrier=true` in staging; validate performance.

Risks & Mitigations
- Risk: Overlay seeding diverges temporarily if DB already has the update; Mitigation: overlay is read‑only cache; DB remains source of truth for persistence; overlay resets each block.
- Risk: Increased logging volume; Mitigation: guard detailed logs behind DEBUG.
- Risk: Seeding from storage might reintroduce stale reads; Mitigation: prefer prior overlay; only fallback to storage if missing.

Acceptance Criteria
- For the known first mismatch (3188→3189), remote and embedded CSV agree on:
  - state_change_count, state_changes_json, state_digest_sha256, and old/new balances.
- No regressions on earlier rows or intra‑block behavior.
- Latency impact within acceptable bounds (document with before/after measurements).

Detailed TODOs

Java (framework)
- [x] Add INFO log in `applyTrc10LedgerChanges(...)` summarizing dirty counts and whether post‑exec flush is enabled and will run.
- [x] Double‑check dirty‑marking coverage for: owner, blackhole, issuer, TOKEN_ID_NUM, BURN_TRX_AMOUNT, asset V1/V2 keys.
- [x] Confirm post‑exec flush is no‑op in EMBEDDED mode and when `remote.resource.sync.postexec=false`.
- [ ] Document JVM flags in README/config notes.

Rust (core service + common config)
- [x] Change overlay seeding to use previous overlay values first, with storage fallback.
- [x] Snapshot previous overlay under read lock before creating new one; do not mutate previous overlay during seeding.
- [x] Fix storage engine accessor usage (`engine()`), or use `EngineBackedEvmStateStore` for reads.
- [x] Update `read_account_from_storage` to accept `&StorageEngine` or remove if using adapter.
- [x] Add DEBUG per‑address seeding logs and INFO summary.
- [x] Keep `overlay_seed_shadow_trc10` default false; document flag semantics.
- [x] Ensure `last_block_trc10_touched` lifecycle: insert on AssetIssue/Participate, clear on new overlay creation; log sizes.

Rust (optional barrier)
- [ ] Define a storage refresh/snapshot API (no‑op by default) in storage engine.
- [ ] Call barrier at block boundary when `storage_block_barrier=true`; add metrics/logs.

Observability & Metrics
- [ ] Add counters and timers (exposed via existing metrics) for overlay seeding and resource sync flush.
- [ ] Ensure log messages include block number, txId (when applicable), and address counts.

Testing & Validation
- [ ] Script reproducible test for 3188→3189 mismatch case and capture CSV comparison.
- [ ] Test ParticipateAssetIssue across block boundary (both blackhole and burn modes).
- [ ] Run intra‑block parity checks to ensure overlay still yields correct old_account within the same block.
- [ ] Performance benchmark with and without post‑exec flush; with and without seeding.

Documentation
- [ ] Update `rust-backend/config.toml` example with new flags and defaults.
- [ ] Add a brief “How to enable/disable” section to project docs for both Java and Rust flags.

File Touch List (planned)
- Java
  - `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` (INFO log for dirty counts)
  - `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncContext.java` (expose counts if needed)
  - `framework/src/main/java/org/tron/core/db/Manager.java` (no change; confirms begin/flush/finish sequence)
- Rust
  - `rust-backend/crates/core/src/service/mod.rs` (overlay seeding logic; engine access; logging)
  - `rust-backend/crates/common/src/config.rs` (docstrings only; flags already present)
  - `rust-backend/crates/core/src/service/overlay.rs` (no change; used for state cache)
  - `rust-backend/crates/storage/src/lib.rs` (optional: barrier API)

Definition of Done
- Cross‑block TRC‑10 parity achieved for the known mismatch and generalizes to ParticipateAssetIssue.
- Flags documented; defaults safe. Logs and metrics provide sufficient visibility for triage.
- No functional regressions in embedded mode or intra‑block behavior.

