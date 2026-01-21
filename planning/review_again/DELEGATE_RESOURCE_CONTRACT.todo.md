# TODO / Fix Plan: `DELEGATE_RESOURCE_CONTRACT` parity

## Goal

Make Rust `DELEGATE_RESOURCE_CONTRACT` validation match Java `DelegateResourceActuator.validate()` semantics, especially the “available FreezeV2 after usage” rule for BANDWIDTH and ENERGY.

Primary Java oracles to match:
- `actuator/src/main/java/org/tron/core/actuator/DelegateResourceActuator.java` (`validate()`)
- `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java` (`updateUsageForDelegated`)
- `chainbase/src/main/java/org/tron/core/db/EnergyProcessor.java` (`updateUsage`)
- `actuator/src/main/java/org/tron/core/vm/utils/FreezeV2Util.java` (`getV2NetUsage`, `getV2EnergyUsage`)

---

## Checklist (tactical)

- [ ] Confirm intended contract boundary
  - [ ] Decide whether Rust must fully validate (authoritative) or can assume Java already validated.
  - [ ] If Rust is authoritative, treat missing checks as correctness bugs.

- [ ] Implement Java-equivalent “available FreezeV2” calculation in Rust
  - [ ] Read global totals from dynamic props:
    - [ ] `TOTAL_NET_WEIGHT`, `TOTAL_NET_LIMIT`
    - [ ] `TOTAL_ENERGY_WEIGHT`, `TOTAL_ENERGY_CURRENT_LIMIT`
  - [ ] Read owner usage inputs (net/energy usage + last-consume times + window sizes)
    - [ ] Decide source-of-truth: Account proto vs `AccountAext` store (remote path already ships AEXT snapshots)
    - [ ] Ensure units match Java (slots = timestamp_ms / 3000; default window size = 28800)
  - [ ] Reproduce Java’s “updateUsage” recovery step (usage decay to `now`)
    - [ ] For BANDWIDTH: parity with `BandwidthProcessor.updateUsageForDelegated`
    - [ ] For ENERGY: parity with `EnergyProcessor.updateUsage`
    - [ ] Implement the exact `ResourceProcessor.increase(..., usage=0, ...)` math (divideCeil + decay + round) or a proven-equivalent simplification for the usage=0 case.
  - [ ] Reproduce Java’s scaling to SUN usage units
    - [ ] BANDWIDTH: `netUsage = (long) (accountNetUsage * TRX_PRECISION * ((double) totalNetWeight / totalNetLimit))`
    - [ ] ENERGY: `energyUsage = (long) (accountEnergyUsage * TRX_PRECISION * ((double) totalEnergyWeight / totalEnergyCurrentLimit))`
    - [ ] Match Java truncation/casting semantics (`(long)` truncation after double arithmetic).
  - [ ] Reproduce `FreezeV2Util.getV2NetUsage` / `getV2EnergyUsage`
    - [ ] BANDWIDTH:
      - [ ] `v2NetUsage = max(0, netUsage - frozenBalanceV1 - acquiredDelegatedFrozenV1 - acquiredDelegatedFrozenV2)`
    - [ ] ENERGY:
      - [ ] `v2EnergyUsage = max(0, energyUsage - energyFrozenBalanceV1 - acquiredDelegatedFrozenV1 - acquiredDelegatedFrozenV2)`
  - [ ] Enforce:
    - [ ] `frozenV2BalanceFor{Bandwidth,Energy} - v2{Net,Energy}Usage >= delegateBalance`
    - [ ] Preserve Java’s exact error strings.

- [ ] Handle Java’s BANDWIDTH “transaction create” estimate (optional parity refinement)
  - [ ] Determine whether Rust ever sees `tx.isTransactionCreate()` equivalent.
    - [ ] If yes, add a metadata flag in the gRPC request and replicate:
      - [ ] `TransactionUtil.estimateConsumeBandWidthSize(...)`
    - [ ] If no (Rust only used for in-block execution), document and intentionally omit.

- [ ] Tests / fixtures
  - [ ] Add a targeted regression/conformance test case where:
    - [ ] `frozen_v2_balance >= delegateBalance` but `(frozen_v2_balance - v2Usage) < delegateBalance`
    - [ ] Java rejects with the “available Freeze*V2 balance” error, and Rust must match.
  - [ ] Cover both resources:
    - [ ] BANDWIDTH path with non-zero `net_usage` and relevant frozen/acquired fields
    - [ ] ENERGY path with non-zero `energy_usage`
  - [ ] Cover lock=true and lock=false (locking is separate; availability check should be independent).

- [ ] Validate end-to-end
  - [ ] Run existing conformance tests that cover resource delegation (fixtures under `framework/src/test/.../ResourceDelegationFixtureGeneratorTest.java`).
  - [ ] If remote execution is used, run a remote-vs-embedded parity diff on a delegation-heavy fixture set.

---

## Suggested implementation touchpoints (where)

- Rust contract logic:
  - `rust-backend/crates/core/src/service/mod.rs`:
    - `execute_delegate_resource_contract(...)` (replace the current “raw frozen_v2 only” check)
    - Add helper(s): `compute_available_freeze_v2_bandwidth(...)`, `compute_available_freeze_v2_energy(...)`

- Rust dynamic property access:
  - `rust-backend/crates/execution/src/storage_adapter/engine.rs` already has getters:
    - `get_total_net_weight`, `get_total_net_limit`
    - `get_total_energy_weight`, `get_total_energy_limit` (maps to `TOTAL_ENERGY_CURRENT_LIMIT`)

- Usage inputs (decide & standardize):
  - If using Account proto fields: read `net_usage`, `energy_usage`, `latest_consume_time`, etc from `protocol::Account`.
  - If using AEXT store: add explicit reads via `get_account_aext(...)` and define the mapping to Java fields.

