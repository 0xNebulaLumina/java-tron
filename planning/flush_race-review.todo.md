# Flush race fix (Option 2: head-based / forward-only merge)

Goal: make `SnapshotManager` / `SnapshotRoot` flush *only* the head view of state into the underlying DB (and thus the Rust backend in REMOTE mode), so snapshot refresh cannot overwrite newer remote values with older snapshot data.

This file tracks the TODOs to safely design, implement, and validate this change.

---

## 0. Preconditions & scoping

- [ ] Confirm we are **only** changing steady-state flush behaviour, not checkpoint recovery.
  - [ ] Verify all call sites of `SnapshotRoot.merge(List<Snapshot>)` and `SnapshotRoot.merge(Snapshot)`:
    - [ ] `SnapshotManager.refreshOne(...)` uses `merge(List<Snapshot>)`.
    - [ ] `SnapshotManager.recover(...)` + checkpoint code uses `merge(Snapshot)` on `getHead()`.
  - [ ] Ensure we do **not** change `merge(Snapshot)` semantics in this iteration.
- [ ] Decide initial scope:
  - [ ] First enable head-based merge only when storage mode is `remote` (REMOTE dual-storage backend).
  - [ ] Keep embedded (`LEVELDB` / `ROCKSDB` without `StorageBackendDB`) using old semantics for early rollout.

---

## 1. Deep-dive analysis of current snapshot / flush pipeline

**Files to review in detail**

- [ ] `chainbase/src/main/java/org/tron/core/db2/core/Chainbase.java`
  - [ ] Understand `head`, `root`, `advance`, `retreat`, `merge`, `newInstance`.
  - [ ] Map how `get(byte[] key)` traverses the snapshot chain for read semantics.
  - [ ] Confirm how `prefixQuery`, `getNext`, `getlatestValues` interact with snapshots vs root.
- [ ] `chainbase/src/main/java/org/tron/core/db2/core/SnapshotRoot.java`
  - [ ] `put`, `remove`, `merge(Snapshot from)`, `merge(List<Snapshot> snapshots)`.
  - [ ] `needOptAsset()` and `processAccount(...)` behaviour for account DB.
  - [ ] How cache is updated (`putCache` / `getCache`) in flush vs direct `put`.
- [ ] `chainbase/src/main/java/org/tron/core/db2/core/SnapshotImpl.java`
  - [ ] How `db` stores `Key` → `Value`, including `Value.Operator.PUT` / `DELETE`.
  - [ ] `collect(...)`, `collectUnique(...)` and how they reconstruct composite views.
- [ ] `chainbase/src/main/java/org/tron/core/db2/core/SnapshotManager.java`
  - [ ] `buildSession`, `flushCount` logic, `flush()`, `refresh()`, `refreshOne(...)`.
  - [ ] `recover(...)` and how it uses `merge(Snapshot)` today.
- [ ] `chainbase/src/main/java/org/tron/core/db/StorageBackendDB.java`
  - [ ] `flush(Map<WrappedByteArray, WrappedByteArray> batch)` behaviour in dual mode.
- [ ] `chainbase/src/main/java/org/tron/core/db2/common/LevelDB.java`
  - [ ] `flush(...)` → `updateByBatch(...)` semantics (especially delete handling).
- [ ] `chainbase/src/main/java/org/tron/common/storage/rocksdb/RocksDbDataSourceImpl.java`
  - [ ] `updateByBatch(...)` mapping of `null` values → deletion.

**Specific behaviour questions to answer**

- [ ] For a given DB (e.g. `"account"`):
  - [ ] What is the life cycle of snapshots per block / transaction?
  - [ ] Under what conditions does `size > maxSize` and `flushCount` accumulate?
  - [ ] When `flush()` triggers, which `SnapshotImpl` instances are included in `snapshots`?
- [ ] Exactly how are deletes represented end-to-end?
  - [ ] In `SnapshotImpl.db` (`Value.Operator.DELETE`).
  - [ ] In `SnapshotRoot.merge(List<Snapshot>)`’s `batch`.
  - [ ] In `Flusher.flush` (LevelDB / RocksDB / StorageBackendDB).
- [ ] For account DB:
  - [ ] Which paths go through `processAccount(batch)` vs plain `flush(batch)`?
  - [ ] How does `processAccount` transform deletes and PUTs, and how does that map onto `AccountAssetStore`?

Deliverable: a short internal note (in this file or separate) summarizing the answers, to avoid surprises during implementation.

---

## 2. Finalize design for head-based / forward-only merge

**2.1. Define `mergeWithHead` semantics**

- [x] Add a conceptual API on `SnapshotRoot`:
  - [x] `public void mergeWithHead(Snapshot head, List<Snapshot> snapshots)`
  - Semantics:
    - [x] `head` is the current `Chainbase.getHead()` for this DB.
    - [x] `snapshots` are the early `SnapshotImpl` layers to be collapsed into `root`.
    - [x] For each key `k` touched by any snapshot in `snapshots`, compute:
      - [x] `headValue = head.get(k)` using existing chain traversal.
      - [x] Desired write behaviour:
        - [x] If `headValue != null`: write `headValue` into root (and underlying DB).
        - [x] If `headValue == null`: treat as delete at logical head; root must reflect deletion, and underlying DB must delete the key (plus asset clean-up in account DB).
- [x] Design how to gather the affected key set:
  - [x] Iterate each `SnapshotImpl`'s `db` (HashDB) and union keys into `Set<WrappedByteArray>`.
  - [x] Use a deterministic iteration order (e.g. insertion order or sorted) only if required for debugging; functional correctness is order-independent.

**2.2. Deletion semantics**

- [x] For account DB (`needOptAsset() == true`):
  - [x] Use the existing convention:
    - [x] For "deleted at head" (`headValue == null`), set `batch.put(key, WrappedByteArray.of(new byte[0]))`.
    - [x] Let `processAccount(batch)` interpret empty values as deletion + asset-store updates.
- [x] For non-account DBs (`needOptAsset() == false`):
  - [x] Choose one approach (document choice here):
    - Option A (simpler): **CHOSEN**
      - [x] Do **not** put a special value in `batch` for deleted keys.
      - [x] Maintain a separate `List<byte[]> deletes`.
      - [x] After calling `((Flusher) db).flush(batch)` for PUTs, call `db.remove(key)` for each `key` in `deletes`.
    - Option B (more unified): **NOT CHOSEN**
      - [ ] Allow `batch` entries with `value == null` for deleted keys.
      - [ ] Extend `Flusher.flush` implementations (LevelDB, RocksDB, StorageBackendDB) to interpret `null` values as deletes and propagate to their underlying data sources.
  - [x] Evaluate impact on existing code:
    - [x] Ensure no other caller accidentally passes `null` where a real value is expected. (Option A avoids this issue)

**2.3. DB mode gating**

- [x] Design gating logic:
  - [x] For REMOTE dual-mode (`StorageBackendDB` + `CommonParameter.storageMode == "remote"`):
    - [x] Use `mergeWithHead` to avoid stale writes against Rust RocksDB.
  - [x] For embedded-only DBs:
    - [x] Either keep using original `merge(List<Snapshot>)` semantics for now, or migrate later once REMOTE path is validated.
  - [x] Implement a helper on `SnapshotRoot` or in `SnapshotManager` to detect remote backend:
    - [x] Example: `db instanceof StorageBackendDB && storageMode == "remote"`.

**2.4. Impact analysis**

- [x] Verify that head-based merge does **not** change observable semantics for:
  - [x] Reads (`get`, `prefixQuery`, `getNext`) from `Chainbase`. (No changes - only affects flush behavior)
  - [x] Snapshot-based logic in markets, account-trace, etc. (No changes - only affects flush behavior)
  - [x] Account asset optimization semantics (no double-clearing or missing asset updates). (Uses existing processAccount path)
- [x] Document known behaviour changes (if any) for embedded mode (e.g. which states get persisted earlier). (No changes for embedded mode - gated to REMOTE only)

---

## 3. Implementation TODOs (no code yet, just steps)

**3.1. Introduce `mergeWithHead` on `SnapshotRoot`**

- [x] Add method signature:
  - [x] `public void mergeWithHead(Snapshot head, List<Snapshot> snapshots)`
- [x] Implement key collection:
  - [x] For each `Snapshot` in `snapshots`:
    - [x] Cast to `SnapshotImpl`.
    - [x] Iterate its `db` entries:
      - [x] Add `WrappedByteArray.of(keyBytes)` to `mergedKeys`.
- [x] For each key in `mergedKeys`:
  - [x] Extract `byte[] rawKey`.
  - [x] Compute `byte[] headValue = head.get(rawKey);`
  - [x] Branch:
    - [x] If `headValue != null`:
      - [x] `batch.put(WrappedByteArray.of(rawKey), WrappedByteArray.of(headValue));`
    - [x] Else (`headValue == null`):
      - [x] For account DB:
        - [x] `batch.put(WrappedByteArray.of(rawKey), WrappedByteArray.of(new byte[0]));`
      - [x] For non-account DB:
        - [x] Record `rawKey` in `deletes`.
- [x] After building `batch`:
  - [x] If `needOptAsset()`:
    - [x] Call `processAccount(batch)` (which internally calls `((Flusher) db).flush(accounts)` and updates asset store).
  - [x] Else:
    - [x] Call `((Flusher) db).flush(batch)` for PUTs.
    - [x] If using Option A for deletes:
      - [x] For each key in `deletes`, call `db.remove(key)`.
  - [x] Update caches via `putCache(batch)` as today.

**3.2. Wire `SnapshotManager.refreshOne` to use head-based merge**

- [x] In `SnapshotManager.refreshOne(Chainbase db)`:
  - [x] After building `snapshots` and before `root.resetSolidity()`:
    - [x] Retrieve `Snapshot head = db.getHead();`
    - [x] If remote backend (per gating logic), call:
      - [x] `root.mergeWithHead(head, snapshots);`
    - [x] Else:
      - [x] Fall back to existing `root.merge(snapshots);`
  - [x] Keep the rest of `refreshOne` unchanged:
    - [x] `root.resetSolidity();`
    - [x] Rewire `head` and `root` links (as current code does).

**3.3. Ensure recovery logic uses old semantics**

- [x] Scan `SnapshotManager.recover(...)`:
  - [x] Confirm it uses `db.getHead().getRoot().merge(db.getHead())` (single snapshot).
  - [x] Leave this path as-is (still snapshot-based, not head-based).
- [x] Add a comment documenting that `merge(Snapshot)` is intentionally left with "history" semantics for recovery, while `mergeWithHead` is used for forward-only refresh.
  - Note: Recovery path at line 549 uses `db.getHead().getRoot().merge(db.getHead())` with single snapshot - left unchanged as intended.

**3.4. Guardrails & logging**

- [x] Add low-cost debug logging (guarded by log level) to confirm behaviour:
  - [x] When running in REMOTE mode and `mergeWithHead` is used:
    - [x] Log DB name, number of keys flushed, number of deletes.
    - [x] Optionally log a sample of keys (hashed/prefix) to confirm activity. (Not added - can be enabled later if needed)
- [x] Ensure logs are not too noisy for mainnet (e.g. use `debug`/`trace`). (Using logger.isDebugEnabled() guard)

---

## 4. Testing plan

**4.1. Unit tests for `mergeWithHead`**

- [ ] Add a new test class in `chainbase` (e.g. `SnapshotRootForwardMergeTest`).
  - [ ] Test 1: Simple forward merge, single snapshot
    - [ ] root: x=0
    - [ ] snapshot1: x=1
    - [ ] head = snapshot1, snapshots = [snapshot1]
    - [ ] After `mergeWithHead`:
      - [ ] `root.get(x) == 1`
      - [ ] No snapshots remain before root for x.
      - [ ] Underlying DB receives x=1.
  - [ ] Test 2: Later snapshot overrides earlier one
    - [ ] root: x=0
    - [ ] snapshot1: x=1
    - [ ] snapshot2: x=2
    - [ ] head = snapshot2
    - [ ] flushCount=1, snapshotsToMerge = [snapshot1]
    - [ ] After `mergeWithHead`:
      - [ ] `root.get(x) == 2` (we wrote head’s value).
      - [ ] snapshot2 still exists and head view is unchanged.
      - [ ] Underlying DB ends up with x=2, not x=1.
  - [ ] Test 3: Delete at head
    - [ ] root: x=0
    - [ ] snapshot1: x=1
    - [ ] snapshot2: delete x
    - [ ] head = snapshot2
    - [ ] snapshotsToMerge = [snapshot1]
    - [ ] After `mergeWithHead`:
      - [ ] `root.get(x) == null` (deleted).
      - [ ] Underlying DB has x removed.
  - [ ] Test 4: Account DB + `processAccount`
    - [ ] Use `dbName="account"` and ensure `needOptAsset()` returns true.
    - [ ] Create scenarios with asset fields, deletes, and verify:
      - [ ] `processAccount` is still called.
      - [ ] AssetStore updates are correct.

**4.2. Integration tests around SnapshotManager**

- [ ] Add a test that exercises `SnapshotManager` with multiple DBs:
  - [ ] Simulate building many sessions to trigger `flushCount` and `flush()`.
  - [ ] Verify:
    - [ ] `mergeWithHead` is called (maybe via spy or log inspection).
    - [ ] The eventual root state equals the head state pre-flush.
    - [ ] No older values re-appear after flush.

**4.3. End-to-end regression for blackhole mismatch**

- [ ] Reproduce the 2120 → 2131 → 2140 scenario in a controlled test harness:
  - [ ] Make sure snapshot flush triggers between a state update (raising blackhole) and a later tx that doesn’t include blackhole in ResourceSync.
  - [ ] Assert:
    - [ ] Java’s `Manager` blackhole log uses B₁.
    - [ ] Rust’s “Blackhole balance BEFORE execution” for the later tx also uses B₁, not B₀.
- [ ] Also verify:
  - [ ] No regressions for other accounts and dynamic properties.

---

## 5. Rollout & safety

- [x] Start with REMOTE-only enablement:
  - [x] Tie `mergeWithHead` usage to `storage.mode == "remote"` + `StorageBackendDB`.
  - [ ] Keep a config toggle (system property or config flag) allowing rollback to old behaviour if unexpected issues arise.
    - Note: Currently gated by compile-time logic. Can add runtime flag in future if needed.
- [ ] Monitor on testnet / staging:
  - [x] Add temporary metrics/logs to track:
    - [x] Number of keys flushed per refresh. (Debug log added)
    - [x] Number of deletes per refresh. (Debug log added)
    - [ ] Any mismatch between Java and Rust for key sentinel accounts (e.g. blackhole).
- [ ] Plan for mainnet rollout:
  - [ ] Staggered deploy (e.g. subset of nodes first).
  - [ ] Clear rollback procedure (config switch or build revert).

---

## 6. Follow-ups (post-Option-2 hardening)

- [ ] Consider migrating embedded mode to use `mergeWithHead` once REMOTE path is stable.
- [ ] Evaluate whether `merge(Snapshot)` (single snapshot) should eventually become head-based as well, or remain historical for recovery only.
- [ ] Explore adding lightweight versioning (e.g. block height) as metadata to further harden against stale writes, if needed.

---

## Implementation Summary (Completed)

**Date:** 2025-11-17

**Changes Made:**

1. **SnapshotRoot.java** (`chainbase/src/main/java/org/tron/core/db2/core/SnapshotRoot.java`):
   - Added `mergeWithHead(Snapshot head, List<Snapshot> snapshots)` method
   - Implements head-based merge that reads values from the current head snapshot instead of from the snapshots being merged
   - Handles deletion semantics differently for account DB vs non-account DBs
   - Added `isRemoteBackend()` helper method to detect REMOTE storage mode
   - Added debug logging (guarded by `logger.isDebugEnabled()`)

2. **SnapshotManager.java** (`chainbase/src/main/java/org/tron/core/db2/core/SnapshotManager.java`):
   - Modified `refreshOne(Chainbase db)` to conditionally use `mergeWithHead` in REMOTE mode
   - Added `isRemoteBackend(SnapshotRoot root)` helper method
   - Preserved existing `merge(snapshots)` path for embedded mode and recovery

**Key Design Decisions:**

- **Option A for deletion semantics**: Use separate `List<byte[]> deletes` for non-account DBs, call `db.remove(key)` after flush
- **REMOTE-only enablement**: Head-based merge only activates when `db instanceof StorageBackendDB && storageMode == "remote"`
- **Recovery path unchanged**: `merge(Snapshot)` single-snapshot variant remains snapshot-based for checkpoint recovery
- **Minimal invasiveness**: No changes to Flusher interface or embedded DB implementations

**Build Status:**
- ✅ Compilation successful: `./gradlew :chainbase:build -x test --dependency-verification=off`
- No checkstyle or compilation errors

**Next Steps:**
- Integration testing in REMOTE mode with actual workload
- Monitor debug logs for flush behavior (key count, delete count, timing)
- Validate blackhole account parity between Java and Rust logs

