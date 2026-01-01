# FreezeV2FixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/FreezeV2FixtureGeneratorTest.java` fixture generation
  so conformance covers major validation branches and boundary behaviors for V2 freeze/unfreeze:
  - `FreezeBalanceV2Contract` (54)
  - `UnfreezeBalanceV2Contract` (55)

Non-Goals
- Do not change actuator validation/execution logic; only add/adjust fixtures to reflect current behavior.
- Do not refactor fixture generator infrastructure unless needed for determinism (e.g., stable timestamps).

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, `expected/post_db/`, and `metadata.json`.
- Validation failures produce:
  - `metadata.json.expectedStatus == "VALIDATION_FAILED"`
  - `metadata.json.expectedErrorMessage` equals the thrown `ContractValidateException` message.
- Happy fixtures execute successfully and mutate expected DB state.
- If a fixture mutates votes, `metadata.json.databasesTouched` includes `votes` and both `pre_db/` and
  `expected/post_db/` capture it.

Checklist / TODO

Phase 0 — Confirm Baselines and Error Strings
- [x] Record exact validate error messages (do not rely on substring matching):
  - [x] `actuator/src/main/java/org/tron/core/actuator/FreezeBalanceV2Actuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceV2Actuator.java`
  - [x] `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java` (`supportUnfreezeDelay`, flags)
- [x] Confirm baseline dynamic props used by this generator:
  - [x] `framework/src/test/java/org/tron/core/conformance/ConformanceFixtureTestSupport.java` (`initCommonDynamicPropsV2`)
- [ ] Run the current generator test once to establish baseline fixture output:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.FreezeV2FixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`

Phase 1 — FreezeBalanceV2Contract (54) Missing Fixtures

Owner/address/account branches
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`
  - [x] Expect: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking address not in `AccountStore`
  - [x] Expect: `"Account[" + owner + "] not exists"`.

Frozen balance branches
- [x] Add `validate_fail_frozen_balance_zero`:
  - [x] `frozenBalance = 0`
  - [x] Expect: `"frozenBalance must be positive"`.
- [x] Add `validate_fail_frozen_balance_negative`:
  - [x] `frozenBalance = -1`
  - [x] Expect: `"frozenBalance must be positive"`.
- [x] Add `validate_fail_frozen_balance_lt_1_trx`:
  - [x] `frozenBalance = ONE_TRX - 1`
  - [x] Expect: `"frozenBalance must be greater than or equal to 1 TRX"`.
- [x] Add `happy_path_frozen_balance_exact_1_trx`:
  - [x] `frozenBalance = 1 * ONE_TRX`
  - [x] Expect: `SUCCESS`.
- [x] Add `happy_path_frozen_balance_equal_account_balance`:
  - [x] Seed account with balance `X`, freeze `X`
  - [x] Expect: `SUCCESS` and post balance is `0`.

Resource code validation / coverage
- [x] Add `happy_path_freeze_v2_tron_power`:
  - [x] Ensure `ALLOW_NEW_RESOURCE_MODEL = 1` (baseline)
  - [x] `resource = TRON_POWER`
  - [x] Expect: `SUCCESS`.
- [x] Add `validate_fail_tron_power_when_new_resource_model_off`:
  - [x] Set `DynamicPropertiesStore.saveAllowNewResourceModel(0)` (keep `unfreezeDelayDays > 0`)
  - [x] `resource = TRON_POWER`
  - [x] Expect: `"ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]"`.
- [x] Add `validate_fail_resource_unrecognized_value`:
  - [x] Use `FreezeBalanceV2Contract.Builder#setResourceValue(999)`
  - [x] Expect: resource-code error message matching `ALLOW_NEW_RESOURCE_MODEL` state.

Execution semantics (edge fixtures)
- [x] Add `edge_freeze_bandwidth_twice_accumulates`:
  - [x] Pre-state has an existing `frozenV2(BANDWIDTH)` amount
  - [x] Freeze again; expect post amount is the sum and totalNetWeight delta matches flooring rules.
- [x] Add `edge_freeze_amount_not_multiple_of_trx_precision`:
  - [x] Freeze `N*ONE_TRX + 1` to pin weight rounding (floor division by `TRX_PRECISION`).

Phase 2 — UnfreezeBalanceV2Contract (55) Missing Fixtures

Feature gating (V2 disabled)
- [x] Add `validate_fail_feature_not_enabled` for unfreeze:
  - [x] Set `DynamicPropertiesStore.saveUnfreezeDelayDays(0)`
  - [x] Expect: `"Not support UnfreezeV2 transaction, need to be opened by the committee"`.

Owner/address/account branches
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`
  - [x] Expect: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking address not in `AccountStore`
  - [x] Expect: `"Account[" + owner + "] does not exist"`.

Resource coverage gaps
- [x] Add `happy_path_unfreeze_v2_energy`:
  - [x] Seed `frozenV2(ENERGY) > 0`
  - [x] Expect: `SUCCESS`.
- [x] Add `validate_fail_no_frozen_balance_energy`:
  - [x] No `frozenV2(ENERGY)`
  - [x] Expect: `"no frozenBalance(Energy)"`.
- [x] Add `happy_path_unfreeze_v2_tron_power` (enabled model):
  - [x] Ensure `ALLOW_NEW_RESOURCE_MODEL = 1`, seed `frozenV2(TRON_POWER) > 0`
  - [x] Expect: `SUCCESS` (and consider vote-side-effects fixtures below).
- [x] Add `validate_fail_no_frozen_balance_tron_power`:
  - [x] No `frozenV2(TRON_POWER)`
  - [x] Expect: `"no frozenBalance(TronPower)"`.
- [x] Add `validate_fail_tron_power_when_new_resource_model_off`:
  - [x] Set `DynamicPropertiesStore.saveAllowNewResourceModel(0)`
  - [x] `resource = TRON_POWER`
  - [x] Expect: `"ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]"`.
- [x] Add `validate_fail_resource_unrecognized_value`:
  - [x] Use `UnfreezeBalanceV2Contract.Builder#setResourceValue(999)`
  - [x] Expect: resource-code error message matching `ALLOW_NEW_RESOURCE_MODEL` state.

Unfreeze amount boundaries
- [x] Add `validate_fail_unfreeze_balance_zero`:
  - [x] `unfreezeBalance = 0`
  - [x] Expect: `"Invalid unfreeze_balance, [0] is error"`.
- [x] Add `validate_fail_unfreeze_balance_negative`:
  - [x] `unfreezeBalance = -1`
  - [x] Expect: `"Invalid unfreeze_balance, [-1] is error"`.
- [x] Add `happy_path_unfreeze_balance_equal_frozen_amount`:
  - [x] `unfreezeBalance == frozenAmount`
  - [x] Expect: `SUCCESS` and verify whether `frozenV2` entry is kept at `0` vs removed.
- [x] Add `edge_unfreeze_amount_not_multiple_of_trx_precision`:
  - [x] Seed `frozenV2 = 100*ONE_TRX`, unfreeze `1` (1 SUN)
  - [x] Expect: `SUCCESS` and pin down rounding behavior in total weight updates.

Unfreezing-times limit (UNFREEZE_MAX_TIMES = 32)
- [x] Add `validate_fail_unfreezing_times_over_limit`:
  - [x] Seed account with 32 `unfrozenV2` entries where `unfreezeExpireTime > now`
  - [x] Expect: `"Invalid unfreeze operation, unfreezing times is over limit"`.
- [x] Add `edge_unfreezing_times_at_31_succeeds`:
  - [x] Seed 31 pending entries, then execute unfreeze
  - [x] Expect: `SUCCESS`.

Expired sweep behavior (extend existing coverage)
- [x] Add `edge_sweep_multiple_expired_unfrozen_v2_entries`:
  - [x] Seed 2+ expired entries; expect `withdrawExpireAmount` is the sum.
- [x] Add `edge_sweep_mixed_expired_and_unexpired_unfrozen_v2_entries`:
  - [x] Seed one expired + one unexpired; expect expired removed and unexpired preserved.
- [x] Add `edge_sweep_expire_time_equals_now`:
  - [x] Seed entry with `unfreezeExpireTime == now`; expect it is swept (`<= now`).

Vote side effects (optional but high value for cross-impl conformance)
- [ ] Add `edge_unfreeze_clears_votes_on_new_resource_model_transition`:
  - [ ] Ensure `ALLOW_NEW_RESOURCE_MODEL = 1`
  - [ ] Seed account with non-empty `votesList` and `oldTronPower == 0` (default)
  - [ ] Execute an unfreeze (any resource) and verify votes cleared per `updateVote(...)` logic.
  - [ ] Include `votes` in `databasesTouched`.
- [ ] Add `edge_unfreeze_rescales_votes_when_legacy_model`:
  - [ ] Set `ALLOW_NEW_RESOURCE_MODEL = 0`
  - [ ] Seed votes such that owned tron power becomes insufficient after unfreeze
  - [ ] Verify vote rescaling behavior and `VotesStore` updates.
  - [ ] Include `votes` in `databasesTouched`.

Phase 3 — Validate Fixture Output
- [ ] Regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.FreezeV2FixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
- [ ] Spot-check generated `metadata.json`:
  - [ ] `expectedStatus`/`expectedErrorMessage` match actual actuator behavior.
  - [ ] `databasesTouched` includes all mutated DBs (`account`, `dynamic-properties`, plus `votes` where applicable).

