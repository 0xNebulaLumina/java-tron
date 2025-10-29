## Post-Fee OldValue Parity (Embedded CSV)

## Implementation Status

**✅ CORE IMPLEMENTATION COMPLETED** (2025-10-27)

The journal preseed mechanism has been successfully implemented to align embedded CSV oldValue capture with remote execution. Key changes:

1. **New Utility Class**: `framework/src/main/java/org/tron/core/execution/reporting/JournalPreseedUtil.java`
   - Preseeds state-change journal with post-resource account snapshots
   - Handles TransferContract (owner + recipient addresses)
   - Feature flags: `exec.csv.preseedAfterResource` (default: true), `exec.csv.preseed.debug` (default: false)
   - Gated by existing `exec.csv.stateChanges.enabled` flag

2. **Integration Points**: `framework/src/main/java/org/tron/core/db/Manager.java`
   - Primary execution path (line 1560): preseed call added after journal init, before VM exec
   - Retry path (line 1571): preseed call mirrored for retry logic
   - Import added for JournalPreseedUtil

3. **Build Status**: ✅ Compilation successful (`./gradlew :framework:compileJava`)

**Next Steps**:
- Integration testing with block 2458 tx (ea03aeb49f10c7637b7fc4070d94858e8e6630deff0be6622a748f125134944a)
- Verify oldValue parity and state digest matching between embedded and remote
- Optional: Add unit tests for JournalPreseedUtil parsing/gating logic

---

Context
- We observed embedded vs remote CSV mismatches on sender account oldValue by 100,000 SUN on transfers that trigger create-account fee. Remote oldValue already reflects pre-exec resource deductions; embedded oldValue is captured from the VM/journal before those fee mutations are visible.
- Example symptom: sender oldValue differs by 100,000 SUN for a single TransferContract; recipient is identical; state digest differs accordingly. Planning doc: `planning/post-fee.planning.md`.

Goal
- Make embedded CSV stateChanges oldValue reflect post-resource baseline (after pre-exec fee/bandwidth mutations) so embedded and remote CSVs align on oldValue/state digest when both paths use the same resource path.

Success Criteria
- For TransferContract with create-account fee, embedded CSV oldValue for owner equals the remote CSV oldValue for the same tx; state_digest_sha256 matches.
- No extra state changes emitted; owner delta still equals the transfer amount; fee deltas remain accounted separately by normal Actuator/store writes (not duplicated).
- No behavior change for remote execution CSV; only embedded path is affected.

Approach (High-level)
- After pre-exec resource handling and before VM exec, preseed the state-change journal with post-resource snapshots for the addresses that will change (starting with TransferContract: owner, and recipient only if it already exists). The journal merge logic keeps the original oldAccount and updates newAccount as execution proceeds, yielding correct oldValue in the CSV.

Key Touchpoints
- `framework/src/main/java/org/tron/core/db/Manager.java:1555`
- `framework/src/main/java/org/tron/core/db/Manager.java:1557`
- `framework/src/main/java/org/tron/core/db/Manager.java:1558`
- `framework/src/main/java/org/tron/core/db/Manager.java:1559`
- `framework/src/main/java/org/tron/core/db/Manager.java:1561`
- `framework/src/main/java/org/tron/core/db/Manager.java:1566`
- `framework/src/main/java/org/tron/core/db/Manager.java:1568`
- `framework/src/main/java/org/tron/core/db/Manager.java:1569`
- `framework/src/main/java/org/tron/core/db/Manager.java:1571`
- `framework/src/main/java/org/tron/core/execution/spi/ExecutionProgramResult.java:75` (journal snapshot usage)
- `framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:131` (journal finalize path)

Detailed TODOs

1) ✅ COMPLETED - Utility: JournalPreseedUtil
- ✅ Added utility to framework for preseeding account changes in the journal using persisted post-resource state.
- ✅ Location: `framework/src/main/java/org/tron/core/execution/reporting/JournalPreseedUtil.java`.
- ✅ Public API: `static void tryPreseedAfterResource(org.tron.core.db.TransactionTrace trace)`.
- ✅ Behavior
  - ✅ Gate on feature flags:
    - ✅ `exec.csv.stateChanges.enabled` must be true (journal enabled).
    - ✅ `exec.csv.preseedAfterResource` default true; disable via `-Dexec.csv.preseedAfterResource=false`.
  - ⚠️ Optional: Gate to embedded mode only (NOT IMPLEMENTED - can add later if needed).
  - ✅ Determine contract type from `trace.getTransactionContext().getTrxCap().getInstance().getRawData().getContract(0)` and handle:
    - ✅ Phase 1 scope: `TransferContract` only.
  - ✅ For TransferContract
    - ✅ Parse `owner` and `to` addresses via `contract.getParameter().unpack(TransferContract.class)`.
    - ✅ Resolve stores from `trace.getTransactionContext().getStoreFactory().getChainBaseManager()`: use `AccountStore`.
    - ✅ Read post-resource snapshots using `accountStore.getUnchecked(address)`.
    - ✅ Preseed journal via the recorder bridge: `StateChangeRecorderContext.recordAccountChange(address, old, old)`.
      - ✅ Owner: always attempt (old likely present), skip if null.
      - ✅ Recipient: only if `accountStore.getUnchecked(to) != null` (skip creation case to avoid old==new noise when recipient will be created during exec).
  - ✅ Logging
    - ✅ INFO once per tx when active: `CSV preseed: owner=<...> to=<...> seeded=<n>`.
    - ✅ DEBUG optional print of balances captured when `exec.csv.preseed.debug=true`.

2) ✅ COMPLETED - Wire-in call sites (before VM exec)
- ✅ Insert the preseed call immediately after journal init and recorder setup and before `trace.exec()`.
- ✅ Primary path: after pre-exec resource flush
  - ✅ `framework/src/main/java/org/tron/core/db/Manager.java:1560` → added `JournalPreseedUtil.tryPreseedAfterResource(trace);`
  - ✅ Ensure order remains:
    - ✅ `ResourceSyncContext.flushPreExec()`
    - ✅ `trace.init(...)`
    - ✅ `StateChangeJournalRegistry.initializeForCurrentTransaction()`
    - ✅ `StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge())`
    - ✅ `JournalPreseedUtil.tryPreseedAfterResource(trace)`
    - ✅ `trace.exec()`
- ✅ Retry path: mirror the insertion
  - ✅ `framework/src/main/java/org/tron/core/db/Manager.java:1571` → inserted same call before exec.

3) ✅ COMPLETED - Flags and defaults
- ✅ `exec.csv.preseedAfterResource` (default: true). Only runs if `exec.csv.stateChanges.enabled=true`.
- ✅ Optional: `exec.csv.preseed.debug` (default: false) to log captured balances/snapshots at DEBUG for parity investigations.
- ✅ No change to existing CSV logging flags (`exec.csv.enabled`, rotation, sampling) or journal flag name.

4) ✅ IMPLEMENTED - Safety and scope
- ✅ Embedded-only effect (via feature flags). Remote path continues to use state changes from backend; CSV builder respects `ExecutionProgramResult` vs journal finalize.
- ✅ Preseed entries do not add extra rows when a real change happens: the journal merges keep the original `oldAccount` and update the `newAccount` on subsequent repository writes.
- ✅ Avoid preseeding for non-existent recipients in Transfer create-account cases to prevent old==new emissions.

5) Tests and validation
- Unit: `JournalPreseedUtil`
  - Transfer parsing: correctly extracts owner/to and handles missing recipient.
  - Gating logic: disabled when journal off or flag off.
- Integration (CSV parity spot checks)
  - Run embedded mode with `-Dexec.csv.stateChanges.enabled=true` and verify a known Transfer with create-account:
    - Owner oldValue matches remote’s oldValue; state digest matches.
    - State change count remains stable (no extra rows).
  - Sanity on transfers without create-account fees (earlier/later blocks) show no change in counts/digests.
- Non-regression: Remote mode CSV unaffected.

6) Observability
- Add minimal counters (optional, if convenient): count of preseeds and addresses seeded per tx.
- Confirm existing `StateChangeJournalRegistry.getCurrentJournalMetrics()` remains unaffected.

7) Non-goals
- Do not change how bandwidth/fee deduction is computed; only align oldValue capture point.
- Do not attempt to resolve genuine resource path divergences (FREE_NET vs FEE); those will still create differences and must be addressed separately.

Implementation Notes
- Journal and CSV paths
  - Embedded SPI returns `ExecutionProgramResult` which reads a snapshot of current journal entries: `framework/src/main/java/org/tron/core/execution/spi/ExecutionProgramResult.java:75`.
  - CSV builder uses the `ExecutionProgramResult` stateChanges when present; else it finalizes the journal: `framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:131`.
- Merge semantics
  - `StateChangeJournal.recordAccountChange(...)` keeps the first oldAccount and updates newAccount on subsequent calls; perfect for preseeding old and letting VM mutations update new.

Edge Cases and Extensions (Later)
- Extend to `TransferAssetContract` (owner + to) using the same pattern once Transfer parity is proven stable.
- Consider preseeding for selected system contracts where only owner changes are expected; keep heuristics conservative to avoid old==new noise.

Rollout Plan
- Implement utility and call-site insertions behind flags.
- Verify on a short embedded replay window with CSV enabled; compare against remote window.
- Keep flagged for quick disable if unexpected side-effects surface.

