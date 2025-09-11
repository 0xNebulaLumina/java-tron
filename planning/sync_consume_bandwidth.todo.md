# Remote Storage Resource Sync — Detailed TODO/Checklist

Objective: After Java applies non‑EVM resource/bandwidth/fee mutations locally, immediately persist those deltas so the Rust backend (remote storage) observes the same “old” next time (before any subsequent remote execution/read).

This plan introduces a lightweight Resource Sync context + service, instruments the small number of mutation hotspots, and flushes deltas just before remote execution. It uses existing StorageSPI (already wired to the Rust backend) and avoids heavy refactors.

---

## 0) Scope and Success Criteria

- [ ] Scope covers non‑EVM and pre‑exec mutations to:
  - [ ] Account resource fields: bandwidth (`netUsage`, `freeNetUsage`, window timestamps), energy (`energyUsage`, V2 window sizes, timestamps), account creations, fee deductions/blackhole moves.
  - [ ] Dynamic properties mutated by these paths: `publicNetUsage`, `publicNetTime`, `TOTAL_TRANSACTION_COST`, `TOTAL_CREATE_ACCOUNT_COST`, `BLOCK_ENERGY_USAGE`, energy average/limits (when changed in‑block), `TRANSACTION_FEE_POOL`, `BURN_TRX_AMOUNT`, related histories.
  - [ ] Asset issue(±v2) public free bandwidth usage when TRC‑10 owner subsidy is used.
- [ ] Success criteria
  - [ ] Immediately before any remote execution or state query in the same transaction, the Rust backend can read the exact state produced by Java resource updates.
  - [ ] No material performance regression (batching minimizes gRPC calls).
  - [ ] Disabled by flag without functional changes elsewhere.

---

## 1) Configuration and Flags

- [x] Default enablement: turn ON automatically when `StorageSpiFactory.determineStorageMode() == REMOTE`.
- [x] System properties/env
  - [x] `-Dremote.resource.sync.enabled=true|false` (default true in REMOTE; false in EMBEDDED)
  - [x] `-Dremote.resource.sync.debug=false` (extra logs)
  - [x] `-Dremote.resource.sync.confirm=false` (optional read‑back diagnostics)

---

## 2) Core Types and Service

Add new classes in `framework/src/main/java/org/tron/core/storage/sync/`:

- [x] `ResourceSyncContext`
  - [x] Thread‑local holder (similar to `StateChangeJournalRegistry`)
  - [x] API: `begin(TransactionContext ctx)`, `recordAccountDirty(byte[] addr)`, `recordDynamicKeyDirty(byte[] key)`, `recordAssetIssueDirtyV1(byte[] assetName)`, `recordAssetIssueDirtyV2(byte[] assetId)`, `flushPreExec()`, `finish()`
  - [x] Holds minimal sets: `{accounts, dynamicKeys, assetIssueV1Keys, assetIssueV2Keys}`
  - [x] No heavy serialization in hot path; only keys bookkeeping

- [x] `ResourceSyncService`
  - [x] Resolve DB names: `account`, `properties`, `asset-issue`, `asset-issue-v2`
  - [x] Build batches on `flushPreExec()` by reading latest values from stores:
    - [x] Accounts: `AccountStore.getUnchecked(addr)` → put serialized capsule bytes
    - [x] Dynamic props: each dirty key → `DynamicPropertiesStore.getUnchecked(key)`
    - [x] Asset issue V1/V2: `AssetIssueStore.get(assetName)`, `AssetIssueV2Store.get(assetId)`
  - [x] Batch calls per DB: `StorageSPI.batchWrite(dbName, Map<byte[],byte[]>)`
  - [x] Ordering: asset issue → accounts → dynamic props (see §6)
  - [x] Error handling: log and continue; do not abort tx. Optional circuit‑breaker to auto‑disable after N failures.
  - [x] Metrics (see §8)

---

## 3) Hook Points and Instrumentation

Minimal, focused hooks to mark dirties and to flush once per tx before remote exec.

### 3.1 Manager (central coordination)

Files: `framework/src/main/java/org/tron/core/db/Manager.java`

- [x] In `processTransaction(...)`:
  - [x] Call `ResourceSyncContext.begin(context)` right after `TransactionTrace trace = ...` and before any resource consumption.
  - [x] After `consumeBandwidth(trxCap, trace)` / `consumeMultiSignFee(...)` / `consumeMemoFee(...)`, and before `trace.exec()`:
    - [x] `ResourceSyncContext.flushPreExec()` to push all pre‑exec deltas to remote storage.
  - [x] After CSV logging / finalization: `ResourceSyncContext.finish()` to clear thread‑local.

- [x] In `consumeMultiSignFee(...)`:
  - [x] After balance deduction and burn/blackhole move, record:
    - [x] `recordAccountDirty(ownerAddress)`
    - [x] If burn: mark dynamic key dirty for `BURN_TRX_AMOUNT`
    - [x] If fee pool: mark dynamic key dirty for `TRANSACTION_FEE_POOL`

- [x] In `consumeMemoFee(...)`:
  - [x] Same as above for memo fee path.

### 3.2 BandwidthProcessor (non‑VM net usage, fees, public usage)

File: `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java`

- [x] After `accountStore.put(accountCapsule.createDbKey(), accountCapsule)` (owner)
  - [x] `recordAccountDirty(owner)`
- [x] If issuer path (TRC‑10): after issuer `accountStore.put(...)`
  - [x] `recordAccountDirty(issuer)`
- [x] After `dynamicPropertiesStore.savePublicNetUsage(...)` and `savePublicNetTime(...)`
  - [x] `recordDynamicKeyDirty(PUBLIC_NET_USAGE)` and `recordDynamicKeyDirty(PUBLIC_NET_TIME)`
- [x] After `dynamicPropertiesStore.addTotalTransactionCost(fee)`
  - [x] `recordDynamicKeyDirty(TOTAL_TRANSACTION_COST)`
- [x] Asset owner subsidy path (TRC‑10): after public free‑asset usage updates
  - [x] For V1: `recordAssetIssueDirtyV1(assetName)`
  - [x] For V2: `recordAssetIssueDirtyV2(assetId)`
- [x] Fee fallback (`useTransactionFee` → `consumeFeeForBandwidth`) paths:
  - [x] Mark payer account dirty; mark burn/pool related dynamic keys dirty as applicable.

### 3.3 EnergyProcessor (non‑VM energy usage & block counters)

File: `chainbase/src/main/java/org/tron/core/db/EnergyProcessor.java`

- [x] After `accountStore.put(accountCapsule.createDbKey(), accountCapsule)`
  - [x] `recordAccountDirty(address)`
- [x] After `dynamicPropertiesStore.saveBlockEnergyUsage(...)`
  - [x] `recordDynamicKeyDirty(BLOCK_ENERGY_USAGE)`

### 3.4 VMActuator (freeze‑v2 pre‑merge windows)

File: `actuator/src/main/java/org/tron/core/vm/VMActuator.java`

- [ ] In V2 flows that update usage/windows and call `rootRepository.updateAccount(...)`:
  - [ ] Mark updated accounts dirty (caller and/or creator): `recordAccountDirty(address)`

### 3.5 Native resource processors (delegate/undelegate)

Files: `actuator/src/main/java/org/tron/core/vm/nativecontract/*DelegateResource*Processor.java`

- [ ] After updating owner/receiver windows/usage and persisting:
  - [ ] `recordAccountDirty(owner)` and `recordAccountDirty(receiver)`

---

## 4) Service Behavior and Ordering Rules

- [ ] Only flush if `remote.resource.sync.enabled == true` and storage mode is REMOTE.
- [ ] Collect keys incrementally during tx; perform 1 flush just before `trace.exec()`.
- [ ] Ordering inside flush:
  1. [ ] Asset issue V1/V2 (issuer/public free usage)
  2. [ ] Accounts (all changed addresses)
  3. [ ] Dynamic properties
- [ ] Batching: one `batchWrite` per DB.
- [ ] Optional confirm: if `remote.resource.sync.confirm`, follow with `batchGet` to verify presence (diagnostics only).

---

## 5) Error Handling & Fallbacks

- [ ] On gRPC error during flush:
  - [ ] Log error with counts and first few keys (debug‑safe truncation)
  - [ ] Increment failure counter; if failures exceed threshold within sliding window, auto‑disable sync and warn once.
  - [ ] Do NOT fail transaction execution.

---

## 6) Tests

### 6.1 Unit Tests

- [ ] `ResourceSyncContextTest`
  - [ ] Begin/record/flush/finish lifecycle
  - [ ] Thread‑local isolation

- [ ] `ResourceSyncServiceTest`
  - [ ] Given mocked Stores + SPI, flush builds correct per‑DB batches and calls `batchWrite` in order (asset → account → props)
  - [ ] Confirm flag triggers `batchGet`
  - [ ] Error path triggers counters and disables after threshold

### 6.2 Processor Unit Tests (extend existing)

- [ ] `BandwidthProcessorTest`
  - [ ] Verify recordAccountDirty invoked for owner/issuer; dynamic keys marked; asset issue marked for V1/V2

- [ ] `EnergyProcessorTest`
  - [ ] Verify recordAccountDirty and block energy key tagged

### 6.3 Integration Tests

- [ ] `DualStorageModeIntegrationTest` (extend or add)
  - [ ] Run with `STORAGE_MODE=remote` and a mocked `RemoteStorageSPI` (no network) that records batchWrite inputs
  - [ ] Submit tx that triggers bandwidth fee + memo fee path; assert pre‑exec flush includes:
    - [ ] Owner account bytes
    - [ ] `TOTAL_TRANSACTION_COST`, `MEMO_FEE` impact (balance change + burn/pool)
  - [ ] For TRC‑10 path (when enabled): assert asset issue and issuer account changes included

### 6.4 Manual (with Rust backend)

- [ ] Start `tron-backend` (`rust-backend/`), run node with `STORAGE_MODE=remote` and enable sync
- [ ] Send transactions per scenarios; verify backend logs/metrics show updated values prior to execution

---

## 7) Metrics & Logging

- [ ] Counters
  - [ ] `resource_sync.flush.count`
  - [ ] `resource_sync.flush.error.count`
  - [ ] `resource_sync.keys.accounts` / `...keys.dynamic` / `...keys.assets`
- [ ] Timers
  - [ ] `resource_sync.flush.latency_ms`
- [ ] Gauges (optional)
  - [ ] `resource_sync.failures.window`
- [ ] Log lines (debug): tx id, batch sizes per DB, latency, confirm miss count (if confirm enabled)

---

## 8) Documentation

- [ ] Add a section to `build.md` or `docs/`:
  - [ ] What is synced and when
  - [ ] Flags
  - [ ] How to troubleshoot (enable debug + confirm, inspect logs)
  - [ ] Known limitations (no multi‑DB transaction binding yet in SPI)

---

## 9) Risk & Compatibility

- [ ] Double writes safety: Existing store `put(...)` already writes via SPI; pre‑exec flush re‑reads current values and overwrites idempotently (same bytes). This is about timing/visibility, not duplication.
- [ ] Performance: O(1) flush per tx with 3 batch calls; bounded set sizes (only dirties).
- [ ] Backward compat: Fully gated by `remote.resource.sync.enabled` and REMOTE mode detection.

---

## 10) Concrete Code Targets (Checklist by file)

- [x] Add: `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncContext.java`
- [x] Add: `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java`
- [x] Update: `framework/src/main/java/org/tron/core/db/Manager.java`
  - [x] `processTransaction(...)`: begin → flushPreExec → finish
  - [x] `consumeMemoFee(...)`: record dirties
  - [x] `consumeMultiSignFee(...)`: record dirties
- [x] Update: `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java`
  - [x] recordAccountDirty(owner/issuer), recordDynamicKeyDirty(public net usage/time, total tx cost)
  - [x] recordAssetIssueDirty(V1/V2)
- [x] Update: `chainbase/src/main/java/org/tron/core/db/EnergyProcessor.java`
  - [x] recordAccountDirty, recordDynamicKeyDirty(block energy usage)
- [ ] Update: `actuator/src/main/java/org/tron/core/vm/VMActuator.java`
  - [ ] recordAccountDirty for creator/caller in V2 window pre‑merge writes
- [ ] Update: native resource processors (delegate/undelegate)
  - [ ] recordAccountDirty(owner/receiver)

---

## 11) Ordering/Key Reference (for DynamicPropertiesStore)

When marking dynamic keys, use the same byte[] keys used in `DynamicPropertiesStore`:

- Public net: `publicNetUsage`, `publicNetTime`
- Costs/pools: `TOTAL_TRANSACTION_COST`, `TOTAL_CREATE_ACCOUNT_COST`, `TRANSACTION_FEE_POOL`, `BURN_TRX_AMOUNT`
- Energy block usage: `BLOCK_ENERGY_USAGE`

(Look up the exact private constants inside `DynamicPropertiesStore` and reference them via a small helper if needed to avoid package‑private access issues.)

---

## 12) Rollout Plan

- [ ] Ship behind flag (enabled in REMOTE)
- [ ] Enable debug + confirm in staging, validate consistency
- [ ] Monitor metrics, then disable confirm and reduce log level

---

## 13) Open Questions / Future Enhancements

- [ ] SPI transaction IDs: adopt `transaction_id` on storage RPCs when the Java side plumbs it, to guarantee atomicity across DBs on the backend.
- [ ] Broaden sync to other non‑VM mutations if discovered (market, proposals mid‑block), gated behind same mechanism.

---

## Appendix: Reference Hotspots Mapped (for implementers)

- Manager
  - `processTransaction(...)` (pre‑exec resource consumption precedes journaling)
  - `consumeMemoFee(...)`, `consumeMultiSignFee(...)`
- Bandwidth
  - `chainbase/.../BandwidthProcessor.consume(...)` paths: `useAccountNet`, `useFreeNet`, `useTransactionFee`, `consumeForCreateNewAccount`, TRC‑10 asset public free usage
- Energy
  - `chainbase/.../EnergyProcessor.useEnergy(...)`
  - VM v2 windows: `actuator/.../VMActuator.getAccountEnergyLimitWithFixRatio(...)`, `getTotalEnergyLimitWithFixRatio(...)` (the V2 paths that update usage and call `rootRepository.updateAccount`) 
- Delegation
  - `actuator/.../DelegateResourceProcessor`, `UnDelegateResourceProcessor`

---

End of TODO.

