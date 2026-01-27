# RuntimeSpiImpl.postExecMirror — Use `batchGet()` for Phase B Mirror (TODO)

Status: in-progress
Owners: `framework` runtime/VM (`RuntimeSpiImpl`), storage SPI (`StorageSPI` / `RemoteStorageSPI`), rust-backend storage (optional follow-up)
Target: improve remote-remote throughput while preserving Phase B correctness

## Problem Statement
In Phase B (`write_mode=PERSISTED`), Java skips applying state changes and instead runs **B-镜像 (B-mirror)** via `RuntimeSpiImpl.postExecMirror(...)` to refresh Java's local revoking head from the remote "root" state.

Today, `postExecMirror(...)` does a **blocking per-key `storageSPI.get(db, key)`** for every non-delete touched key. This results in ~2–3 gRPC reads per tx in the common case, which is a large fraction of remote-remote runtime cost.

Goal: change mirror reads from **O(#touched keys)** RPCs to **O(#touched dbs)** RPCs by using `StorageSPI.batchGet(db, keys)` (with chunking + correctness safeguards).

## Key References
- Mirror site: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` (`postExecMirror`)
- Touched keys source: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java` (`touched_keys`)
- Storage SPI: `framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java`
- Remote implementation: `framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java`
- Proto semantics: `framework/src/main/proto/backend.proto` (`BatchGetResponse.success`, `KeyValue.found`)
- Rust engine: `rust-backend/crates/storage/src/engine.rs` (`batch_get`)

## Constraints / Correctness Traps (must address)
### 1) `Map<byte[], byte[]>` key identity hazard
`StorageSPI.batchGet()` returns `Map<byte[], byte[]>`.

In Java, `byte[]` keys in `HashMap` compare by **identity**, not content.
- `EmbeddedStorageSPI.batchGet(...)` uses the input key objects as map keys → `map.get(originalKey)` works.
- `RemoteStorageSPI.batchGet(...)` currently uses `kv.getKey().toByteArray()` → **new arrays**, so `map.get(originalKey)` does **not** work.

If mirror assumes identity lookups, it will incorrectly treat "found" values as missing → deleting local keys (catastrophic).

### 2) `BatchGetResponse.success=false` must be treated as error
`backend.proto` defines `BatchGetResponse { bool success; string error_message; repeated KeyValue pairs; }`.

If `success=false`, the response may contain empty pairs; treating that as "all missing" would delete keys locally.
`RemoteStorageSPI.batchGet(...)` must check `success` and fail fast (throw) so mirror can fallback safely.

## Implementation Strategy Overview
1) **Per-db batching**: group touched keys by db; delete ops apply locally; non-delete keys fetched in a single `batchGet`.
2) **Dedup / last-write-wins**: touched key list may include duplicates; resolve final op before fetching.
3) **Chunking**: split into bounded batches to avoid oversized gRPC messages (even if typical sizes are small).
4) **Safe map lookup**:
   - Preferred: fix `RemoteStorageSPI.batchGet()` to return a map keyed by the **original input key objects**, aligning with `EmbeddedStorageSPI`.
   - Fallback: convert response to `Map<ByteArrayWrapper, byte[]>` and lookup by content if needed.
5) **Fallback on failure**: if batchGet fails (RPC error or `success=false`), fallback to per-key `get` for that batch (correctness > performance).
6) **Feature-gate**: add kill-switch JVM properties to revert to old behavior quickly if needed.

---

## TODOs — Phase 0: Baseline & Observability
- [ ] Capture baseline mirror cost:
  - [ ] Count touched keys distribution from logs (keys/tx, dbs/tx) on a representative run.
  - [ ] Record remote-remote throughput over fixed height (recommended) or fixed time: blocks/sec, tx rows/sec.
- [ ] Add lightweight mirror metrics (plan only; implement later):
  - [ ] Counters: `mirror.touched_keys`, `mirror.deletes`, `mirror.batch_get_calls`, `mirror.fallback_get_calls`, `mirror.errors`.
  - [ ] Timers: `mirror.total_ms`, `mirror.batch_get_ms`, `mirror.apply_ms`.
  - [ ] Decide output: periodic log line vs Prometheus (repo already has InstrumentedAppender).

Acceptance for Phase 0
- [ ] Have a baseline "before" number for blocks/sec and tx rows/sec on the same stopping height.

---

## TODOs — Phase 1: Fix `RemoteStorageSPI.batchGet` Semantics (required)
File: `framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java`

### 1.1 Respect `BatchGetResponse.success`
- [x] After `blockingStub.batchGet(...)`, if `!response.getSuccess()`:
  - [x] Throw a `RuntimeException` including `dbName` and `error_message`.
  - [x] Do **not** return an empty map on error.

### 1.2 Preserve input-key identity in returned map (preferred)
Goal: make `batchGet(db, keys)` return a `Map<byte[], byte[]>` where:
- Keys are the **same `byte[]` objects** from the `keys` argument.
- Values are `byte[]` or `null` (not found).

Plan:
- [x] Build a `Map<ByteArrayWrapper, Integer>` from request keys to index (content-based), to locate the matching request object.
- [x] For each `KeyValue kv`:
  - [x] Locate the corresponding request key index by content (`ByteArrayWrapper`).
  - [x] `result.put(requestKeys.get(index), kv.found ? kv.value : null)`
- [x] Ensure every request key is present in the result map (fill missing as null) to match embedded behavior.
- [x] If response includes unknown keys or mismatched sizes:
  - [x] Log WARN and still produce safe results (fill unknowns ignored; missing keys -> null).

### 1.3 (Alternative) Mirror-side wrapper map only
If we decide not to modify `RemoteStorageSPI`, mirror must not use `map.get(originalKey)`.
This is acceptable but slower and spreads key-wrapping logic into the hot path.

Decision TODO
- [x] Choose approach:
  - [x] A: Fix `RemoteStorageSPI.batchGet` identity + success (recommended). ✅ IMPLEMENTED
  - [ ] B: Leave SPI as-is and implement wrapper-based lookup in mirror.

Acceptance for Phase 1
- [x] `batchGet` never silently treats backend failure as "missing keys".
- [ ] A unit/regression test proves lookups work in remote mode.

---

## TODOs — Phase 2: Refactor `postExecMirror` to `batchGet`
File: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`

### 2.1 Add feature flags / knobs (kill switches)
- [x] Add JVM property to enable batchGet mirror:
  - [x] `remote.exec.postexec.mirror.batchGet` (default `true` once stable; start with `false` if we want cautious rollout)
- [x] Add JVM property for chunk size:
  - [x] `remote.exec.postexec.mirror.batchGet.maxKeys` (default `256`)
- [x] Add JVM property to control fallback behavior:
  - [x] `remote.exec.postexec.mirror.batchGet.fallbackToSingleGet` (default `true`)

### 2.2 Normalize and dedupe touched keys (last-write-wins)
Rationale: a tx may touch the same `(db,key)` multiple times; we must apply the final op.

Implementation plan:
- [x] Convert `List<TouchedKey>` into `Map<String, LinkedHashMap<ByteArrayWrapper, Boolean /*isDelete*/>>`
  - [x] Use insertion order (LinkedHashMap) to preserve "last occurrence wins" deterministically.
  - [x] Store the latest `byte[]` instance alongside the wrapper so we can apply with consistent bytes.

### 2.3 Apply deletes locally without remote reads
- [x] For each db group:
  - [x] Resolve `TronStoreWithRevoking<?> store = getStoreByDbName(dbName, chainBaseManager)`
  - [x] For keys where `isDelete=true`, call `store.delete(keyBytes)`

### 2.4 Batch fetch non-delete keys by db (with chunking)
- [x] For each db group:
  - [x] Collect `readKeys` (non-delete).
  - [x] Chunk `readKeys` into `N` chunks of size <= `maxKeys`.
  - [x] For each chunk:
    - [x] `Map<byte[], byte[]> values = storageSPI.batchGet(dbName, chunkKeys).get()`
    - [x] If `batchGet` throws and fallback is enabled:
      - [x] For each key in chunk, call `storageSPI.get(dbName, key).get()`
    - [x] Apply results:
      - [x] if value != null → `store.putRawBytes(key, value)`
      - [x] if value == null → `store.delete(key)` (treat missing as delete, matching current semantics)

### 2.5 Safe lookup of batch results (depends on Phase 1 decision)
- [x] If Phase 1.2 (identity-preserving map) is implemented:
  - [x] Use `values.get(keyBytes)` directly.
- [ ] Else:
  - [ ] Convert `values.entrySet()` into `Map<ByteArrayWrapper, byte[]>` and lookup by wrapper.

### 2.6 Logging cleanup (avoid INFO per tx in hot path)
Today `postExecMirror` logs per tx at INFO.
For throughput runs, this produces huge logs and can throttle.

Plan:
- [x] Demote per-tx mirror logs to DEBUG, or throttle:
  - [x] `Phase B mirror: Refreshing ...` at DEBUG
  - [x] `Phase B mirror: Completed ...` at DEBUG
- [x] Keep WARN/ERROR for real issues (unknown db, batchGet failure, fallback activation).

### 2.7 Keep behavior parity
Must preserve existing semantics:
- [x] `isDelete=true` always deletes locally (no remote read)
- [x] non-delete but remote missing → delete locally
- [x] unknown dbName → skip with error accounting, no crash
- [x] do not introduce concurrency that risks revokingDB thread-safety (apply locally on current thread)

Acceptance for Phase 2
- [x] Mirror performs <= 1 `batchGet` per db per tx (plus chunking only when needed).
- [x] On any batchGet failure, mirror remains correct via fallback.

---

## TODOs — Phase 3: Tests (Java)

### 3.1 Unit tests for mirror logic (recommended via extractable helper)
Problem: `postExecMirror` is private and depends on `TransactionContext` + stores.

Plan:
- [ ] Extract core mirror logic into a small helper class (package-private) so it's testable:
  - [ ] Inputs: `StorageSPI storageSPI`, `ChainBaseManager cbm`, `List<TouchedKey> touchedKeys`
  - [ ] Output: stats object (counts, errors) for assertions (optional)
- [x] Tests (in `RemoteStorageBatchGetTest.java`):
  - [x] Dedup: same key touched twice → only final op applied (`testNormalizeTouchedKeys_lastWriteWins`)
  - [ ] Delete-only: no storage reads occur (requires mock StorageSPI)
  - [ ] Missing value: remote returns null → local delete (requires mock StorageSPI)
  - [ ] Unknown db: logs warning, counts errors, continues (requires mock stores)
  - [ ] Batch failure: fallback to single get, still applies correct values (requires mock StorageSPI)
- [x] Additional tests:
  - [x] Key identity: `testBatchGetKeyIdentity_canLookupByOriginalKey`
  - [x] ByteArrayWrapper equality: `testByteArrayWrapperEquality`
  - [x] Chunking: `testChunking_splitsByMaxKeys`
  - [x] Multiple keys/dbs: `testNormalizeTouchedKeys_multipleKeysMultipleDbs`

### 3.2 RemoteStorageSPI regression tests
Files: `framework/src/test/java/...`

Plan:
- [ ] Add a focused test for `RemoteStorageSPI.batchGet` behavior:
  - [ ] When backend returns `success=false`, ensure `batchGet` throws (requires running Rust backend)
  - [x] Identity behavior: ensure returned map keys can be looked up using the same `byte[]` instances passed in (logic tested in `RemoteStorageBatchGetTest`)

Acceptance for Phase 3
- [x] Tests cover the key identity failure mode (in `RemoteStorageBatchGetTest.java`)
- [ ] Tests cover the success=false failure mode (requires integration test with backend)

---

## TODOs — Phase 4: Perf Validation & Rollout

### 4.1 Perf validation protocol
- [ ] Use fixed block height stopping for apples-to-apples:
  - [ ] Set `node.shutdown.BlockHeight` and run with `SLEEP_DURATION=0`
- [ ] Compare:
  - [ ] blocks/sec (or final height reached in fixed time)
  - [ ] tx CSV rows/sec
  - [ ] number of mirror gRPC calls (instrumentation or log-derived)

### 4.2 Rollout plan
- [x] Land Phase 1 + Phase 2 behind `remote.exec.postexec.mirror.batchGet` flag.
- [ ] Enable by default only after:
  - [ ] No CSV mismatches on long common prefix
  - [ ] No mirror-related WARN spikes
  - [ ] Measurable throughput improvement

Acceptance for Phase 4
- [ ] Remote-remote reaches materially higher height (or more tx rows) within the same time budget without mismatches.

---

## Optional Follow-Up: Rust `batch_get` optimization (multi_get)
Files:
- `rust-backend/crates/storage/src/engine.rs`

Plan:
- [ ] Replace per-key `db.get(key)` loop with RocksDB `multi_get` (or iterator-based equivalent) if available.
- [ ] Preserve stable response semantics:
  - [ ] 1 response entry per request key, in request order
  - [ ] found flag accurate; value empty only when found=false

Acceptance
- [ ] Measurable reduction in backend CPU time per batchGet under load.

