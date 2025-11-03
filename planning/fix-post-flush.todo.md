# Fix Post‑Exec Flush — TODOs (Make TRC‑10 apply visible to remote DB)

Status: Detailed plan (no code changes yet)

Goal: Ensure TRC‑10 apply mutations (e.g., AssetIssue owner debit + blackhole credit) are actually flushed to the remote storage after execution in REMOTE mode, so the very next transaction observes the updated balances when reading from the backend.

---

## Background

- Pre‑exec: Manager initializes `ResourceSyncContext`, consumes resources, then calls a pre‑exec flush so remote reads are consistent before execution.
  - Reference: framework/src/main/java/org/tron/core/db/Manager.java:1530
- Execution: Remote backend runs; Java `RuntimeSpiImpl` applies TRC‑10 ISSUE/PARTICIPATE to Java stores and marks accounts/dynamic keys dirty.
  - Apply entry: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:486
- Post‑exec: Runtime claims to flush TRC‑10 mutations, but calls the same pre‑exec API which returns early if already flushed during pre‑exec.
  - Call site: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:469
  - Guard: framework/src/main/java/org/tron/core/storage/sync/ResourceSyncContext.java:208

Problem: `ResourceSyncContext.flushPreExec()` sets a `flushed=true` flag. The post‑exec path calls the same method and exits early ("already flushed"), so no write happens even though new dirty keys were recorded after execution.

---

## Design Summary

- Introduce multi‑phase flushing to `ResourceSyncContext`:
  - Track whether any new mutations were recorded since the last flush (e.g., `dirtySinceFlush`).
  - A dedicated `flushPostExec()` (or a unified `flushNow()`) flushes when there is anything dirty and resets the flag.
  - All `record*Dirty(...)` methods must toggle `dirtySinceFlush=true` to signal a pending flush.
- Runtime post‑exec flush should call the new API and only log success if an actual flush occurred.
- Keep pre‑exec behavior unchanged; retain gating flags and circuit‑breaker behavior.

---

## Detailed TODOs

### A) ResourceSyncContext: multi‑phase flush support

File: framework/src/main/java/org/tron/core/storage/sync/ResourceSyncContext.java

- Data model
  - [ ] Add `boolean dirtySinceFlush` to `ResourceSyncData` and initialize to `false`.
  - [ ] In `clear()`, reset `dirtySinceFlush=false`.
- Marking mutations
  - [ ] In `recordAccountDirty(...)`: after adding to set, set `dirtySinceFlush=true`.
  - [ ] In `recordDynamicKeyDirty(...)`: set `dirtySinceFlush=true`.
  - [ ] In `recordAssetIssueDirtyV1(...)`: set `dirtySinceFlush=true`.
  - [ ] In `recordAssetIssueDirtyV2(...)`: set `dirtySinceFlush=true`.
- Flushing APIs
  - Option 1 (preferred): add explicit stages
    - [ ] Rename current `flushPreExec()` to `flushInternal(String stage)` (private) returning `boolean flushed`.
    - [ ] New `public static boolean flushPreExec()` → calls `flushInternal("pre")`.
    - [ ] New `public static boolean flushPostExec()` → calls `flushInternal("post")`.
    - [ ] In `flushInternal`:
      - [ ] If context is null → return false.
      - [ ] If no dirty keys OR `dirtySinceFlush==false` → log `debug` "skip (nothing dirty)" and return false.
      - [ ] Build batches (accounts → dynamic → asset V1 → asset V2) and call `ResourceSyncService.flushResourceDeltas(...)`.
      - [ ] On success: clear all dirty sets and set `dirtySinceFlush=false`.
      - [ ] Return true on success, false on exception.
  - Option 2 (simple): unify
    - [ ] Replace `flushed` with clearing of dirty sets after each flush; `flushNow()` runs whenever sets are non‑empty.
- Metrics/diagnostics
  - [ ] Update `getCurrentMetrics()` to include `dirtySinceFlush` and counts.
  - [ ] Add debug log when skipping due to nothing dirty since last flush.

### B) RuntimeSpiImpl: call correct post‑exec flush

File: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java

- Post‑exec path
  - [ ] In `flushPostExecTrc10Mutations(...)` (around 438): call `ResourceSyncContext.flushPostExec()` (or `flushNow()`) instead of `flushPreExec()`.
  - [ ] Capture the boolean return to drive logging:
    - [ ] If false → `DEBUG` "No post‑exec resource mutations to flush for tx ...".
    - [ ] If true → `INFO` "Successfully flushed TRC‑10 post‑exec mutations for tx ...".
  - [ ] Keep existing flag gate `-Dremote.resource.sync.postexec=true` and REMOTE‑mode check.

### C) Logging truthfulness and safety

- ResourceSyncContext
  - [ ] Avoid logging "already flushed" in a way that masks post‑exec work; prefer a clear skip message including dirty counts.
- RuntimeSpiImpl
  - [ ] Only log success when a flush actually performed writes (returned true).

### D) Tests (deterministic)

- Unit: ResourceSyncContext
  - [ ] Begin → record dirty → `flushPreExec()` returns true and clears sets.
  - [ ] Record more dirty → `flushPostExec()` returns true and clears sets.
  - [ ] Calling `flushPostExec()` with nothing dirty returns false and logs skip.
- Unit: RuntimeSpiImpl.flushPostExecTrc10Mutations
  - [ ] With pending dirty keys: logs success exactly once; with none: logs skip.
- Integration smoke (REMOTE mode)
  - [ ] Execute AssetIssue then a tx that reads blackhole (e.g., WitnessCreate). Assert blackhole oldValue in tx2 reflects the +fee from tx1.

### E) Flags / Config

- [ ] No new flags required; defaults remain:
  - `remote.resource.sync.enabled=true` in REMOTE mode
  - `remote.resource.sync.postexec=true`
- [ ] Optional: expose a hidden debug flag to force post‑exec flush even if nothing is dirty (for troubleshooting only).

### F) Rollback plan

- [ ] Revert Runtime to call pre‑exec flush only and disable post‑exec via `-Dremote.resource.sync.postexec=false`.
- [ ] ResourceSyncContext can keep the new API; it’s safe when unused.

### G) Risks & Mitigations

- Duplicate writes: Clearing dirty sets after a flush eliminates repeated writes; batch writes remain idempotent at the DB layer.
- Throughput: Additional per‑tx flush may add small latency; kept gated to REMOTE mode, and only runs when there are new mutations.
- Logging confusion: Update messages to avoid false positives (don’t log success on skipped flush).

### H) Acceptance Criteria

- Post‑exec flush writes TRC‑10 owner/blackhole deltas to remote storage.
- Next tx in the same block observes the updated balances from the backend.
- CSV `state_changes_json` and `state_digest_sha256` converge with embedded for the verified window.
- Unit tests pass; logs are truthful about whether a flush occurred.

---

## Implementation Notes (for devs)

- Keep changes surgical: only `ResourceSyncContext` and `RuntimeSpiImpl.flushPostExecTrc10Mutations` behavior.
- Do not alter Manager’s pre‑exec flow.
- Maintain circuit‑breaker behavior in ResourceSyncService; failures should not abort tx flow.
- Prefer adding return boolean to flush methods for precise calling‑site logs.

