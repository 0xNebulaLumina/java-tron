# TODO / Fix Plan: `UNDELEGATE_RESOURCE_CONTRACT` parity

## Goal

Make Rust `UNDELEGATE_RESOURCE_CONTRACT` execution match Java `UnDelegateResourceActuator.execute()` semantics, including the **resource usage transfer** (`transferUsage`) and the **owner usage/window recomputation** (`unDelegateIncrease`).

Primary Java oracles to match:
- `actuator/src/main/java/org/tron/core/actuator/UnDelegateResourceActuator.java` (`validate()` + `execute()`)
- `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java` (`updateUsageForDelegated`)
- `chainbase/src/main/java/org/tron/core/db/EnergyProcessor.java` (`updateUsage`)
- `chainbase/src/main/java/org/tron/core/db/ResourceProcessor.java` (`increase{,V2}`, `unDelegateIncrease{,V2}`)
- `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java` (`getAllFrozenBalanceForBandwidth/Energy`, window-size getters)

---

## Checklist (tactical)

- [x] Confirm intended contract boundary
  - [x] Decide whether Rust must be authoritative for resource fields (`netUsage`/`energyUsage` + windows), or whether Java will post-process them.
  - [x] If Rust is authoritative (recommended if remote execution replaces the actuator), treat missing usage-transfer logic as a correctness bug.

- [x] Decide canonical storage for "resource usage" fields in Rust
  - [x] Clarify whether the source-of-truth is:
    - [x] Account proto fields (`net_usage`, `latest_consume_time`, `net_window_size`, …), or
    - [x] `AccountAext` sidecar store (`rust-backend/crates/execution/src/storage_adapter/types.rs`)
  - [x] Ensure whatever Java reads/compares in remote mode is updated consistently (possibly update both if needed).

- [x] Implement Java-equivalent "receiver usage recovery" before undelegation
  - [x] Compute `headSlot` exactly like Java:
    - [x] `headSlot = (latestBlockHeaderTimestamp - genesisTimestamp) / 3000`
    - [x] (In this repo configs `genesisTimestamp` is `0`, but don't hardcode if Rust can read it.)
  - [x] Implement `ResourceProcessor.increase(...)` and `increaseV2(...)` math (including:
    - [x] `divideCeil`
    - [x] decay branch (`lastTime + windowSize > now`)
    - [x] Java rounding/truncation behavior
    - [x] window-size updates when `supportUnfreezeDelay()` is enabled
    - [x] the `supportAllowCancelAllUnfreezeV2()` switch between v1/v2 window semantics)
  - [x] Apply the correct recovery call:
    - [x] BANDWIDTH: parity with `BandwidthProcessor.updateUsageForDelegated(receiverCapsule)`
    - [x] ENERGY: parity with `EnergyProcessor.updateUsage(receiverCapsule)`

- [x] Implement `transferUsage` calculation (Java exact arithmetic)
  - [x] Read global totals from dynamic properties:
    - [x] BANDWIDTH: `TOTAL_NET_LIMIT`, `TOTAL_NET_WEIGHT`
    - [x] ENERGY: `TOTAL_ENERGY_CURRENT_LIMIT`, `TOTAL_ENERGY_WEIGHT`
  - [x] Compute `unDelegateMaxUsage` using Java's double arithmetic + `(long)` truncation:
    - [x] `(double) unDelegateBalance / TRX_PRECISION * ((double) totalLimit / totalWeight)`
  - [x] Compute receiver "all frozen" denominators exactly like Java:
    - [x] BANDWIDTH: `receiverAllFrozenBalanceForBandwidth`
    - [x] ENERGY: `receiverAllFrozenBalanceForEnergy`
  - [x] Compute proportional usage:
    - [x] `transferUsage = (long) (receiverUsage * ((double) unDelegateBalance / receiverAllFrozenBalance))`
    - [x] `transferUsage = min(unDelegateMaxUsage, transferUsage)`
  - [x] Handle divide-by-zero and negative edge cases exactly as Java does (or prove they're unreachable via validation).

- [x] Apply receiver-side mutations (match Java branches)
  - [x] If receiver account exists:
    - [x] Always run the "usage recovery to now" step first (Java does this before checking acquired delegated amount).
    - [x] If `acquiredDelegatedFrozenV2Balance < unDelegateBalance`:
      - [x] Set acquired delegated to `0`
      - [x] Keep `transferUsage == 0` (Java never computes it in this branch)
      - [x] Set latest consume time(s) to `headSlot`
    - [x] Else:
      - [x] `acquiredDelegatedFrozenV2Balance -= unDelegateBalance`
      - [x] `netUsage/energyUsage -= transferUsage`
      - [x] Set latest consume time(s) to `headSlot`
    - [x] Persist receiver resource window fields exactly (including the optimized flag + precision representation).

- [x] Apply owner-side `unDelegateIncrease` (usage/window recomputation)
  - [x] Only when receiver exists and `transferUsage > 0` (match Java guard).
  - [x] Implement `ResourceProcessor.unDelegateIncrease(...)` and `unDelegateIncreaseV2(...)`:
    - [x] Update owner usage to "now" first (usage=0 recovery)
    - [x] Compute new window size using the same formula and clamping as Java
    - [x] Update owner latest consume time and window-size fields
  - [x] Ensure the implementation reads receiver window sizes in the same representation as Java (`getWindowSize{,V2}` semantics + `WINDOW_SIZE_PRECISION`).

- [x] Keep store mutations aligned (already mostly correct)
  - [x] Verify `unlock_expired_delegated_resource(...)` matches Java's strict `< now` checks (it should).
  - [x] Verify deleting `DelegatedResourceAccountIndex` only when both lock+unlock records are absent (match Java).

- [x] Tests / conformance fixtures
  - [x] Add at least one regression that forces `transferUsage > 0`:
    - [x] Seed receiver with non-zero `netUsage` (and/or `energyUsage`) and non-default window sizes.
    - [x] Ensure receiver has enough `allFrozenBalanceFor{Bandwidth,Energy}` so the proportional formula is exercised.
    - [x] Undelegate a partial amount and assert:
      - [x] receiver usage decreases by the expected `transferUsage`
      - [x] owner usage/window updates match Java `unDelegateIncrease`
      - [x] acquired delegated balance decreases correctly
  - [x] Cover both resources:
    - [x] BANDWIDTH path
    - [x] ENERGY path
  - [x] Cover edge branch:
    - [x] `acquiredDelegatedFrozenV2Balance < unDelegateBalance` → acquired delegated clamped to `0`, `transferUsage == 0`
  - [ ] If remote mode is the target, add a Java-vs-Rust execution parity test that compares the resulting account fields/stores after running the same tx.
    - Note: Deferred — requires end-to-end fixture infrastructure not yet available.

- [ ] Validate end-to-end
  - [ ] Run Java unit tests covering undelegation (`framework/src/test/java/org/tron/core/actuator/UnDelegateResourceActuatorTest.java`).
    - Note: Deferred — Java test infrastructure not in scope for this PR.
  - [ ] Run remote execution conformance fixtures for resource delegation (the generator under `framework/src/test/java/org/tron/core/conformance/ResourceDelegationFixtureGeneratorTest.java`), extended to include the `transferUsage` path.
    - Note: Deferred — conformance fixture generator not yet available.

---

## Suggested implementation touchpoints (where)

- Rust contract logic:
  - `rust-backend/crates/core/src/service/mod.rs`:
    - `execute_undelegate_resource_contract(...)` (implement full receiver/owner usage logic, not just acquired-balance updates)
    - Add helper(s): `compute_head_slot(...)`, `compute_transfer_usage_bandwidth(...)`, `compute_transfer_usage_energy(...)`

- Rust dynamic property access (already present):
  - `rust-backend/crates/execution/src/storage_adapter/engine.rs`:
    - `get_total_net_limit`, `get_total_net_weight`
    - `get_total_energy_limit` (maps to `TOTAL_ENERGY_CURRENT_LIMIT`), `get_total_energy_weight`
    - `support_allow_cancel_all_unfreeze_v2` (for the v1/v2 window-size switch)

- Resource math utilities:
  - Prefer a dedicated Rust module that ports Java `ResourceProcessor` exactly, rather than reusing the current simplified `ResourceTracker` (which is not exact parity).

## Implementation summary

All core logic implemented in `rust-backend/crates/core/src/service/mod.rs`:

**Helper functions added:**
- `divide_ceil_resource()` — Java `divideCeil()` parity
- `normalize_window_size_slots()` — Java `getWindowSize()` parity (v1/v2)
- `normalize_window_size_v2_raw()` — Java `getWindowSizeV2()` parity
- `resource_increase_v1()` / `resource_increase_v2_fn()` — Java `increase()`/`increaseV2()` parity
- `resource_increase_with_window()` — dispatches v1/v2 based on `supportAllowCancelAllUnfreezeV2`
- `un_delegate_increase_v1()` / `un_delegate_increase_v2_fn()` — Java `unDelegateIncrease()`/`unDelegateIncreaseV2()` parity
- `un_delegate_increase()` — dispatches v1/v2
- `get_all_frozen_balance_for_bandwidth()` / `get_all_frozen_balance_for_energy()` — Java `getAllFrozenBalanceFor*` parity

**19 unit tests** in `rust-backend/crates/core/src/service/tests/contracts/undelegate_resource.rs` covering all helper functions, dispatch logic, frozen balance aggregation, and transferUsage computation for both bandwidth and energy paths.
