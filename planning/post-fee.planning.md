• I’m going to diff the two CSVs, locate the first mismatch, and correlate it with the Java/Rust logs.

— Found the first mismatch and pulled the related logs.

First Mismatch

- Tx: ea03aeb49f10c7637b7fc4070d94858e8e6630deff0be6622a748f125134944a
- Row refs:
    - output-directory/execution-csv/20251023-085609-24d1fac9-embedded-embedded.csv:1064
    - output-directory/execution-csv/20251024-092229-8782213d-remote-remote.csv:1064
- Context: block 2458, tx_index_in_block=0, contract=TransferContract, owner=4147d8f622c81c6512502a1a3b219ad89518d42cd2
- Difference: state_changes_json for the sender account (owner) has a 100,000 SUN discrepancy in the oldValue/newValue balances
    - Embedded old/new words (first 32-byte word = balance):
        - old=519,989,600,000, new=504,989,600,000
    - Remote old/new:
        - old=519,989,500,000, new=504,989,500,000
- State digest differs accordingly (embedded: a1ff…, remote: 52dc…)
- Recipient (4130af…): identical on both sides (credited 15,000,000,000 SUN)

What This Indicates

- The transfer amount is identical on both sides (15,000,000,000 SUN). The mismatch comes from the sender’s balance being 100,000 SUN lower in the remote run prior to applying the transfer.
- 100,000 SUN is the “create account” fee burned when a Transfer triggers account creation and free bandwidth is insufficient.

Correlated Logs

- For this same tx (ea03…):
    - Embedded Java logs show path=CREATE_ACCOUNT:
        - 1.embedded-java.log:83394-83395
    - Remote Java logs show the same path and fee:
        - remote-java.085b867.log:114887-114888
- Earlier tx that precedes 2458 and uses free net (no fee), to show sequence is aligned:
    - Tx d03ce5fe…, block 2418:
        - 1.embedded-java.log:82495-82497 (path=FREE_NET)
        - remote-java.085b867.log:113887-113899 (path=FREE_NET)
- Remote Rust shows execution summaries but not per-tx IDs by default; however, resource/fee handling is visible in Java logs and storage adapter traces for the account.

Why Embedded vs Remote Diverge Here

    - ExecutionProgramResult.stateChanges are collected from the local journal (StateChangeJournalRegistry) in ExecutionProgramResult.fromProgramResult(...) (framework/src/main/java/org/tron/core/execution/
    spi/ExecutionProgramResult.java).
    - Those journaled “oldValue” snapshots are taken from the VM/Runtime’s view which appears to be captured before the pre-exec fee mutation of the account balance is visible in the “oldValue” for the
    transfer. Hence old=519,989,600,000.
- In remote execution:
    - RemoteExecutionSPI converts the backend’s response (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java: convertExecuteTransactionResponse). The backend computes state changes
    against storage that already reflects the pre-exec fee deduction, so the “oldValue” for the same account includes the already-charged 100,000 SUN. Hence old=519,989,500,000.
- Conclusion: ordering/visibility difference for “oldValue” capture relative to pre-exec fee application. Both paths charge the fee (logs confirm), but the embedded path snapshots “oldValue” before it, while
the remote path snapshots after it.

Supporting Evidence

- The sender’s subsequent transactions show the remote “oldValue” keeps trailing the embedded by additional 100,000 SUN per new account-creation transfer, consistent with fee timing/visibility:
    - Extracted balances for owner address across blocks:
        - Embedded: (2458) 519,989,600,000 → 504,989,600,000
        - Remote:   (2458) 519,989,500,000 → 504,989,500,000
        - Next few transfers show the remote “oldValue” lag increasing by 100,000 per additional create-account fee event (consistent with logs for that owner).
- “Tx create-account used FEE: owner=TGX6…” lines exist in both runs; they confirm the fee is applied by the Java BandwidthProcessor (chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java and
ResourceProcessor.consumeFeeForNewAccount), which adjusts the account’s balance and records the dynamic changes (ResourceSyncContext).

Root Cause Hypothesis

- Not a difference in charging policy; it’s a difference in when “oldValue” is captured for CSV purposes relative to pre-exec resource mutations:
    - Embedded: state change journal captures VM-level account change oldValue before the BandwidthProcessor fee hits the balance (or doesn’t see that change in the same snapshot).
    - Remote: the backend’s account change oldValue is read after fee deduction is applied to storage.

Next Checks

- Verify the precise ordering in Java call paths:
    - Where StateChangeJournalRegistry snapshots are taken in embedded flow vs when BandwidthProcessor.consumeFeeForCreateNewAccount is called.
- Decide on normalization:
    - Option A: Adjust embedded capture to snapshot after pre-exec resource fees (align with remote).
    - Option B: Adjust remote capture to snapshot before pre-exec fees (align with embedded).
- If you want, I can:
    - Add a small comparator utility that focuses on state_digest_sha256 and the normalized per-address deltas (ignoring “oldValue”).
    - Instrument embedded flow to log a “pre-exec fee applied” snapshot and “pre-VM oldValue” to nail the exact timing.


----


think harder.

No need to start implementing, just read through and understand the codebase and give me a detailed plan on Option A.


---


• I dug through the CSV/trace path and the resource charging path. Here’s a concrete plan to implement Option A: make embedded CSV “oldValue” reflect the post–pre‑exec (bandwidth/fees) state, aligning with
remote.

Objective

- Ensure embedded CSV state_changes_json “oldValue” is captured after pre‑execution resource mutations (bandwidth, create-account fee paths), so its baseline matches remote, without emitting extra fee
entries.

How It Works Today (key points)

- Pre‑exec resource charging runs before VM:
    - Manager.processTransaction(..) calls:
        - consumeBandwidth(..) (BandwidthProcessor) and related fees
        - ResourceSyncContext.flushPreExec()
        - then initializes the state-change journal and executes VM
    - Ref: framework/src/main/java/org/tron/core/db/Manager.java:1558–1593
- Embedded state changes:
    - Journal initialized right before trace.exec():
        - StateChangeJournalRegistry.initializeForCurrentTransaction()
        - StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge())
    - During VM execution, changes are recorded via AccountStore.put() and RepositoryImpl hooks into the journal.
    - ExecutionProgramResult.fromProgramResult(..) snapshots StateChangeJournalRegistry.getCurrentTransactionStateChanges().
- CSV assembly uses that snapshot:
    - ExecutionCsvRecordBuilder.extractExecutionResults(..) reads ExecutionProgramResult.stateChanges.
    - Ref: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java

The mismatch you saw comes from “oldValue” not consistently reflecting post‑resource state when embedded and remote diverge on fee timing/visibility. Option A fixes the capture point for embedded.

Design

- Preseed the journal with the account snapshots (oldAccount) after flushPreExec() but before VM execution, so journal’s “old” baseline reflects post‑resource state. Let the normal VM flow update
“newAccount”; the journal already merges “keep first old, update new”.
- Scope initial preseed to TransferContract (owner and to addresses). Optional: extend to other contracts as needed.

Changes Overview

- Add a small utility to preseed target addresses into the journal with their current post‑resource AccountCapsule.
- Call it immediately after journal initialization, before trace.exec().
- Gate with feature flags to be safe, and only apply for embedded execution mode.

Step‑by‑Step Plan

1. Add Journal preseed helper

- New file: framework/src/main/java/org/tron/core/execution/reporting/JournalPreseedUtil.java
- Responsibilities:
    - Given TransactionTrace (has TransactionContext and StoreFactory) and the first contract, derive addresses to preseed.
    - Fetch current account(s) from AccountStore as of post‑resource state.
    - Call StateChangeJournalRegistry.recordAccountChange(address, oldClone, oldClone) to seed “old” baseline (use cloned AccountCapsule to avoid later mutation of the same object).
- Notes:
    - Use AccountCapsule account = accountStore.getUnchecked(addr) to get persisted state that already includes pre‑exec changes.
    - Preseed only addresses that you are confident will change to avoid accidental old==new noise; for TransferContract that’s owner and to.
    - Keep it no‑op if journal feature is disabled (StateChangeJournal.isEnabled() false).

2. Insert preseed call in Manager

- Edit: framework/src/main/java/org/tron/core/db/Manager.java: after journal init, before trace.exec():
    - At approximately Manager.java:1561–1570 sequence:
        - After StateChangeJournalRegistry.initializeForCurrentTransaction();
        - After StateChangeRecorderContext.setRecorder(new StateChangeRecorderBridge());
        - Insert: JournalPreseedUtil.tryPreseedAfterResource(trace);
- Ensure call site order remains:
    - Pre‑exec resource consumption
    - ResourceSyncContext.flushPreExec()
    - trace.init(...)
    - journal initialize + set recorder
    - PRESEED (new step)
    - trace.exec() (VM execution)

3. Address detection (first pass: Transfer)

- In JournalPreseedUtil:
    - Determine contract type: trxCap.getInstance().getRawData().getContract(0).getType()
    - For TransferContract:
        - Parse owner and to via unpack into TransferContract (same as TransferActuator.validate()).
        - Preseed both if present in store; skip missing addresses silently (recipient may not exist pre‑VM when created by actuator; only preseed owner in that case).

4. Feature flags and safety

- Add a system property gate, default on:
    - exec.csv.preseedAfterResource (true by default)
    - Also only run when journal feature is enabled: exec.csv.stateChanges.enabled must be true (existing flag journal uses).
- Optional: Only run in embedded execution mode:
    - Check ExecutionSpiFactory.determineExecutionMode() == EMBEDDED to avoid touching remote mode.
- Add minimal INFO logging:
    - “CSV preseed: owner=<...> to=<...>” once per tx when active.

5. Keep CSV emissions unchanged

- Do not emit extra fee mutations as separate state changes.
- Leverage journal’s merge semantics (keep first old, update new); no change needed in:
    - ExecutionProgramResult.fromProgramResult(...)
    - ExecutionCsvRecordBuilder.extractExecutionResults(...)
    - StateChangeJournal merge logic

6. Verify locally

- Target case: block 2458 tx index 0 (ea03...).
- Steps:
    - Run embedded execution with exec.csv.stateChanges.enabled=true and the new exec.csv.preseedAfterResource=true.
    - Confirm embedded CSV “oldValue” for owner equals post‑resource baseline (matches remote CSV’s oldValue).
    - Confirm:
        - Address set unchanged (still 2 entries: recipient, owner)
        - owner delta equals 15,000,000,000 (fees excluded from that delta)
        - state_digest_sha256 matches remote for that tx
- Sanity check a few earlier/later transfers (e.g., blocks 2418/2520) to ensure no extra rows or digest drift.

7. Edge cases and extensions

- Recipient creation:
    - If to does not exist pre‑VM, skip preseed for recipient (you still get one account change once it’s created, with old empty and new present).
- Other contracts:
    - If needed, extend preseed logic for TransferAssetContract (owner + to), and selected system contracts (owner only).
    - Keep heuristics conservative to avoid old==new emissions. If you later broaden scope, add a filter to drop account changes where old==new right before CSV build; for now, limiting scope avoids that
    need.
- Remote mode:
    - No effect (embedded‑only gate). Remote state changes still come from backend via RemoteExecutionSPI.

8. Observability

- Add counters/metrics (optional):
    - Number of preseeds performed
    - Number of addresses seeded per tx
- Log at DEBUG the account balances captured for preseeding when a debug flag is on, to simplify parity checks.

9. Non‑goals

- This plan doesn’t change BandwidthProcessor logic or resolve cases where the two runs genuinely take different resource paths (FREE_NET vs FEE) due to state/time drift. It aligns “oldValue” capture to the
post‑resource state for embedded, so when both paths are consistent, CSVs match. If your first mismatch stems from a real path difference (remote charged fee; embedded didn’t), you’ll still see the 100k
offset unless you address the resource divergence separately.

Key References

- Manager execution order: framework/src/main/java/org/tron/core/db/Manager.java:1558–1593
- CSV builder: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java
- Journal APIs: framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournalRegistry.java, StateChangeJournal.java
- Account change hooks:
    - chainbase/src/main/java/org/tron/core/store/AccountStore.java:69
    - actuator/src/main/java/org/tron/core/vm/repository/RepositoryImpl.java:744, 813
- Resource sync: framework/src/main/java/org/tron/core/storage/sync/ResourceSyncContext.java
