Global Resource Totals Emission (Phase 2) — Detailed Plan

Summary
- Problem: Remote execution emits FreezeLedgerChange but not GlobalResourceTotalsChange. Java applies account‑level freeze deltas but does not update TOTAL_NET_WEIGHT/TOTAL_NET_LIMIT (and energy totals) immediately. The next tx in the same block computes netLimit=0 and falls back to FREE_NET, diverging from embedded which uses ACCOUNT_NET.
- Evidence: For block 2153, tx 8ab04add… (VoteWitnessContract), embedded uses ACCOUNT_NET; remote uses FREE_NET due to netLimit=0. Remote Java logs for the prior freeze at block 2142 show freeze=1, global=0, so global totals stayed stale.
- Goal: Have the Rust backend emit GlobalResourceTotalsChange alongside FreezeLedgerChange for freeze/unfreeze transactions. Java already applies these in RuntimeSpiImpl.applyFreezeLedgerChanges(), which updates DynamicPropertiesStore totals before the next tx in the same block.

Scope and Non‑Goals
- In scope: Emitting global totals deltas from the backend and mapping them through gRPC to Java. Computing totals correctly from backend state.
- Out of scope: Refactoring Java BandwidthProcessor semantics; changing CSV serializer; energy pricing or fee accounting changes; broad storage engine redesign.

Acceptance Criteria
- With `emit_freeze_ledger_changes=true` and the new `emit_global_resource_changes=true`, replaying a freeze followed by a VoteWitness in the same block yields ACCOUNT_NET (not FREE_NET) in Java and removes the first CSV mismatch at block 2153.
- Java logs show “Applying freeze ledger changes … (freeze>=1, global>=1)” for freeze/unfreeze txs.
- ExecuteTransactionResponse carries non‑empty `global_resource_changes` when freezes occur.

Design Overview
- Add a new config flag `execution.remote.emit_global_resource_changes` (default false; enable in config.toml for parity runs).
- Extend execution result type to include `global_resource_changes: Vec<GlobalResourceTotalsChange>`.
- In freeze/unfreeze handlers, when the flag is enabled, compute current global totals and attach a single GlobalResourceTotalsChange per tx.
- Map the new field to protobuf in the gRPC response.
- Java already applies these global totals in `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` (applyGlobalResourceChange), so no Java code change is required.

Detailed TODOs

1) Configuration Wiring
- Add field to `RemoteExecutionConfig`:
  - `emit_global_resource_changes: bool`
- Set defaults in loader:
  - `Config.load()` default: `execution.remote.emit_global_resource_changes=false`
  - `impl Default for RemoteExecutionConfig`: `emit_global_resource_changes: false`
- Startup logging:
  - In `rust-backend/src/main.rs`, log the flag next to existing “Emit freeze ledger changes”.
- Sample config (enable for parity runs):
  - `rust-backend/config.toml` → `execution.remote.emit_global_resource_changes = true`

2) Execution Result Types (execution crate)
- In `rust-backend/crates/execution/src/tron_evm.rs`:
  - Define:
    - `pub struct GlobalResourceTotalsChange { pub total_net_weight: i64, pub total_net_limit: i64, pub total_energy_weight: i64, pub total_energy_limit: i64 }`
  - Extend `TronExecutionResult` with:
    - `pub global_resource_changes: Vec<GlobalResourceTotalsChange>`
  - Initialize `global_resource_changes` to `vec![]` in all constructors.

3) Storage Adapter Computations
- Add helpers in `rust-backend/crates/execution/src/storage_adapter.rs`:
  - `pub fn compute_total_net_weight(&self) -> Result<i64>`
    - Sum BANDWIDTH (resource=0) frozen amounts across all `freeze-records`, divide by TRX_PRECISION (1_000_000) with integer division to obtain “weight” (match Java’s `frozeBalance / TRX_PRECISION`).
    - Use `StorageEngine::prefix_query` or an iterator over `freeze-records` and filter by resource byte.
  - `pub fn compute_total_energy_weight(&self) -> Result<i64>`
    - Sum ENERGY (resource=1) frozen amounts similarly, divide by TRX_PRECISION.
  - Reuse existing getters:
    - `get_total_net_limit()` already present.
  - Energy total limit:
    - If a dynamic property getter exists (e.g., `TOTAL_ENERGY_CURRENT_LIMIT`), use it; otherwise, emit `0` for now (Java will set it, harmless if not used).
- Notes:
  - Favor correctness (full recompute) for Phase 2 parity; add an optimization task later for incremental maintenance.

4) Emit From System Contract Handlers (core service)
- Files: `rust-backend/crates/core/src/service.rs`
- Handlers to update:
  - `execute_freeze_balance`
  - `execute_unfreeze_balance`
  - `execute_freeze_balance_v2`
  - `execute_unfreeze_balance_v2`
- After persistence of freeze ledger and building `freeze_changes`:
  - If `emit_global_resource_changes` is true:
    - Compute:
      - `total_net_weight = storage_adapter.compute_total_net_weight()`
      - `total_net_limit = storage_adapter.get_total_net_limit()`
      - `total_energy_weight = storage_adapter.compute_total_energy_weight()`
      - `total_energy_limit = /* getter if present else 0 */`
    - Push a single `GlobalResourceTotalsChange { total_net_weight, total_net_limit, total_energy_weight, total_energy_limit }` into `result.global_resource_changes`.
    - Log emitted totals at INFO for traceability.

5) Protobuf Mapping (core service)
- In `convert_execution_result_to_protobuf(...)` (around `state_changes` and `freeze_changes` mapping):
  - Map each `GlobalResourceTotalsChange` to `crate::backend::GlobalResourceTotalsChange` and assign to `ExecutionResult.global_resource_changes`.
- Confirm: tonic includes `global_resource_changes` (present and currently set to `vec![]`).

6) Java Integration Verification (no code changes)
- Java side already applies:
  - `RuntimeSpiImpl.applyFreezeLedgerChanges(...)` calls `applyGlobalResourceChange(...)` → saves TOTAL_NET_WEIGHT, TOTAL_NET_LIMIT, TOTAL_ENERGY_WEIGHT, TOTAL_ENERGY_CURRENT_LIMIT and records dynamic keys.
  - Ensure JVM property `-Dremote.exec.apply.freeze=true` (default) remains enabled.
- Ordering:
  - Remote execution currently applies account state changes first, then freeze/global changes in the same method — sufficient because BandwidthProcessor is invoked per next tx.

7) Tests
- Rust unit tests (core service):
  - Construct a config with both `emit_freeze_ledger_changes=true` and `emit_global_resource_changes=true`.
  - Execute FreezeBalance with a known amount; assert response has `freeze_changes.len()==1` and `global_resource_changes.len()==1` with `total_net_weight` increased by `amount/TRX_PRECISION`.
  - Execute UnfreezeBalance; assert totals decrease accordingly (or to the reference baseline).
  - Multi‑owner scenario: two freezes by different owners → totals equal sum.
- Rust property/consistency checks:
  - Recompute totals by a fresh scan and compare with emitted values.
- Java integration smoke:
  - Run a short chain segment with a freeze followed by a VoteWitness in the same block; assert Java logs show ACCOUNT_NET for the vote and `Applying freeze ledger changes … (global=1)` for the freeze.

8) Observability and Logging
- Backend INFO logs when emitting global totals:
  - “Emitting global resource change: netWeight=X, netLimit=Y, energyWeight=U, energyLimit=V”.
- Java INFO logs already exist when applying global changes; confirm count.
- Optional metrics hooks for emitted global totals (future).

9) Performance Considerations
- Phase 2 baseline: full scan of `freeze-records` on each freeze/unfreeze is O(n). Acceptable for parity and small datasets.
- Follow‑ups for optimization:
  - Maintain and persist running `TOTAL_NET_WEIGHT`/`TOTAL_ENERGY_WEIGHT` in backend dynamic properties; adjust by deltas on freeze/unfreeze to O(1).
  - Key prefix iteration to avoid scanning non‑matching resources.

10) Edge Cases and Semantics
- TRX_PRECISION: use 1_000_000 (match Java `ChainConstant.TRX_PRECISION`).
- Integer division (floor) for weight: same as Java.
- Multiple operations in one block: totals must be recomputed/updated after each operation; Java applies changes per tx — correct order is maintained.
- Energy totals: emit weight and limit; safe to set limit to 0 until wired to dynamic properties.

11) Backward Compatibility and Rollback
- Disabled by default; enabling is config‑only.
- If issues are found, set `emit_global_resource_changes=false` (and/or `emit_freeze_ledger_changes=false`) and redeploy without code changes.

12) Milestones and Task Breakdown
- M1 — Config and Types (1d)
  - Add flag to `RemoteExecutionConfig`, defaults, loader, and startup logging.
  - Add `GlobalResourceTotalsChange` and extend `TronExecutionResult`.
- M2 — Totals Computation (1–2d)
  - Implement `compute_total_net_weight` and `compute_total_energy_weight`.
  - Add/get dynamic property accessors as needed (energy limit optional).
- M3 — Emission + Protobuf Mapping (1d)
  - Update freeze/unfreeze handlers to emit global totals behind the flag.
  - Map to protobuf in `convert_execution_result_to_protobuf`.
  - Add INFO logs.
- M4 — Tests + Verification (1–2d)
  - Rust unit/integration tests for emission and correctness.
  - Java smoke run validating ACCOUNT_NET path after freeze.
- M5 — Performance/Polish (optional)
  - Document optimization plan; light refactors if needed.

13) File Touchpoints (Checklist)
- Config
  - `rust-backend/crates/common/src/config.rs` (add flag + defaults)
  - `rust-backend/src/main.rs` (log flag)
  - `rust-backend/config.toml` (enable flag for parity runs)
- Execution types
  - `rust-backend/crates/execution/src/tron_evm.rs` (struct + field)
- Storage adapter
  - `rust-backend/crates/execution/src/storage_adapter.rs` (compute totals helpers)
- Core service
  - `rust-backend/crates/core/src/service.rs` (emit in handlers + map to proto)
- Docs
  - `rust-backend/docs/FREEZE_BALANCE_PHASE2_SUMMARY.md` (mention new flag and behavior)

14) Post‑Deploy Validation
- Reproduce the exact run where the first mismatch occurred and confirm:
  - After the freeze at block 2142, Java logs show global changes applied.
  - VoteWitness at block 2153 uses ACCOUNT_NET; the CSV’s state digest for that tx matches embedded.
  - Overall mismatch count across the run drops materially.

Appendix — Pseudocode Snippets

- compute_total_net_weight()
```
let mut total_sun: u128 = 0;
for kv in storage_engine.prefix_query("freeze-records", &[])? {
  let key = kv.key; // 0x41 + 20-byte addr + 1-byte resource
  if key.len() == 22 && key[21] == 0 /* BANDWIDTH */ {
    let rec = FreezeRecord::deserialize(&kv.value)?;
    total_sun = total_sun.checked_add(rec.frozen_amount as u128)
      .ok_or("overflow")?;
  }
}
let weight = (total_sun / 1_000_000u128) as i64; // TRX_PRECISION
Ok(weight)
```

- Emission in handler
```
if cfg.remote.emit_global_resource_changes {
  let net_w = storage_adapter.compute_total_net_weight()?;
  let net_l = storage_adapter.get_total_net_limit()?;
  let energy_w = storage_adapter.compute_total_energy_weight()?;
  let energy_l = 0; // or getter when available
  result.global_resource_changes.push(GlobalResourceTotalsChange{
    total_net_weight: net_w,
    total_net_limit: net_l,
    total_energy_weight: energy_w,
    total_energy_limit: energy_l,
  });
}
```

