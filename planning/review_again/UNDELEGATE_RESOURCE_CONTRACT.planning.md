# Review: `UNDELEGATE_RESOURCE_CONTRACT` parity (rust-backend vs java-tron)

## TL;DR

Rust matches Java’s **gating**, **basic validation**, and the **core delegated-balance mutations** (accounts + `DelegatedResource*` stores).

But it **does not match Java execution semantics** for the most important runtime behavior:
Java **moves resource usage** (bandwidth/energy “usage”) proportionally from the receiver back to the owner during undelegation, via:
- `BandwidthProcessor.updateUsageForDelegated(...)` / `EnergyProcessor.updateUsage(...)`
- `transferUsage` calculation (bounded by global totals)
- `ResourceProcessor.unDelegateIncrease(...)` (owner window/usage recomputation)

Rust currently does **none** of that; it only adjusts acquired delegated balances and (partially) stamps window/time fields. If Rust becomes authoritative for this contract, that gap can cause **state divergence** (especially `netUsage`/`energyUsage` and window-size fields).

---

## What the Rust side does today (summary)

Rust entrypoint:
- `rust-backend/crates/core/src/service/mod.rs` → `execute_undelegate_resource_contract(...)`

High-level flow:
1. Gate checks: `support_dr()` and `support_unfreeze_delay()`
2. Owner address validation from `transaction.metadata.from_raw` and owner existence check
3. Parse `UnDelegateResourceContract` bytes (skips owner field), validate receiver address and receiver != owner
4. Validate `balance > 0`, resource is BANDWIDTH/ENERGY, and that undelegation amount is available:
   - available = unlock record + (expired lock record for that resource)
5. Execution mutations:
   - Receiver account (if exists): reduce `acquired_delegated_frozen_v2_balance_for_*` by `balance`
     - also sets `*_window_size` and `latest_consume_time*` fields in a “minimal parity” way
   - DelegatedResourceStore: `undelegate_resource(...)` which internally:
     - calls `unlock_expired_delegated_resource(...)` (Java parity for `unLockExpireResource`)
     - subtracts from the unlock record and deletes it if both balances hit zero
   - DelegatedResourceAccountIndex: removed only if both lock+unlock records are gone
   - Owner account: `delegated_frozen_v2_balance_for_* -= balance`, `frozen_v2_* += balance`

Storage adapter detail:
- `rust-backend/crates/execution/src/storage_adapter/engine.rs`:
  - `unlock_expired_delegated_resource(...)` matches Java `DelegatedResourceStore.unLockExpireResource(...)`
  - `undelegate_resource(...)` matches Java’s “subtract from unlock record; delete if empty” behavior

---

## Java side oracle behavior

Primary Java reference:
- `actuator/src/main/java/org/tron/core/actuator/UnDelegateResourceActuator.java`
  - `validate()` performs the same gating + availability checks (unlock + expired lock only).
  - `execute()` performs the state mutations *and* the resource-usage transfer logic.

Key runtime semantics that happen in Java `execute()`:

### 1) Receiver usage is recovered to “now” before adjustments

- BANDWIDTH: `BandwidthProcessor.updateUsageForDelegated(receiverCapsule)`
  - `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java`
- ENERGY: `EnergyProcessor.updateUsage(receiverCapsule)`
  - `chainbase/src/main/java/org/tron/core/db/EnergyProcessor.java`

These calls apply TRON’s windowed decay (`ResourceProcessor.increase`) and can also adjust window-size fields depending on feature flags.

### 2) Java computes `transferUsage` and subtracts it from the receiver

For BANDWIDTH:
- `unDelegateMaxUsage = (unDelegateBalance / TRX_PRECISION) * (totalNetLimit / totalNetWeight)`
- `transferUsage = receiverNetUsage * (unDelegateBalance / receiverAllFrozenBalanceForBandwidth)`
- `transferUsage = min(unDelegateMaxUsage, transferUsage)`
- receiver:
  - `acquiredDelegatedFrozenV2BalanceForBandwidth -= unDelegateBalance`
  - `netUsage -= transferUsage`
  - `latestConsumeTime = headSlot`

For ENERGY:
- same structure, using `totalEnergyCurrentLimit / totalEnergyWeight` and `energyUsage`

### 3) Java increases the owner’s usage/window using `unDelegateIncrease`

If receiver exists and `transferUsage > 0`:
- `BandwidthProcessor.unDelegateIncrease(ownerCapsule, receiverCapsule, transferUsage, BANDWIDTH, headSlot)`
- `EnergyProcessor.unDelegateIncrease(ownerCapsule, receiverCapsule, transferUsage, ENERGY, headSlot)`

Implementation:
- `chainbase/src/main/java/org/tron/core/db/ResourceProcessor.java` → `unDelegateIncrease(...)` / `unDelegateIncreaseV2(...)`

This mutates owner usage and window-size fields to preserve correct decay semantics after “moving” usage from receiver to owner.

---

## Concrete mismatches (why it is not equivalent)

### 1) Missing `transferUsage` computation + receiver usage reduction (major)

Rust does not:
- call the equivalent of `BandwidthProcessor.updateUsageForDelegated(...)` / `EnergyProcessor.updateUsage(...)`
- compute `unDelegateMaxUsage` from global totals
- compute and apply `transferUsage`
- reduce receiver `netUsage` / `energyUsage`

So the receiver’s usage accounting remains as-if delegation never changed.

### 2) Missing owner `unDelegateIncrease` usage/window adjustment (major)

Rust does not implement `ResourceProcessor.unDelegateIncrease{,V2}` semantics.
Owner window sizes and usage are not updated to incorporate the transferred usage that Java applies.

### 3) Receiver window/time stamping is only a “minimal parity” approximation (parity risk)

Rust updates some receiver window/time fields (and only if the receiver exists), but it does so without:
- the preceding usage recovery step, and
- the subsequent usage decrement step.

This is not how Java transitions these fields, and it can further diverge later resource computations.

### 4) Owner-address source differs (parity risk)

Java validates/uses `owner_address` from the protobuf contract.

Rust ignores field 1 in the contract bytes and instead uses:
- `transaction.metadata.from_raw`

This is likely equivalent in the normal path (signature owner == contract owner), but it is still a validation divergence if those ever differ.

---

## Conclusion

Rust `UNDELEGATE_RESOURCE_CONTRACT` currently matches Java for the **delegated-balance bookkeeping** (account frozen/delegated fields + DelegatedResource stores), but it **does not match Java’s resource usage transfer semantics**.

If Rust execution is enabled for this contract and used as the source of truth, expect divergence in:
- receiver `netUsage` / `energyUsage` (and related window/time fields)
- owner usage/window evolution after undelegation

See `planning/review_again/UNDELEGATE_RESOURCE_CONTRACT.todo.md` for a fix plan/checklist.

