# Review: `WITHDRAW_BALANCE_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_withdraw_balance_contract()` in `rust-backend/crates/core/src/service/contracts/withdraw.rs`
  - Delegation reward port used by the handler: `delegation::withdraw_reward()` in `rust-backend/crates/core/src/service/contracts/delegation.rs`
- **Java reference**: `WithdrawBalanceActuator` in `actuator/src/main/java/org/tron/core/actuator/WithdrawBalanceActuator.java`
  - Reward logic called by the actuator: `MortgageService.withdrawReward/queryReward` in `chainbase/src/main/java/org/tron/core/service/MortgageService.java`

Goal: determine whether the Rust implementation matches java-tron’s **validation + state transition** semantics for `WithdrawBalanceContract`, and call out any mismatches that could affect conformance or consensus state.

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`WithdrawBalanceActuator.validate`)

Key checks (and messages), in order:

1. Contract presence / stores presence:
   - `any == null` → `"No contract!"`
   - `chainBaseManager == null` → `"No account store or dynamic store!"`
2. Contract type + unpack:
   - `any.is(WithdrawBalanceContract.class)` else:
     - `"contract type error, expected type [WithdrawBalanceContract], real type[...]"`.
   - `any.unpack(...)` else: exception message
3. Address:
   - `DecodeUtil.addressValid(ownerAddress)` else: `"Invalid address"`
4. Account exists:
   - else `"Account[<hex>] not exists"`
5. Guard representative restriction:
   - owner is one of `CommonParameter.getInstance().getGenesisBlock().getWitnesses()` addresses →
     - `"Account[<hex>] is a guard representative and is not allowed to withdraw Balance"`
6. Cooldown:
   - `latestWithdrawTime = accountCapsule.getLatestWithdrawTime()`
   - `now = dynamicStore.getLatestBlockHeaderTimestamp()`
   - `cooldownMs = dynamicStore.getWitnessAllowanceFrozenTime() * FROZEN_PERIOD`
   - if `now - latestWithdrawTime < cooldownMs` →
     - `"The last withdraw time is <latestWithdrawTime>, less than 24 hours"`
7. Reward existence:
   - if `accountCapsule.getAllowance() <= 0 && mortgageService.queryReward(ownerAddress) <= 0` →
     - `"witnessAccount does not have any reward"`
8. Overflow check:
   - `LongMath.checkedAdd(balance, allowance)`; if it throws →
     - error message (typically `"overflow: checkedAdd(<balance>, <allowance>)"`)

Notes:

- There is **no explicit “must be witness” check** in this repo’s `WithdrawBalanceActuator` (despite the wording in error messages and some older comments/tests).
- `queryReward(owner)` returns **`reward + allowance`** when delegation rewards are enabled; it is non-mutating.

### 2) Execution (`WithdrawBalanceActuator.execute`)

State transition (fee is always `0`):

1. `mortgageService.withdrawReward(ownerAddress)` (mutates delegation store and may increase `Account.allowance`).
2. Reload `AccountCapsule` and read:
   - `oldBalance = account.getBalance()`
   - `allowance = account.getAllowance()` (this includes the reward added by `withdrawReward`)
3. `now = dynamicStore.getLatestBlockHeaderTimestamp()`
4. Persist account update:
   - `balance = oldBalance + allowance`
   - `allowance = 0`
   - `latestWithdrawTime = now`
5. Receipt:
   - `ret.setWithdrawAmount(allowance)`
   - `ret.setStatus(0, SUCESS)`

---

## Rust implementation behavior (what it currently does)

`execute_withdraw_balance_contract()`:

### 1) Validation-like checks (within the handler)

- Owner address validity:
  - uses `transaction.metadata.from_raw` as the canonical TRON address bytes
  - checks `len == 21` and `from_raw[0] == storage_adapter.address_prefix()`
  - error: `"Invalid address"`
- Owner account exists:
  - uses `storage_adapter.get_account(&transaction.from)` (20-byte EVM address)
  - error: `"Account[<hex>] not exists"` (hex is from `from_raw`)
- Guard representative restriction:
  - `is_genesis_guard_representative(from_raw, prefix)` blocks owners in a **hardcoded** mainnet/testnet genesis list
  - error: `"Account[<hex>] is a guard representative and is not allowed to withdraw Balance"`
- Cooldown:
  - `now_ms = storage_adapter.get_latest_block_header_timestamp()`
  - `witnessAllowanceFrozenDays = storage_adapter.get_witness_allowance_frozen_time()`
  - `latestWithdrawTime = storage_adapter.get_account_latest_withdraw_time(owner)`
  - checks `now_ms - latestWithdrawTime < witnessAllowanceFrozenDays * 86_400_000`
  - error: `"The last withdraw time is <latestWithdrawTime>, less than 24 hours"`
- Reward existence:
  - optionally computes `delegation_reward` (see next section)
  - reads `base_allowance = storage_adapter.get_account_allowance(owner)`
  - checks `base_allowance <= 0 && delegation_reward <= 0`
  - error: `"witnessAccount does not have any reward"`
- Overflow:
  - checks `checked_add(old_balance, base_allowance)` first and returns
    - `"overflow: checkedAdd(<balance>, <allowance>)"`
  - also checks `checked_add(base_allowance, delegation_reward)` and `checked_add(balance, total_allowance)` and returns Rust-specific errors if they overflow.

### 2) Delegation reward handling

Controlled by config:

- If `execution.remote.delegation_reward_enabled` is **false**:
  - `delegation_reward = 0` (Phase 1 behavior: allowance-only)
- If **true**:
  - calls `delegation::withdraw_reward(storage_adapter, owner)`
  - this is a port of `MortgageService.withdrawReward()`:
    - updates delegation begin/end cycle and vote snapshots in the delegation store
    - returns the reward amount (in SUN) so the withdraw handler can include it in the withdrawal

### 3) Execution/state updates

- Computes `allowance = base_allowance + delegation_reward`
- Updates the owner’s balance by `+allowance` via `storage_adapter.set_account(...)`.
- Allows two write models:
  - Always emits a `WithdrawChange { owner, amount, latest_withdraw_time }` sidecar for Java apply.
  - If `execution.remote.rust_persist_enabled` is true, it also directly writes to the Account proto:
    - `allowance = 0`
    - `latest_withdraw_time = now_ms`
- Receipt passthrough:
  - emits `tron_transaction_result` bytes with `withdraw_amount = allowance` (field 15).
- Resource accounting:
  - `energy_used = 0`
  - `bandwidth_used = calculate_bandwidth_usage(transaction)` (Rust-side estimate; Java bandwidth is handled outside the actuator by `BandwidthProcessor`)

---

## Does it match java-tron?

### What matches (good parity)

- **Validation messages and ordering** for the conformance-relevant checks:
  - `"Invalid address"`, `"Account[<hex>] not exists"`, guard rep rejection, cooldown rejection, no-reward rejection, and `LongMath.checkedAdd` overflow messaging are aligned.
- **Cooldown semantics**:
  - uses `latest_block_header_timestamp` and `WITNESS_ALLOWANCE_FROZEN_TIME * 86_400_000` with the same `<` boundary rule.
- **Core state transition**:
  - final intended embedded state (`balance += allowance`, `allowance = 0`, `latestWithdrawTime = now`) is represented via `AccountChange + WithdrawChange` (and directly persisted when `rust_persist_enabled` is enabled).
- **Receipt field parity**:
  - `withdraw_amount` is carried through `tron_transaction_result` bytes, mirroring `ret.setWithdrawAmount(...)`.

### Where it diverges (real mismatches / risk areas)

1. **Guard representative list is hardcoded in Rust**

- Java: guard reps are read from the active node config (`CommonParameter.genesisBlock.witnesses`).
- Rust: uses embedded lists in `rust-backend/crates/core/src/service/contracts/withdraw.rs` and picks mainnet vs testnet solely by detected address prefix.

Impact: running with a **custom genesis witness list** (private nets, altered configs, forks) can diverge: Java would block different addresses than Rust.

2. **Delegation reward inclusion is config-gated in Rust**

- Java always calls `MortgageService.withdrawReward()` in `execute()` (it self-gates on `allowChangeDelegation()` from dynamic properties).
- Rust only computes delegation rewards if `execution.remote.delegation_reward_enabled` is enabled.

Impact: with `delegation_reward_enabled=false`, Rust withdraws **only** `Account.allowance` even when Java would also include vote rewards.

3. **No “contract type/unpack” parity in Rust**

- Java validation checks `any.is(WithdrawBalanceContract.class)` and throws a specific `"contract type error ..."` message on mismatch/unpack failure.
- Rust does not validate `transaction.metadata.contract_parameter.type_url` nor decode the protobuf; it implicitly trusts `transaction.from`.

Impact: malformed/mismatched requests (or future fixture cases) could yield different error messages/behavior.

4. **No-reward check is slightly weaker than Java in pathological states**

- Java’s condition is effectively based on `queryReward(owner) = allowance + reward`, so it rejects cases where **total** withdrawable reward is `<= 0`.
- Rust checks `base_allowance <= 0 && delegation_reward <= 0`, which is equivalent only if both are guaranteed non-negative.

Impact: if `Account.allowance` can become **negative** (should not happen in normal operation), Rust could accept a transaction where `base_allowance + delegation_reward <= 0`, producing a negative “withdrawal” and decreasing balance; Java would reject.

5. **Bandwidth accounting is not java-tron equivalent**

- Java’s bandwidth consumption is computed outside the actuator (tx-size based, resource-window based).
- Rust reports `bandwidth_used` via a simplified estimator used by other system-contract handlers.

Impact: receipt/resource reports (and any AEXT “tracked” mode accounting) won’t match embedded `BandwidthProcessor` behavior.

---

## Bottom line

For the common/mainnet+testnet paths (standard prefixes, standard genesis witness lists) and with `delegation_reward_enabled=true`, Rust’s `WITHDRAW_BALANCE_CONTRACT` implementation is **very close** to java-tron’s validation + state transition semantics, including the expected error strings and receipt `withdraw_amount`.

It is **not fully equivalent** in three meaningful edge dimensions: (1) guard rep selection is not config-driven, (2) delegation reward inclusion is optional via config, and (3) the no-reward predicate can differ if allowance/reward invariants are violated.

