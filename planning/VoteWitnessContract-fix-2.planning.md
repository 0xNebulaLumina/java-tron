› Think harder.

I want to compare the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,

The result csv are
+ output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv
+ output-directory/execution-csv/output-directory/execution-csv/20251007-055437-2604b59f-remote-remote.csv
respectively.

you will help me debug and figure out why there are mismatches, starting from the first tx.

(embedded execution + embedded storage) java log: 5.embedded-java.log
(remote execution + remote storage) java log: remote-java.ec5b4fc.log
(remote execution + remote storage) rust log: remote-rust.ec5b4fc.log

---

• I compared both CSVs and traced the first divergence through the logs. Here’s the precise mismatch and why it happens.

First mismatch

- Tx: VoteWitnessContract
- Block/tx: block 2153, tx_index 0
- Tx id: 8ab04add0d317bba53d2f3abe3ed79dbd675234903fcb3764c2adf91e7f6856e
- Embedded result: SUCCESS with 1 state change (owner no-op parity)
    - output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv:1044
- Remote result: REVERT, runtime_error “The total number of votes[1000000] is greater than the tronPower[0]”
    - output-directory/execution-csv/20251007-055437-2604b59f-remote-remote.csv:1044

Key context (preceding tx)

- The same owner froze TRX shortly before:
    - Both runs show FreezeBalanceContract success at block 2142
    - Embedded: output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv:1042
    - Remote: output-directory/execution-csv/20251007-055437-2604b59f-remote-remote.csv:1042
- Remote Rust logs confirm the freeze and DB creation:
    - remote-rust.ec5b4fc.log:11466 “FreezeBalance owner=… amount=1000000 resource=Bandwidth”
    - remote-rust.ec5b4fc.log:11471 “Auto-initialized database: freeze-records”

What the remote logs say for the failing vote

- Remote Rust computes tronPower=0 despite the freeze:
    - remote-rust.ec5b4fc.log:11490 “sum=1 TRX (1000000 SUN), tronPower=0 SUN, new_model=false”
    - remote-rust.ec5b4fc.log:11493 “Transaction execution failed: Non-VM execution error: The total number of votes[1000000] is greater than the tronPower[0]”

Root cause

- The Rust backend’s tron power calculation only checks a non-existent “TRON_POWER” resource bucket (resource=2), ignoring actual freeze records for Bandwidth (0) and Energy (1).
- Code: rust-backend/crates/execution/src/storage_adapter.rs:1229
    - get_tron_power_in_sun() looks up TRON_POWER (2) only, so it returns 0 even after a valid Bandwidth freeze.
- This directly explains the remote REVERT while embedded succeeds (embedded correctly derives voting power from frozen balance).

Why this matches the evidence

- Freeze succeeds and balance decreases in both runs; the vote immediately after succeeds embedded but fails remote due to tronPower=0.
- Rust logs explicitly show tronPower=0 for this and subsequent votes across addresses, consistent with the incorrect resource lookup.

Recommended fix

- Update get_tron_power_in_sun to sum frozen amounts across resource 0 (Bandwidth) and 1 (Energy) and/or mirror Java’s AccountCapsule getAllTronPower() behavior.
    - File to change: rust-backend/crates/execution/src/storage_adapter.rs:1229
    - Minimal parity approach: tronPower = sum(freeze_record(BANDWIDTH).frozen_amount + freeze_record(ENERGY).frozen_amount).
    - Longer-term parity: parse account protobuf and implement full getAllTronPower() logic, handling V1/V2 freeze and delegations.
- Optional: keep emit_freeze_ledger_changes=false (CSV parity), since the ledger can remain internal.

Want me to patch the Rust method now and rerun the remote path to confirm the CSVs align for this tx?


---


› think harder.

No need to start implementing, just read through and understand the codebase and give me a detailed plan on:

Update get_tron_power_in_sun to sum frozen amounts across resource 0 (Bandwidth) and 1 (Energy) and/or mirror Java’s AccountCapsule getAllTronPower() behavior.


---


• Goal

- Make remote VoteWitnessContract parity with embedded by fixing tron power computation in the Rust backend:
    - Update get_tron_power_in_sun to sum freeze ledger amounts for BANDWIDTH (0) and ENERGY (1), and include TRON_POWER (2) when present.
    - Optionally mirror Java’s AccountCapsule getAllTronPower/getTronPower selection via ALLOW_NEW_RESOURCE_MODEL, within the constraints of our storage model.

Why This Matters

- First mismatch at tx 8ab04a… in block 2153 is due to the Rust backend returning tronPower=0 after a successful FreezeBalance (v1) in block 2142. The current code only checks resource=2 (TRON_POWER) and ignores v1 freeze amounts recorded
under BANDWIDTH (0). File: rust-backend/crates/execution/src/storage_adapter.rs:1229.

Current State

- Freeze ledger:
    - DB: freeze-records with key format [0x41 | 20-byte address | resource].
    - Methods exist: get_freeze_record, set_freeze_record, add_freeze_amount, remove_freeze_record. File: rust-backend/crates/execution/src/storage_adapter.rs:717, 722, 1080–1240.
- Tron power method:
    - get_tron_power_in_sun(&self, address, _new_model) currently only reads resource=2 and returns 0 if not present. File: rust-backend/crates/execution/src/storage_adapter.rs:1229.
- Dynamic properties:
    - support_allow_new_resource_model() present. File: rust-backend/crates/execution/src/storage_adapter.rs:980–1010.
    - support_unfreeze_delay() present. File: rust-backend/crates/execution/src/storage_adapter.rs:1016–1052.

Design Constraints

- The Rust backend’s “account” DB stores EVM AccountInfo serialized by our adapter, not Protocol.Account; so we cannot compute tron power from Java’s AccountCapsule directly in remote mode without additional ingestion. We should rely on our
freeze-records ledger for correctness and parity.

Plan

- Implementation Strategy
    - Compute tron power as the sum of freeze ledger amounts for:
        - BANDWIDTH (resource=0)
        - ENERGY (resource=1)
        - TRON_POWER (resource=2, legacy/back-compat if ever present)
    - Keep using support_allow_new_resource_model() only for logging/telemetry; given our storage, the sum above is invariant and aligns with Java behavior for both models in practice.
    - Preserve the method signature and return type.
- Detailed Steps
    - Update get_tron_power_in_sun to:
        - Define constants: const BANDWIDTH: u8 = 0; const ENERGY: u8 = 1; const TRON_POWER: u8 = 2;
        - For each resource in [0, 1, 2]:
            - Call get_freeze_record(address, resource)
            - If Some(record), accumulate record.frozen_amount using checked_add, returning an error on overflow.
        - Return the total.
        - Add tracing::info! logs that include:
            - new_model flag from support_allow_new_resource_model()
            - Found amounts per resource and total
    - Leave emit_freeze_ledger_changes behavior untouched (CSV parity).
    - Do not read from Protocol.Account (not available in our DB).
- Edge Cases
    - Missing freeze-records DB table: method just returns 0 (no records found).
    - Overflow: use checked_add and propagate an error; this is extremely unlikely but keeps behavior safe.
    - Negative votes/invalid params: orthogonal; handled by VoteWitness validation.
    - UNFREEZE_DELAY_DAYS: we do not need to adjust; freeze/unfreeze handlers should maintain ledger amounts, so the ledger remains the source of truth.
    - Delegation fields:
        - Do not add any delegated_* or acquired_delegated_* fields; voting rights remain with the owner’s frozen amount.
- Logging & Observability
    - When computing tron power, log:
        - Owner address (tron format)
        - new_model (from dynamic property)
        - Per-resource amounts and total
    - On overflow, log an error with all intermediate values and return error.
- Tests
    - Unit tests in rust-backend/crates/execution/src/storage_adapter.rs:
        - test_tron_power_bandwidth_only:
            - Set freeze-records for resource=0 with 1_000_000; expect power=1_000_000.
        - test_tron_power_energy_only:
            - Resource=1 only; expect power=amount.
        - test_tron_power_sum_bandwidth_and_energy:
            - Resource=0=1_000_000, Resource=1=2_000_000; expect sum=3_000_000.
        - test_tron_power_includes_legacy_tron_power:
            - Resource=2=500_000 with no 0/1; expect 500_000.
        - test_tron_power_all_three:
            - 0+1+2 accumulated; expect correct sum.
        - test_tron_power_overflow_protection:
            - Set near u64::MAX across two resources and assert error.
    - Integration tests in crates/core/src/service.rs:
        - vote_witness_after_freeze_v1_succeeds:
            - Execute FreezeBalance (v1) for 1_000_000 SUN on owner, then VoteWitness 1_000_000 → expect success (no REVERT), and state_changes includes owner no-op (CSV parity).
        - Optional: multi-freeze accumulation and later votes.
- Backward/Forward Compatibility
    - Compatible with existing freeze-records schema and config.
    - Does not depend on Java AccountCapsule storage, avoiding cross-format dependencies.
    - If we later ingest Protocol.Account snapshots, we can add a feature-gated branch:
        - Prefer AccountCapsule-derived computation when available (for exact parity with getAllTronPower()), otherwise fall back to freeze ledger sum.
- Rollout & Validation
    - Build Rust backend: cd rust-backend && cargo build --release
    - Re-run the remote path on the same block window around 2142–2153.
    - Verify CSVs:
        - The two rows at 1042–1044 for both CSVs should match (is_success=true for the vote).
        - Confirm state_digest parity does not change (we’re not changing state emissions).
    - Quick spot-check logs:
        - remote-rust: look for VoteWitness lines; tronPower should be 1_000_000 SUN, no REVERT warnings.
- Risks & Limitations
    - If some historical chains used a legacy “TRON_POWER” (resource=2) only and not BANDWIDTH/ENERGY, we still cover it by including resource=2.
    - If the Java node wrote AccountCapsule-only power (old_tron_power) with no corresponding v1/v2 freezes in our ledger (e.g., bootstrapped state), we would undercount. This is unlikely in our remote-backed run because freeze is executed
    via remote and ledger is created on first freeze; but if needed, we can add a one-time migration to seed freeze-records from AccountCapsule snapshots.

Change Points

- File: rust-backend/crates/execution/src/storage_adapter.rs:1229
    - Replace TRON_POWER-only lookup with sum over resources 0, 1, and 2.
- No changes to config defaults; optional future flag (not required now).

If you want, I can draft the exact code snippet for get_tron_power_in_sun and a couple of precise tests next.