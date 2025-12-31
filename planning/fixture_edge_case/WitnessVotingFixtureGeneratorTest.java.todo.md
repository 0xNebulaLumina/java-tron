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
- “Happy”/“edge” fixtures execute successfully and produce the intended DB mutations.
- Fixture `caseCategory`/`description` align with the observed outcome (avoid “validate_fail but SUCCESS”).

Checklist / TODO

Phase 0 — Confirm Baselines
- [ ] Skim the validate paths and constants to ensure fixtures map to real branches:
  - [ ] `actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/WitnessCreateActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/WitnessUpdateActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/WithdrawBalanceActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java` (`validUrl`, max 256 bytes)
- [ ] Run (once) to verify the current fixtures are still being generated:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.WitnessVotingFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`

Phase 1 — VoteWitnessContract (4) Fixtures

Owner address + voter existence
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] Build contract with `owner_address = ByteString.EMPTY`.
  - [ ] Expect validate error: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Use a valid-looking owner address not present in `AccountStore`.
  - [ ] Expect validate error contains: `"Account["` and `"does not exist"`.

Votes list structural checks
- [ ] Add `validate_fail_votes_list_empty`:
  - [ ] Build contract without `.addVotes(...)`.
  - [ ] Expect validate error: `"VoteNumber must more than 0"`.
- [ ] Add `validate_fail_votes_count_over_max_30`:
  - [ ] Provide 31 vote entries (distinct witness addresses).
  - [ ] Expect validate error: `"VoteNumber more than maxVoteNumber 30"`.

Vote entry address + existence checks
- [ ] Add `validate_fail_vote_address_invalid`:
  - [ ] Use a `vote_address` with wrong length (e.g. 10 bytes).
  - [ ] Expect validate error: `"Invalid vote address!"`.
- [ ] Add `validate_fail_vote_target_account_not_exist`:
  - [ ] Use a valid-looking vote address that does not exist in `AccountStore`.
  - [ ] Expect validate error contains: `"Account["` and `"does not exist"`.

Boundary + state transition fixtures (execution semantics)
- [ ] Add `edge_tron_power_exact_match`:
  - [ ] Freeze exactly `N * ONE_TRX` and cast votes totalling `N` (TRX units).
  - [ ] Expect `SUCCESS` (validation uses `>` not `>=` after scaling).
- [ ] Add `edge_revoting_replaces_previous_votes`:
  - [ ] Pre-seed the voter with existing votes in `AccountCapsule` and an existing `VotesStore` entry.
  - [ ] Submit a second vote tx with a different set of witnesses / counts.
  - [ ] Expect post-state: old votes cleared and replaced (no merge).

Phase 2 — WitnessCreateContract (5) Fixtures

Owner address + account existence
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`.
  - [ ] Expect validate error: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Use a valid-looking owner address not present in `AccountStore`.
  - [ ] Expect validate error contains: `"account["` and `"does not exist"`.

URL boundary validation
- [ ] Add `validate_fail_url_too_long_257`:
  - [ ] `url` length 257 bytes (e.g. 257 `'a'` bytes).
  - [ ] Expect validate error: `"Invalid url"`.

Balance boundary + burn/blackhole behavior
- [ ] Add `edge_balance_equals_upgrade_cost`:
  - [ ] Set owner balance exactly `ACCOUNT_UPGRADE_COST` and create witness.
  - [ ] Expect `SUCCESS` and cost is applied.
- [ ] Add `edge_blackhole_optimization_burns_trx`:
  - [ ] Set `ALLOW_BLACK_HOLE_OPTIMIZATION = 1`.
  - [ ] Create witness and verify post-state reflects burn path (not blackhole credit).

Phase 3 — WitnessUpdateContract (8) Fixtures

Owner address + account existence
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`.
  - [ ] Expect validate error: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Use a valid-looking owner address not present in `AccountStore`.
  - [ ] Expect validate error: `"account does not exist"`.

URL boundary validation
- [ ] Add `validate_fail_url_too_long_257`:
  - [ ] `update_url` length 257 bytes.
  - [ ] Expect validate error: `"Invalid url"`.

Phase 4 — WithdrawBalanceContract (13) Fixtures

Owner address + account existence
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`.
  - [ ] Expect validate error: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Use a valid-looking owner address not present in `AccountStore`.
  - [ ] Expect validate error contains: `"Account["` and `"does not exist"`.

Genesis guard representative restriction
- [ ] Add `validate_fail_guard_representative_withdraw`:
  - [ ] Use an address from `CommonParameter.getInstance().getGenesisBlock().getWitnesses()`.
  - [ ] Ensure the account exists and has non-zero allowance (or reward).
  - [ ] Expect validate error contains: `"guard representative"` and `"not allowed"`.

Cooldown boundary (strict inequality)
- [ ] Add `edge_withdraw_at_exact_cooldown_boundary`:
  - [ ] Compute `now = dynamicStore.getLatestBlockHeaderTimestamp()`.
  - [ ] Set `latestWithdrawTime = now - (witnessAllowanceFrozenTime * FROZEN_PERIOD)`.
  - [ ] Ensure allowance > 0.
  - [ ] Expect `SUCCESS` (validation fails only when `<`, not `<=`).

Reward-path happy (allowance == 0 but reward > 0)
- [ ] Add `happy_withdraw_reward_via_mortgage_reward` (if feasible):
  - [ ] Seed the reward source used by `mortgageService.queryReward(owner)` so it returns > 0.
  - [ ] Keep `allowance == 0` before execution.
  - [ ] Expect `SUCCESS` and the withdrawn amount reflects the reward path.
  - [ ] If reward seeding is too invasive, document the limitation and skip this fixture.

Overflow guard
- [ ] Add `validate_fail_balance_allowance_overflow`:
  - [ ] Set `balance` near `Long.MAX_VALUE` and `allowance > 0`.
  - [ ] Expect validate error from `LongMath.checkedAdd` overflow.

Phase 5 — Verify Output
- [ ] Run the generator test class and regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.WitnessVotingFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
- [ ] Spot-check a few generated `metadata.json` files for expectedStatus and error messages.
- [ ] Ensure each fixture includes all relevant DBs in `FixtureMetadata` (`account`, `witness`, `votes`, `dynamic-properties`).

