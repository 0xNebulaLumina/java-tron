# Remote AccountCapsule Resource Usage Serialization – Plan & TODOs

This document defines the design and a step‑by‑step TODO checklist to ensure remote storage/execution includes AccountCapsule resource usage fields (beyond balance/nonce/codeHash/code) in the account change serialization/deserialization flow.

Goal: when executing with the remote backend, Java receives account changes whose serialized form carries resource usage and related timing/window metadata and applies them to `AccountCapsule`. Embedded journaling should also emit the same format to allow parity testing.


## Scope
- Add resource usage fields to the account change serialization format consumed/produced by Java when syncing state from remote execution and from embedded journaling.
- Keep backward compatibility with existing minimal format (balance/nonce/codeHash/code).
- Provide a pathway to enrich the Rust proto later without breaking current Java.

### Non‑Goals
- Changing how full `protocol.Account` objects are stored in Rocks (they already include resource fields).
- Implementing remote‑side computation of TRON resource windows/usage in Rust immediately (we’ll provide a hook and keep fields optional initially).


## Current State (as of repo)
- Java emits and consumes account changes in a minimal, custom format:
  - Base layout: `[balance(32)] + [nonce(8)] + [code_hash(32)] + [code_len(4)] + [code(bytes)]`.
  - Emitters:
    - Embedded journal: `framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournal.java:260` (serializeAccountInfo(AccountCapsule))
    - Remote bridge: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:476` (serializeAccountInfo(AccountInfo proto))
  - Consumer:
    - Remote apply: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:358` (deserializeAccountInfo(byte[])) → updates only balance (logs code/codeHash), does not touch resource fields.
- Proto: `framework/src/main/proto/backend.proto:590` `message AccountInfo` contains address/balance/nonce/code_hash/code only.
- AccountCapsule has resource usage API in `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java`:
  - Usage: `getNetUsage()`, `getFreeNetUsage()`, `getEnergyUsage()`
  - Timestamps: `getLatestConsumeTime()`, `getLatestConsumeFreeTime()`, `getAccountResource().getLatestConsumeTimeForEnergy()`
  - Window: `getWindowSize(V1)`, `getWindowSizeV2(...)`, `getWindowOptimized(...)` with BANDWIDTH/ENERGY variants.


## Design Overview
We extend the account change wire format with an optional, versioned tail that carries resource usage attributes. The base remains unchanged for backward compatibility.

- Base (unchanged):
  - balance[32] (big‑endian)
  - nonce[8] (big‑endian)
  - code_hash[32]
  - code_len[4] (big‑endian)
  - code[code_len]
- Optional Tail (AEXT v1): appended immediately after code bytes
  - magic: 4 bytes ASCII "AEXT"
  - version: u16 BE (1)
  - length: u16 BE (payload length in bytes)
  - payload v1 (big‑endian for integers):
    - netUsage: i64 (8)
    - freeNetUsage: i64 (8)
    - energyUsage: i64 (8)
    - latestConsumeTime: i64 (8)
    - latestConsumeFreeTime: i64 (8)
    - latestConsumeTimeForEnergy: i64 (8)
    - netWindowSize: i64 (8)         // logical units; see Notes
    - energyWindowSize: i64 (8)
    - netWindowOptimized: bool (1)   // 0x00 or 0x01
    - energyWindowOptimized: bool (1)
    - reserved/padding: u16 (2)      // pad to 4‑byte boundary; set 0

Notes:
- All integer fields are signed 64‑bit big‑endian to match Java long semantics.
- Window size values: prefer stable logical units consistent with getters used in code paths. If we use `getWindowSizeV2()` (precision‑scaled), document clearly; otherwise use raw/unoptimized values. See TODO decision below.
- If the tail is absent, consumers must treat it as no resource updates present.

This keeps older backends and logs valid; newer Java can parse/emit AEXT without impacting legacy components.


## Implementation Plan (Java)

### 1) Deserialization: parse AEXT and apply to AccountCapsule
File: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`

- [x] Update `deserializeAccountInfo(byte[] data)`:
  - After base parsing finishes, if `offset + 4 <= data.length` and `data[offset..offset+4] == 'A','E','X','T'`, then parse tail:
    - Read version (u16 BE); if != 1, skip with warning.
    - Read length (u16 BE); ensure `offset + length <= data.length`.
    - Parse payload fields in listed order; on any bounds error → log warn and ignore tail.
  - Return `AccountInfo` extended to carry parsed resource fields (introduce fields in nested class or a new record type).
- [x] Update `updateAccountState(...)` to set resource usage fields on `AccountCapsule` if present:
  - Usage: `setNetUsage`, `setFreeNetUsage`, `setEnergyUsage`
  - Times: `setLatestConsumeTime`, `setLatestConsumeFreeTime`, `setLatestConsumeTimeForEnergy`
  - Window: `setNewWindowSize(BANDWIDTH, netWindowSize)`, `setWindowOptimized(BANDWIDTH, netWindowOptimized)` and same for ENERGY
  - Ensure null‑safety: only apply when tail present.
- [x] Logging: add concise debug of tail detection and values; warn on malformed tails; do not throw.

Open Decision: which window size getter to mirror
- [x] Decide and document whether tail carries `getWindowSize()` (logical) or `getWindowSizeV2()` (precision‑scaled). For parity and simplicity, use `getWindowSize()` logical units; record decision in this doc and in code comments.
  - **Decision**: Use `getWindowSize(BANDWIDTH|ENERGY)` for logical units as documented in code comments.

### 2) Embedded journal: append AEXT in account change serialization
File: `framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournal.java`

- [x] Update `serializeAccountInfo(AccountCapsule account)`:
  - Keep current base buffer creation unchanged.
  - Compute AEXT payload from AccountCapsule via getters:
    - netUsage, freeNetUsage, energyUsage
    - latestConsumeTime, latestConsumeFreeTime, latestConsumeTimeForEnergy
    - netWindowSize = `getWindowSize(BANDWIDTH)`, energyWindowSize = `getWindowSize(ENERGY)`
    - netWindowOptimized = `getWindowOptimized(BANDWIDTH)`, energyWindowOptimized = `getWindowOptimized(ENERGY)`
  - Append magic/version/length (BE) and payload to the base buffer.
  - Add system property gate (default true): `-Dremote.exec.accountinfo.resources.enabled=true`. When false, do not append tail.
- [x] Add defensive try/catch with warn logs on serialization issues; return base buffer if tail build fails.

### 3) Remote response bridge: optionally append AEXT based on proto
File: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`

- [x] Update `serializeAccountInfo(tron.backend.BackendOuterClass.AccountInfo accountInfo)`:
  - Keep base serialization path intact.
  - Tail emission:
    - Short‑term: only append tail if the proto adds and populates resource fields (see Proto Changes). Otherwise omit; do not synthesize values.
    - Respect the same property `remote.exec.accountinfo.resources.enabled` as an additional gate.

### 4) Tests (Java)
- [ ] Add unit tests for round‑trip in embedded journal path:
  - Given an AccountCapsule with non‑default resource usage values, `serializeAccountInfo` then `deserializeAccountInfo` → verify all fields equal.
  - Cases: with tail enabled, with tail disabled, with truncated/malformed tail → resource fields ignored and no exception.
- [ ] Add integration test in remote mode scaffold (if possible) to ensure `RuntimeSpiImpl.applyStateChangesToLocalDatabase` applies tail fields when present.
- [ ] Ensure existing tests that only expect base format remain green.


## Proto Changes (Optional, Recommended)
File: `framework/src/main/proto/backend.proto`

Extend `message AccountInfo` to include optional resource usage fields. All new fields must be optional (proto3 presence via non‑zero defaults or wrapper types is acceptable). Proposed fields and tags:

- [x] Add (after existing 1..5):
  - `int64 net_usage = 6;`
  - `int64 free_net_usage = 7;`
  - `int64 energy_usage = 8;`
  - `int64 latest_consume_time = 9;`
  - `int64 latest_consume_free_time = 10;`
  - `int64 latest_consume_time_for_energy = 11;`
  - `int64 net_window_size = 12;`
  - `bool net_window_optimized = 13;`
  - `int64 energy_window_size = 14;`
  - `bool energy_window_optimized = 15;`

Build/regen:
- [x] Rebuild Java (gradle) and Rust (cargo); ensure tonic stubs are regenerated (Rust uses `rust-backend/crates/core/build.rs`).
- [x] Document field semantics in proto comments.


## Rust Backend Changes (Staged)
File: `rust-backend/crates/core/src/service.rs` (in `convert_execution_result_to_protobuf` mapping state changes)

Phase 1 (safe default):
- [ ] Leave the new `AccountInfo` resource fields unset until backend can source correct values; Java won’t append AEXT in remote bridge in this case.

Phase 2 (enrichment paths – optional):
- [ ] Decide a source of truth for the account resource usage fields when emitting account changes:
  - Option A: Read TRON `protocol.Account` bytes from storage for the address and decode resource usage fields (requires storage adapter support for account DB reads and TRON proto decode helpers). Populate proto fields from decoded structure.
  - Option B: Derive minimal fields from execution metadata for the current tx (e.g., usage deltas). Prefer absolute values to avoid ambiguity; if absolute not available, leave unset.
- [ ] Populate proto `AccountInfo` new fields accordingly so Java can append AEXT tail and apply them.
- [ ] Add Rust unit tests to verify `AccountInfo` population when data sources are available.


## Config & Rollout
- [ ] Add and document system property: `remote.exec.accountinfo.resources.enabled` (default true in REMOTE mode; optional true in EMBEDDED for journaling parity tests). When false, Java won’t append AEXT tail.
- [ ] No changes required to `StorageSPI` or DBs.
- [ ] Backward compatible: consumers ignore tail if absent.


## Observability & Metrics
- [ ] Add debug logs on tail detection and application in `RuntimeSpiImpl.updateAccountState` with address and which fields were applied.
- [ ] Add counters (optional) for number of account changes with AEXT vs without.


## Validation & Acceptance Criteria
- [ ] Unit tests: Java round‑trip pass with and without tail; malformed tail safely ignored.
- [ ] Remote mode: when tail present, `AccountCapsule` reflects expected resource usage fields after `applyStateChangesToLocalDatabase`.
- [ ] No regressions in existing tests; minimal format remains accepted.
- [ ] Documentation: this file plus inline code comments documenting the AEXT layout and window size decision.


## Detailed TODO Checklist (by file)

1) `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- [ ] Extend inner `AccountInfo` to carry optional resource fields or create a new holder type.
- [ ] Enhance `deserializeAccountInfo(byte[])` to parse AEXT tail (magic/version/length/payload), set fields accordingly.
- [ ] In `updateAccountState(...)`, after balance handling, apply resource fields when present.
- [ ] Add debug/warn logs for tail detection, malformed data, and applied values.

2) `framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournal.java`
- [ ] In `serializeAccountInfo(AccountCapsule)`, after base serialization, build and append AEXT payload when property enabled.
- [ ] Use `getWindowSize(BANDWIDTH|ENERGY)` and `getWindowOptimized(...)` (document decision) for window fields.
- [ ] Add try/catch with warn; on failure, fall back to base only.

3) `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- [ ] Update `serializeAccountInfo(AccountInfo proto)` to optionally read new proto fields and append AEXT when property enabled and values present.
- [ ] Keep base format unchanged; do not synthesize values.
- [ ] Add debug log summarizing appended tail sizes.

4) `framework/src/main/proto/backend.proto`
- [ ] Add fields 6..15 to `AccountInfo` with doc comments.
- [ ] Regenerate stubs (Java + Rust). Ensure `rust-backend/crates/core/build.rs` still points to `framework/src/main/proto`.

5) Rust (staged)
- [ ] Leave fields unset initially.
- [ ] Plan sources and implement population when available (decode TRON account from storage or derive absolute metrics).
- [ ] Add tests once fields can be populated.

6) Tests
- [ ] Java unit tests for serialize/deserialize round‑trip (with/without tail, malformed tail).
- [ ] Java integration test validating `RuntimeSpiImpl.applyStateChangesToLocalDatabase` updates an existing `AccountCapsule` resource usage from state change bytes containing AEXT.

7) Docs
- [ ] Record window size decision (logical vs V2 precision) here and in Java comments.
- [ ] Brief spec of AEXT binary layout in this file (Appendix) and pointer in code headers.


## Risks & Mitigations
- Proto drift: AEXT tail is optional and versioned; older consumers ignore it; adding proto fields is additive.
- Incorrect absolute vs delta semantics: AEXT uses absolute values; Java applies as authoritative snapshots to avoid ambiguity.
- Rust not populating fields: Tail is omitted; Java resource processors continue to maintain usage; behavior unchanged.


## Appendix A — AEXT v1 Binary Layout
- magic: 0x41 0x45 0x58 0x54 ("AEXT")
- version: 0x00 0x01
- length: 0x00 0x52 (example; exact depends on final payload bytes)
- payload order and sizes (big‑endian):
  1. netUsage (8)
  2. freeNetUsage (8)
  3. energyUsage (8)
  4. latestConsumeTime (8)
  5. latestConsumeFreeTime (8)
  6. latestConsumeTimeForEnergy (8)
  7. netWindowSize (8)
  8. energyWindowSize (8)
  9. netWindowOptimized (1)
  10. energyWindowOptimized (1)
  11. reserved/padding (2)

Total payload bytes: 8*8 + 2 + 2 = 68 + 4 = 72 bytes (length=72). Header adds 8 bytes → tail total 80 bytes.


## Appendix B — Compatibility Matrix
- Java (old) + Rust (old): base format only → still works; no tail.
- Java (new) + Rust (old): remote bridge omits tail (no proto fields) → works; embedded journaling can emit tail guarded by property.
- Java (old) + Rust (new): Rust sends proto with resource fields; Java old ignores fields; remote bridge base remains unchanged → works.
- Java (new) + Rust (new): full behavior; AEXT appended in remote path; Java applies resource usage.


## Timeline (suggested)
1. Java deserialization + embedded journaling tail + unit tests (low risk)
2. Property gating + integration test in remote mode (tail present via synthetic path if needed)
3. Proto field additions (no backend usage yet) and builds
4. Rust population of fields (optional) + tests
5. Enable in CI scenarios validating parity

