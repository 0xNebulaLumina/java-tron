• Here’s what I found by diffing the two CSVs and tracing the logs to the first divergence.

    - Divergent field(s): state_digest_sha256 and the account entry for 4177944d19c052b73ee2286823aa83f8138cb7032f within state_changes_json
    - What exactly differs: the account’s serialized “balance(32)” segment per AccountInfoCodec
        - Old balance (embedded): 51,124.400000 TRX; (remote): 50,100.400000 TRX → diff = 1,024.000000 TRX
        - New balance (embedded): 61,123.400000 TRX; (remote): 60,099.400000 TRX → diff = 1,024.000000 TRX
    - The 1,024 TRX gap points to TRC-10 AssetIssue fees (default fee = 1,024 TRX) not being reflected in the remote storage, even though they appear in the CSV.
    - Immediately prior row 1123 is AssetIssueContract that touches the same address and shows a blackhole credit of exactly 1,024,000,000 SUN in both CSVs (so the CSVs agree for that tx).
- Evidence snapshots
    - CSV diff at row 1124 shows different balance bytes only (32-byte prefix of old/new):
        - embedded old: ...8000000be7406f80 → 51,124.400000 TRX
        - remote   old: ...8000000baa376f80 → 50,100.400000 TRX
        - delta = 1,024.000000 TRX
    - Row 1123 (AssetIssueContract) blackhole credit is 1,024,000,000 SUN in both CSVs:
        - output-directory/execution-csv/20251030-055539-b3bf2f9c-embedded-embedded.csv:1123
        - output-directory/execution-csv/20251031-053510-1ef051c2-remote-remote.csv:1123
    - Remote Rust execution around 3189 confirms two account changes (owner debit and blackhole credit) and lists the blackhole address:
        - remote-rust.e9135e7.log:12539–12553
- Conclusion
    - The first mismatch is caused by TRC-10 AssetIssue fees being reported into the remote CSV but not persisted to the remote storage overlay. When the blackhole account is later touched by the
WitnessCreate, the remote “oldValue” is short by 1,024 TRX, causing digest divergence.
    - This is a CSV/reporting vs state-application gap in the remote path for TRC-10 synthesis.
- Next steps
    - Decide intent: should TRC-10 CSV synthesis also persist to storage (Rust) for parity, or should CSV exclude synthesized TRC-10 entries to match storage?
    - If persisting: update the Rust backend (or the Java RemoteExecutionSPI application layer) to apply the same owner/blackhole balance deltas to the account store when TRC-10 ISSUE/PARTICIPATE occur, not
just emit “shadow” CSV changes.
    - As a quick validation, I can scan earlier rows to list all AssetIssue touches of 4177944d... and confirm only one fee’s worth is missing in remote storage. Want me to run that and summarize?


---

update the Java RemoteExecutionSPI application layer to apply the same owner/blackhole balance deltas to the account store when TRC-10 ISSUE/PARTICIPATE occur, not just emit “shadow” CSV changes

---

• Here’s a concrete, surgical plan to make remote mode actually apply TRC‑10 owner/blackhole deltas to the Java account store (and keep Rust in sync), not just surface them in CSV.

- Goals
    - Persist TRC‑10 ISSUE/PARTICIPATE effects in Java AccountStore during remote execution.
    - Ensure the blackhole credit (or burn) matches Rust behavior for the block’s rules.
    - Flush these account mutations to the remote storage before the next tx reads them.
    - Keep CSV synthesis intact; the source of truth should now be the store, not “shadow” data.
- Current Behavior
    - Remote result includes trc10Changes (ISSUE/PARTICIPATE); Java synthesizes CSV parity entries, but mismatches show state wasn’t persisted to remote storage for subsequent tx.
    - Conversion from gRPC → Java result occurs in RemoteExecutionSPI.convertExecuteTransactionResponse (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:840–1040).
    - Java applies state changes and TRC‑10 deltas in RuntimeSpiImpl (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:486), then calls a post‑exec flush (framework/src/main/java/org/tron/
    common/runtime/RuntimeSpiImpl.java:438).
    - Flush is implemented by ResourceSyncService.flushResourceDeltas writing to ACCOUNT_DB et al. (framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java:120).
- Design Overview
    - Use the trc10Changes already parsed in RemoteExecutionSPI as the authoritative signal to mutate Java stores.
    - Apply:
        - ISSUE: owner TRX fee debit; credit blackhole (unless burning).
        - PARTICIPATE: owner TRX debit + token credit; issuer TRX credit + token debit.
    - Prefer remote‑provided values when present:
        - Fee amount: use Trc10LedgerChange.feeSun if provided; fallback to DynamicPropertiesStore.getAssetIssueFee().
        - Blackhole vs burn: align with Rust’s rule for the block; if Rust exposes a boolean (future), prefer it; otherwise use Java DynamicPropertiesStore.supportBlackHoleOptimization().
    - After applying to Java stores, mark dirty and synchronously flush the account changes to remote storage before running the next tx (post‑exec flush is already designed for this).
- Implementation Steps
    - Ensure TRC‑10 parsing is complete and stable
        - Verify we capture feeSun and all ISSUE/PARTICIPATE fields in RemoteExecutionSPI.convertExecuteTransactionResponse (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:840–
        1040). Keep sort/determinism there.
    - Apply TRC‑10 to Java stores (already scaffolded; refine)
        - For ISSUE: update logic to prefer feeSun when non‑null (currently CSV synthesizer does this; mirror in apply path).
            - Touchpoint: applyTrc10AssetIssue(...) (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:585).
            - Debit owner balance; if !supportBlackHoleOptimization(), credit blackhole account; else burnTrx(fee).
            - Record both owner and blackhole accounts as dirty via ResourceSyncContext.recordAccountDirty(...).
        - For PARTICIPATE: keep existing TRX/token movements (owner↔issuer) with V1/V2 handling and mark both accounts dirty.
            - Touchpoint: applyTrc10AssetParticipate(...) (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:760).
    - Flush Java mutations to remote storage
        - Keep post‑exec flush after TRC‑10 apply (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:438). It calls ResourceSyncContext.flushPreExec() synchronously, which uses
        StorageSPI.batchWrite(...).
        - Confirm ResourceSyncService.buildAccountBatch(...) serializes AccountCapsule.getData() (framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java:268) which matches the Rust
        backend’s account encoding (remote log shows it consumes Tron Account protobuf bytes).
    - Defensive alignment with Rust rules
        - If backend later exposes “use_blackhole” for ISSUE, plumb it through Trc10LedgerChange and override Java’s dynamic rule to guarantee identical behavior at specific historical heights.
        - Until then, continue using DynamicPropertiesStore.supportBlackHoleOptimization() (same rule Rust logs show).
    - Keep CSV logic intact
        - No change required: ExecutionCsvRecordBuilder merges base state changes and synthesized TRC‑10 entries (framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:129–
        151).
        - With store mutations flushed, subsequent remote state changes will reflect the updated balance as oldValue.
- Config and Flags
    - Ensure these stay default‑on for remote mode:
        - -Dremote.exec.apply.trc10=true (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:486)
        - -Dremote.resource.sync.postexec=true (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:520)
    - Optional safety toggles:
        - -Dremote.resource.sync.enabled=true for remote storage mode (framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java:75)
        - -Dremote.exec.trc10.enabled=true only matters if we route TRC‑10 execution to Rust (not required for this plan).
- Validation Plan
    - Focused replay around the failing case:
        - Execute row 1123 (AssetIssueContract) then 1124 (WitnessCreateContract).
        - Before 1124:
            - Confirm applyTrc10AssetIssue debits owner and credits blackhole in Java AccountStore.
            - Confirm ResourceSyncContext recorded blackhole and owner as dirty.
            - Confirm ResourceSyncService.flushResourceDeltas enqueued these accounts into ACCOUNT_DB batch and awaited completion.
        - During 1124 remote execution:
            - In remote‑rust log, the blackhole “Successfully deserialized account – balance” must include the +1,024 TRX from row 1123.
    - End‑to‑end CSV parity:
        - The state_changes_json for the blackhole at row 1124 in both CSVs should match; state digest SHA‑256 should converge.
    - Metrics/log assertions:
        - Add debug logs for “Applied ISSUE feeSun=X, useBlackhole=Y, owner delta=−X, blackhole delta=+X”.
        - Add a one‑line summary in flushResourceDeltas for the count of dirty accounts and whether the batch contained the blackhole address.
- Risks and Mitigations
    - Race/ordering: ensure post‑exec flush is blocking; it already awaits .get() on composed futures.
    - Fee mismatch: prefer feeSun when present to avoid dynamic store drift on historical blocks.
    - Encoding mismatch: both sides use Tron Account protobuf; retain existing AccountCapsule.getData(); verify by sampling one post‑flush read (optional, guarded by -Dremote.resource.sync.confirm=true).
    - Double‑apply: only apply when execResult.isSuccess(); current flow already checks success before CSV/flush.
- Stretch (optional)
    - TRC‑10 TRANSFER: add apply path similar to PARTICIPATE once Rust emits those ops.
    - Plumb “use_blackhole” boolean through protobuf to eliminate any policy drift.
- Summary of Touchpoints
    - RemoteExecutionSPI (parse) → framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:840
    - Apply TRC‑10 ISSUE/PARTICIPATE → framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:486, :549, :585, :760
    - Post‑exec flush → framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:438
    - Resource sync batching → framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java:240, :268


---

