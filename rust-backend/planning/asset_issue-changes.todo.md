# AssetIssue/Participate CSV Parity — Option A Plan

Context
- Embedded vs Remote mismatch: TRC-10 AssetIssue/Participate transactions show different  and  because remote CSV only includes a placeholder  (old==new) and omits the actual balance effects (owner fee debit, blackhole credit; owner/issuer TRX transfer for participate).
- Remote execution emits TRC-10 effects as , which Java applies to the local DB post-exec (). The CSV builder only serializes , not .
- Objective: Without changing remote execution semantics, synthesize account-level state changes for TRC-10 operations during CSV building so the remote CSV reflects the same balance updates as embedded.

Goal (Option A)
- Extend CSV generation to include synthesized account-level  entries derived from  and post-apply account snapshots, merging them into the list used for , , and .

Non-Goals
- Do not alter execution behavior in Rust or Java (no new ledger mutations).
- Do not emit freeze/global resource changes in CSV at this stage.
- Do not serialize TRC-10 token maps; CSV  encoding remains balance/nonce/code(+AEXT) only.

Design Principles
- Deterministic: Synthesis must be idempotent and deterministic for digest parity.
- Minimal: Touch only CSV building and a small helper; avoid invasive changes to runtime or stores.
- Safe rollback: Feature flag gated; can be turned off quickly.

High-Level Approach
1) In Java CSV builder, detect successful remote execution with non-empty .
2) Synthesize account-level  entries for TRC-10 operations based on the post-apply balances in the local stores and the known deltas (fees or trxAmount).
3) Replace placeholder owner entries (old==new) and merge synthesized entries into the final list used for CSV output.

Key Code References
- CSV builder: 
- Canonicalization: 
- Embedded journal serialization: 
- Remote program result: 
- Java-side TRC-10 apply: 
- Rust AssetIssue emission (context only): 

Scope
- In: TRC-10 , .
- Out: TRC-10  (not yet emitted), Freeze/Unfreeze ledger changes, Global resource totals.

Feature Flags
-  (default true): enable TRC-10 ledger synthesis into CSV.
- : if true, fail closed (skip synthesis entirely for the tx) when any required account snapshot is missing.

Detailed TODOs

1) Shared AccountInfo codec (to avoid duplication and ensure parity)
- [x] Create `AccountInfoCodec` in `framework/src/main/java/org/tron/core/execution/reporting/` with:
  - [x] `serialize(AccountCapsule)` method
  - [x] `serializeWithAext(AccountCapsule, boolean)` and `serializeAextTail(AccountCapsule)` (package-private)
  - [x] `PROPERTY_INCLUDE_AEXT` system property (guarded by `remote.exec.accountinfo.resources.enabled`)
  - [x] Use the exact logic now inside `StateChangeJournal` to keep byte-for-byte parity
- [x] Refactor `StateChangeJournal` to call `AccountInfoCodec.serialize()` instead of its private copy
- [x] Add minimal unit tests for `AccountInfoCodec` (balance round-trip encoding, AEXT presence toggle)

2) TRC-10 ledger synthesis helper
- [x] Add `LedgerCsvSynthesizer` in `framework/src/main/java/org/tron/core/execution/reporting/`
  - [x] `synthesize(ExecutionProgramResult, TransactionTrace)` method
  - [x] For each `Trc10LedgerChange`:
    - [x] AssetIssue (op=ISSUE):
      - [x] Determine fee from `feeSun` if not null, otherwise from `dynamicStore.getAssetIssueFee()`
      - [x] Determine burn vs blackhole via `dynamicStore.supportBlackHoleOptimization()`
      - [x] Owner new: `accountStore.get(ownerAddress)`; Owner old: clone with `balance+fee`
      - [x] If blackhole: Blackhole new: `accountStore.getBlackhole()`; old: clone with `balance-fee`
      - [x] Build `StateChange` entries for owner (and blackhole if applicable) using `AccountInfoCodec`
    - [x] Participate (op=PARTICIPATE):
      - [x] Inputs: `ownerAddress`, `toAddress` (issuer), `amount` (trxAmount)
      - [x] Owner new: `accountStore.get(ownerAddress)`; old: clone with `balance+amount`
      - [x] Issuer new: `accountStore.get(toAddress)`; old: clone with `balance-amount`
      - [x] Build `StateChange` entries for owner and issuer
  - [x] Error handling:
      - [x] If any required account is missing and `strictMode=true`, abort synthesis and return empty list
      - [x] Otherwise log WARN and skip that particular change
  - [x] Logging: INFO summary per tx; DEBUG per-address delta

3) Merge and placeholder replacement
- [x] In `ExecutionCsvRecordBuilder` (remote branch):
  - [x] Detect remote mode (via `isRemoteMode()` helper) and success
  - [x] If `LedgerCsvSynthesizer.isEnabled()` and `trc10Changes` not empty:
    - [x] Call `LedgerCsvSynthesizer.synthesize()` to obtain `ledgerChanges`
    - [x] Merge with `baseStateChanges` using `mergeReplacePlaceholders()`:
      - [x] Key = `address:keyHex`; keyHex empty for account-level
      - [x] If base has an entry for address with `old==new` (placeholder), replace with synthesized entry
      - [x] Else add synthesized entry if not present
    - [x] Use `mergedStateChanges` for CSV fields: count/json/digest

4) Configuration and toggles
- [x] Honor `exec.csv.include.trc10` (default true) via `LedgerCsvSynthesizer.isEnabled()`
- [x] Honor `exec.csv.ledger.strict` (default false) via `LedgerCsvSynthesizer.isStrictMode()`
- [x] Keep existing `remote.exec.accountinfo.resources.enabled` for AEXT tail (default true)

5) Tests
- Unit: AccountInfoCodec
  - [x] Serialize empty account → expected zero balance/nonce/code hash, optional AEXT tail obeys toggle
  - [x] Changing only balance results in expected last-8-bytes delta in 32-byte balance region
- Unit: LedgerCsvSynthesizer
  - [ ] ISSUE (blackhole):
    - [ ] Mock stores: owner new balance B, blackhole new balance H; fee F
    - [ ] Verify owner old has B+F; blackhole old has H-F; both entries produced
  - [ ] ISSUE (burn):
    - [ ] Verify only owner entry synthesized
  - [ ] PARTICIPATE:
    - [ ] Owner old has B+Amt; issuer old has I-Amt
  - [ ] Strict mode: missing account → empty result when strict=true; partial skip when strict=false
- Integration (lightweight):
  - [ ] Build a TransactionTrace fixture that routes to stores with pre-seeded accounts; invoke builder and assert state_change_count/digest equal expected for known vectors
- Regression on known mismatch (doc-only):
  - [ ] Validate the first mismatch (block 3188, AssetIssue) becomes parity: 2 changes and matching digest

6) Observability
- [x] INFO log per tx: "CSV ledger synthesis: added N TRC-10 state changes for tx {txId} (ops={})"
- [x] DEBUG per synthesized address: address, balance old→new, fee/amount, burn vs blackhole

7) Performance & safety
- [x] Ensure minimal store accesses (owner, issuer, blackhole), one read per address
- [x] No exceptions propagate to CSV builder; guard with try/catch and strictMode flags

8) Rollback plan
- [x] Single flag `exec.csv.include.trc10` disables synthesis without code removal
- [x] Keep placeholder logic intact in base list for quick revert

Acceptance Criteria
- [ ] For TRC-10 Issue txs, remote CSV shows owner+blackhole entries (or owner-only for burn), matching embedded counts and digest
- [ ] For TRC-10 Participate txs, remote CSV shows owner+issuer entries matching embedded parity
- [ ] Non-TRC-10 txs unaffected
- [ ] Feature flags honored; disabling restores current behavior

Risks / Edge Cases
- Blackhole account synthesis requires reliable retrieval via 
- If runtime applies multiple TRC-10 ops in one tx in future, ensure merge handles multiple addresses
- AEXT tail variability: if AEXT is toggled differently between runs, digests will differ; keep default consistent

Open Questions
- Should we ever consult  when present (hint from Rust) vs dynamic store fee? Proposed: prefer dynamic store for parity with Java apply.
- Do we need to support future TRC-10 TRANSFER in synthesis? Out of current scope; document for Phase 2.

Notes for Rust Team
- No code changes required in Rust for Option A.
- Keep emission consistent:  must remain single entry per op for Java to map deterministically.
- Ensure config docs continue to say: CSV parity achieved via Java synthesis; Rust does not emit TRX ledger  for TRC-10.

References
- embedded→csv paths: 
- remote→csv paths: 
- Java TRC-10 apply: 
- Rust TRC-10 emission: 

