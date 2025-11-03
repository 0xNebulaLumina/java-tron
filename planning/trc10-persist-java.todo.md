# TRC-10 Persist in Java — TODOs (ISSUE/PARTICIPATE owner/blackhole deltas)

Status: Actionable plan (no code yet)

Goal: When executing in REMOTE mode, apply the same owner/blackhole TRX deltas for TRC‑10 AssetIssue and ParticipateAssetIssue to the Java `AccountStore` (and flush to remote storage), so subsequent transactions in the same block see the updated balances. Keep CSV parity; eliminate “shadow-only” mismatches like the WitnessCreate vs AssetIssue gap.

---

## Problem Statement (observed)

- Remote CSV shows a blackhole credit for TRC‑10 AssetIssue, but the next transaction’s account read did not reflect it, leading to a balance gap (e.g., +1,024 TRX missing in the remote store’s oldValue).
- Root symptom: `trc10Changes` were emitted and included in CSV synthesis, but Java store mutations weren’t visible to subsequent remote reads.
- We must ensure Java applies those deltas and then flushes them to the remote storage backend before the next tx uses them.

---

## Scope

- Java only (no Rust code changes):
  - Runtime apply path and post‑exec flush behavior
  - Flags and defaults
  - Logging/metrics
  - Tests and validation scaffolding

Out of scope: Changing protobufs, altering Rust execution semantics, adding TRC‑10 TRANSFER (tracked separately).

---

## Current Flow (key touchpoints)

- SPI conversion: gRPC → Java result with `trc10Changes`
  - `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java` (convertExecuteTransactionResponse)
- Runtime execution wrapper (remote):
  - `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
    - `applyStateChangesToLocalDatabase`
    - `applyFreezeLedgerChanges`
    - `applyTrc10LedgerChanges`
    - `flushPostExecTrc10Mutations`
- Resource sync to remote storage:
  - `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncContext.java`
  - `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java`

---

## Design Principles

- Single source of truth: Store mutations (owner debit, blackhole credit or burn) must live in AccountStore and be flushed; CSV should reflect what the store persists.
- Prefer backend‑supplied amounts when present (e.g., `feeSun`) to avoid dynamic store drift for historical blocks.
- Deterministic, idempotent apply path; non‑throwing on apply failures (log and continue), to not break tx flow.
- Easy rollback: flags to disable apply or flush if needed.

---

## Tasks

### A. Verify and tighten SPI result plumbing

- [x] Ensure `RemoteExecutionSPI.convertExecuteTransactionResponse(...)` maps `protoResult.getTrc10ChangesList()` to `ExecutionSPI.Trc10LedgerChange` including:
  - [x] `op` (ISSUE/PARTICIPATE)
  - [x] `ownerAddress`, `toAddress`
  - [x] `totalSupply`, `precision`, `trxNum`, `num`, `startTime`, `endTime`, `frozenSupply[]`
  - [x] Optional `feeSun`
  - [x] Deterministic ordering (op → owner bytes → asset_id bytes) before attaching to result
- [x] Confirm `ExecutionProgramResult` carries `trc10Changes` end‑to‑end (getter/setter), and `RuntimeSpiImpl.execute(...)` receives them.

### B. Apply TRC‑10 ISSUE in Java (parity with actuator)

File: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`

- [x] In `applyTrc10AssetIssue(...)`:
  - [x] Determine fee: use `trc10Change.feeSun` when non‑null; fallback to `DynamicPropertiesStore.getAssetIssueFee()`.
  - [x] Apply owner TRX debit:
    - [x] Load owner `AccountCapsule`; check sufficient balance; subtract fee; mark dirty.
  - [x] Apply blackhole vs burn:
    - [x] If `DynamicPropertiesStore.supportBlackHoleOptimization()` is true: call `dynamicStore.burnTrx(fee)` and mark `BURN_TRX_AMOUNT` dirty.
    - [x] Else: load blackhole account, credit fee, persist, mark dirty.
  - [x] Asset metadata and remain supply:
    - [x] Build V1/V2 `AssetIssueCapsule` from change; assign token id (increment TOKEN_ID_NUM).
    - [x] Respect `ALLOW_SAME_TOKEN_NAME`: V1+V2 (precision forced to 0 for V2) vs V2‑only.
    - [x] `remainSupply = totalSupply − sum(frozenSupply)`, credit to owner's asset maps (V1/V2 as appropriate).
    - [x] Append frozen supply list to owner account with proper expire times.
  - [x] Persist owner account and asset stores; mark all mutated keys dirty in `ResourceSyncContext`.
  - [x] INFO/DEBUG logs with amounts, blackhole/burn decision, token id, remainSupply.

### C. Apply TRC‑10 PARTICIPATE in Java (parity with actuator)

- [x] In `applyTrc10AssetParticipate(...)`:
  - [x] Resolve asset via V1+V2 or V2 rules (ALLOW_SAME_TOKEN_NAME).
  - [x] Calculate `exchangeAmount = (amount * num) / trxNum` (floor division). Validate amounts > 0.
  - [x] TRX movement: owner −= trxAmount; issuer += trxAmount (overflow safe via Math.addExact checks; log on failure).
  - [x] Token movement: owner token map += exchangeAmount; issuer token map −= exchangeAmount (handle V1/V2 helpers).
  - [x] Persist owner/issuer accounts; mark both dirty; INFO log summary.

### D. Post‑execution flush ordering and semantics

- [x] Keep apply order in `RuntimeSpiImpl.execute(...)`: base state changes → freeze → TRC‑10 → post‑exec flush.
- [x] Ensure post‑exec flush waits for remote write completion:
  - [x] `ResourceSyncService.flushResourceDeltas(...)` composes futures and `.get()`; confirm this path is invoked with `-Dremote.resource.sync.postexec=true` (default).
- [ ] Optional: add a light confirmation (guarded by `-Dremote.resource.sync.confirm=true`) for 1–3 mutated accounts (blackhole, owner) post‑flush.

### E. CSV parity remains intact

- [x] Keep `ExecutionCsvRecordBuilder` synthesis (`LedgerCsvSynthesizer.synthesize(...)`) active. With store mutations flushed, subsequent tx's "oldValue" will match across modes.
- [x] Ensure `LedgerCsvSynthesizer` continues to prefer `feeSun` if present when constructing synthetic owner/blackhole changes; no change needed, but verify.

### F. Flags and defaults

- [x] `-Dremote.exec.apply.trc10=true` (default): controls Java apply path.
- [x] `-Dremote.resource.sync.postexec=true` (default): enables synchronous flush after apply.
- [x] `-Dremote.resource.sync.enabled=true` in REMOTE mode (service auto‑enables); verify defaults.
- [x] Leave `-Dremote.exec.trc10.enabled=false` by default unless we actively route TRC‑10 execution to Rust (separate concern).

### G. Observability

- [x] Add INFO logs summarizing ISSUE/PARTICIPATE apply:
  - [x] Owner and blackhole addresses, fee, remainSupply, token id, exchange amounts
- [x] Add metrics via `MetricsCallback` or simple counters:
  - [x] `remote.trc10.issue.apply_count`, `remote.trc10.participate.apply_count`
- [x] Resource sync: one‑line summary already exists; extend to print whether blackhole/owner addresses were included in the account batch when debug enabled.

### H. Tests (deterministic)

- [ ] Unit tests for `applyTrc10AssetIssue`:
  - [ ] V1 (ALLOW_SAME_TOKEN_NAME=0): V1+V2 stores populated, V2 precision forced to 0, remainSupply, owner debit, blackhole/burn paths, dirty keys recorded.
  - [ ] V2 only: V2 store only.
  - [ ] `feeSun` override vs dynamic fee.
- [ ] Unit tests for `applyTrc10AssetParticipate`:
  - [ ] Resolve asset; TRX/token deltas correct; insufficient balance path logs and does not throw; dirty keys recorded.
- [ ] Light integration (block‑local consistency):
  - [ ] Execute AssetIssue then WitnessCreate in REMOTE mode; assert blackhole oldValue in the second tx reflects the fee credit.

### I. Edge cases & safety

- [ ] Handle overflow/underflow with `Math.addExact`/`Math.subtractExact` and log errors without throwing.
- [ ] Missing blackhole account: create or log+continue per existing actuator semantics; mark as strict‑off by default.
- [ ] Keep AEXT tail accounting untouched; account serialization for store flush remains Tron Account protobuf bytes via `AccountCapsule.getData()`.
- [ ] Deterministic ordering for multiple TRC‑10 changes in a single tx (future‑proofing).

### J. Rollback & guardrails

- [ ] Rapid rollback by disabling `-Dremote.exec.apply.trc10=false` (apply off) or `-Dremote.resource.sync.postexec=false` (flush off), or circuit breaker in ResourceSyncService.
- [ ] Debug toggles: `-Dremote.resource.sync.debug=true`, `-Dremote.resource.sync.confirm=true` for deeper inspection.

---

## Validation Checklist

- [ ] Reproduce prior mismatch window: run two consecutive txs (AssetIssue then WitnessCreate) in REMOTE mode; verify blackhole balance in tx2 oldValue includes the +fee from tx1.
- [ ] CSV parity: `state_changes_json` and `state_digest_sha256` match between embedded‑embedded and remote‑remote for the same window.
- [ ] No regressions in bandwidth accounting or freeze changes application.
- [ ] Checkstyle/tests green.

---

## Deliverables

- Java apply path finalized for TRC‑10 ISSUE/PARTICIPATE in `RuntimeSpiImpl`.
- Reliable post‑exec flush to remote storage for mutated accounts and dynamic keys.
- Unit tests covering core apply logic and flags.
- Docs: brief note in run.md on required flags for REMOTE parity.

