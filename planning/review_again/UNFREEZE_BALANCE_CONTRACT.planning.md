# Review: `UNFREEZE_BALANCE_CONTRACT` parity (rust-backend vs java-tron)

## TL;DR

Rust’s `UNFREEZE_BALANCE_CONTRACT` implementation is **close** to Java’s `UnfreezeBalanceActuator` for the core mechanics:
- validation gates for self-unfreeze vs delegated-unfreeze
- state mutations of `Account` frozen fields and delegated resource records
- receiver acquired-delegation updates (incl. Constantinople / Solidity059 special-cases)
- global weight deltas (`TOTAL_NET_WEIGHT` / `TOTAL_ENERGY_WEIGHT` / `TOTAL_TRON_POWER_WEIGHT`)
- vote clearing + `oldTronPower` init/invalidation under the new resource model

But it **does not fully match Java semantics** in a few places that can matter for state parity:
1) **Missing `MortgageService.withdrawReward(ownerAddress)` side-effects** (delegation reward → `Account.allowance` + delegation-store cycle state).
2) **“new reward” gating differs** (Java uses `ALLOW_NEW_REWARD`; Rust uses `CURRENT_CYCLE_NUMBER >= NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE`).
3) **Delegation index updates only implement the legacy (non-optimized) layout**; Java switches layouts when `ALLOW_DELEGATE_OPTIMIZATION == 1`.
4) **Edge-case validation parity gaps** (unknown enum values; missing `Any.is(...)`/`type_url` checks).

If Rust execution is enabled and becomes authoritative for UnfreezeBalance, (1) is the biggest consensus/state mismatch.

---

## Rust entrypoint + summary

Rust entrypoint:
- `rust-backend/crates/core/src/service/contracts/freeze.rs` → `BackendService::execute_unfreeze_balance_contract(...)`

High-level flow (Rust):
1. Parse `resource` (field 10) and optional `receiver_address` (field 15) from protobuf-encoded `transaction.data`
2. Validate owner address (from `transaction.metadata.from_raw`) and load owner `Account` proto
3. Read key dynamic properties:
   - `latest_block_header_timestamp` as “now”
   - `ALLOW_NEW_RESOURCE_MODEL`, `supportDR()`
   - `ALLOW_TVM_CONSTANTINOPLE`, `ALLOW_TVM_SOLIDITY_059`
   - “new reward” gating (see mismatch section)
4. Initialize `old_tron_power` when new resource model is enabled and it is `0`
5. Execute either:
   - **delegated-unfreeze** (when `receiver_address` present and `supportDR()`): mutate `DelegatedResource` + owner delegated frozen balances + receiver acquired delegated balances
   - **self-unfreeze**: mutate `account.frozen[]` (bandwidth) / `account_resource.frozen_balance_for_energy` / `account.tron_power`
6. Update global totals (`TOTAL_NET_WEIGHT` / `TOTAL_ENERGY_WEIGHT` / `TOTAL_TRON_POWER_WEIGHT`)
7. Clear votes conditionally (`needToClearVote`) and invalidate `old_tron_power`
8. Persist updated account proto(s) + delegated resource store changes

Note: Rust also updates a Rust-side freeze ledger record store and optionally emits sidecar changes; those are not part of Java’s on-chain DB layout.

---

## Java oracle behavior

Primary Java reference:
- `actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceActuator.java`
  - `validate()` defines error ordering/messages and all gate conditions
  - `execute()` performs:
    - `mortgageService.withdrawReward(ownerAddress)` (delegation rewards)
    - account + delegation mutations
    - global totals updates
    - vote clearing + oldTronPower invalidation

Key supporting Java references:
- `chainbase/src/main/java/org/tron/core/service/MortgageService.java` (`withdrawReward`)
- `chainbase/src/main/java/org/tron/core/store/DelegatedResourceAccountIndexStore.java` (optimized index layout + `convert/unDelegate`)
- `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java` (`allowNewReward()`, `supportDR()`, `supportAllowDelegateOptimization()`)

---

## Where Rust matches Java (important)

- **Owner + receiver validation**
  - owner address prefix/length validation (`Invalid address`)
  - delegated path receiver constraints:
    - receiver != owner (`receiverAddress must not be the same as ownerAddress`)
    - receiver address valid (`Invalid receiverAddress`)
    - receiver existence gated by `ALLOW_TVM_CONSTANTINOPLE == 0`
- **Delegated resource checks**
  - `delegated Resource does not exist`
  - “no delegated frozen balance” per resource:
    - bandwidth: `no delegatedFrozenBalance(BANDWIDTH)`
    - energy: `no delegateFrozenBalance(Energy)`
  - expiry-time gating and the shared message: `It's not time to unfreeze.`
  - acquired delegated balance validation with Constantinople / Solidity059 conditions
- **Self-unfreeze checks**
  - bandwidth: requires at least one expired frozen entry; unfreezes all `expireTime <= now`
  - energy: requires `frozen_balance_for_energy > 0` and `expireTime <= now`
  - tron power: allowed only when `ALLOW_NEW_RESOURCE_MODEL` is enabled; expiry-time gating matches Java
- **State mutations**
  - adds unfreeze amount back to `Account.balance`
  - clears the correct frozen fields
  - clears votes and persists a votes record when required
  - snapshots/invalidates `oldTronPower` similarly to Java’s `initializeOldTronPower()` / `invalidateOldTronPower()`
- **Global totals updated**
  - net/energy/tron-power totals are updated in the same places as Java (modulo “new reward” gating; see below)

---

## Concrete mismatches / parity risks

### 1) Missing `MortgageService.withdrawReward(ownerAddress)` (major)

Java `execute()` begins with:
- `mortgageService.withdrawReward(ownerAddress);`

This is not a no-op:
- it updates `DelegationStore` begin/end cycle state and `accountVote` snapshots
- it adjusts `Account.allowance` (witness/delegation reward) via `adjustAllowance(...)`

Rust `execute_unfreeze_balance_contract(...)` does not call the Rust port of this logic:
- `rust-backend/crates/core/src/service/contracts/delegation.rs` contains a `withdraw_reward(...)` port (used by `WITHDRAW_BALANCE_CONTRACT`), but UnfreezeBalance never calls it nor updates `Account.allowance`.

Impact:
- account allowance + delegation-store state will diverge vs Java after UnfreezeBalance when delegation rewards are enabled on-chain (`allowChangeDelegation == true`).

### 2) “New reward” gating differs (potentially major)

Java:
- `weight = dynamicStore.allowNewReward() ? decrease : -unfreezeBalance / TRX_PRECISION`
- `allowNewReward()` is `ALLOW_NEW_REWARD == 1`

Rust:
- computes “allow new reward” as `CURRENT_CYCLE_NUMBER >= NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE`

Impact:
- wrong weight deltas applied to `TOTAL_*_WEIGHT` if those dynamic properties are not strictly equivalent in a given DB / fixture.

### 3) DelegatedResourceAccountIndex layout (major when ALLOW_DELEGATE_OPTIMIZATION=1)

Java behavior in UnfreezeBalance delegated path:
- if `supportAllowDelegateOptimization() == false`: update legacy index record at key = `address`, value = lists (remove owner/receiver from lists)
- else:
  - `convert(owner)` and `convert(receiver)` (migrate old lists → new prefix keys)
  - `unDelegate(owner, receiver)` which deletes prefixed keys:
    - `0x01 || owner || receiver` and `0x02 || receiver || owner`

Rust delegated-unfreeze cleanup:
- always calls `delete_delegated_resource_v1(...)` + `undelegate_resource_account_index_v1(...)`
- `undelegate_resource_account_index_v1(...)` explicitly implements only the legacy (non-optimized) layout

Impact:
- in optimized mode, Rust will not delete the same keys Java deletes, and the index store can diverge.

### 4) Unknown enum values + `Any.is(...)` validation (edge-case parity)

- Java preserves unknown enum values and routes them through `validate()` switch/default logic (with resource-code error messages that depend on `ALLOW_NEW_RESOURCE_MODEL`).
- Rust rejects unknown resource values during protobuf parsing (`Invalid resource code: ...`), producing different error strings/order.
- Java also validates `Any.is(UnfreezeBalanceContract.class)`; Rust does not validate `transaction.metadata.contract_parameter.type_url` when present.

These are mostly relevant for malformed-fixture parity, not normal mainnet txs.

---

## Conclusion

For valid UnfreezeBalance V1 transactions, Rust’s implementation tracks Java closely for the on-chain frozen-balance mechanics and most gating conditions. However, it is **not fully equivalent** today: the missing `withdrawReward` side-effects and the delegation index optimization branch are real parity gaps, and the “new reward” gating can also diverge depending on dynamic property settings.

See `planning/review_again/UNFREEZE_BALANCE_CONTRACT.todo.md` for a concrete fix checklist.

