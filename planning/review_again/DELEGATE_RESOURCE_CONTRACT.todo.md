# TODO / Fix Plan: `DELEGATE_RESOURCE_CONTRACT` parity

## Goal

Make Rust `DELEGATE_RESOURCE_CONTRACT` validation match Java `DelegateResourceActuator.validate()` semantics, especially the "available FreezeV2 after usage" rule for BANDWIDTH and ENERGY.

Primary Java oracles to match:
- `actuator/src/main/java/org/tron/core/actuator/DelegateResourceActuator.java` (`validate()`)
- `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java` (`updateUsageForDelegated`)
- `chainbase/src/main/java/org/tron/core/db/EnergyProcessor.java` (`updateUsage`)
- `actuator/src/main/java/org/tron/core/vm/utils/FreezeV2Util.java` (`getV2NetUsage`, `getV2EnergyUsage`)

---

## Checklist (tactical)

- [x] Confirm intended contract boundary
  - [x] Decide whether Rust must fully validate (authoritative) or can assume Java already validated.
    - **Decision**: Rust is authoritative for validation - treat missing checks as correctness bugs.
  - [x] If Rust is authoritative, treat missing checks as correctness bugs.

- [x] Implement Java-equivalent "available FreezeV2" calculation in Rust
  - [x] Read global totals from dynamic props:
    - [x] `TOTAL_NET_WEIGHT`, `TOTAL_NET_LIMIT` - via `storage_adapter.get_total_net_weight()` and `get_total_net_limit()`
    - [x] `TOTAL_ENERGY_WEIGHT`, `TOTAL_ENERGY_CURRENT_LIMIT` - via `storage_adapter.get_total_energy_weight()` and `get_total_energy_limit()`
  - [x] Read owner usage inputs (net/energy usage + last-consume times + window sizes)
    - [x] Decide source-of-truth: Account proto vs `AccountAext` store (remote path already ships AEXT snapshots)
      - **Decision**: Use Account proto fields directly (`net_usage`, `energy_usage`, `latest_consume_time`, `net_window_size`, etc.)
    - [x] Ensure units match Java (slots = timestamp_ms / 3000; default window size = 28800)
      - Implemented: `head_slot = now_timestamp / 3000`, default `WINDOW_SIZE_SLOTS = 28800`
  - [x] Reproduce Java's "updateUsage" recovery step (usage decay to `now`)
    - [x] For BANDWIDTH: parity with `BandwidthProcessor.updateUsageForDelegated`
      - Implemented in `calculate_decayed_usage()` called by `compute_available_freeze_v2_bandwidth()`
    - [x] For ENERGY: parity with `EnergyProcessor.updateUsage`
      - Implemented in `calculate_decayed_usage()` called by `compute_available_freeze_v2_energy()`
    - [x] Implement the exact `ResourceProcessor.increase(..., usage=0, ...)` math (divideCeil + decay + round) or a proven-equivalent simplification for the usage=0 case.
      - Implemented: `calculate_decayed_usage()` with divideCeil, decay formula, and f64 round
  - [x] Reproduce Java's scaling to SUN usage units
    - [x] BANDWIDTH: `netUsage = (long) (accountNetUsage * TRX_PRECISION * ((double) totalNetWeight / totalNetLimit))`
    - [x] ENERGY: `energyUsage = (long) (accountEnergyUsage * TRX_PRECISION * ((double) totalEnergyWeight / totalEnergyCurrentLimit))`
    - [x] Match Java truncation/casting semantics (`(long)` truncation after double arithmetic).
      - Implemented: `(... as f64 * ... * ...) as i64`
  - [x] Reproduce `FreezeV2Util.getV2NetUsage` / `getV2EnergyUsage`
    - [x] BANDWIDTH:
      - [x] `v2NetUsage = max(0, netUsage - frozenBalanceV1 - acquiredDelegatedFrozenV1 - acquiredDelegatedFrozenV2)`
        - Implemented in `get_v2_net_usage()`
    - [x] ENERGY:
      - [x] `v2EnergyUsage = max(0, energyUsage - energyFrozenBalanceV1 - acquiredDelegatedFrozenV1 - acquiredDelegatedFrozenV2)`
        - Implemented in `get_v2_energy_usage()`
  - [x] Enforce:
    - [x] `frozenV2BalanceFor{Bandwidth,Energy} - v2{Net,Energy}Usage >= delegateBalance`
      - Implemented in `execute_delegate_resource_contract()` validation step 6
    - [x] Preserve Java's exact error strings.
      - Error string: `"delegateBalance must be less than or equal to available FreezeBandwidthV2 balance"` / `"... FreezeEnergyV2 balance"`

- [ ] Handle Java's BANDWIDTH "transaction create" estimate (optional parity refinement)
  - [ ] Determine whether Rust ever sees `tx.isTransactionCreate()` equivalent.
    - **Note**: This is an optional refinement. Java adds `TransactionUtil.estimateConsumeBandWidthSize()` to `accountNetUsage` only when `tx.isTransactionCreate()` is true. This applies to special transaction scenarios.
    - [ ] If yes, add a metadata flag in the gRPC request and replicate:
      - [ ] `TransactionUtil.estimateConsumeBandWidthSize(...)`
    - [ ] If no (Rust only used for in-block execution), document and intentionally omit.

- [ ] Tests / fixtures
  - [ ] Add a targeted regression/conformance test case where:
    - [ ] `frozen_v2_balance >= delegateBalance` but `(frozen_v2_balance - v2Usage) < delegateBalance`
    - [ ] Java rejects with the "available Freeze*V2 balance" error, and Rust must match.
  - [ ] Cover both resources:
    - [ ] BANDWIDTH path with non-zero `net_usage` and relevant frozen/acquired fields
    - [ ] ENERGY path with non-zero `energy_usage`
  - [ ] Cover lock=true and lock=false (locking is separate; availability check should be independent).

- [ ] Validate end-to-end
  - [ ] Run existing conformance tests that cover resource delegation (fixtures under `framework/src/test/.../ResourceDelegationFixtureGeneratorTest.java`).
  - [ ] If remote execution is used, run a remote-vs-embedded parity diff on a delegation-heavy fixture set.

---

## Implementation Summary

### Files Modified (2026-02-05)

**`rust-backend/crates/core/src/service/mod.rs`**:

1. **New constants** (line ~7247):
   - `PRECISION`: 1,000,000 (matches Java's PRECISION)
   - `WINDOW_SIZE_MS`: 24 * 3600 * 1000 (24 hours in ms)
   - `BLOCK_PRODUCED_INTERVAL`: 3000 (3 seconds)
   - `WINDOW_SIZE_SLOTS`: 28800 (default window size in slots)

2. **New helper functions** (lines ~7250-7410):
   - `get_frozen_v1_balance_for_bandwidth()` - Sum of account.frozen[].frozen_balance
   - `get_frozen_v1_balance_for_energy()` - account_resource.frozen_balance_for_energy.frozen_balance
   - `get_acquired_delegated_frozen_v1_balance_for_bandwidth()` - V1 acquired delegated
   - `get_acquired_delegated_frozen_v1_balance_for_energy()` - V1 acquired delegated for energy
   - `calculate_decayed_usage()` - Implements ResourceProcessor.increase() decay logic
   - `get_v2_net_usage()` - FreezeV2Util.getV2NetUsage() parity
   - `get_v2_energy_usage()` - FreezeV2Util.getV2EnergyUsage() parity
   - `compute_available_freeze_v2_bandwidth()` - Full BANDWIDTH availability calculation
   - `compute_available_freeze_v2_energy()` - Full ENERGY availability calculation

3. **Modified `execute_delegate_resource_contract()`** (validation step 6):
   - Now computes head_slot from timestamp
   - Fetches global totals (net_weight, net_limit, energy_weight, energy_limit)
   - Calls `compute_available_freeze_v2_bandwidth()` or `compute_available_freeze_v2_energy()`
   - Validates against available balance after usage, not raw frozen balance
   - Added debug logging for validation parameters

---

## Suggested implementation touchpoints (where)

- Rust contract logic:
  - `rust-backend/crates/core/src/service/mod.rs`:
    - `execute_delegate_resource_contract(...)` (replace the current "raw frozen_v2 only" check) - **DONE**
    - Add helper(s): `compute_available_freeze_v2_bandwidth(...)`, `compute_available_freeze_v2_energy(...)` - **DONE**

- Rust dynamic property access:
  - `rust-backend/crates/execution/src/storage_adapter/engine.rs` already has getters:
    - `get_total_net_weight`, `get_total_net_limit` - **Used**
    - `get_total_energy_weight`, `get_total_energy_limit` (maps to `TOTAL_ENERGY_CURRENT_LIMIT`) - **Used**

- Usage inputs (decide & standardize):
  - If using Account proto fields: read `net_usage`, `energy_usage`, `latest_consume_time`, etc from `protocol::Account`. - **Implemented**
  - If using AEXT store: add explicit reads via `get_account_aext(...)` and define the mapping to Java fields. - **Not needed**

