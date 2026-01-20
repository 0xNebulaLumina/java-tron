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

- [ ] Confirm intended contract boundary
  - [ ] Decide whether Rust must be authoritative for resource fields (`netUsage`/`energyUsage` + windows), or whether Java will post-process them.
  - [ ] If Rust is authoritative (recommended if remote execution replaces the actuator), treat missing usage-transfer logic as a correctness bug.

- [ ] Decide canonical storage for “resource usage” fields in Rust
  - [ ] Clarify whether the source-of-truth is:
    - [ ] Account proto fields (`net_usage`, `latest_consume_time`, `net_window_size`, …), or
    - [ ] `AccountAext` sidecar store (`rust-backend/crates/execution/src/storage_adapter/types.rs`)
  - [ ] Ensure whatever Java reads/compares in remote mode is updated consistently (possibly update both if needed).

- [ ] Implement Java-equivalent “receiver usage recovery” before undelegation
  - [ ] Compute `headSlot` exactly like Java:
    - [ ] `headSlot = (latestBlockHeaderTimestamp - genesisTimestamp) / 3000`
    - [ ] (In this repo configs `genesisTimestamp` is `0`, but don’t hardcode if Rust can read it.)
  - [ ] Implement `ResourceProcessor.increase(...)` and `increaseV2(...)` math (including:
    - [ ] `divideCeil`
    - [ ] decay branch (`lastTime + windowSize > now`)
    - [ ] Java rounding/truncation behavior
    - [ ] window-size updates when `supportUnfreezeDelay()` is enabled
    - [ ] the `supportAllowCancelAllUnfreezeV2()` switch between v1/v2 window semantics)
  - [ ] Apply the correct recovery call:
    - [ ] BANDWIDTH: parity with `BandwidthProcessor.updateUsageForDelegated(receiverCapsule)`
    - [ ] ENERGY: parity with `EnergyProcessor.updateUsage(receiverCapsule)`

- [ ] Implement `transferUsage` calculation (Java exact arithmetic)
  - [ ] Read global totals from dynamic properties:
    - [ ] BANDWIDTH: `TOTAL_NET_LIMIT`, `TOTAL_NET_WEIGHT`
    - [ ] ENERGY: `TOTAL_ENERGY_CURRENT_LIMIT`, `TOTAL_ENERGY_WEIGHT`
  - [ ] Compute `unDelegateMaxUsage` using Java’s double arithmetic + `(long)` truncation:
    - [ ] `(double) unDelegateBalance / TRX_PRECISION * ((double) totalLimit / totalWeight)`
  - [ ] Compute receiver “all frozen” denominators exactly like Java:
    - [ ] BANDWIDTH: `receiverAllFrozenBalanceForBandwidth`
    - [ ] ENERGY: `receiverAllFrozenBalanceForEnergy`
  - [ ] Compute proportional usage:
    - [ ] `transferUsage = (long) (receiverUsage * ((double) unDelegateBalance / receiverAllFrozenBalance))`
    - [ ] `transferUsage = min(unDelegateMaxUsage, transferUsage)`
  - [ ] Handle divide-by-zero and negative edge cases exactly as Java does (or prove they’re unreachable via validation).

- [ ] Apply receiver-side mutations (match Java branches)
  - [ ] If receiver account exists:
    - [ ] Always run the “usage recovery to now” step first (Java does this before checking acquired delegated amount).
    - [ ] If `acquiredDelegatedFrozenV2Balance < unDelegateBalance`:
      - [ ] Set acquired delegated to `0`
      - [ ] Keep `transferUsage == 0` (Java never computes it in this branch)
      - [ ] Set latest consume time(s) to `headSlot`
    - [ ] Else:
      - [ ] `acquiredDelegatedFrozenV2Balance -= unDelegateBalance`
      - [ ] `netUsage/energyUsage -= transferUsage`
      - [ ] Set latest consume time(s) to `headSlot`
    - [ ] Persist receiver resource window fields exactly (including the optimized flag + precision representation).

- [ ] Apply owner-side `unDelegateIncrease` (usage/window recomputation)
  - [ ] Only when receiver exists and `transferUsage > 0` (match Java guard).
  - [ ] Implement `ResourceProcessor.unDelegateIncrease(...)` and `unDelegateIncreaseV2(...)`:
    - [ ] Update owner usage to “now” first (usage=0 recovery)
    - [ ] Compute new window size using the same formula and clamping as Java
    - [ ] Update owner latest consume time and window-size fields
  - [ ] Ensure the implementation reads receiver window sizes in the same representation as Java (`getWindowSize{,V2}` semantics + `WINDOW_SIZE_PRECISION`).

- [ ] Keep store mutations aligned (already mostly correct)
  - [ ] Verify `unlock_expired_delegated_resource(...)` matches Java’s strict `< now` checks (it should).
  - [ ] Verify deleting `DelegatedResourceAccountIndex` only when both lock+unlock records are absent (match Java).

- [ ] Tests / conformance fixtures
  - [ ] Add at least one regression that forces `transferUsage > 0`:
    - [ ] Seed receiver with non-zero `netUsage` (and/or `energyUsage`) and non-default window sizes.
    - [ ] Ensure receiver has enough `allFrozenBalanceFor{Bandwidth,Energy}` so the proportional formula is exercised.
    - [ ] Undelegate a partial amount and assert:
      - [ ] receiver usage decreases by the expected `transferUsage`
      - [ ] owner usage/window updates match Java `unDelegateIncrease`
      - [ ] acquired delegated balance decreases correctly
  - [ ] Cover both resources:
    - [ ] BANDWIDTH path
    - [ ] ENERGY path
  - [ ] Cover edge branch:
    - [ ] `acquiredDelegatedFrozenV2Balance < unDelegateBalance` → acquired delegated clamped to `0`, `transferUsage == 0`
  - [ ] If remote mode is the target, add a Java-vs-Rust execution parity test that compares the resulting account fields/stores after running the same tx.

- [ ] Validate end-to-end
  - [ ] Run Java unit tests covering undelegation (`framework/src/test/java/org/tron/core/actuator/UnDelegateResourceActuatorTest.java`).
  - [ ] Run remote execution conformance fixtures for resource delegation (the generator under `framework/src/test/java/org/tron/core/conformance/ResourceDelegationFixtureGeneratorTest.java`), extended to include the `transferUsage` path.

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

