Think harder.

I want to compare the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,

The result csv are
+ output-directory/execution-csv/20251027-040703-4abab9f8-embedded-embedded.csv
+ output-directory/execution-csv/20251028-051920-350511e4-remote-remote.csv
respectively.

you will help me debug and figure out why there are mismatches, starting from the first tx.

(embedded execution + embedded storage) java log: 2.embedded-java.log
(remote execution + remote storage) java log: remote-java.94a82d9.log
(remote execution + remote storage) rust log: remote-rust.94a82d9.log

---


• Here’s the first concrete mismatch and why it happens.

- First mismatch tx
    - block: 2153, tx_index: 0
    - tx_id_hex: 8ab04add0d317bba53d2f3abe3ed79dbd675234903fcb3764c2adf91e7f6856e
    - contract_type: VoteWitnessContract
    - Both runs report SUCCESS with energy_used=0 and 1 state change, but state digests differ:
        - embedded digest: 80fac92e0944c8f4a74cd29b6fae49042b9594a33f88c5bcf7d3684b034a5c93
        - remote digest: 12eb235be4b6912556f7dac1cfa1bcff55b6a9f4068c76f7e7505be4c98d747f
- CSV evidence
    - The mismatch appears at row 1044 (1-based) when iterating in order.
    - Both rows show state_change_count=1 and mutate the same owner: 41e552f6487585c2b58bc2c9bb4492bc1f17132cd0.
    - The serialized account newValue differs starting around byte ~182 (AEXT area), consistent with different bandwidth fields being updated (free vs
    account net).
- Log evidence (bandwidth path diverges)
    - Embedded run charges ACCOUNT_NET for this tx:
        - 2.embedded-java.log:78905-78916 show block 2153; 78915 shows cost/txs; and specifically:
            - 78915: path=ACCOUNT_NET (BandwidthProcessor.java:166)
            - 78895-78896: TxId ... bandwidth bytes: 209; owner TWsm8H... path=ACCOUNT_NET
        - File ref: 2.embedded-java.log:78895
    - Remote run rejects ACCOUNT_NET and falls back to FREE_NET:
        - remote-java.94a82d9.log:
            - 97556: TxId ... bandwidth bytes: 209 (BandwidthProcessor.java:131)
            - 97557: ACCOUNT_NET insufficient: owner=TWsm8H..., bytes=209, netLimit=0, ... available=0 (BandwidthProcessor.java:532)
            - 97558: owner ... path=FREE_NET bytes=209 (BandwidthProcessor.java:172)
        - File refs: remote-java.94a82d9.log:97556, remote-java.94a82d9.log:97557, remote-java.94a82d9.log:97558
- Why ACCOUNT_NET is 0 in remote mode here
    - The prior tx for this owner is a FreezeBalanceContract at block 2142, which should increase the owner’s bandwidth weight and the global total net
    weight:
        - Both CSVs show the freeze at index 1042; owner is the same address.
    - In the remote run, Java applied only account-level freeze changes, but did not update the global resource totals (totalNetWeight/totalNetLimit):
        - remote-java.94a82d9.log: shows for the freeze tx:
            - 97202: Applying 1 state changes (the account change)
            - 97207: Applying freeze ledger changes (freeze=1, global=0)
            - 97208: Successfully applied freeze ledger changes
        - File refs: remote-java.94a82d9.log:97202, remote-java.94a82d9.log:97207, remote-java.94a82d9.log:97208
    - BandwidthProcessor.calculateGlobalNetLimit uses:
        - account frozen balance (owner-level) and
        - DynamicPropertiesStore totalNetWeight/totalNetLimit (global totals)
        - If totalNetWeight == 0, netLimit is 0 and ACCOUNT_NET becomes insufficient.
        - See: chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java:486-503
    - Conclusion: remote run updated the owner’s freeze ledger, but did not update global totals, so totalNetWeight remained 0 at the time of the
    VoteWitness tx, causing FREE_NET to be used. Embedded updated both, so ACCOUNT_NET was used.
- Resulting state difference
    - Since the owner account’s resource fields differ (ACCOUNT_NET vs FREE_NET counters/timestamps), the serialized account newValue differs; hence the
    state digest mismatch.
    - You can see the divergence in the serialized AEXT region around char ~182: the numbers representing resource usage/time are different between the
    two runs (free vs account net).
- Root cause summary
    - Remote execution returns freeze ledger changes but no global resource totals changes for the freeze at block 2142.
    - Java applies freeze changes, but because the remote result has global=0, DynamicPropertiesStore.totalNetWeight/totalNetLimit are not updated in the
    remote run.
    - Next tx (VoteWitness) computes netLimit=0 in remote mode and consumes FREE_NET, diverging from embedded path and altering account AEXT bytes →
    different state digest.
- What to do next
    - Verify remote backend emits GlobalResourceTotalsChange for freeze/unfreeze and similar operations.
        - Check that tron-backend includes totalNetWeight/totalNetLimit updates when freeze/unfreeze occurs.
        - RemoteExecutionSPI already maps and applies these (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:393).
    - If backend intentionally omits global totals, consider a temporary Java-side fallback in RuntimeSpiImpl.applyFreezeLedgerChanges to recompute/
    update totals based on FreezeLedgerChange, but the correct fix is to have the backend provide consistent global totals.
    - After enabling global totals emission, rerun and recompare; this should eliminate this mismatch and likely many later FREE_NET vs ACCOUNT_NET
    divergences.

Want me to:

- add a small comparator script to summarize all mismatches and categorize them (e.g., FREE_NET vs ACCOUNT_NET, digest-only)?
- patch the backend config or code to emit GlobalResourceTotalsChange, or add a Java fallback for testing?


---

No need to start implementing, just read through and understand the codebase and give me a detailed plan on:

patch the backend config or code to emit GlobalResourceTotalsChang

---

• Here’s a precise, implementation‑ready plan to make the Rust backend emit GlobalResourceTotalsChange so Java updates TOTAL_NET_WEIGHT/TOTAL_NET_LIMIT
(and energy totals) immediately after freeze/unfreeze — fixing the FREE_NET vs ACCOUNT_NET divergence you saw at block 2153.

Goal

- Emit a GlobalResourceTotalsChange alongside existing FreezeLedgerChange for freeze/unfreeze contracts.
- Java already consumes and applies these in RuntimeSpiImpl.applyGlobalResourceChanges (framework/src/main/java/org/tron/
common/runtime/RuntimeSpiImpl.java:393), so subsequent transactions in the same block will see updated totalNetWeight/Limit in
BandwidthProcessor.calculateGlobalNetLimit (chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java:486).

Why This Fixes The Mismatch

- The remote run froze balance at block 2142 but did not update TOTAL_NET_WEIGHT, so BandwidthProcessor computed netLimit=0 and fell back to FREE_NET for
the VoteWitness at block 2153. Emitting and applying GlobalResourceTotalsChange makes TOTAL_NET_WEIGHT non‑zero; VoteWitness then uses ACCOUNT_NET like
embedded, aligning the account AEXT fields and the state digest.

Backend Changes

- Config and Flags
    - Add a new flag execution.remote.emit_global_resource_changes (bool).
    - Defaults:
        - Default false in code for backward/CSV parity (matching emit_freeze_ledger_changes defaults).
        - Enable true in the shipped config.toml for your runs.
    - Update loader defaults and startup logs:
        - Add default in rust-backend/crates/common/src/config.rs (Config.load): execution.remote.emit_global_resource_changes=false.
        - Extend RemoteExecutionConfig with a new field emit_global_resource_changes: bool, default false.
        - Log flag at startup in rust-backend/src/main.rs alongside “Emit freeze ledger changes”.
        - File: rust-backend/config.toml — add:
            - execution.remote.emit_global_resource_changes = true
- Data Structures (execution crate)
    - Define a GlobalResourceTotalsChange struct with:
        - total_net_weight: i64
        - total_net_limit: i64
        - total_energy_weight: i64
        - total_energy_limit: i64
    - Extend TronExecutionResult to include:
        - global_resource_changes: Vec<GlobalResourceTotalsChange>
    - File: rust-backend/crates/execution/src/tron_evm.rs (where FreezeLedgerChange and TronExecutionResult live: rust-backend/crates/execution/src/
    tron_evm.rs:173, 189–207)
- Storage Adapter helpers
    - Implement helpers to compute current totals from backend storage:
        - compute_total_net_weight() → i64
            - Sum BANDWIDTH freeze amounts across freeze-records (resource=0), divide by TRX_PRECISION (1_000_000) for “weight”.
            - Use StorageEngine iterator to scan “freeze-records” DB and filter entries whose last byte is resource 0.
            - Prefer full recompute for correctness; can optimize with incremental tracking later.
        - compute_total_energy_weight() → i64
            - Same, but for ENERGY (resource=1).
        - get_total_net_limit() → already available as storage_adapter.get_total_net_limit() (rust-backend/crates/execution/src/storage_adapter.rs:1345).
        - get_total_energy_limit() → if not present, add getter against “TOTAL_ENERGY_CURRENT_LIMIT” or return a sensible default (e.g., 0) until the
        actual dynamic property is wired.
    - File: rust-backend/crates/execution/src/storage_adapter.rs
- Emit on system contract handlers (core service)
    - For each freeze/unfreeze system contract path, after persisting freeze ledger and building freeze_changes, conditionally emit global totals:
        - Contracts to update:
            - FreezeBalanceContract handler: rust-backend/crates/core/src/service.rs:1300–1440 (emits freeze_changes now)
            - UnfreezeBalanceContract handler: rust-backend/crates/core/src/service.rs:1428–1560
            - FreezeBalanceV2Contract handler: rust-backend/crates/core/src/service.rs:1617–1768
            - UnfreezeBalanceV2Contract handler: rust-backend/crates/core/src/service.rs:1768–1925
        - Pattern:
            - If execution.remote.emit_global_resource_changes is true:
                - net_w = compute_total_net_weight()
                - net_l = storage_adapter.get_total_net_limit()
                - energy_w = compute_total_energy_weight()
                - energy_l = 0 or a dynamic getter if available
                - Push a single GlobalResourceTotalsChange { total_net_weight: net_w, total_net_limit: net_l, total_energy_weight: energy_w,
                total_energy_limit: energy_l } into TronExecutionResult.global_resource_changes.
            - Log emitted totals (info) for traceability.
- Protobuf mapping (core service)
    - Convert execution result’s global_resource_changes to the gRPC response:
        - Modify convert_execution_result_to_protobuf to map Vec<GlobalResourceTotalsChange> into crate::backend::GlobalResourceTotalsChange.
        - Currently global_resource_changes is always vec![] (rust-backend/crates/core/src/service.rs:5183–5200). Fill it from
        result.global_resource_changes.
    - Ensure tonic proto for ExecutionResult already contains repeated GlobalResourceTotalsChange; it’s referenced at rust-backend/crates/core/src/
    service.rs:5183–5194.

Validation

- Unit tests (core service)
    - Add tests that run a FreezeBalance followed by a conversion:
        - With emit_freeze_ledger_changes=true and emit_global_resource_changes=true:
            - Execute FreezeBalance → response.result.global_resource_changes[0].total_net_weight > 0.
            - Execute UnfreezeBalance → total_net_weight decreases accordingly or to expected reference.
    - Add a multi-owner test to ensure totals reflect sum across accounts.
- Manual verification (end-to-end)
    - Build and run backend with both flags enabled:
        - execution.remote.emit_freeze_ledger_changes = true
        - execution.remote.emit_global_resource_changes = true
    - Replay the block range including 2142 (freeze) and 2153 (VoteWitness).
    - In Java logs, confirm:
        - After the freeze tx, you see “Applying freeze ledger changes … (freeze=1, global=1)”.
        - Before the VoteWitness, TOTAL_NET_WEIGHT reflects the updated value.
        - BandwidthProcessor path for the VoteWitness is ACCOUNT_NET (not FREE_NET).
    - Re-diff CSVs; the first mismatch at block 2153 should disappear.
Rounding and Semantics

- Weight calculation should match Java:
    - totalNetWeight = sum_over_accounts(frozen_sun_for_bandwidth) / TRX_PRECISION (integer division).
    - Use the same TRX_PRECISION as Java (1_000_000).
- totalNetLimit can be emitted from the current dynamic property (default 43_200_000_000) — Java will simply persist whatever backend reports; no need
to derive.
- Energy totals:
    - Emit energy weight using the same scheme across resource=1; set limit to 0 if a precise dynamic property is not available yet. Java will accept and
    set it; if needed, later wire a getter similar to net limit.

Performance and Future Work

- Full DB scans are O(n) per freeze/unfreeze. Acceptable for parity work. Improve later by:
    - Maintaining a running TOTAL_NET_WEIGHT dynamic property in backend storage and adjusting it incrementally on freeze/unfreeze (add
    set_total_net_weight() and update it with exact delta).
    - Adding an iterator prefix for “freeze-records” that quickly filters resource 0 keys to reduce scanning overhead.

Docs and Samples

- Update rust-backend/docs/FREEZE_BALANCE_PHASE2_SUMMARY.md to add the new flag and describe the emission (alongside the existing
emit_freeze_ledger_changes).
- Update rust-backend/config.toml with commented guidance and set emit_global_resource_changes=true for your parity runs.

Key File Touchpoints

- rust-backend/crates/common/src/config.rs: add flag + default/loaders
- rust-backend/src/main.rs: log flag at startup
- rust-backend/config.toml: add emit_global_resource_changes = true
- rust-backend/crates/execution/src/tron_evm.rs: add GlobalResourceTotalsChange, extend TronExecutionResult
- rust-backend/crates/execution/src/storage_adapter.rs: add compute_total_net_weight(), compute_total_energy_weight()
- rust-backend/crates/core/src/service.rs:
    - Freeze/Unfreeze handlers: compute + attach global_resource_changes
    - convert_execution_result_to_protobuf: map global_resource_changes to proto
- Optional: add getter for TOTAL_ENERGY_CURRENT_LIMIT if available later

This patch set makes the backend explicitly tell Java “here are the updated global totals right now,” so Java applies them in the same tx
(RuntimeSpiImpl.applyFreezeLedgerChanges, framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:172, 204, 393). That removes the lag that
caused FREE_NET in your remote run and aligns behavior with embedded.

---

