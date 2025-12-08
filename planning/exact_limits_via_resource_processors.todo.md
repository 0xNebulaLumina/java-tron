Title: Exact Per-Account Limits via Resource Processors — Detailed Plan and TODOs

Context
- We need accurate per-account net_limit and energy_limit in `account_resource_usage_changes_json`.
- These limits are derived (not stored in account bytes/AEXT) and depend on frozen balances, delegations, and dynamic global totals. The single source of truth is the runtime logic in the resource processors:
  - Bandwidth: `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java:calculateGlobalNetLimit(AccountCapsule)`
  - Energy: `chainbase/src/main/java/org/tron/core/db/EnergyProcessor.java:calculateGlobalEnergyLimit(AccountCapsule)`
- Current CSV builder approximates limits from AEXT windows. We will replace this with processor-based computation to match on-chain behavior.

Non‑Goals
- Do not change the CSV header/columns.
- Do not change processor logic or global totals semantics.

Design Summary
- Compute old (pre‑state) and new (post‑state) limits per affected account using the same processors:
  - Remote mode: capture pre‑state inputs in `PreStateSnapshotRegistry` before applying remote results; compute old via snapshot wrappers + processors, new via processors against live stores after apply.
  - Embedded mode: reconstruct pre‑state inputs from `DomainChangeJournal` freeze and global deltas; compute old via wrappers + processors, new via processors against live stores.
- Enrich `AccountResourceUsageDelta` in the CSV builder before canonicalization, setting `net_limit:{old,new}` and `energy_limit:{old,new}`.

Key References
- Bandwidth processor: `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java:486`
- Energy processor: `chainbase/src/main/java/org/tron/core/db/EnergyProcessor.java:96`
- CSV builder (remote + embedded): `framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java`
- AEXT deltas (source for address set): `framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java`
- Remote pre‑state capture (existing): `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:1240`
- Pre‑state registry (existing): `framework/src/main/java/org/tron/core/execution/reporting/PreStateSnapshotRegistry.java`
- Domain journals (embedded): `framework/src/main/java/org/tron/core/execution/reporting/DomainChangeJournal*.java`

Implementation Plan

1) Snapshot Inputs (Remote Pre‑State)
- [x] Add per‑account frozen snapshots to `PreStateSnapshotRegistry`:
  - API: `captureAccountFrozenTotals(byte[] address, long frozenBw, long frozenEnergy)`
  - API: `getAccountFrozenTotals(byte[] address)` → `AccountFrozenTotals(frozenBw, frozenEnergy)`
- [x] In `RuntimeSpiImpl.capturePreStateSnapshot(...)`:
  - [x] Build set of affected addresses from `ExecutionProgramResult.getStateChanges()` (empty key ⇒ account changes) and from `getFreezeChanges()` owners.
  - [x] For each address, load `AccountCapsule` and capture `getAllFrozenBalanceForBandwidth()` and `getAllFrozenBalanceForEnergy()`.
  - [x] Ensure we already capture global totals pre‑state (present):
        `total_net_weight`, `total_net_limit`, `total_energy_weight`, `total_energy_current_limit`.
  - [x] Flags (`supportUnfreezeDelay`, `allowNewReward`) are read from live store at computation time (stable intra‑tx).

2) Embedded Pre‑State Reconstruction
- [x] From `DomainChangeJournalRegistry` obtain freeze deltas and global resource deltas.
  - [x] For each account address we will enrich, derive pre‑state frozen sums by resource:
        `frozenBw_old` = sum of `old_amount_sun` for `BANDWIDTH`,
        `frozenEnergy_old` = sum of `old_amount_sun` for `ENERGY`.
  - [x] Derive global totals at pre‑state:
        if journal contains `total_*` entries, take their `old` values; else read current totals as approximation (no change occurred).
- [x] If an account has no freeze deltas, assume its frozen sums did not change in tx; use post‑state values for both old/new (logged as fallback).

3) Snapshot Wrappers for Processor Reuse
- [x] Create `SnapshotDynamicPropertiesStore` (framework) that delegates to the real store except:
  - [x] Override getters used by processors to return pre‑state totals:
        `getTotalNetWeight()`, `getTotalNetLimit()`, `getTotalEnergyWeight()`, `getTotalEnergyCurrentLimit()`
  - [x] Flags delegated to live store (stable intra‑tx).
- [x] Create `SnapshotAccountView` (framework) for processor inputs:
  - [x] Provide account `getAllFrozenBalanceForBandwidth() / Energy()` from snapshot values.
- [x] Limit calculation implemented directly in `AccountLimitEnricher` using the same formulas as processors (avoids complex wrapping).

4) AccountLimitEnricher Helper
- [x] Create `AccountLimitEnricher` (framework) to enrich AEXT deltas with limits.
  - API: `enrichLimits(List<AccountResourceUsageDelta> deltas, TransactionTrace trace, Mode mode)` where `Mode={REMOTE, EMBEDDED}`.
  - [x] Resolve `ChainBaseManager` via `trace.getTransactionContext().getStoreFactory().getChainBaseManager()`.
  - [x] Build processors for new (post‑state):
        `newNetLimit = BandwidthProcessor.calculateGlobalNetLimit(liveAccount)`
        `newEnergyLimit = EnergyProcessor.calculateGlobalEnergyLimit(liveAccount)`
  - [x] Build snapshot inputs for old (pre‑state):
        REMOTE → from `PreStateSnapshotRegistry` per address, plus globals.
        EMBEDDED → from `DomainChangeJournalRegistry` freeze/globals; fallback when missing.
  - [x] Compute `oldNetLimit` and `oldEnergyLimit` using snapshot formula matching processor logic.
  - [x] Find the matching `AccountResourceUsageDelta` by address and set:
        `net_limit:{old,new}` and `energy_limit:{old,new}`.
  - [x] Log when old is unavailable and we fallback.

5) Builder Integration
- [x] ExecutionCsvRecordBuilder (remote): `extractFromExecutionProgramResult(...)`
  - [x] After computing AEXT deltas and after apply is done, call `AccountLimitEnricher.enrichLimits(..., REMOTE)`.
  - [x] Then pass enriched deltas to `DomainCanonicalizer.accountAextToJsonAndDigest(...)`.
- [x] ExecutionCsvRecordBuilder (embedded): `extractFromEmbeddedExecution(...)`
  - [x] After computing AEXT deltas, call `AccountLimitEnricher.enrichLimits(..., EMBEDDED)`.
  - [x] Then canonicalize.

6) Canonicalization
- [x] DomainCanonicalizer already includes `net_limit` and `energy_limit` field writers in `accountAextDeltaToJson`. No header changes required.

7) Tests
- Unit
  - [ ] Bandwidth/Energy snapshot wrappers: given pre‑state totals and frozen sums, ensure `oldNetLimit/oldEnergyLimit` match processor outputs.
  - [ ] AccountLimitEnricher with mocked snapshot inputs computes old/new limits correctly for representative accounts (V1 and V2 via `supportUnfreezeDelay`).
  - [ ] Ensure deltas include `net_limit` and `energy_limit` only when present; verify deterministic digest stability unaffected.
- Integration (Remote)
  - [ ] Craft `ExecutionProgramResult` with an account that changes freezes; capture pre‑state in `PreStateSnapshotRegistry`; run builder; verify limits old/new as expected.
- Integration (Embedded)
  - [ ] Execute a tx that changes freezes; `DomainChangeJournal` records old/new; run builder; verify limits old/new match processors when computed against pre/post‑state.

8) Observability
- [ ] Add debug logs that show old/new inputs for a few enriched accounts (address, frozen sums, totals, computed limits) behind a debug flag.
- [ ] Add a metric counter for accounts enriched per tx (optional).

9) Performance
- Expected O(A) per tx where A = number of accounts with AEXT deltas; processor calls are lightweight.
- Snapshot wrapper is simple and does not persist.

10) Acceptance Criteria
- [ ] For txs with account resource usage, CSV rows include accurate `net_limit` and `energy_limit` old/new values that match Bandwidth/Energy processors.
- [ ] Remote and embedded paths produce identical limits (given the same block/tx set).
- [ ] No CSV header changes; digest determinism preserved.

11) Rollout & Fallback
- [x] Guard enrichment behind `-Dexec.csv.limit.enrichment.enabled=true` (default true) alongside other CSV features.
- [x] If snapshot inputs are missing in embedded mode, set old = new and log a debug note (temporary, to avoid partial data).

Open Questions
- Do we need to snapshot feature flags (`supportUnfreezeDelay`, `allowNewReward`) per tx for absolute fidelity? If these are stable intra‑tx, live lookups suffice; snapshotting is safer for edge cases.
- For embedded mode when no domain deltas exist for freezes/globals, do we prefer skipping old fields entirely or duplicating new values? Current proposal duplicates new values and logs debug.

Quick Task Index (Files Touched)
- [x] `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` — extend pre‑state capture (section 4: capture per-account frozen totals)
- [x] `framework/src/main/java/org/tron/core/execution/reporting/PreStateSnapshotRegistry.java` — per‑account frozen capture APIs (AccountFrozenTotals class + capture/get methods)
- [x] `framework/src/main/java/org/tron/core/execution/reporting/SnapshotDynamicPropertiesStore.java` — NEW: wrapper for pre-state global totals
- [x] `framework/src/main/java/org/tron/core/execution/reporting/SnapshotAccountView.java` — NEW: wrapper for pre-state frozen balances
- [x] `framework/src/main/java/org/tron/core/execution/reporting/AccountLimitEnricher.java` — NEW: enrichment helper with limit computation
- [x] `framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java` — call enrichment (remote + embedded)
- `framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java` — no change needed for JSON fields
- Tests in `framework/src/test/java/org/tron/core/execution/reporting/` — unit + integration (TODO)

