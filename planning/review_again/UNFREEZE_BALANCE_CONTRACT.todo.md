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

- [x] Confirm parity scope (what "matching Java" means here)
  - [x] Decide whether Rust must match Java only for valid txs, or also for malformed tx error strings/order (conformance).
    - Rust matches Java for both valid txs and malformed tx error strings (already implemented).
  - [x] Decide whether Rust execution is authoritative (must fully validate) vs best-effort (Java already validated before calling Rust).
    - Rust fully validates, matching Java's error strings and ordering.

- [x] Implement `MortgageService.withdrawReward(ownerAddress)` side-effects for UnfreezeBalance (major)
  - [x] Match Java ordering: call withdrawReward **before** applying the unfreeze mutation (Java does it at the start of `execute()`).
  - [x] Rust touchpoints
    - [x] `rust-backend/crates/core/src/service/contracts/freeze.rs` (`execute_unfreeze_balance_contract(...)`)
    - [x] `rust-backend/crates/core/src/service/contracts/delegation.rs` (`withdraw_reward(...)`) — already existed
  - [x] Implementation approach
    - [x] Call `self.compute_delegation_reward_if_enabled(storage_adapter, &owner)` (reuses withdraw.rs helper, gated by `delegation_reward_enabled` config).
    - [x] If returned `reward > 0`, apply Java's `adjustAllowance(owner, reward)` effect:
      - [x] load/update `Account.allowance` in the owner proto via `new_owner_proto.allowance += reward`
      - [x] check overflow via `checked_add`
      - [x] persist updated owner account (same write model as other account proto writes — single put at end)
    - [x] Ensure delegation-store begin/end cycle state + `accountVote` snapshots are persisted exactly once (the withdraw_reward port already writes these).
  - [x] Tests / verification
    - [x] Gated by `delegation_reward_enabled` config flag — no behavior change when flag is false.
    - [x] `cargo test --workspace` passes (342 passed, 3 pre-existing failures unrelated).
    - [x] Regression test `test_unfreeze_balance_withdraw_reward_updates_allowance`: verifies allowance += delegation_reward and delegation store cycle state updates.
    - [x] Regression test `test_unfreeze_balance_no_reward_when_delegation_disabled`: verifies no behavior change when CHANGE_DELEGATION=0.

- [x] Align "new reward" gating with Java's `ALLOW_NEW_REWARD` (potentially major)
  - [x] Java uses `dynamicStore.allowNewReward()` which is `ALLOW_NEW_REWARD == 1`
  - [x] Rust already reads `ALLOW_NEW_REWARD` via `storage_adapter.allow_new_reward()` (engine.rs:2841) — was already aligned.
  - [x] Rust touchpoints
    - [x] Dynamic property getter `allow_new_reward()` already exists in engine.rs (returns `get_allow_new_reward() == 1`).
    - [x] `execute_unfreeze_balance_contract` already uses it correctly for weight_delta selection.
  - [x] Additional fix: Added weight clamping to `add_total_net_weight`, `add_total_energy_weight`, `add_total_tron_power_weight` in engine.rs.
    - [x] Java clamps `max(0, new_value)` when `allowNewReward()` is true — Rust now matches.
    - [x] Also added Java's `if (amount == 0) return` skip optimization.
  - [x] Regression test `test_unfreeze_balance_weight_clamping_with_allow_new_reward`: verifies clamping to 0 with ALLOW_NEW_REWARD=1 and negative total with ALLOW_NEW_REWARD=0.

- [x] Implement `ALLOW_DELEGATE_OPTIMIZATION` branch for V1 delegated unfreeze cleanup (major when enabled)
  - [x] Add dynamic property getter:
    - [x] `ALLOW_DELEGATE_OPTIMIZATION` already existed (`support_allow_delegate_optimization()` in engine.rs).
  - [x] Match Java's two modes when delegated resource becomes fully zeroed:
    - [x] If optimization **disabled**:
      - [x] Legacy behavior via `undelegate_resource_account_index_v1()` — already implemented.
    - [x] If optimization **enabled**:
      - [x] `convert_delegated_resource_account_index_v1()` called for both owner and receiver — **newly added**.
      - [x] `undelegate_v1_optimized()` deletes prefixed keys (`0x01 || from21 || to21`, `0x02 || to21 || from21`) — already implemented.
  - [x] Rust touchpoints
    - [x] `rust-backend/crates/execution/src/storage_adapter/engine.rs`
      - [x] Helpers already existed: `convert_delegated_resource_account_index_v1()` (line 4266), `undelegate_v1_optimized()` (line 4372).
    - [x] `rust-backend/crates/core/src/service/contracts/freeze.rs`
      - [x] Added `convert()` calls before `undelegate_v1_optimized()` to match Java's `convert(owner) + convert(receiver) + unDelegate(owner, receiver)` pattern.
  - [x] Regression test `test_unfreeze_delegated_optimized_deletes_prefixed_keys`: verifies prefixed keys are deleted and no stale legacy records remain after delegated unfreeze with ALLOW_DELEGATE_OPTIMIZATION=1.

- [x] Preserve Java behavior for unknown `resource` values (edge-case parity)
  - [x] Already implemented: Unknown resource values handled via `FreezeResource::Unknown` variant.
  - [x] Emit Java-equivalent error strings:
    - [x] new resource model disabled: `ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]`
    - [x] new resource model enabled: `ResourceCode error.valid ResourceCode[BANDWIDTH、Energy、TRON_POWER]`

- [x] Implement `Any.is(...)`-equivalent validation for UnfreezeBalance (edge-case parity)
  - [x] Already implemented: `type_url.ends_with("UnfreezeBalanceContract")` check at the start of `execute_unfreeze_balance_contract`.

---

## Verification / rollout checklist

- [x] `cargo test` under `rust-backend/` with new regression tests
  - 342 passed (4 new tests added), 3 pre-existing failures (vote_witness tests, unrelated), 3 ignored.
- [x] Run the existing conformance fixtures for `unfreeze_balance_contract/*` with Rust enabled:
  - 3 new conformance fixtures added and all pass:
    - `edge_withdraw_reward_updates_allowance` — verifies withdrawReward + allowance parity
    - `edge_weight_clamping_with_allow_new_reward` — verifies weight clamping to 0 with ALLOW_NEW_REWARD=1
    - `edge_delegated_unfreeze_with_optimization` — verifies delegated resource cleanup with ALLOW_DELEGATE_OPTIMIZATION=1
  - Fix: `AccountVoteSnapshot::deserialize` updated to handle full Account protobuf format (Java's `DelegationStore.setAccountVote()` stores `accountCapsule.getData()`)
- [ ] Run a small remote-vs-embedded parity slice including a case where `withdrawReward` is non-zero
- [x] Keep `execution.remote.unfreeze_balance_enabled` gated until parity is confirmed
  - Default is `false` in config.rs.

