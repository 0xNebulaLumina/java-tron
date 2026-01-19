# Review: `FREEZE_BALANCE_CONTRACT` parity (rust-backend vs java-tron)

## TL;DR

Rust’s `FreezeBalanceContract` implementation is **very close** to Java’s `FreezeBalanceActuator` for the “happy path”:
- validation rules (amount, duration, resource, receiver)
- expiration-time computation (`latestBlockHeaderTimestamp + frozenDuration * FROZEN_PERIOD`)
- state mutations (self-freeze vs delegated-freeze) and global weight totals

However it **does not fully match Java semantics** in a few places that can matter for consensus/state parity:
1) **Missing `oldTronPower` initialization** when `ALLOW_NEW_RESOURCE_MODEL` is enabled (Java does this in `execute()` for *every* FreezeBalance).
2) **New-reward weight delta gating differs** (Java uses `ALLOW_NEW_REWARD`; Rust uses `CURRENT_CYCLE_NUMBER >= NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE`).
3) **Delegation index updates only implement the legacy (non-optimized) layout**; Java switches layouts when `ALLOW_DELEGATE_OPTIMIZATION == 1`.
4) **Malformed/edge-case validation parity gaps** (unknown enum values, `Any.is(...)` behavior).

If Rust execution is enabled and becomes authoritative for FreezeBalance, these differences can lead to state divergence vs embedded Java.

---

## Rust entrypoint + summary

Rust entrypoint:
- `rust-backend/crates/core/src/service/contracts/freeze.rs` → `BackendService::execute_freeze_balance_contract(...)`

High-level flow (Rust):
1. Parse protobuf-encoded params from `transaction.data` (V1 FreezeBalance fields 2/3/10/15)
2. Validate owner address (from `transaction.metadata.from_raw`) and load owner `Account` proto
3. Validate:
   - `frozenBalance > 0` and `>= 1 TRX`
   - `frozenCount == 0 || 1`
   - `frozenBalance <= accountBalance`
   - `MIN_FROZEN_TIME <= frozenDuration <= MAX_FROZEN_TIME`
   - resource code rules (BANDWIDTH/ENERGY always ok; TRON_POWER only when `ALLOW_NEW_RESOURCE_MODEL`)
   - receiver rules when `receiverAddress` present and `supportDR()`
   - `supportUnfreezeDelay()` closes V1 freeze
4. Compute `expireTime = latest_block_header_timestamp + frozenDuration * 86_400_000`
5. Apply either:
   - **delegated-freeze**: update `DelegatedResource`, `DelegatedResourceAccountIndex`, owner delegated balance, receiver acquired delegated balance
   - **self-freeze**: update `account.frozen[0]` (bandwidth) / `account_resource.frozen_balance_for_energy` / `account.tron_power`
6. Update global totals (`TOTAL_NET_WEIGHT`, `TOTAL_ENERGY_WEIGHT`, `TOTAL_TRON_POWER_WEIGHT`) with either “increment” or “amount/TRX_PRECISION”

---

## Java oracle behavior

Java reference:
- `actuator/src/main/java/org/tron/core/actuator/FreezeBalanceActuator.java`
  - `validate()` defines error ordering/messages and gates (duration, receiver, resource, v2-close)
  - `execute()` performs balance + freeze mutations and delegation writes
  - `delegateResource(...)` updates:
    - `DelegatedResourceStore` (key = owner||receiver, 42 bytes)
    - `DelegatedResourceAccountIndexStore` (old vs optimized layout)
    - receiver’s acquired delegated balances

Key additional Java side-effect that matters under the new resource model:
- When `supportAllowNewResourceModel()` and `oldTronPower == 0`, Java calls `initializeOldTronPower()` **before** applying the freeze mutation.
  - `AccountCapsule#initializeOldTronPower()` sets `oldTronPower = getTronPower()` (or `-1` if zero)
  - This snapshots the “legacy” tron power used by `getAllTronPower()` under the new model.

---

## Where Rust matches Java (important)

- **Amount validation**
  - positive, `>= 1 TRX`, and `<= accountBalance`
- **Frozen count constraint**
  - bandwidth freeze list length must be `0` or `1`
- **Duration validation**
  - `MIN_FROZEN_TIME <= frozenDuration <= MAX_FROZEN_TIME`
- **Resource rules**
  - BANDWIDTH/ENERGY always accepted
  - TRON_POWER accepted only when `ALLOW_NEW_RESOURCE_MODEL` is enabled; cannot be delegated
- **Receiver (delegation) validation**
  - only checked when `receiverAddress` is present and `supportDR() == true`
  - receiver must exist, must not equal owner, must not be a contract account when Constantinople is enabled
- **V1 freeze closed when unfreeze delay is enabled**
  - matches `supportUnfreezeDelay()` guard
- **State mutations**
  - self-freeze updates the same account proto fields Java updates
  - delegated-freeze updates `DelegatedResource` and owner/receiver delegated balance fields
- **Global totals updated**
  - net/energy/tron-power weight totals are incremented in the same places as Java (modulo “new reward” gating; see below)

---

## Concrete mismatches / parity risks

### 1) Missing `oldTronPower` initialization (major)

Java `execute()` does:
- if `ALLOW_NEW_RESOURCE_MODEL` && `oldTronPower == 0` → set it to snapshot of `getTronPower()` (or `-1`)

Rust `execute_freeze_balance_contract(...)` currently does **not** perform this initialization step for V1 FreezeBalance.

Why this matters:
- Under the new resource model, Java’s `getAllTronPower()` uses `oldTronPower` to decide how to compute voting power.
- Leaving `old_tron_power == 0` means Rust will compute “all tron power” differently than Java once other operations occur (votes, further freezes/unfreezes, v2 interactions).
- Rust already implements this initialization in other contract paths (e.g. V1 unfreeze / V2 flows), making V1 freeze the odd one out.

### 2) “New reward” weight delta gating differs (potentially major)

Java weight delta logic:
- `weight = dynamicStore.allowNewReward() ? increment : frozenBalance / TRX_PRECISION`
- `allowNewReward()` is `ALLOW_NEW_REWARD == 1` (no cycle compare in this codebase)

Rust currently decides “allow new reward” as:
- `CURRENT_CYCLE_NUMBER >= NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE`

This can diverge if those dynamic properties are not kept in lockstep (e.g. configurations/proposals that set effective-cycle without setting `ALLOW_NEW_REWARD`, or tests that toggle one without the other).

Impact:
- Wrong deltas applied to `TOTAL_NET_WEIGHT` / `TOTAL_ENERGY_WEIGHT` / `TOTAL_TRON_POWER_WEIGHT`.

### 3) DelegatedResourceAccountIndex layout (major when ALLOW_DELEGATE_OPTIMIZATION=1)

Java behavior in `delegateResource(...)`:
- If `supportAllowDelegateOptimization() == false`: update legacy index record at key = `address`, value = lists
- Else:
  - `convert(owner)` and `convert(receiver)` (migrate old lists → new prefix keys)
  - `delegate(owner, receiver, latestBlockHeaderTimestamp)` which writes prefix keys:
    - `0x01 || owner || receiver` and `0x02 || receiver || owner` (plus timestamp in value)

Rust V1 delegation index update:
- only implements the legacy “lists in a single value” layout
- does not implement `convert(...)` nor the `0x01/0x02` prefixed entries

Impact when optimization is enabled:
- Rust will not write the same keys/values Java expects in optimized mode.
- Subsequent unfreeze / index reads can diverge, especially because Java’s `unDelegate(...)` in optimized mode deletes only prefixed keys.

### 4) Unknown enum values and error messages (edge-case parity)

Java:
- protobuf preserves unknown enum values and `validate()` emits `ResourceCode error, valid ResourceCode[...]` messages depending on `ALLOW_NEW_RESOURCE_MODEL`.

Rust:
- `parse_freeze_balance_params(...)` returns an error on unknown `resource` values (before validation), producing a different failure string and ordering.

### 5) Missing `Any.is(...)` contract-parameter validation (edge-case parity)

Java `validate()` checks `any.is(FreezeBalanceContract.class)` and errors with a specific “contract type error…” message when the `Any` type_url is wrong.

Rust FreezeBalance does not currently validate `transaction.metadata.contract_parameter.type_url` (when present), so malformed `Any` inputs can fail differently than Java.

### 6) `checkFrozenTime` test gate (minor)

Java’s duration check is gated by `CommonParameter.checkFrozenTime == 1` (defaults to enabled).
Rust always enforces the duration bound.

This is usually only relevant for tests and custom harnesses, not mainnet behavior.

---

## Conclusion

For valid “normal” FreezeBalance V1 transactions, Rust’s implementation is close to Java and updates the same on-chain fields. But there are **several real parity gaps** (notably `oldTronPower` initialization, delegation index optimization, and new-reward gating) that can produce divergent state under certain network settings or edge-case inputs.

See `planning/review_again/FREEZE_BALANCE_CONTRACT.todo.md` for a concrete fix checklist.

