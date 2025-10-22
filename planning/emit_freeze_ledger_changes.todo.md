# Plan: Emit Freeze Ledger Changes (Remote) and Apply in Java

Goal: Maintain remote execution for FreezeBalanceContract while ensuring Java’s resource accounting (netLimit) matches embedded by emitting and applying freeze ledger and dynamic totals changes.

## Scope Overview

- Configuration
  - Keep `freeze_balance_enabled = true`.
  - Introduce/enable `emit_freeze_ledger_changes = true` to emit storage-level changes for freeze ledger and dynamic totals.

- Rust backend
  - Emit StorageChange records for:
    - Account freeze records (bandwidth; energy/tron power optional but gated the same way).
    - Dynamic totals: `TOTAL_NET_WEIGHT`, `TOTAL_NET_LIMIT` (energy totals optional).

- Java node
  - RuntimeSpiImpl consumes new StorageChange records and applies them to:
    - AccountCapsule freeze ledger (so `getAllFrozenBalanceForBandwidth()` is up-to-date).
    - DynamicPropertiesStore totals (so `getTotalNetWeight/Limit()` reflect remote updates).

- Diagnostics
  - Temporary INFO logs in BandwidthProcessor for: `getAllFrozenBalanceForBandwidth()`, `getTotalNetWeight()`, `getTotalNetLimit()` at CREATE_ACCOUNT decision points.

## Design Details

### 1) Configuration (Rust)

- File: `rust-backend/config.toml`
- Keys (under `[execution.remote]`):
  - `freeze_balance_enabled = true`
  - `emit_freeze_ledger_changes = true` (already present; ensure it is read and plumbed to execution module)
- TODOs
  - [x] Verify `ExecutionConfig` exposes `emit_freeze_ledger_changes` to the core service.
  - [ ] Add a unit test asserting config flag toggles emission.

### 2) Wire Format for Emitted Changes

- Reuse existing state change types delivered to Java (no new RPC surface):
  - Use StorageChange-like record in results (address + key + old/new values).
- Keys and payloads
  - Freeze ledger records (per account):
    - `address`: 21-byte Tron address (prefix 0x41 + 20-byte EVM addr), same as AccountChange.
    - `key`: ASCII string (bytes) – choose one of:
      - `"FREEZE:BW"` (bandwidth)
      - `"FREEZE:EN"` (energy) — optional phase
      - `"FREEZE:TP"` (tron power) — optional phase
    - `value`: 16 bytes (FreezeRecord): `amount[8] (be)` + `expiration_ts[8] (be)`.
    - `oldValue/newValue`: previous vs current records; empty means deletion.
  - Dynamic totals (global scalar keys):
    - `address`: sentinel (two options; pick one and document):
      - ASCII `"DYNPROPS"` (recommended for readability), or
      - 21-byte all-zero address.
    - `key`: ASCII `"TOTAL_NET_WEIGHT"`, `"TOTAL_NET_LIMIT"` (ENERGY totals optional).
    - `value`: 8-byte big-endian u64.
- TODOs
  - [x] Decide sentinel address representation (DYNPROPS vs zeroes) and keep consistent. *(Chose DYNPROPS + zero-check fallback)*
  - [x] Document key strings centrally to avoid drift. *(Documented in RuntimeSpiImpl.java and service.rs)*

### 3) Rust Backend Emission

- Primary locations:
  - `rust-backend/crates/core/src/service.rs`:
    - FreezeBalanceContract/unfreeze handlers — after state mutation, append StorageChange records if `emit_freeze_ledger_changes`.
  - `rust-backend/crates/execution/src/storage_adapter.rs`:
    - Provide helpers:
      - `serialize_freeze_record(amount: u64, expiration: i64) -> [u8;16]` (already present as struct; reuse).
      - `read/write dynamic totals` (helpers already exist for reading; add for emitting changes as needed).
- Emission policy:
  - [x] Only emit when the new value differs from the old (avoid no-op deltas). *(Reads old record, compares via serialize)*
  - [x] Include both `oldValue` and `newValue` for traceability and idempotent application.
- Freeze paths to cover:
  - [x] Freeze balance for bandwidth
  - [ ] Unfreeze (record removal or adjustment) *(Not implemented yet - freeze only)*
  - [ ] (Optional) V2 freeze if your chain uses it — ensure type tagging or key distinguishes.
- Dynamic totals:
  - [ ] After freeze/unfreeze, compute resulting totals; if changed, emit `TOTAL_NET_WEIGHT` and/or `TOTAL_NET_LIMIT` deltas. *(DEFERRED - requires global tracking infrastructure)*
- Testing (Rust):
  - [ ] Unit: freeze → emits `FREEZE:BW` StorageChange with correct old/new bytes.
  - [ ] Unit: freeze → emits `TOTAL_NET_WEIGHT`/`TOTAL_NET_LIMIT` deltas when expected.
  - [ ] Integration: apply repeated freezes/unfreezes and ensure emission is coherent and minimal.

### 4) Java Side Application (RuntimeSpiImpl)

- File: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- Extend change application logic:
  - In `applyStateChangesToLocalDatabase`:
    - [x] Route AccountChange to existing `updateAccountState` (no change).
    - [x] Route StorageChange to `updateAccountStorage`, which must:
      - Recognize `DYNPROPS` (or zero address) and apply totals:
        - `"TOTAL_NET_WEIGHT"` → `dynamicPropertiesStore.saveTotalNetWeight(valueLong)`
        - `"TOTAL_NET_LIMIT"` → `dynamicPropertiesStore.saveTotalNetLimit(valueLong)`
        - Mark via `ResourceSyncContext.recordDynamicKeyDirty(...)`. *(Note: Not implemented yet, TODO)*
      - Recognize `"FREEZE:*"` keys for real addresses:
        - Parse 16-byte freeze record (amount, expiration).
        - Update AccountCapsule freeze ledger so `getAllFrozenBalanceForBandwidth()` matches:
          - V1 path (Frozen) — `setFrozenForBandwidth(amount, expiration)` if applicable. *(Implemented)*
          - V2 path (FreezeV2 with type BANDWIDTH) — add/update entry. *(Not implemented - V1 only)*
        - Persist via `accountStore.put(...)` and mark dirty. *(Implemented)*
      - Handle deletions (empty newValue): clear or zero the freeze for that resource. *(Implemented)*
- Ordering & idempotence:
  - [x] Apply storage changes immediately (same transaction) before the next BandwidthProcessor.consume.
  - [x] Unknown keys: log at DEBUG and ignore.
- Testing (Java):
  - [ ] Unit: applying `FREEZE:BW` updates AccountCapsule; `getAllFrozenBalanceForBandwidth()` reflects change.
  - [ ] Unit: applying `TOTAL_NET_WEIGHT/LIMIT` changes store values.
  - [ ] Integration: feed a synthetic remote result (freeze, then transfer) → BandwidthProcessor sees non-zero netLimit.

### 5) Diagnostics (Temporary)

- File: `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java`
- At CREATE_ACCOUNT decision point, log:
  - [x] `accountFrozenBW = accountCapsule.getAllFrozenBalanceForBandwidth()`
  - [x] `totalNetWeight = dynamicPropertiesStore.getTotalNetWeight()`
  - [x] `totalNetLimit  = dynamicPropertiesStore.getTotalNetLimit()`
  - [x] Already logging bytes, rate, nowSlot, netLimit, netUsage, free/public metrics.
- Rollback plan:
  - [ ] Demote to DEBUG after parity verification.

### 6) CSV / Observability Considerations

- CSV effects:
  - Freeze/unfreeze txs will now include additional storage changes (shape change). Non-freeze txs (like TransferContract) remain 2 changes.
- Observability:
  - [ ] Optionally emit metrics counters for applied freeze/dynamic storage changes.
  - [ ] Add a one-time WARN if remote engine returns freeze/dynamic changes but Java does not recognize the key.

### 7) Validation Plan

- Reproduce the original mismatch window and confirm:
  - [ ] For block 2458 tx `ea03...944a`, embedded and remote CREATE_ACCOUNT logs show the same `netLimit` (≈8639) and both use BANDWIDTH.
  - [ ] Sender baseline offset of 100,000 SUN disappears from remote CSV for that tx.
- Broad checks:
  - [ ] Random sample across following blocks: CREATE_ACCOUNT decisions consistent.
  - [ ] No regressions on energy or other resource paths.

### 8) Risks & Mitigations

- Risk: Applying freeze changes in wrong ledger (V1 vs V2) → incorrect totals.
  - Mitigation: Gate V2 updates behind a config; start with V1 if your historical window predates V2, or update both.
- Risk: Inconsistent dynamicTotals (TOTAL_NET_WEIGHT/LIMIT) if omitted at some blocks.
  - Mitigation: Emit deltas from the authoritative place (execution module) whenever changes happen; apply idempotently in Java.
- Risk: CSV shape changes may break downstream diff tools.
  - Mitigation: Document change, add comparator normalization for freeze txs during rollout.

### 9) Milestones & Checklist

1) Config plumbing (Rust)
- [ ] Ensure `emit_freeze_ledger_changes` is read by core service.

2) Emission (Rust)
- [ ] Implement StorageChange emission for `FREEZE:BW` and dynamic totals.
- [ ] Unit tests for emission.

3) Application (Java)
- [ ] Extend `RuntimeSpiImpl.updateAccountStorage` to apply `DYNPROPS` totals.
- [ ] Extend to apply `FREEZE:BW` to AccountCapsule (V1/V2 as needed).
- [ ] Unit tests for application.

4) Diagnostics
- [ ] Add temporary INFO logs for frozen BW and totals in `BandwidthProcessor`.
- [ ] Validate logs show identical inputs embedded vs remote.

5) Integration validation
- [ ] Replay target window, confirm `netLimit` alignment and CSV parity at 2458.
- [ ] Spot check subsequent blocks.

6) Rollback/demote logs
- [ ] Demote diagnostics to DEBUG once parity is confirmed.

### 10) Open Decisions (confirm before coding)

- [ ] Sentinel address for dynamic props: `"DYNPROPS"` vs 21-byte zeroes. (Recommend `"DYNPROPS"`).
- [ ] Freeze ledger target (V1, V2, or both) for your historical range.
- [ ] Whether to also emit energy totals now or defer to a later phase.

---

Outcome: With remote freeze execution emitting ledger and dynamic totals — and Java applying them immediately — `netLimit` becomes consistent across modes, and CREATE_ACCOUNT decisions (BANDWIDTH vs FEE) match embedded, removing the 100,000 SUN baseline offsets observed in remote CSVs.
