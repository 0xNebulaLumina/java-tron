Emit freeze/resource ledger changes in remote mode (v7)

Context
- Mismatch source: Java BandwidthProcessor decides path pre-execution using local state. In remote mode, prior freeze ledger updates are not reflected in Java’s local stores, so `netLimit`=0 and path falls back to FREE_NET. Subsequent txs diverge in AEXT tail and digest.
- Goal: After each freeze-affecting tx executes remotely, emit precise freeze/resource ledger changes and apply them to Java stores so the next tx observes updated limits. Do not attempt to override path selection for the same tx.

Outcomes
- Remote backend emits structured freeze ledger deltas in ExecuteTransaction response when enabled.
- Java Runtime consumes and applies them to AccountStore/DynamicPropertiesStore (and delegated resource stores, as applicable), marking dirty keys.
- BandwidthProcessor sees correct `netLimit` for subsequent txs; CSV state_digest and AEXT tails match embedded.

Scope
- Contracts: FreezeBalance(V1/V2), UnfreezeBalance(V1/V2), WithdrawExpireUnfreeze, DelegateResource, UnDelegateResource, CancelAllUnfreezeV2. VoteWitness has no direct ledger effect.
- Stores updated: per-account Frozen/FrozenV2, global dynamic totals (TOTAL_NET_WEIGHT/LIMIT, TOTAL_ENERGY_WEIGHT/LIMIT), delegated resource (when applicable).
- Backward compatible; gated by config flag, default off.

Design Overview
1) Proto: add FreezeLedgerChange message (+ optional GlobalResourceTotalsChange) to ExecutionResult.
2) Rust backend: when `emit_freeze_ledger_changes` is true, compute and include relevant changes per tx. Persist to storage engine as it does today; emission is an additional, explicit description for Java.
3) Java: extend RemoteExecutionSPI to parse these messages; extend RuntimeSpiImpl to apply them to local stores immediately after each tx execution; keep existing account-level state change application.

Compatibility and Gating
- Flagged: execution.remote.emit_freeze_ledger_changes (TOML). Default false.
- When false: behavior unchanged; only account-level state change is emitted.
- When true: emit freeze changes in addition to existing state changes (account change remains for CSV parity).
- Java must treat absence of new fields as no-op for older backends.

Proto Changes (framework/src/main/proto/backend.proto)
- [ ] Add enums and messages:
  - FreezeLedgerChange { bytes owner_address; enum Resource { BANDWIDTH=0; ENERGY=1; TRON_POWER=2; } Resource resource; int64 amount; int64 expiration_ms; bool v2_model; }
  - GlobalResourceTotalsChange { int64 total_net_weight; int64 total_net_limit; int64 total_energy_weight; int64 total_energy_limit; }
- [ ] Extend ExecutionResult with:
  - repeated FreezeLedgerChange freeze_changes = <next_tag>;
  - repeated GlobalResourceTotalsChange global_resource_changes = <next_tag>;
Notes
- Semantics of `amount`: Prefer absolute latest value for idempotency. If delta is used, Java must read-modify-write; absolute simplifies retries.
- `v2_model`: true for FreezeV2/UnfreezeV2/Delegate/Undelegate; false for legacy Frozen fields.
- Keep `resource_usage` field unchanged; do not reuse it.

Rust Backend Tasks

Config
- [ ] Update default config/read path to expose flag:
  - rust-backend/config.toml: set `emit_freeze_ledger_changes = true` for test runs; keep default false in code default (crates/common/src/config.rs).
  - Log chosen AEXT mode and emit flag at startup for diagnostics.

Execution service (crates/core/src/service.rs)
- [ ] In execute_freeze_balance_contract: after `storage_adapter.add_freeze_amount(...)`:
  - If flag true, build one FreezeLedgerChange with owner, resource=BANDWIDTH/ENERGY/TRON_POWER, amount, expiration; set v2_model=false for V1 freeze.
  - Optionally compute and append one GlobalResourceTotalsChange if totals are mutated here.
- [ ] Mirror emission for:
  - UnfreezeBalanceContract (remove or adjust amounts; consider zero/or deletion semantics)
  - FreezeBalanceV2Contract / UnfreezeBalanceV2Contract
  - DelegateResourceContract / UnDelegateResourceContract
  - WithdrawExpireUnfreezeContract / CancelAllUnfreezeV2Contract
- [ ] Ensure storage adapter persists ledger as it does today (no change in persistence logic).

Response builder
- [ ] Where `ExecuteTransactionResponse` is constructed, push `freeze_changes` and `global_resource_changes` into `ExecutionResult` when flag is true.
- [ ] Keep account change emission intact (current CSV parity).

Storage adapter (crates/execution/src/storage_adapter.rs)
- [ ] Confirm/augment helpers present:
  - get_freeze_record/set_freeze_record/add_freeze_amount/remove_freeze_record
  - total weights/limits getters and setters for dynamic props; persist to properties DB
- [ ] If not persisting totals yet, either compute on Java side or extend adapter to write and emit totals consistently.

AEXT mode interactions
- [ ] Keep `accountinfo_aext_mode` as "hybrid" by default; optionally test "tracked" for authoritative counters. Emission of freeze changes is orthogonal.

Tests (Rust)
- [ ] Unit: for each freeze-affecting contract, assert:
  - When flag=false: no `freeze_changes` in response.
  - When flag=true: correct `freeze_changes` length and contents (owner/resource/amount/expiration/v2_model).
- [ ] Idempotency: applying same change twice should not corrupt ledger (absolute semantics recommended).
- [ ] Backward compat: response decodes with/without fields.

Logging & Metrics
- [ ] Add structured logs on emission to correlate with Java application.
- [ ] Optional metric counters: emitted_freeze_changes_total.

Java Tasks

Stubs and DTOs
- [ ] Regenerate gRPC stubs after proto change (framework module).
- [ ] Extend ExecutionProgramResult (framework/src/main/java/org/tron/core/execution/spi/ExecutionProgramResult.java) to carry `List<FreezeLedgerChange>` and optional `GlobalResourceTotalsChange`.
- [ ] Define new DTOs under `org.tron.core.execution.spi` mirroring proto (ownerAddress, resource enum, amount, expirationMs, v2Model).

RemoteExecutionSPI (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java)
- [ ] In convertExecuteTransactionResponse(...):
  - Parse `protoResult.getFreezeChangesList()` → populate DTO list.
  - Parse `protoResult.getGlobalResourceChangesList()` if present.
  - Attach to the returned ExecutionResult/ExecutionProgramResult.
- [ ] Keep existing account/storage state change conversion unchanged.

Runtime application (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java)
- [ ] Add `applyFreezeLedgerChanges(ExecutionProgramResult result, TransactionContext context)`:
  - For each FreezeLedgerChange:
    - Load `ChainBaseManager` and `AccountStore`.
    - Resolve owner `AccountCapsule` (create if missing, consistent with embedded behavior for first-time freeze).
    - If v2_model=false:
      - BANDWIDTH: update legacy Frozen via `setFrozenForBandwidth(frozenBalance, expireTime)`.
      - ENERGY/TRON_POWER: update corresponding legacy fields (`setFrozenForEnergy(...)` / `setFrozenForTronPower(...)`) if applicable.
    - If v2_model=true:
      - BANDWIDTH/ENERGY/TRON_POWER: update `FreezeV2` aggregate via `addFrozenBalanceForResource(...)` path or `updateFrozenV2List(...)` (sum to target amount if absolute semantics).
    - Persist with `AccountStore.put(addr, capsule)`; mark `ResourceSyncContext.recordAccountDirty(addr)`.
  - For each GlobalResourceTotalsChange:
    - Update `DynamicPropertiesStore` totals: `saveTotalNetWeight`, `saveTotalNetLimit`, `saveTotalEnergyWeight`, `saveTotalEnergyLimit2` as applicable.
    - Mark dynamic keys as dirty with `ResourceSyncContext.recordDynamicKeyDirty(...)`.
- [ ] Invocation order: call `applyFreezeLedgerChanges(...)` immediately after remote execution returns (within `execute(...)` flow), before or along with `applyStateChangesToLocalDatabase(...)`. This ensures state is persisted before the next tx in the same block is bandwidth-accounted.
- [ ] Leave existing account AEXT application intact (it updates usage counters and windows from backend AccountInfo).

Ordering and Consistency
- BandwidthProcessor runs before remote apply for the same tx: acceptable; the fix relies on remote apply of the freeze tx so the next tx observes updated netLimit.
- Ensure that for the block, tx processing order is preserved: Freeze tx executed and applied before Vote tx.

Dynamic Properties and Delegation
- [ ] If backend emits global totals, apply them; otherwise Java can recompute in its actuators as today.
- [ ] For Delegate/Undelegate, mirror embedded updates to `DelegatedResourceStore` (lock/unlock records) if/when enabled in backend; include corresponding changes or derive on Java side.

Validation Plan
- [ ] Re-run a small window including the Freeze tx preceding the first mismatch and the VoteWitness tx at block 2153.
- [ ] Verify Java logs show `path=ACCOUNT_NET` for the Vote owner (no `ACCOUNT_NET insufficient`).
- [ ] Diff CSVs for the Vote tx: identical `state_changes_json` AEXT tail and `state_digest_sha256`.
- [ ] Sanity-check other freeze-related txs for parity.

Risks & Edge Cases
- Idempotency: absolute vs delta semantics. Prefer absolute to avoid duplication after retries/reorg handling.
- Mixed legacy/V2 freeze models: ensure correct branch per v2_model; if network proposal toggles mid-epoch, guard with feature flags.
- Delegated resources and lock windows: applying partial changes without the companion dynamic totals may skew netLimit; keep totals in sync or recompute.
- Proto bloat: repeated changes per block are small; ensure no unbounded growth.
- Backward compatibility: older Java nodes should ignore unknown fields; newer Java must tolerate backends without the fields.

Rollout
- [ ] Keep flag default false in code; enable in CI/perf tests and for reconciliation runs.
- [ ] Document new setting in `rust-backend/config.toml` and README.
- [ ] Provide a JVM toggle to disable application on Java side (e.g., `-Dremote.exec.apply.freeze=false`) for rapid rollback without redeploying backend.

Acceptance Criteria
- After enabling flag, first divergence (VoteWitness at block 2153, tx 8ab04a...) no longer diverges: state digests match and BandwidthProcessor selects ACCOUNT_NET.
- No regression in unrelated tx types; when flag=false, outputs identical to baseline.

Task Checklist (condensed)

Proto
- [ ] Define messages and wire into ExecutionResult
- [ ] Regenerate stubs (Rust/Java)

Backend
- [ ] Read flag, log config
- [ ] Emit changes in Freeze/Unfreeze/Delegate-related paths
- [ ] Include changes in response builder
- [ ] Tests for emission and gating

Java
- [ ] Extend ExecutionProgramResult to carry changes
- [ ] Parse in RemoteExecutionSPI
- [ ] Apply in RuntimeSpiImpl (accounts + dynamic props)
- [ ] Ordering: apply post-exec per tx

Validation
- [ ] Re-run targeted window and verify parity

Notes for Implementers
- Keep emission granular (per owner/resource) and deterministic ordering.
- For Java application, favor set-to-absolute when possible to ensure idempotent replays.
- Reuse existing AccountCapsule APIs for window sizes and optimization bits (`setNewWindowSize`, `setWindowOptimized`) when backend provides those in AccountInfo/AEXT; freeze-change application should not touch windows directly.

