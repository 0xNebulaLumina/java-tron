# TODO / Fix Plan: `WITHDRAW_BALANCE_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/WITHDRAW_BALANCE_CONTRACT.planning.md`.

## 0) Decide the “parity target” (do this first)

- [ ] Confirm desired scope:
  - [ ] **Actuator-only parity** (match `WithdrawBalanceActuator.validate + execute`)
  - [ ] **End-to-end parity** (also match MortgageService delegation-store mutations + BandwidthProcessor outcomes in remote mode)
- [ ] Confirm which deployment profiles matter:
  - [ ] mainnet/testnet only (prefix `0x41`/`0xa0`)
  - [ ] custom/private nets (custom genesis witnesses, possibly custom prefixes)
- [ ] Confirm config expectations:
  - [ ] should `execution.remote.delegation_reward_enabled` exist as a rollout switch?
  - [ ] or should Rust always behave like Java (delegation reward computed whenever `CHANGE_DELEGATION == 1`)?

## 1) Make guard representative detection config-driven (or explicitly out-of-scope)

Goal: match Java’s `isGP` check:
`CommonParameter.getInstance().getGenesisBlock().getWitnesses()`.

Options:

- [ ] **Option A (strict parity)**: pass the genesis witness address list to Rust.
  - [ ] Add a config field in `rust-backend/config.toml` (e.g., `[execution.remote] genesis_guard_reps_base58 = [...]`).
  - [ ] Use that list in `execute_withdraw_balance_contract()` instead of hardcoded arrays.
  - [ ] Keep exact error message: `"Account[<hex>] is a guard representative and is not allowed to withdraw Balance"`.
- [ ] **Option B (runtime parity)**: have Java send the list over gRPC once per session/block.
  - [ ] Add an RPC or a field in `ExecutionContext` for `genesis_witnesses` (bytes list).
  - [ ] Cache it in the Rust backend and use it for the check.
- [ ] **Option C (explicitly constrain scope)**: document that Rust only supports mainnet/testnet default genesis lists and keep hardcoded arrays.

Verification:

- [ ] Add a fixture/test where a custom genesis witness address is blocked by Java; ensure Rust matches.

## 2) Align delegation reward behavior with Java (remove or clarify config gate)

Goal: mirror embedded semantics:

- Java always calls `MortgageService.withdrawReward(owner)` in `execute()` (which self-gates on `allowChangeDelegation()`).

Checklist:

- [ ] Decide whether `execution.remote.delegation_reward_enabled` should remain:
  - [ ] If **strict parity**: remove the extra gate and always compute reward when `CHANGE_DELEGATION == 1`.
  - [ ] If **rollout gate**: document that remote results can diverge from embedded unless enabled.
- [ ] Ensure the Rust `delegation::withdraw_reward()` output matches Java’s effective allowance delta:
  - [ ] same cycle boundary behavior (`beginCycle`, `endCycle`, `currentCycle`)
  - [ ] same old-vs-new algorithm split via `NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE`
  - [ ] same rounding/truncation behavior (especially old algorithm floating-point division)

Tests to add (if missing):

- [ ] `CHANGE_DELEGATION=1`, `allowance=0`, votes + delegation store data present → withdrawal succeeds and `withdraw_amount > 0`.
- [ ] `CHANGE_DELEGATION=0`, votes + delegation data present → withdrawal uses allowance only (reward excluded).

## 3) Fix the “no reward” predicate to match Java even in corrupted/pathological states

Goal: ensure Rust rejects whenever **total withdrawable** amount is `<= 0`, consistent with Java’s `queryReward(owner) <= 0`.

Implementation plan:

- [ ] Compute `total_allowance = base_allowance + delegation_reward` (checked add).
- [ ] Replace:
  - current: `if base_allowance <= 0 && delegation_reward <= 0`
  - with: `if total_allowance <= 0` (or an equivalent that preserves Java’s intended semantics)
- [ ] Keep the exact error string: `"witnessAccount does not have any reward"`.
- [ ] Ensure negative `total_allowance` can never be applied as a balance decrement.

Tests:

- [ ] Construct a synthetic state with `allowance < 0` and small positive reward so `total_allowance <= 0`:
  - [ ] Java should reject (because `queryReward <= 0`)
  - [ ] Rust should also reject (no negative “withdrawal”)

## 4) Add Any/type_url validation parity (optional but improves robustness)

Goal: match Java’s `"contract type error ..."` behavior when the request payload is malformed/mismatched.

- [ ] In `execute_withdraw_balance_contract()`:
  - [ ] Validate `transaction.metadata.contract_parameter` is present.
  - [ ] Validate `type_url` matches `protocol.WithdrawBalanceContract`.
  - [ ] If mismatched, return a Java-like error message (or decide that this is out-of-scope because Java constructs the request).

## 5) Decide whether to pursue bandwidth parity (end-to-end parity only)

Goal: match Java’s `BandwidthProcessor` resource accounting.

- [ ] If required, replace `calculate_bandwidth_usage(...)` for non-VM system contracts with a Java-equivalent tx-size-based calculation.
- [ ] Ensure time windowing aligns (`headSlot` vs block number).

## 6) Verification checklist

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] Add/extend unit tests for withdraw validation edge cases + delegation reward path.
- [ ] Java:
  - [ ] `./gradlew :framework:test --tests \"org.tron.core.actuator.WithdrawBalanceActuatorTest\"`
  - [ ] If validating remote parity, run the conformance fixtures for `withdraw_balance_contract` and compare expected outputs.

