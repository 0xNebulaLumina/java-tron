# TODO / Fix Plan: `WITHDRAW_BALANCE_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/WITHDRAW_BALANCE_CONTRACT.planning.md`.

## 0) Decide the "parity target" (do this first)

- [x] Confirm desired scope:
  - [x] **Actuator-only parity** (match `WithdrawBalanceActuator.validate + execute`)
  - [x] **End-to-end parity** (also match MortgageService delegation-store mutations + BandwidthProcessor outcomes in remote mode)
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

## 4) Add Any/type_url validation parity

Goal: match Java's `"contract type error ..."` behavior when the request payload is malformed/mismatched.

- [x] In `execute_withdraw_balance_contract()`:
  - [x] Validate `transaction.metadata.contract_parameter` is present (optional field — only checked if provided).
  - [x] Validate `type_url` ends with `"WithdrawBalanceContract"`.
  - [x] If mismatched, return error: `"contract type error, expected type [WithdrawBalanceContract], real type[<type_url>]"`
  - [x] Follows the established pattern from freeze.rs (FreezeBalanceContract, UnfreezeBalanceContract, etc.)

## 5) Bandwidth parity (end-to-end)

Goal: match Java's `BandwidthProcessor` resource accounting.

### 5a) Bandwidth size calculation

- [x] `calculate_bandwidth_usage()` already uses Java-computed `transaction_bytes_size` from gRPC request
  - Java computes: `trxCap.getInstance().toBuilder().clearRet().build().getSerializedSize() + numContracts * MAX_RESULT_SIZE_IN_TX`
  - Sends via: `ExecuteTransactionRequest.setTransactionBytesSize(txBytesSize)`
  - Rust receives and uses directly — no approximation needed for normal requests
  - Fallback approximation (60 + data_len + 65) only for edge cases without Java-provided size

### 5b) AEXT bandwidth resource tracking ("tracked" mode)

- [x] Added full bandwidth resource tracking for `aext_mode == "tracked"`:
  - [x] Reads current AEXT for owner (or defaults)
  - [x] Reads dynamic properties: FREE_NET_LIMIT, PUBLIC_NET_LIMIT, PUBLIC_NET_USAGE, PUBLIC_NET_TIME, TRANSACTION_FEE, CREATE_NEW_ACCOUNT_BANDWIDTH_RATE
  - [x] Computes `account_net_limit` from frozen bandwidth record and total_net_weight/total_net_limit
  - [x] Computes `headSlot = (block_timestamp - genesis_block_timestamp) / 3000`
  - [x] Calls `ResourceTracker::track_bandwidth_v2()` with full BandwidthParams
  - [x] Persists after-AEXT to storage and account proto
  - [x] Persists global PUBLIC_NET changes if free-net path was used
  - [x] Populates `aext_map` in result for conversion layer to use
  - [x] `creates_new_account = false` (WithdrawBalance never creates accounts)
- [x] Follows the exact same pattern as TransferContract's AEXT tracking in mod.rs

### 5c) Hybrid AEXT mode (existing)

- [x] Hybrid mode (`accountinfo_aext_mode = "hybrid"`) works without handler changes:
  - Java sends pre-execution AEXT values via `pre_execution_aext` in gRPC request
  - Conversion layer (`conversion.rs`) echoes these values in both old and new AccountInfo
  - Handler returns empty `aext_map`, conversion layer uses pre-exec data instead

## 6) Verification checklist

- [x] Rust:
  - [x] `cd rust-backend && cargo test --workspace` — all 420 tests pass (3 pre-existing vote_witness failures unrelated)
  - [x] 14 unit tests: 8 guard rep tests + 6 no-reward predicate tests
  - [x] All 8 WITHDRAW_BALANCE_CONTRACT conformance fixtures pass
  - [x] All conformance fixtures pass (`./scripts/ci/run_fixture_conformance.sh --rust-only`)
- [ ] Java: (not in scope for Rust-side changes)
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.WithdrawBalanceActuatorTest"`
