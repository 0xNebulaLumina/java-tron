# TODO / Fix Plan: `WITHDRAW_BALANCE_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/WITHDRAW_BALANCE_CONTRACT.planning.md`.

## 0) Decide the "parity target" (do this first)

- [x] Confirm desired scope:
  - [x] **Actuator-only parity** (match `WithdrawBalanceActuator.validate + execute`)
  - [ ] **End-to-end parity** (also match MortgageService delegation-store mutations + BandwidthProcessor outcomes in remote mode)
- [x] Confirm which deployment profiles matter:
  - [x] mainnet/testnet only (prefix `0x41`/`0xa0`)
  - [x] custom/private nets (custom genesis witnesses, possibly custom prefixes) — supported via config override
- [x] Confirm config expectations:
  - [x] ~~should `execution.remote.delegation_reward_enabled` exist as a rollout switch?~~ Deprecated.
  - [x] Rust always behaves like Java (delegation reward computed whenever `CHANGE_DELEGATION == 1`)

## 1) Make guard representative detection config-driven (or explicitly out-of-scope)

Goal: match Java's `isGP` check:
`CommonParameter.getInstance().getGenesisBlock().getWitnesses()`.

Options:

- [x] **Option A (strict parity)**: pass the genesis witness address list to Rust.
  - [x] Add a config field in `rust-backend/config.toml` (e.g., `[execution.remote] genesis_guard_representatives_base58 = [...]`).
  - [x] Use that list in `execute_withdraw_balance_contract()` instead of hardcoded arrays (when non-empty).
  - [x] Keep exact error message: `"Account[<hex>] is a guard representative and is not allowed to withdraw Balance"`.
- [ ] ~~**Option B (runtime parity)**: have Java send the list over gRPC once per session/block.~~ (Not selected)
- [ ] ~~**Option C (explicitly constrain scope)**: document that Rust only supports mainnet/testnet default genesis lists and keep hardcoded arrays.~~ (Not selected)

Implementation details:
- Added `genesis_guard_representatives_base58: Vec<String>` to `RemoteExecutionConfig`
- Added `from_tron_base58_to_bytes()` to decode Base58 addresses to 21-byte raw form (supports any prefix)
- `is_genesis_guard_representative()` now takes config list; falls back to hardcoded mainnet/testnet lists when empty
- `decode_guard_reps_from_config()` decodes Base58 addresses, logging warnings for invalid entries

Verification:

- [x] Add unit tests for guard rep detection with config override, hardcoded fallback, and invalid inputs (8 tests added)

## 2) Align delegation reward behavior with Java (remove or clarify config gate)

Goal: mirror embedded semantics:

- Java always calls `MortgageService.withdrawReward(owner)` in `execute()` (which self-gates on `allowChangeDelegation()`).

Checklist:

- [x] Decide whether `execution.remote.delegation_reward_enabled` should remain:
  - [x] **Strict parity**: removed the config gate; `compute_delegation_reward_if_enabled()` now always calls `delegation::withdraw_reward()`.
  - [x] `delegation_reward_enabled` field kept in config struct for backward compatibility but marked as deprecated (no effect).
- [x] Ensure the Rust `delegation::withdraw_reward()` output matches Java's effective allowance delta:
  - [x] same cycle boundary behavior (`beginCycle`, `endCycle`, `currentCycle`) — pre-existing implementation
  - [x] same old-vs-new algorithm split via `NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE` — pre-existing implementation
  - [x] same rounding/truncation behavior (old algorithm uses f64 division matching Java) — pre-existing implementation

Tests (pre-existing, verified passing):

- [x] `test_vote_witness_withdraw_reward_with_delegation_enabled` — verifies delegation reward path
- [x] `test_vote_witness_withdraw_reward_noop_no_prior_votes` — verifies no reward when no votes
- [x] `test_unfreeze_v2_withdraw_reward_updates_allowance` — verifies delegation reward via freeze path
- [x] `test_unfreeze_balance_withdraw_reward_updates_allowance` — verifies delegation reward via unfreeze path

## 3) Fix the "no reward" predicate to match Java even in corrupted/pathological states

Goal: ensure Rust rejects whenever **total withdrawable** amount is `<= 0`, consistent with Java's `queryReward(owner) <= 0`.

Implementation plan:

- [x] Replaced the predicate to match Java's `queryReward()` semantics:
  - old: `if base_allowance <= 0 && delegation_reward <= 0`
  - new: `if base_allowance <= 0 && (delegation_reward.wrapping_add(base_allowance)) <= 0`
  - This matches Java's `getAllowance() <= 0 && queryReward() <= 0` where `queryReward() = reward + allowance`
- [x] Keep the exact error string: `"witnessAccount does not have any reward"`.
- [x] Negative `total_allowance` correctly triggers rejection, preventing negative balance decrements.

Tests:

- [x] 6 unit tests added for the no-reward predicate covering all cases:
  - `test_no_reward_predicate_both_zero` — both zero → reject
  - `test_no_reward_predicate_positive_allowance` — allowance > 0 → allow
  - `test_no_reward_predicate_positive_reward_zero_allowance` — reward > 0 → allow
  - `test_no_reward_predicate_negative_allowance_positive_reward_net_negative` — pathological case → reject (THE key fix)
  - `test_no_reward_predicate_negative_allowance_positive_reward_net_positive` — net positive → allow
  - `test_no_reward_predicate_old_behavior_would_be_wrong` — proves old predicate was incorrect

## 4) Add Any/type_url validation parity (optional but improves robustness)

Goal: match Java's `"contract type error ..."` behavior when the request payload is malformed/mismatched.

- [ ] Out of scope for actuator-only parity. Java constructs the gRPC request, so type_url validation is redundant in the Rust handler. The Java RPC layer already validates the contract type before sending to Rust.

## 5) Decide whether to pursue bandwidth parity (end-to-end parity only)

Goal: match Java's `BandwidthProcessor` resource accounting.

- [ ] Out of scope for actuator-only parity. Bandwidth processing happens outside the actuator in Java.

## 6) Verification checklist

- [x] Rust:
  - [x] `cd rust-backend && cargo test --workspace` — all 390+ tests pass (3 pre-existing vote_witness failures unrelated)
  - [x] 14 new unit tests added: 8 guard rep tests + 6 no-reward predicate tests
  - [x] All 8 WITHDRAW_BALANCE_CONTRACT conformance fixtures pass
  - [x] All conformance fixtures pass (`./scripts/ci/run_fixture_conformance.sh --rust-only`)
- [ ] Java: (not in scope for Rust-side changes)
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.WithdrawBalanceActuatorTest"`
