# TODO / Fix Plan: `UNFREEZE_BALANCE_CONTRACT` parity

## Goal

Make Rust `UNFREEZE_BALANCE_CONTRACT` match Java `UnfreezeBalanceActuator` semantics for:
- validation + error ordering/messages for key malformed cases (when feasible)
- state mutations (self-unfreeze + delegated-unfreeze)
- dynamic property side-effects (global weight totals)
- Java side-effects that happen *inside* `execute()` (notably `withdrawReward`)

Primary Java oracles to match:
- `actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceActuator.java` (`validate()`, `execute()`)
- `chainbase/src/main/java/org/tron/core/service/MortgageService.java` (`withdrawReward`)
- `chainbase/src/main/java/org/tron/core/store/DelegatedResourceAccountIndexStore.java` (optimized index layout + `convert/unDelegate`)
- `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java` (`allowNewReward()`, `supportAllowDelegateOptimization()`)

---

## Checklist (tactical)

- [ ] Confirm parity scope (what “matching Java” means here)
  - [ ] Decide whether Rust must match Java only for valid txs, or also for malformed tx error strings/order (conformance).
  - [ ] Decide whether Rust execution is authoritative (must fully validate) vs best-effort (Java already validated before calling Rust).

- [ ] Implement `MortgageService.withdrawReward(ownerAddress)` side-effects for UnfreezeBalance (major)
  - [ ] Match Java ordering: call withdrawReward **before** applying the unfreeze mutation (Java does it at the start of `execute()`).
  - [ ] Rust touchpoints
    - [ ] `rust-backend/crates/core/src/service/contracts/freeze.rs` (`execute_unfreeze_balance_contract(...)`)
    - [ ] `rust-backend/crates/core/src/service/contracts/delegation.rs` (`withdraw_reward(...)`)
  - [ ] Implementation approach
    - [ ] Call `delegation::withdraw_reward(storage_adapter, &owner)` unconditionally (it already no-ops when `allowChangeDelegation == false`).
    - [ ] If returned `reward > 0`, apply Java’s `adjustAllowance(owner, reward)` effect:
      - [ ] load/update `Account.allowance` in the owner proto
      - [ ] check overflow/underflow like Java (`allowance + reward`)
      - [ ] persist updated owner account (same write model as other account proto writes)
    - [ ] Ensure delegation-store begin/end cycle state + `accountVote` snapshots are persisted exactly once (the withdraw_reward port already writes these).
  - [ ] Tests / verification
    - [ ] Add a regression test/fixture where:
      - [ ] `allowChangeDelegation == true`
      - [ ] the account has votes and non-zero reward over cycles
      - [ ] UnfreezeBalance updates `Account.allowance` and delegation-store cycle state exactly like Java.
    - [ ] Ensure no behavior change when `allowChangeDelegation == false`.

- [ ] Align “new reward” gating with Java’s `ALLOW_NEW_REWARD` (potentially major)
  - [ ] Java uses `dynamicStore.allowNewReward()` which is `ALLOW_NEW_REWARD == 1`
  - [ ] Rust currently uses `CURRENT_CYCLE_NUMBER >= NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE`
  - [ ] Rust touchpoints
    - [ ] Add a dynamic property getter in `rust-backend/crates/execution/src/storage_adapter/engine.rs` for `ALLOW_NEW_REWARD`
    - [ ] Use that flag in `execute_unfreeze_balance_contract(...)` to decide between:
      - [ ] `weight_delta = decrease` (new reward enabled)
      - [ ] `weight_delta = -(unfreeze_amount / TRX_PRECISION)` (legacy)
  - [ ] Verification
    - [ ] Add a unit/regression test where `ALLOW_NEW_REWARD=0` but `NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE` would be “active”, and confirm weight deltas match Java.

- [ ] Implement `ALLOW_DELEGATE_OPTIMIZATION` branch for V1 delegated unfreeze cleanup (major when enabled)
  - [ ] Add dynamic property getter:
    - [ ] `ALLOW_DELEGATE_OPTIMIZATION` (used by Java’s `supportAllowDelegateOptimization()`)
  - [ ] Match Java’s two modes when delegated resource becomes fully zeroed:
    - [ ] If optimization **disabled**:
      - [ ] keep legacy behavior (remove entries from lists stored at key=`address`)
    - [ ] If optimization **enabled**:
      - [ ] implement `convert(address)` (migrate legacy list key → prefixed keys) to match Java’s store behavior
      - [ ] delete prefixed keys for this pair (`unDelegate(from,to)`):
        - [ ] `0x01 || from21 || to21`
        - [ ] `0x02 || to21 || from21`
  - [ ] Rust touchpoints
    - [ ] `rust-backend/crates/execution/src/storage_adapter/engine.rs`
      - [ ] add helpers for:
        - [ ] computing prefixed v1 index keys
        - [ ] `convert_delegated_resource_account_index_v1(...)`
        - [ ] `un_delegate_resource_account_index_v1_optimized(...)`
    - [ ] `rust-backend/crates/core/src/service/contracts/freeze.rs`
      - [ ] branch the index-store cleanup based on `ALLOW_DELEGATE_OPTIMIZATION`
  - [ ] Verification
    - [ ] With `ALLOW_DELEGATE_OPTIMIZATION=1`, assert Rust deletes the prefixed keys and does not leave stale legacy records.

- [ ] Preserve Java behavior for unknown `resource` values (edge-case parity)
  - [ ] Change parsing so unknown enum values do not fail early.
    - [ ] Parse `resource` as raw integer; defer validation to match Java switch/default behavior.
  - [ ] Emit Java-equivalent error strings in `validate()`-equivalent checks:
    - [ ] new resource model disabled: `ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]`
    - [ ] new resource model enabled: `ResourceCode error.valid ResourceCode[BANDWIDTH、Energy、TRON_POWER]`

- [ ] Implement `Any.is(...)`-equivalent validation for UnfreezeBalance (edge-case parity)
  - [ ] If `transaction.metadata.contract_parameter` is present:
    - [ ] check `type_url` matches the expected proto type for UnfreezeBalanceContract
    - [ ] if not, return Java’s “contract type error…” message

---

## Verification / rollout checklist

- [ ] `cargo test` under `rust-backend/` with new regression tests
- [ ] Run the existing conformance fixtures for `unfreeze_balance_contract/*` with Rust enabled:
  - [ ] verify error strings for validate-fail fixtures
  - [ ] verify account + delegated resource mutations for happy-path fixtures
- [ ] Run a small remote-vs-embedded parity slice including a case where `withdrawReward` is non-zero
- [ ] Keep `execution.remote.unfreeze_balance_enabled` gated until parity is confirmed

