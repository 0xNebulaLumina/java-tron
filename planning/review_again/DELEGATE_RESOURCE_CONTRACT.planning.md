# Review: `DELEGATE_RESOURCE_CONTRACT` parity (rust-backend vs java-tron)

## TL;DR

The Rust implementation is structurally very close to Java’s `DelegateResourceActuator` for:
- gating (`supportDR`, `supportUnfreezeDelay`)
- address/account validation
- lock-period validation
- state mutations (owner/receiver account fields, `DelegatedResource*` stores, unlock-expired behavior)

But it **does not fully match Java validation semantics** for the most important rule: **available FreezeV2 balance is reduced by current resource usage** (net/energy usage), and Java enforces delegation against that “available” amount. Rust currently checks only the raw `frozenV2` sum.

Result: Rust can accept delegations that Java would reject (and therefore diverge if Rust becomes authoritative for this contract).

---

## What the Rust side does today (summary)

Rust entrypoint:
- `rust-backend/crates/core/src/service/mod.rs` → `execute_delegate_resource_contract(...)`

High-level flow:
1. Gate checks: `support_dr()` and `support_unfreeze_delay()`
2. Validate owner address from `transaction.metadata.from_raw` and load owner account
3. Validate `delegateBalance >= 1 TRX`
4. Validate resource is BANDWIDTH/ENERGY and **only** check `sum(frozen_v2[type]) >= delegateBalance`
5. Validate receiver address and existence, receiver != owner, receiver is not a contract account
6. Lock-period computation and max-lock / remaining-time checks
7. Apply mutations:
   - owner: `delegated_frozen_v2_balance_for_* += balance`, `frozen_v2[*] -= balance`
   - receiver: `acquired_delegated_frozen_v2_balance_for_* += balance`
   - update DelegatedResource record (+ unlock expired lock record first via storage adapter)
   - update DelegatedResourceAccountIndex

Storage adapter parity detail:
- `rust-backend/crates/execution/src/storage_adapter/engine.rs` → `delegate_resource(...)` calls
  `unlock_expired_delegated_resource(...)`, matching Java’s `DelegatedResourceStore.unLockExpireResource(...)`.

---

## Java side oracle behavior

Java reference:
- `actuator/src/main/java/org/tron/core/actuator/DelegateResourceActuator.java`
  - `validate()` performs all checks (including “available FreezeV2” logic).
  - `execute()` performs the state mutations.

The key difference is in validation of “available” frozen v2:

### BANDWIDTH (Java)

In `validate()`:
1. `BandwidthProcessor.updateUsageForDelegated(ownerCapsule)` (recovers `netUsage` to “now”)
2. Compute `netUsage` in SUN units using global totals:
   - `netUsage = (long) (accountNetUsage * TRX_PRECISION * (totalNetWeight / totalNetLimit))`
3. Convert to *V2* usage:
   - `v2NetUsage = FreezeV2Util.getV2NetUsage(ownerCapsule, netUsage, ...)`
   - `getV2NetUsage` subtracts V1 + acquired delegated amounts:
     - `netUsage - frozenBalanceV1 - acquiredDelegatedFrozenV1 - acquiredDelegatedFrozenV2`
4. Enforce:
   - `frozenV2BalanceForBandwidth - v2NetUsage >= delegateBalance`

Source of `getV2NetUsage`:
- `actuator/src/main/java/org/tron/core/vm/utils/FreezeV2Util.java` (`getV2NetUsage`)

### ENERGY (Java)

In `validate()`:
1. `EnergyProcessor.updateUsage(ownerCapsule)` (recovers `energyUsage` to “now”)
2. Compute scaled usage using global totals:
   - `energyUsage = (long) (accountEnergyUsage * TRX_PRECISION * (totalEnergyWeight / totalEnergyCurrentLimit))`
3. Convert to V2 usage:
   - `v2EnergyUsage = FreezeV2Util.getV2EnergyUsage(...)`
4. Enforce:
   - `frozenV2BalanceForEnergy - v2EnergyUsage >= delegateBalance`

---

## Concrete mismatch (why it is not equivalent)

### 1) Missing “available FreezeV2” computation (major)

Rust checks only:
- `sum(frozen_v2[type]) >= delegateBalance`

Java checks:
- `sum(frozen_v2[type]) - v2{Net,Energy}Usage >= delegateBalance`

So if an account has significant `net_usage` / `energy_usage`, Java reduces the delegatable amount; Rust does not.

Rust code path for this check:
- `rust-backend/crates/core/src/service/mod.rs` → uses
  `get_frozen_v2_balance_for_bandwidth(...)` / `get_frozen_v2_balance_for_energy(...)`
  which are simple sums over `account.frozen_v2`.

### 2) Bandwidth "tx create" estimate (minor, but still parity-relevant)

Java's BANDWIDTH validation has a special case:
- If `tx.isTransactionCreate()`, it increases `accountNetUsage` by
  `TransactionUtil.estimateConsumeBandWidthSize(...)` before computing `netUsage`.

Rust has no equivalent "transaction create" flag nor the estimate.

**Resolution (2026-02-10)**: This is an **intentional omission** - no parity issue.
- `isTransactionCreate = true` is only set during API-time validation in `Wallet.createTransactionCapsule()`
- After validation, it's immediately set back to `false` (Wallet.java:484)
- Rust handles **in-block execution only** - transactions come from blocks, not API
- For in-block transactions, `isTransactionCreate` is always `false`
- The estimate is a **pre-broadcast validation** feature to prevent users from delegating their entire frozen balance when they need some to pay for the delegation tx itself
- By the time Rust executes (in-block), bandwidth is charged separately by Java's bandwidth processor
- **No code changes needed in Rust**

### 3) Owner address source (parity risk)

~~Java validates `ownerAddress` from the protobuf contract (`DelegateResourceContract.owner_address`).~~

~~Rust ignores the contract's field 1 (owner) and instead uses:~~
~~- `transaction.metadata.from_raw`~~

~~This is likely equivalent in the remote path (Java populates `from_raw` consistently), but it's still a divergence:~~
~~if `from_raw` and contract owner ever disagree, Rust and Java will not validate the same address.~~

**Resolution (2026-02-10)**: This parity issue has been fixed.
- Rust now parses `owner_address` from `DelegateResourceContract` protobuf (field 1) in `parse_delegate_resource_contract()`
- The `DelegateResourceInfo` struct now includes `owner_address` field
- `execute_delegate_resource_contract()` uses `delegate_info.owner_address` instead of `transaction.metadata.from_raw`
- Tests updated to include `owner_address` in protobuf data

---

## Conclusion

~~The Rust implementation matches Java for most *structural* logic and state updates, but it **does not match the Java-side validation that constrains delegation by "available FreezeV2 after usage"**.~~

~~If Rust execution is enabled and used as the source of truth (or if Java's pre-validation is bypassed), this mismatch can cause acceptance of invalid transactions or state divergence.~~

**Update (2026-02-10)**: All major issues have been addressed:
1. ✅ **"available FreezeV2" validation** - Implemented in Rust with `compute_available_freeze_v2_bandwidth()` and `compute_available_freeze_v2_energy()` functions
2. ✅ **Bandwidth "tx create" estimate** - Resolved as intentional omission (Rust handles in-block execution only, where `isTransactionCreate = false`)
3. ✅ **Owner address source** - Fixed to use contract's `owner_address` field (field 1) from DelegateResourceContract protobuf, matching Java's `DelegateResourceActuator.getOwnerAddress()`

See `planning/review_again/DELEGATE_RESOURCE_CONTRACT.todo.md` for the full checklist and implementation details.

