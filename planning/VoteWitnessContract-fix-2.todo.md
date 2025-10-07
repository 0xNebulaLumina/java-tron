# VoteWitnessContract Tron Power Parity Fix (Plan v2)

Owner: core-exec team
Status: planned
Target: remote execution + remote storage parity with embedded for VoteWitnessContract

## Problem Summary
- First CSV mismatch appears at block 2153, tx 0 (VoteWitnessContract).
- Embedded: SUCCESS with 1 AccountChange (CSV parity no-op on owner).
- Remote: REVERT with error "The total number of votes[1000000] is greater than the tronPower[0]".
- Remote Rust logs show tronPower=0 SUN immediately after a successful FreezeBalance(v1) of 1_000_000 SUN by the same owner.

## Root Cause
- `get_tron_power_in_sun()` in Rust backend only reads TRON_POWER (resource=2) from the `freeze-records` ledger and ignores BANDWIDTH (0) and ENERGY (1).
  - File: `rust-backend/crates/execution/src/storage_adapter.rs:1229`
- Our remote mode persists freeze ledger entries under resource 0/1 for FreezeBalance(v1). Therefore tronPower is undercounted (0) during VoteWitness, causing REVERT.

## Objectives
- Compute tron power from the existing freeze ledger by summing relevant resources so VoteWitnessContract succeeds when the owner has frozen balance.
- Maintain CSV parity (state_change_count and digest) with embedded.
- Avoid introducing Protocol.Account parsing or schema coupling beyond current storage for this fix.

## Design Options
1) Minimal Parity (Recommended now)
- Sum `freeze-records` for resources: BANDWIDTH (0) + ENERGY (1) and include TRON_POWER (2) if present.
- Continue to read `ALLOW_NEW_RESOURCE_MODEL` only for logging/telemetry; the sum is model-agnostic for this phase.

2) Full AccountCapsule Parity (Future)
- Parse `Protocol.Account` (old_tron_power, frozen V1/V2, delegated amounts, etc.) and implement `getAllTronPower()` logic exactly.
- Requires storing AccountCapsule in remote DB or a translation layer. Not viable for this immediate fix.

3) Hybrid (Future)
- Prefer AccountCapsule-derived calculation when available; otherwise fall back to freeze ledger sum.

## Scope
- In scope:
  - Update `get_tron_power_in_sun()` logic (Rust backend execution adapter).
  - Add unit tests (execution adapter) and integration tests (core service VoteWitness + Freeze).
  - Add structured logs for visibility.
- Out of scope:
  - Changing CSV emission behavior (keep `emit_freeze_ledger_changes=false`).
  - Implementing AccountCapsule ingestion/parsing or V2 delegation parity.
  - Altering Java code.

## Detailed TODOs

1) Execution Adapter: Implement resource-sum tron power
- [x] File: `rust-backend/crates/execution/src/storage_adapter.rs`
- [x] Update `get_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64>` to:
  - [x] Define `const BANDWIDTH: u8 = 0; const ENERGY: u8 = 1; const TRON_POWER: u8 = 2;`
  - [x] Initialize `total: u64 = 0`.
  - [x] For each resource in `[BANDWIDTH, ENERGY, TRON_POWER]`:
    - [x] Call `get_freeze_record(address, resource)`.
    - [x] If `Some(record)`, `total = total.checked_add(record.frozen_amount).ok_or(anyhow!("Tron power overflow"))?`.
  - [x] `Ok(total)`.
  - [x] Logging: `info!(address, new_model, bandwidth, energy, tron_power_legacy, total)`.
- [x] Do not change ledger emission config.
- [x] Also added same implementation to `InMemoryStorageAdapter` for testing

2) Unit Tests: storage adapter tron power
- [x] Add tests in `rust-backend/crates/execution/src/storage_adapter.rs` (existing test module):
  - [x] `test_tron_power_bandwidth_only` → set freeze record 0 to 1_000_000; expect 1_000_000.
  - [x] `test_tron_power_energy_only` → set freeze record 1; expect amount.
  - [x] `test_tron_power_sum_bw_energy` → set 0+1; expect sum.
  - [x] `test_tron_power_includes_tron_power_legacy` → set 2; expect amount.
  - [x] `test_tron_power_all_three` → set 0+1+2; expect sum.
  - [x] `test_tron_power_overflow_protection` → near-`u64::MAX` across two resources; expect error.
  - [x] `test_tron_power_no_freeze_records` → no records; expect 0.

3) Integration Tests: VoteWitness with Freeze
- [x] File: `rust-backend/crates/core/src/tests.rs` (dedicated integration test module)
  - [x] `test_vote_witness_after_freeze_v1_succeeds`
    - [x] Create owner with sufficient balance.
    - [x] Set freeze record manually (resource=BANDWIDTH, amount=1_000_000).
    - [x] Execute VoteWitness transaction.
    - [x] Expect success (no REVERT), verify state changes include owner AccountChange.
  - [x] `test_vote_witness_multi_freeze_accumulates` (bonus test)
    - [x] Set multiple freeze records (BANDWIDTH + ENERGY).
    - [x] Verify total tron power is sum of both.
    - [x] Execute VoteWitness with accumulated power.
  - [x] Already implemented as bonus test above

4) Observability
- [x] Ensure logs around tron power computation include:
  - [x] Address (debug format), `new_model` flag from parameter.
  - [x] Per-resource frozen amounts and computed total.
  - [x] Overflow errors include intermediate values with descriptive message.

5) Validation Procedure (Local)
- [x] Build Rust backend: `cd rust-backend && cargo build --release` ✓ Succeeded
- [ ] Re-run remote execution for the block window [2142..2153]:
  - [ ] Use existing harness or minimal replay to re-generate `output-directory/execution-csv/...-remote-remote.csv`.
- [ ] Compare CSVs at rows 1042–1044:
  - [ ] `is_success` must be true for the VoteWitness at row 1044.
  - [ ] `state_change_count` and `state_digest_sha256` match embedded.
- [ ] Broader check:
  - [ ] Run CSV diff for the full file and confirm no new mismatches are introduced.

6) Rollout
- [ ] Land behind existing config; no new flags required.
- [ ] Document behavior in `docs/` (optional) and changelog.
- [ ] Monitor logs for tron power computations after deployment.

## Acceptance Criteria
- The previously failing VoteWitness (block 2153, tx 0) succeeds in remote mode with identical CSV fields to embedded (except run_id/ts_ms).
- No regression in earlier rows; the first mismatch moves past 1044 or disappears if this was the earliest.
- State digest for that tx matches embedded (owner AccountChange only; no storage change emissions).

## Risks & Edge Cases
- Legacy TRON_POWER-only states: we include resource=2 in the sum, so covered.
- Absence of freeze ledger for accounts that only have AccountCapsule-based power: still returns 0; acceptable for this phase (we do not ingest AccountCapsule in remote DB yet).
- Freeze V2/Delegations: not considered in this change; remote currently handles FreezeBalance(v1). If V2 is processed in embedded path, power may be understated until V2 handling is added to remote.
- Overflow: highly unlikely; guarded by `checked_add` with explicit error.

## Future Work (Out of Scope)
- Full `getAllTronPower()` parity by parsing/storing Protocol.Account; include V1/V2 frozen amounts and delegated balances per AccountCapsule logic.
- Emit freeze ledger changes to CSV (`emit_freeze_ledger_changes=true`) once embedded parity strategy is defined.
- Add Unfreeze/Delegation parity handlers in remote path.

## References
- CSVs: `output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv`, `output-directory/execution-csv/20251007-055437-2604b59f-remote-remote.csv` (rows 1042–1044)
- Rust tron power method: `rust-backend/crates/execution/src/storage_adapter.rs:1229`
- Freeze ledger API: `get_freeze_record`, `add_freeze_amount` (same file)
- Dynamic properties: `support_allow_new_resource_model()` (same file)
- Core handler: `rust-backend/crates/core/src/service.rs` (VoteWitness execution)
