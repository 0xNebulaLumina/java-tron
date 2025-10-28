Emit Freeze Ledger Changes v7 — Review Plan & TODOs

Context
- Goal: Ensure remote mode emits and applies freeze/resource ledger changes so Java’s Bandwidth/Energy processors see updated limits before the next transaction, restoring CSV/digest parity with embedded mode.
- Inputs: planning/emit_freeze_ledger_changes-7.planning.md, planning/emit_freeze_ledger_changes-7.todo.md, last 7 commits.
- Current: Proto fields exist; Rust emits freeze_changes for V1/V2; Java parses and applies; tests exist. Key gaps: V2 application uses delta add* instead of absolute set semantics; no dirty marks; no Java rollback toggle; global totals emission deferred.

Acceptance Criteria
- Parity: First divergence case (block 2153 VoteWitness) disappears when flag on; state_digest_sha256 and state_changes_json match embedded for the window.
- Path selection: Java logs show ACCOUNT_NET path (no FREE_NET fallback with netLimit=0) for the affected vote tx.
- Idempotency: Applying the same freeze change twice does not change outcomes (absolute semantics).
- Gating: With flags off, behavior identical to baseline (Phase 1 parity).

Decisions & Semantics
- Emission semantics: Emit absolute amounts (post-op totals) for both V1 and V2; v2_model=true indicates FrozenV2 path. amount=0 indicates full unfreeze.
- Java application semantics:
  - V1: setFrozenForBandwidth/energy with absolute amount and expiration.
  - V2: set-to-absolute using FrozenV2 list (update existing entry amount or add; if amount=0, remove entry or set to 0 consistently). Prefer removal when zero if it doesn’t break readers.
- TRON_POWER mapping: In V1, treat as BANDWIDTH (consistent with embedded). In V2, update TronPower V2 aggregate.
- Ordering: Freeze ledger application must complete before the next tx in the same block is bandwidth-accounted. Order relative to account state sync can be before or after, but both must occur before the next tx.
- Global totals: Keep emission deferred initially; Java may recompute where needed. Add emission later behind the same flag if required.

Detailed TODOs

Proto & Wire (verification-only)
- [ ] Verify tag numbers for new repeated fields are stable and not colliding: `framework/src/main/proto/backend.proto:589,612–613`.
- [ ] Confirm regenerated stubs are committed for both Java and Rust builds.
- [ ] Ensure deterministic ordering of `freeze_changes` emission per transaction (sort by resource, then owner if multiple) to aid testing.

Rust Backend
- Config & Logging
  - [ ] On startup, log `remote.emit_freeze_ledger_changes`, `remote.freeze_balance_*_enabled`, and `remote.accountinfo_aext_mode` with module name and version.
  - [ ] Keep code defaults to `false` for all freeze flags (`crates/common/src/config.rs`); allow overriding via `config.toml`/env.

- Emission Semantics
  - [ ] Ensure emitted `amount` for V1 is absolute (post-aggregation) and `v2_model=false`.
  - [ ] Ensure emitted `amount` for V2 is absolute and `v2_model=true`.
  - [ ] For full unfreeze V1/V2, emit `amount=0` and `expiration_ms=0`.
  - [ ] Confirm owner address uses 21-byte Tron format in protobuf: `add_tron_address_prefix()` in response builder.
  - [ ] Add explicit resource mapping tests for BANDWIDTH/ENERGY/TRON_POWER.

- Response Builder
  - [ ] Keep single AccountChange emission for CSV parity; include `freeze_changes` only when flag is true.
  - [ ] Add unit test asserting protobuf freeze_changes round-trip address/resource correctness.

- Contract Coverage (existing + backlog)
  - [x] FreezeBalance V1 emission (done)
  - [x] UnfreezeBalance V1 emission (done)
  - [x] FreezeBalance V2 emission (done)
  - [x] UnfreezeBalance V2 emission (done)
  - [ ] DelegateResource/UnDelegateResource (emit v2_model=true deltas as absolute totals)
  - [ ] WithdrawExpireUnfreeze/CancelAllUnfreezeV2 (emit absolute remaining or zero)

- Tests
  - [ ] Add tests asserting absolute semantics: executing the same freeze tx twice with emission on does not change post-op totals beyond first persist (emission remains absolute and idempotent).
  - [ ] Add tests for amount=0 (full unfreeze) paths; ensure correctness of `expiration_ms`.
  - [ ] Add ordering and determinism test: multiple changes sort order stable.
  - [ ] Optional: integration test ensuring emitted changes are present/absent based on flag.

Java Integration
- RuntimeSpiImpl apply (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java)
  - [ ] Add JVM toggle `-Dremote.exec.apply.freeze` (default `true`). If `false`, skip `applyFreezeLedgerChanges()` entirely for rapid rollback.
  - [ ] V1 absolute set (already correct): continue using `setFrozenForBandwidth(amount, expirationMs)` and `setFrozenForEnergy(amount, expirationMs)`.
  - [ ] V2 absolute set (fix): replace add* calls with absolute set logic:
    - Load current `FrozenV2` entry by resource.
    - If `amount>0`: replace or add entry with `FreezeV2{ type, amount }` using `updateFrozenV2List(...)` or `addFrozenV2List(...)`.
    - If `amount==0`: remove entry or set to 0 consistently (choose removal unless downstream requires presence).
  - [ ] TRON_POWER V2: use appropriate AccountCapsule helper for TronPower (absolute set behavior mirroring BANDWIDTH/ENERGY logic).
  - [ ] Dirty marks: after account updates, call `ResourceSyncContext.recordAccountDirty(ownerAddress)`.
  - [ ] Global totals apply (if present): use `DynamicPropertiesStore.saveTotalNetWeight(...)`, `saveTotalNetLimit(...)`, `saveTotalEnergyWeight(...)`, `saveTotalEnergyCurrentLimit(...)` and mark keys via `ResourceSyncContext.recordDynamicKeyDirty(...)` with the exact key bytes used elsewhere.
  - [ ] Logging: log owner base58, resource, amount, expiration, v2 flag at debug; warn on unknown resource; no-op cleanly when lists empty.
  - [ ] Ordering review: ensure freeze changes + state changes apply before the next tx in block. Consider moving freeze apply before or alongside `applyStateChangesToLocalDatabase(...)` (not strictly required if both complete before next tx).

- RemoteExecutionSPI parse (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java)
  - [ ] Verify resource enum mapping has a strict default (warn + skip unknowns instead of defaulting to BANDWIDTH silently).
  - [ ] Keep freeze/global changes order; do not reorder on client.
  - [ ] Metrics: emit `remote.freeze_changes_count` and include `remote.apply.freeze.enabled` gauge (from JVM toggle).

- EmbeddedExecutionSPI (framework/src/main/java/org/tron/core/execution/spi/EmbeddedExecutionSPI.java)
  - [ ] Continue returning empty freeze/global change lists for API consistency.

Java Tests
- [ ] Unit tests for `RuntimeSpiImpl.applyFreezeLedgerChanges`:
  - V1: set-to-absolute for BANDWIDTH and ENERGY; idempotency (apply twice = same state).
  - V2: set-to-absolute for BANDWIDTH/ENERGY/TRON_POWER; create/update/remove behavior; idempotency.
  - Dirty marks: verify `ResourceSyncContext` receives account dirty markers (can be via a test double if available).
  - Global totals: when provided, totals written and dynamic keys marked.
  - JVM toggle: when `-Dremote.exec.apply.freeze=false`, no changes applied.

Validation Plan
- Targeted window replay (blocks 2150–2155):
  - [ ] Backend: start with `execution.remote.emit_freeze_ledger_changes=true`, `execution.remote.accountinfo_aext_mode="hybrid"`.
  - [ ] Java: run in remote storage mode with JVM toggle default (apply enabled).
  - [ ] Verify logs show ACCOUNT_NET chosen for the first VoteWitness (no FREE_NET fallback with netLimit=0).
  - [ ] CSV parity: `state_digest_sha256` and AEXT tails match embedded at the divergence index.
- Idempotency check:
  - [ ] Simulate duplicate apply by re-running `applyFreezeLedgerChanges` on the same `ExecutionProgramResult` in a test harness; confirm no state drift.
- Backward compatibility:
  - [ ] With backend flag off or JVM toggle off, outputs identical to baseline.

Observability & Docs
- [ ] Add debug logs around freeze emission and application; include addresses/resources/amounts.
- [ ] Document new flags:
  - Rust: `[execution.remote] emit_freeze_ledger_changes`, `*_enabled`, `accountinfo_aext_mode` in `rust-backend/config.toml`.
  - Java: `-Dremote.exec.apply.freeze=true|false` and `-Dremote.exec.accountinfo.resources.enabled` (if applicable).
- [ ] README updates: how to enable feature and validate parity.

Rollout & Risk Mitigation
- [ ] Default backend emit flag to false; enable only in validation runs.
- [ ] Provide fast rollback via Java JVM toggle without redeploying backend.
- [ ] Guard V2 absolute set code with robust null/empty checks to avoid NPEs on new accounts.
- [ ] Monitor logs for unknown resource types or proto mismatches.

Open Questions
- Should V2 `amount=0` remove the FreezeV2 entry or set amount to 0? Default to removal if no code relies on zero entries; otherwise set 0 for explicit state.
- When should we implement delegated resource and withdraw-expire-unfreeze emissions? Propose follow-up task after core parity is validated.
- Do we need global totals emission now, or can Java recompute reliably in remote mode for processors? If required, add emission behind the same flag with deterministic ordering.
- Any consumer reading FrozenV2 lists expecting strictly positive amounts? Audit and decide on the zero-vs-remove behavior accordingly.

Owners & Timeline (proposal)
- Week 1: Java V2 absolute apply + dirty marks + JVM toggle + unit tests; backend logging; validation replay.
- Week 2: Delegate/UnDelegate + WithdrawExpire/CancelAllUnfreeze emissions; optional global totals emission; extended tests; docs.

