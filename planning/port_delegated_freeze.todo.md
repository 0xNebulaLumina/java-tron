# Port Delegated Freeze Parity (Phase 2)

This plan brings remote execution + remote storage into parity with embedded Java for delegation and tron power computation, eliminating schedule mismatches around maintenance and vote-based SR ordering.

Scope focuses on:
- Delegated freeze ledgers (V1, V2) read/write parity
- Tron power parity (old vs new model)
- State sync between Rust and Java for delegation
- Optional: enrich remote Account protobuf for storage parity

Non-goals:
- Changing committee semantics or governance toggles
- Reworking Java-side business rules (follow Java as source of truth)

---

## Functional Parity Specification

- Terminology
  - V1 freeze: legacy frozen for BANDWIDTH (Account.Frozen) and ENERGY (AccountResource.frozenBalanceForEnergy)
  - V2 freeze: Account.FreezeV2 list entries with type = BANDWIDTH or ENERGY (exclude TRON_POWER)
  - Delegated-out: amounts a delegator has delegated to others (counts toward delegator tron power)
  - Acquired delegated: amounts a receiver obtains from others (does NOT count toward receiver tron power)

- Tron Power (Java reference)
  - Old model: tronPower = own V1(BW+EN) + own V2(BW+EN) + delegated-out (V1+V2, BW+EN). Exclude TRON_POWER.
  - New model: same as old, plus legacy/TRON_POWER semantics if committee toggles apply via AccountCapsule#getAllTronPower (Java’s ultimate source of truth). Align behavior to what Java uses when `supportAllowNewResourceModel()` is true.

- Vote validation (Java parity)
  - Vote address must be a valid account and an existing witness
  - Sum(votes) in TRX ≤ tronPower (but allow slippage only until delegation parity is fully ported; once parity implemented, enforce strictly)

---

## Design Overview

We mirror Java’s delegation structures and behavior in the Rust backend:

- Add DelegatedResource store (lock/unlock records) with V2-compatible keys
- Extend execution to mutate delegation on Delegate/UnDelegate and on expiry
- Update tron power computation to include delegated-out totals
- Surface delegation changes over gRPC; apply them on Java to keep local stores consistent
- Optional: enrich remote Account protobuf bytes with delegated totals and freeze lists used by Java reads

---

## Detailed TODOs

### 1) Rust Storage: Delegated Resources DB

Files:
- rust-backend/crates/execution/src/storage_adapter/engine.rs

Tasks:
- [ ] Define database name: `"DelegatedResource"` (match Java store name)
- [ ] Key formats (mirror Java):
  - V2 unlocked key: `0x01 || from(21) || to(21)` (see Java V2_PREFIX)
  - V2 lock key: `0x02 || from(21) || to(21)` (see Java V2_LOCK_PREFIX)
- [ ] Value format fields:
  - frozen_balance_for_bandwidth: i64
  - frozen_balance_for_energy: i64
  - expire_time_for_bandwidth: i64 (ms)
  - expire_time_for_energy: i64 (ms)
- [ ] CRUD helpers:
  - `get_delegation(from, to, lock: bool) -> Delegation`
  - `set_delegation(from, to, lock: bool, record)`
  - `remove_delegation(from, to, lock: bool)`
  - `unlock_expired(now_ms)` — iterate lock entries and move expired amounts to unlocked, zeroing locks
- [ ] Prefix queries:
  - by from: sum delegated-out totals (BW/EN)
  - by to: sum acquired totals (BW/EN)
- [ ] Performance: maintain optional per-address cached totals; invalidate on mutations

### 2) Rust Execution: Contract Handlers

Files:
- rust-backend/crates/core/src/service/mod.rs

Tasks:
- [ ] Implement `DelegateResourceContract`:
  - Create/update lock record (resource, amount, expire_time = block_time + duration)
  - Emit state changes:
    - Delegator account: increment delegated-out totals (V1 or V2 according to model), both BW/EN paths
    - Receiver account: increment acquired delegated totals (BW/EN)
- [ ] Implement `UnDelegateResourceContract`:
  - Reduce lock/unlock record amounts accordingly; if zero, remove
  - Emit matching account state changes
- [ ] Expiry processing per block:
  - Before/after tx batch in a block (consistent with Java), run `unlock_expired(block_time)` for all lock entries; emit account changes for moved amounts
- [ ] Extend execution result with `DelegationChange` list carrying:
  - from, to, resource (BW/EN), amount, expire_time, v2_model, operation (add/remove/unlock)

### 3) Rust: Tron Power Computation

Files:
- rust-backend/crates/execution/src/storage_adapter/engine.rs

Tasks:
- [ ] Compute own freezes: V1(BW, EN) + V2 entries type in {BANDWIDTH, ENERGY} (exclude TRON_POWER)
- [ ] Add delegated-out totals (V1+V2, BW+EN) to tron power for owner
- [ ] Old vs new model switch:
  - Old: exclude TRON_POWER entirely
  - New: match Java `getAllTronPower()` behavior under `supportAllowNewResourceModel()`
- [ ] Unit tests for multiple combinations (only BW, only EN, BW+EN, with/without delegation)

### 4) gRPC: Extend Execution Response Schema

Files:
- Execution protos used by `org.tron.common.client.ExecutionGrpcClient` (add a `DelegationChange` repeated field in execution result)
- framework/src/main/java/org/tron/common/client/ExecutionGrpcClient.java

Tasks:
- [ ] Define proto message:
  - `message DelegationChange { bytes from; bytes to; uint32 resource; int64 amount; int64 expire_ms; bool v2_model; enum Op { ADD=0; REMOVE=1; UNLOCK=2; } Op op; }`
- [ ] Add `repeated DelegationChange delegation_changes = N;` to the execution result
- [ ] Map in Rust response population and Java client parsing

### 5) Java Apply: RuntimeSpiImpl

Files:
- framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java

Tasks:
- [ ] Add handler similar to `applyFreezeLedgerChanges`:
  - `applyDelegationChanges(List<DelegationChange>, ChainBaseManager, TransactionContext)`
- [ ] For each change:
  - Delegator: update AccountCapsule delegated totals
    - V1: `set/addDelegatedFrozenBalanceForBandwidth/ForEnergy`
    - V2: `set/addDelegatedFrozenV2BalanceForBandwidth` and AccountResource.V2 energy
  - Receiver: update acquired delegated totals (for BW/EN) — `set/addAcquiredDelegated...`
  - Persist DelegatedResourceStore entry (createDbKeyV2 lock/unlock accordingly)
  - Record dirty via `ResourceSyncContext.recordAccountDirty(...)`
- [ ] Invoke from transaction flow after freeze/global changes application

### 6) Java Chainbase: Delegated Stores Parity

Files:
- chainbase/src/main/java/org/tron/core/store/DelegatedResourceStore.java
- chainbase/src/main/java/org/tron/core/capsule/DelegatedResourceCapsule.java
- chainbase/src/main/java/org/tron/core/capsule/DelegatedResourceAccountIndexCapsule.java

Tasks:
- [ ] Ensure writes done by RuntimeSpiImpl mirror fields Java normally updates in embedded path
- [ ] Maintain indices (from/to lists in DelegatedResourceAccountIndexCapsule) if required by consumers

### 7) Storage SPI: Remote DB and Account Serialization (Optional)

Files:
- framework/src/main/java/org/tron/core/storage/spi/StorageSpiFactory.java (DB names mapping)
- rust-backend/crates/execution/src/storage_adapter/engine.rs (serialize_account)

Tasks:
- [ ] Expose `"DelegatedResource"` DB through StorageSPI to Rust backend
- [ ] Enrich remote Account serialization to include:
  - V1 frozen (BW, EN) fields
  - V2 FreezeV2 entries for BANDWIDTH/ENERGY
  - Delegated-out totals (V1+V2) for BW/EN
  - Acquired delegated totals for receiver (for resource accounting)
  - Keep fields Java reads intact even after restart

### 8) Resource Accounting Parity

Files:
- rust-backend/crates/execution/src/storage_adapter/resource.rs (if exists) or service/mod.rs
- framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java

Tasks:
- [ ] Ensure receiver’s acquired delegated fields influence bandwidth/energy accounting (not tron power)
- [ ] Continue emitting/consuming global resource totals (TOTAL_NET_WEIGHT/LIMIT, TOTAL_ENERGY_WEIGHT/LIMIT)

### 9) Expiry Semantics

Files:
- rust-backend/crates/execution/src/storage_adapter/engine.rs
- chainbase/src/main/java/org/tron/core/store/DelegatedResourceStore.java

Tasks:
- [ ] Implement lock → unlock transfer at expiry (per resource) with zeroing of expired sides
- [ ] Emit DelegationChange(UNLOCK) for Java apply
- [ ] Add consistency checks/invariants (no negative amounts, no mixed negative deltas)

### 10) Flags & Rollout

Tasks:
- [ ] Add execution feature flag: `remote.exec.delegation.enabled=true` to guard activation
- [ ] Add vote strictness flag: `remote.exec.vote.strict_power_check=true` (defaults to true once parity is verified; during rollout keep it false to avoid false negatives)

### 11) Observability

Tasks:
- [ ] Add structured logs for:
  - Delegation ops (from, to, resource, amount, expire_ms, op)
  - Tron power components (own V1, own V2, delegated-out V1/V2, total)
  - Vote validation decisions
- [ ] Metrics counters/timers for delegation mutations and expiry processing

### 12) Testing

Rust Unit Tests:
- [ ] tron power with: only BW, only EN, BW+EN, with and without delegation (old/new model)
- [ ] Delegate/UnDelegate lifecycle (lock, unlock, expiry)
- [ ] Receiver acquired accounting unaffected tron power

Integration Tests (end-to-end):
- [ ] Freeze → Delegate → VoteWitness; verify java vs rust parity for vote accept/reject and SR ordering after maintenance
- [ ] Expiry during epoch boundary
- [ ] Toggle new resource model flag mid-run

Performance Tests:
- [ ] Scan costs for totals; validate caching effectiveness

### 13) Migration / Bootstrap

Tasks:
- [ ] At backend startup, ensure `DelegatedResource` DB exists remotely
- [ ] (Optional) Reconcile existing entries to rebuild cached totals
- [ ] Validate a few sampled accounts: Java AccountCapsule delegated totals match sums from delegated DB

### 14) Risks & Mitigations

Risks:
- Remote scans for totals could be expensive
- Partial parity leading to schedule drift
- Serialization mismatches on Account protobuf

Mitigations:
- Add per-address cached totals; invalidate on mutation
- Gate with `remote.exec.delegation.enabled`; keep strict power check off until validated
- Golden tests on mainnet slices around maintenance boundaries

---

## Work Breakdown Summary

1) Storage (engine.rs): DelegatedResource DB + helpers
2) Execution (service/mod.rs): handlers + expiry + DelegationChange
3) Tron power (engine.rs): include delegated-out, old/new model handling
4) gRPC/protos + client: add DelegationChange in responses
5) Java RuntimeSpiImpl: apply delegation changes + store updates + sync
6) Optional: Account serialization enrichment in remote storage
7) Tests + instrumentation + rollout flags

---

## Acceptance Criteria

- VoteWitness parity against embedded for a multi-hour mainnet slice including a maintenance boundary
- No `ValidBlock failed` witness schedule mismatches in remote mode
- Measured tron power components match Java for sampled accounts (with/without delegation)
- Stable performance with delegation-heavy workloads

