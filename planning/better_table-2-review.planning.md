Verdict

- Broadly aligned with the plan/todo: CSV header expansion, domain canonicalization + digests, embedded journaling (TRC‑10, votes, freezes, globals), and builder integration are implemented.
- Not fully aligned in remote “old/new” semantics and one digest detail. A few items remain partial/incomplete per the TODO.

What Aligns

- CSV schema and order match the TODO, including the legacy state_changes triplet and ts_ms.
    - Header: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecord.java:695
    - Column count = 50, with tests: framework/src/test/java/org/tron/core/execution/reporting/ExecutionCsvRecordTest.java:204
- DomainCanonicalizer: deterministic ordering, hex casing, digest mechanics, JSON shapes per domain.
    - Implementation: framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java:1
    - Tests present: framework/src/test/java/org/tron/core/execution/reporting/DomainCanonicalizerTest.java
- Embedded journaling plumbing for domain deltas:
    - DomainChangeJournal + Registry + Bridge: framework/src/main/java/org/tron/core/execution/reporting/DomainChangeJournal.java:1, framework/src/main/java/org/tron/core/execution/reporting/
    DomainChangeJournalRegistry.java:33, framework/src/main/java/org/tron/core/execution/reporting/DomainChangeRecorderBridge.java:1
    - Recorder context interface: chainbase/src/main/java/org/tron/core/db/DomainChangeRecorder.java:1, chainbase/src/main/java/org/tron/core/db/DomainChangeRecorderContext.java:1
    - Manager wires lifecycle + gating via exec.csv.stateChanges.enabled (same flag as planned): framework/src/main/java/org/tron/core/db/Manager.java:1573
    - Hooks:
        - TRC‑10 balance: chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java:754
        - TRC‑10 issuance: actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java:107
        - Vote deltas: actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java:140
        - Freeze V2 deltas: actuator/src/main/java/org/tron/core/actuator/FreezeBalanceV2Actuator.java:108
        - Global totals deltas: chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java:1263
- Builder integrates remote vs embedded flows and fills domain triplets:
    - framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:98

Gaps vs Plan/TODO

- Remote “old/new” values are placeholders; TODO calls for pre/post reads to fill absolute old/new:
    - TRC‑10 balance (remote): uses oldBalance="0" and sender newBalance set to negative delta, not absolute new. Should be absolute old/new from store.
        - framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:240
    - Votes (remote): oldVotes hardcoded to "0". Should compare program result vs AccountStore for old/new.
        - framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java:1606
    - Global resources (remote): oldValue set to "0". Should read pre-state from DynamicPropertiesStore.
        - framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java:1669
- Empty-array digest inconsistency:
    - Code uses empty string for empty arrays (matches the planning.md), but TODO specifies sha256("") for empty arrays.
        - computeDigest behavior: framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java:1772
        - TODO rule: planning/better_table-2.todo.md:39
- Minor: In builder, return_data_len isn’t set when returnDataHex(String) is used; only set when passing bytes explicitly. Not called out in the plan but the base column exists.
    - framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:70
    - String setter: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecord.java:252

Observations

- AEXT parsing and account_resource_usage changes are implemented via account state changes and match plan intent; limits remain null (AEXT doesn’t include limits), which is acceptable since unchanged/absent
fields can be omitted.
    - framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java:1682
- Domain journaling is gated by exec.csv.stateChanges.enabled as planned, initialized per-transaction and cleared after CSV logging.
    - framework/src/main/java/org/tron/core/db/Manager.java:1573

Recommendations

- Decide on empty-array digest convention and align code or TODO:
    - Either change computeDigest("[]") => sha256("") or update the TODO to match the planning.md’s “empty string” approach.
- Implement pre/post-state lookups for remote path to produce absolute old/new:
    - TRC‑10 balances: read from AccountStore (old), compute new after apply.
    - Votes: compare pre Account.votes vs program result vote list.
    - Global resources: read old from DynamicPropertiesStore; use result fields as new.
- Normalize TRC‑10 balance JSON to absolute old/new (not negative delta) for sender in remote path.
- Optionally set return_data_len when returnDataHex(String) is used.

