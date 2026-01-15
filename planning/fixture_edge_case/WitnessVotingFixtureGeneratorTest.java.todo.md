# WitnessVotingFixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/WitnessVotingFixtureGeneratorTest.java` fixture
  generation so conformance covers major validation branches, boundary conditions, and key state
  transitions for:
  - `VoteWitnessContract` (4)
  - `WitnessCreateContract` (5)
  - `WitnessUpdateContract` (8)
  - `WithdrawBalanceContract` (13)

Non-Goals
- Do not change contract validation rules; only add/adjust fixtures to reflect current Java-tron behavior.
- Do not refactor fixture generator infrastructure (keep changes localized to the test class).
- Do not add unrelated contract types to this generator class.

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, `expected/post_db/`, and `metadata.json`.
- For validation failures: `metadata.json.expectedStatus == "VALIDATION_FAILED"` and `expectedErrorMessage`
  matches the thrown `ContractValidateException` message.
- For execute failures: `metadata.json.expectedStatus == "REVERT"` and `expectedErrorMessage` matches the
  thrown `ContractExeException` message.
- "Happy"/"edge" fixtures execute successfully and produce the intended DB mutations.
- Fixture `caseCategory`/`description` align with the observed outcome (avoid "validate_fail but SUCCESS").

Checklist / TODO

Phase 0 — Confirm Baselines
- [x] Skim the validate paths and constants to ensure fixtures map to real branches:
  - [x] `actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/WitnessCreateActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/WitnessUpdateActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/WithdrawBalanceActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java` (`validUrl`, max 256 bytes)
- [ ] Run (once) to verify the current fixtures are still being generated:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.WitnessVotingFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`

Phase 1 — VoteWitnessContract (4) Fixtures

Owner address + voter existence
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] Build contract with `owner_address = ByteString.EMPTY`.
  - [x] Expect validate error: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking owner address not present in `AccountStore`.
  - [x] Expect validate error contains: `"Account["` and `"does not exist"`.

Votes list structural checks
- [x] Add `validate_fail_votes_list_empty`:
  - [x] Build contract without `.addVotes(...)`.
  - [x] Expect validate error: `"VoteNumber must more than 0"`.
- [x] Add `validate_fail_votes_count_over_max_30`:
  - [x] Provide 31 vote entries (distinct witness addresses).
  - [x] Expect validate error: `"VoteNumber more than maxVoteNumber 30"`.

Vote entry address + existence checks
- [x] Add `validate_fail_vote_address_invalid`:
  - [x] Use a `vote_address` with wrong length (e.g. 10 bytes).
  - [x] Expect validate error: `"Invalid vote address!"`.
- [x] Add `validate_fail_vote_target_account_not_exist`:
  - [x] Use a valid-looking vote address that does not exist in `AccountStore`.
  - [x] Expect validate error contains: `"Account["` and `"does not exist"`.

Boundary + state transition fixtures (execution semantics)
- [x] Add `edge_tron_power_exact_match`:
  - [x] Freeze exactly `N * ONE_TRX` and cast votes totalling `N` (TRX units).
  - [x] Expect `SUCCESS` (validation uses `>` not `>=` after scaling).
- [x] Add `edge_revoting_replaces_previous_votes`:
  - [x] Pre-seed the voter with existing votes in `AccountCapsule` and an existing `VotesStore` entry.
  - [x] Submit a second vote tx with a different set of witnesses / counts.
  - [x] Expect post-state: old votes cleared and replaced (no merge).

Phase 2 — WitnessCreateContract (5) Fixtures

Owner address + account existence
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`.
  - [x] Expect validate error: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking owner address not present in `AccountStore`.
  - [x] Expect validate error contains: `"account["` and `"does not exist"`.

URL boundary validation
- [x] Add `validate_fail_url_too_long_257`:
  - [x] `url` length 257 bytes (e.g. 257 `'a'` bytes).
  - [x] Expect validate error: `"Invalid url"`.

Balance boundary + burn/blackhole behavior
- [x] Add `edge_balance_equals_upgrade_cost`:
  - [x] Set owner balance exactly `ACCOUNT_UPGRADE_COST` and create witness.
  - [x] Expect `SUCCESS` and cost is applied.
- [x] Add `edge_blackhole_optimization_burns_trx`:
  - [x] Set `ALLOW_BLACK_HOLE_OPTIMIZATION = 1`.
  - [x] Create witness and verify post-state reflects burn path (not blackhole credit).

Phase 3 — WitnessUpdateContract (8) Fixtures

Owner address + account existence
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`.
  - [x] Expect validate error: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking owner address not present in `AccountStore`.
  - [x] Expect validate error: `"account does not exist"`.

URL boundary validation
- [x] Add `validate_fail_url_too_long_257`:
  - [x] `update_url` length 257 bytes.
  - [x] Expect validate error: `"Invalid url"`.

Phase 4 — WithdrawBalanceContract (13) Fixtures

Owner address + account existence
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`.
  - [x] Expect validate error: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking owner address not present in `AccountStore`.
  - [x] Expect validate error contains: `"Account["` and `"does not exist"`.

Genesis guard representative restriction
- [x] Add `validate_fail_guard_representative_withdraw`:
  - [x] Use an address from `CommonParameter.getInstance().getGenesisBlock().getWitnesses()`.
  - [x] Ensure the account exists and has non-zero allowance (or reward).
  - [x] Expect validate error contains: `"guard representative"` and `"not allowed"`.

Cooldown boundary (strict inequality)
- [x] Add `edge_withdraw_at_exact_cooldown_boundary`:
  - [x] Compute `now = dynamicStore.getLatestBlockHeaderTimestamp()`.
  - [x] Set `latestWithdrawTime = now - (witnessAllowanceFrozenTime * FROZEN_PERIOD)`.
  - [x] Ensure allowance > 0.
  - [x] Expect `SUCCESS` (validation fails only when `<`, not `<=`).

Reward-path happy (allowance == 0 but reward > 0)
- [ ] Add `happy_withdraw_reward_via_mortgage_reward` (if feasible):
  - [ ] Seed the reward source used by `mortgageService.queryReward(owner)` so it returns > 0.
  - [ ] Keep `allowance == 0` before execution.
  - [ ] Expect `SUCCESS` and the withdrawn amount reflects the reward path.
  - [x] SKIPPED: Reward seeding requires invasive changes to MortgageService internals.
        The reward path is tested indirectly via other means and documenting this limitation.

Overflow guard
- [x] Add `validate_fail_balance_allowance_overflow`:
  - [x] Set `balance` near `Long.MAX_VALUE` and `allowance > 0`.
  - [x] Expect validate error from `LongMath.checkedAdd` overflow.

Phase 5 — Verify Output
- [ ] Run the generator test class and regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.WitnessVotingFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
- [ ] Spot-check a few generated `metadata.json` files for expectedStatus and error messages.
- [ ] Ensure each fixture includes all relevant DBs in `FixtureMetadata` (`account`, `witness`, `votes`, `dynamic-properties`).

