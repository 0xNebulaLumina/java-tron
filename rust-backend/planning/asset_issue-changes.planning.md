• Here’s the first concrete mismatch and why it happens.

What mismatched

- Transaction: block 3188, tx_index 0, tx_id 58cc49cd47ba0ef70662823552cb45c0d1da375616fc79ddbd171dcbc97829fd
- Contract: AssetIssueContract
- Field deltas:
    - state_change_count: embedded=2 vs remote=1
    - state_changes_json: remote is missing one account change and the remaining change shows old==new
    - state_digest_sha256 differs accordingly

CSV evidence

- First differing row (ignoring the run/mode columns and final ts_ms):
    - embedded: state_change_count=2 with two account updates
        - address 414d1ef8673f916debb7e2515a8f3ecaf2611034aa (owner): oldValue→newValue changed
        - address 4177944d19c052b73ee2286823aa83f8138cb7032f (blackhole): oldValue→newValue increased
    - remote: state_change_count=1 with only owner address and oldValue==newValue; blackhole entry absent
- How I found it: computed first mismatch after dropping columns 1–3 and last column, line 1124 of the comparable streams

Correlated logs

- Remote Java shows 1 “state change” + separate TRC‑10 application
    - remote-java.0c28597.log:159048 “Applying 1 state changes… for transaction: 58cc49…”
    - remote-java.0c28597.log:159053 “Applying TRC-10 ledger changes… (count=1)”
- Remote Rust shows state_changes: 1 for AssetIssue
    - remote-rust.0c28597.log:05:10:16.298254 “Non-VM contract executed successfully … state_changes: 1”
- Embedded Java emits no RuntimeSpiImpl logs (not used), but CSV records two balance-affecting account changes as expected for AssetIssue.

Why this happens

- Remote execution intentionally does not emit TRX balance/ledger mutations for TRC‑10 contracts as “state_changes”; it only emits a placeholder AccountChange with old==new, and returns the real TRC‑10
effects separately as trc10_changes. See:
    - rust-backend/crates/core/src/service/mod.rs:1478–1508
        - For AssetIssue, builds state_changes with exactly one AccountChange (owner, old==new) and puts the actual issuance and fee info in trc10_changes.
    - framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:457–508
        - Java side applies trc10Changes to the local DB after execution (“Applying TRC-10 ledger changes…”).
- The CSV builder only serializes ExecutionProgramResult.getStateChanges and ignores trc10Changes:
    - framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:85–137
        - For remote runs (ExecutionProgramResult), it uses result.getStateChanges() only to populate state_change_count/state_changes_json/state_digest_sha256.
- Result: embedded CSV includes both owner and blackhole balance changes; remote CSV shows only the placeholder, so counts and digest differ.

Code references

- RuntimeSpiImpl applying ledger changes: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:148, 457
- CSV builder excludes trc10_changes: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:104–137
- Remote AssetIssue emits 1 placeholder state change, real effects in trc10_changes: rust-backend/crates/core/src/service/mod.rs:1478–1508

What to do next

- Option A (CSV parity fix in Java): Extend ExecutionCsvRecordBuilder to merge trc10Changes (and freeze/global changes where appropriate) into the CSV’s state_changes_json/state_digest for remote runs. This
keeps the CSV a “what changed” record regardless of where changes are applied.
- Option B (emit richer state_changes in Rust): Teach execute_asset_issue_contract to compute and include the real TRX ledger changes as AccountChange entries (owner/blackhole) in state_changes. This requires
fee calculation availability on Rust side (today delegated to Java), so Option A is lower-risk short-term.
- Option C (journal capture during Java apply): Have RuntimeSpiImpl record StateChangeJournal entries when applying TRC‑10/freeze changes to local DB, and update the CSV builder for remote runs to merge
programResult.stateChanges with StateChangeJournalRegistry.finalizeForCurrentTransaction(). This preserves the “source of truth is what actually got applied”.

Want me to:

- implement Option A in the CSV builder so remote CSV lines include TRC‑10 balance changes (owner and blackhole), or
- proceed to analyze the second mismatch to confirm it’s the same root cause across TRC‑10 operations?


---

No need to start implementing, just read through and understand the codebase and give me a detailed plan on: option A

---

Got it — here’s a concrete, code-aware plan to implement Option A: make the remote CSV include the ledger effects (TRC‑10) so state_change_count/state_changes_json/state_digest_sha256 match embedded.

Why Option A is viable

- The CSV state_changes use a normalized “AccountInfo” byte encoding that only includes balance/nonce/code(+AEXT), not TRC‑10 asset maps.
- TRC‑10 Issue/Participate modify TRX balances on a few accounts (owner, issuer, blackhole), which we can reconstruct deterministically.
- We can synthesize account-level StateChange entries from the already-applied ledger changes (post-exec DB state) without altering the execution path.

Plan Overview

- Extend the CSV builder to synthesize account-level StateChange entries from ExecutionProgramResult.trc10Changes for remote runs, using post-apply accounts from stores and reversing deltas to recover the
pre-apply “old” snapshot.
- Merge these synthetic changes with ExecutionProgramResult.stateChanges (replacing placeholder old==new entries) before computing state_change_count, state_changes_json, and state_digest_sha256.

Scope and Criteria

- Scope: TRC‑10 Issue and ParticipateAssetIssue only.
- Success: For TRC‑10 transactions, remote CSV rows show correct count and digest parity with embedded, e.g., the first mismatch at block 3188 now has two changes (owner fee debit + blackhole credit).

Key Code Touchpoints

- ExecutionCsvRecordBuilder: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:85–137
- Canonicalizer/serialization used by embedded: framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournal.java:252 (serializeAccountInfo)
- Remote program result with ledger changes: framework/src/main/java/org/tron/core/execution/spi/ExecutionProgramResult.java
- Java-side TRC‑10 apply (for semantic parity): framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:457, 518, 675

Detailed Steps

1. Add a synthesizer to CSV building path

- In ExecutionCsvRecordBuilder.extractExecutionResults (remote path where programResult instanceof ExecutionProgramResult):
    - Get base list: List<StateChange> base = execResult.getStateChanges().
    - If remote execution and execResult.getTrc10Changes() is non-empty and tx succeeded:
        - Invoke a helper to synthesize ledger state changes: List<StateChange> ledgerChanges = LedgerCsvSynthesizer.synthesize(execResult, trace).
        - Merge: resultList = mergeAndDedupe(base, ledgerChanges) with rules:
            - If a placeholder entry exists (same address, empty key, old==new), replace it with a real owner change.
            - Avoid duplicates by address+key.
        - Use resultList for builder.stateChangeCount/stateChanges/state_digest.

2. Implement LedgerCsvSynthesizer (new helper class)

- Location: framework/src/main/java/org/tron/core/execution/reporting/LedgerCsvSynthesizer.java
- Inputs: ExecutionProgramResult execResult, TransactionTrace trace
- Responsibilities:
    - Resolve ChainBaseManager via trace.getTransactionContext().getStoreFactory().getChainBaseManager().
    - For each Trc10LedgerChange:
        - AssetIssue
            - Determine fee: dynamicStore.getAssetIssueFee() (and not from Rust hint unless explicitly present and desired).
            - Decide burn vs blackhole: dynamicStore.supportBlackHoleOptimization()
            - Build two account-level changes:
                - Owner: oldBalance = newBalance + fee; newBalance = post-apply store value.
                - Blackhole (if not burning): oldBalance = newBalance − fee.
            - Use account store to fetch post-apply AccountCapsule for new; clone and adjust only balance for old.
            - Serialize old/new via StateChangeJournal.serializeAccountInfo(AccountCapsule).
            - Build ExecutionSPI.StateChange with address, key = empty, oldBytes, newBytes.
        - ParticipateAssetIssue
            - Owner: oldBalance = newBalance + trxAmount.
            - Issuer: oldBalance = newBalance − trxAmount.
            - Same cloning/serialization process; omit token map handling (not encoded in CSV serialization).
    - Sorting not required here; canonicalizer sorts.

3. Serialization strategy

- Reuse StateChangeJournal.serializeAccountInfo(AccountCapsule) to produce the same “AccountInfo” encoding used by embedded for CSV (balance/nonce/code + optional AEXT tail).
- This avoids reinventing serializers and guarantees canonical parity (AEXT tail contents taken from store fields).

4. Replacement and dedupe logic

- If base contains a placeholder for owner (old==new) from Rust (e.g., AssetIssue emits one), replace it with the synthesized real change.
- Ensure we do not end up with both placeholder and real changes for the same address (address+key empty match).
- If for any reason synthesis fails for a specific address (e.g., account missing), keep base unchanged and continue with others; add a strict toggle to fail closed if desired.

5. Configuration flags

- Add JVM props to control behavior:
    - exec.csv.include.trc10=true (default true): turn on TRC‑10 synthesis.
    - exec.csv.ledger.strict=false: if true, when any ledger change can’t be synthesized, drop all ledger synthesis for that tx (safer for parity).
- Do not include freeze/global changes in CSV (serializer ignores frozen fields/global props, and embedded CSV does not record them). Keep this explicit in code to avoid confusion.

6. Edge cases and correctness notes

- Burn vs blackhole:
    - Check dynamicStore.supportBlackHoleOptimization() used by RuntimeSpiImpl.applyTrc10AssetIssue.
    - If burning is active, only owner change should be synthesized (no blackhole change).
- Missing accounts:
    - If an account is missing post-apply (shouldn’t happen for owner/blackhole/issuer), log warn/debug and skip that synthetic entry.
- Token maps:
    - Not included in AccountInfo encoding, so we do not change or need to reverse token map deltas for CSV parity.

7. Testing Plan

- Unit tests:
    - Create synthetic AccountCapsule instances with balances and AEXT fields; call serializeAccountInfo; verify balance encoding shifts only the balance bytes in output.
    - Given a fake Trc10LedgerChange ISSUE with fee=9999000000:
        - Mock DynamicPropertiesStore/supportBlackHoleOptimization and AccountStore.get(owner/blackhole) to return capsules with new balances.
        - Verify LedgerCsvSynthesizer produces 2 StateChange entries with correct owner/blackhole old/new balances encoded.
    - Given PARTICIPATE with amount A:
        - Mock owner/issuer new snapshots; verify 2 StateChange entries with correct reversed deltas.
- Integration sanity:
    - Run scripts/execution_csv_compare.py against the provided CSVs before/after to confirm block 3188 parity.
- Golden vectors:
    - If any tests exist that assert CSV digest format, add/adjust expectations for remote TRC‑10 parity.

8. Logging and observability

- Add INFO in synthesizer: “CSV ledger synthesis: added N TRC‑10 state changes for tx {txId} (issue/participate)”.
- Add DEBUG per-address: address, oldBalance → newBalance, burn vs blackhole, issuer involvement.

9. Performance

- Per TRC‑10 tx: 1–2 account store reads and simple clones — negligible overhead.
- Canonicalization remains unchanged.

10. Rollout and safety

- Behind exec.csv.include.trc10 flag (default true). Can be disabled quickly if needed.
- Strict mode flag exec.csv.ledger.strict for environments requiring all-or-nothing parity.

11. Known non-goals

- Do not emit freeze/global changes into CSV — embedded serializer ignores these fields, so adding them would not affect digest and could mislead.
- Do not modify RuntimeSpiImpl behavior; CSV synthesis is passive and read-only.

Pseudocode sketch (where to hook)

- File: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:104

if (programResult instanceof ExecutionProgramResult) {
ExecutionProgramResult exec = (ExecutionProgramResult) programResult;
List<StateChange> base = exec.getStateChanges();
List<StateChange> merged = new ArrayList<>(base);

    if (isRemoteMode() && exec.isSuccess() && includeTrc10() && !exec.getTrc10Changes().isEmpty()) {
        List<StateChange> ledger = LedgerCsvSynthesizer.synthesize(exec, trace);
        merged = mergeReplacePlaceholders(merged, ledger);
    }

    builder.stateChangeCount(merged.size())
            .stateChanges(merged)
            .stateDigestSha256(StateChangeCanonicalizer.computeStateDigest(merged));

}

Files to add/adjust

- Add: framework/src/main/java/org/tron/core/execution/reporting/LedgerCsvSynthesizer.java
- Update: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java: add synthesize+merge call
- No change to StateChangeJournal; we only reuse its private serialization pattern by either:
    - Duplicating minimal serializer (preferred: extract to a small package-private utility to avoid private access), or
    - Moving serializeAccountInfo into a small AccountInfoCodec utility in the same package and using it both from StateChangeJournal and the synthesizer (cleaner).

How this fixes the observed mismatch

- For AssetIssue in block 3188:
    - Synthesizer adds 2 state changes: owner fee debit (old=new+fee), blackhole credit (old=new−fee).
    - Replaces the placeholder owner old==new.
    - state_change_count becomes 2; digest matches embedded for that row.


---


