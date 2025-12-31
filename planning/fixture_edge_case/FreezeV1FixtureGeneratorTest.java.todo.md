# FreezeV1FixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/FreezeV1FixtureGeneratorTest.java` fixture generation
  so conformance covers major validation branches and boundary behaviors for V1 freeze/unfreeze:
  - `FreezeBalanceContract` (11)
  - `UnfreezeBalanceContract` (12)

Non-Goals
- Do not change actuator validation/execution logic; only add/adjust fixtures to reflect current behavior.
- Do not refactor fixture generator infrastructure unless necessary for determinism/duplication reduction.

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, `expected/post_db/`, and `metadata.json`.
- Validation failures produce:
  - `metadata.json.expectedStatus == "VALIDATION_FAILED"`
  - `metadata.json.expectedErrorMessage` equals the thrown `ContractValidateException` message.
- Happy fixtures execute successfully and mutate expected DB state.
- For delegation-enabled fixtures, the additional touched DBs (`DelegatedResource`, `DelegatedResourceAccountIndex`)
  appear in `databasesTouched` and are captured in both `pre_db/` and `expected/post_db/`.

Checklist / TODO

Phase 0 — Confirm Baselines and Error Strings
- [ ] Skim validate/execute paths and record exact error messages to lock in:
  - [ ] `actuator/src/main/java/org/tron/core/actuator/FreezeBalanceActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceActuator.java`
  - [ ] `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java` (defaults for min/max frozen time)
- [ ] Confirm which flags actually gate delegation:
  - [ ] `DynamicPropertiesStore.supportDR()` (delegated resources) is controlled by `ALLOW_DELEGATE_RESOURCE`.
  - [ ] `DynamicPropertiesStore.allowChangeDelegation()` (reward delegation) is controlled by `CHANGE_DELEGATION`.
- [ ] Run the current generator test to see baseline fixture outputs:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.FreezeV1FixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

Phase 1 — FreezeBalanceContract (11) Missing Fixtures (delegation OFF, new resource model OFF)

Owner/address/account branches
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`
  - [ ] Expect: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Use a valid-looking address not in `AccountStore`
  - [ ] Expect: contains `"Account["` and `"not exist"` (exact string per actuator).

Frozen balance branches
- [ ] Add `validate_fail_frozen_balance_zero`:
  - [ ] `frozenBalance = 0`
  - [ ] Expect: `"frozenBalance must be positive"`.
- [ ] Add `happy_path_frozen_balance_exact_1_trx`:
  - [ ] `frozenBalance = 1 * ONE_TRX`
  - [ ] Expect: `SUCCESS`.
- [ ] Add `happy_path_frozen_balance_equal_account_balance`:
  - [ ] Seed account with balance `X`, freeze `X`
  - [ ] Expect: `SUCCESS` and post balance is `0`.

Frozen duration branches (checkFrozenTime=1)
- [ ] Add `validate_fail_frozen_duration_too_short`:
  - [ ] Use `frozenDuration = minFrozenTime - 1` (default min is `3`)
  - [ ] Expect: message starts with `"frozenDuration must be less than"`.
- [ ] Add `validate_fail_frozen_duration_too_long`:
  - [ ] Use `frozenDuration = maxFrozenTime + 1` (default max is `3`)
  - [ ] Expect: same duration error message (but distinct input).

Pre-state guard (rare but explicitly validated)
- [ ] Add `validate_fail_frozen_count_not_0_or_1`:
  - [ ] Seed account with 2 `Account.Frozen` entries (use `AccountCapsule#setFrozen(...)` test helper)
  - [ ] Expect: `"frozenCount must be 0 or 1"`.

Resource code validation
- [ ] Add `validate_fail_resource_tron_power_when_new_resource_model_off`:
  - [ ] Ensure `ALLOW_NEW_RESOURCE_MODEL = 0`
  - [ ] `resource = TRON_POWER`
  - [ ] Expect: `"ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]"`.

Receiver set while delegation is OFF (edge: ignored by Java-tron)
- [ ] Add `edge_receiver_address_ignored_when_delegation_off`:
  - [ ] Ensure `ALLOW_DELEGATE_RESOURCE = 0`
  - [ ] Set a non-empty `receiverAddress`
  - [ ] Expect: `SUCCESS` and behavior matches self-freeze (no delegated DBs touched).

Multi-freeze execution semantics
- [ ] Add `edge_freeze_bandwidth_twice_accumulates`:
  - [ ] Execute two freeze transactions for the same owner (separate fixtures or a single fixture that seeds
        pre-state with an existing frozen entry).
  - [ ] Expect: post frozen balance is the sum; expireTime reflects the second freeze’s `now + duration`.

Phase 2 — FreezeBalanceContract (11) Delegation-Enabled Fixtures (optional but high value)

Enable delegation mode for this phase
- [ ] In per-test setup, set `DynamicPropertiesStore.saveAllowDelegateResource(1)` (and optionally
      `saveChangeDelegation(1)` if reward delegation side effects matter).
- [ ] Include DBs in metadata:
  - [ ] `account`, `dynamic-properties`, `DelegatedResource`, `DelegatedResourceAccountIndex`

Happy delegation
- [ ] Add `happy_path_delegate_freeze_bandwidth`:
  - [ ] `receiverAddress` is a different, existing account
  - [ ] Expect: `SUCCESS` and delegated stores updated.
- [ ] Add `happy_path_delegate_freeze_energy`:
  - [ ] Same but `resource = ENERGY`.

Delegation validation failures
- [ ] Add `validate_fail_receiver_same_as_owner`:
  - [ ] Expect: `"receiverAddress must not be the same as ownerAddress"`.
- [ ] Add `validate_fail_receiver_invalid_address`:
  - [ ] Expect: `"Invalid receiverAddress"`.
- [ ] Add `validate_fail_receiver_account_not_exist`:
  - [ ] Expect: `"Account[" + receiver + "] not exists"` (exact string).
- [ ] (Optional) Add `validate_fail_delegate_to_contract_address`:
  - [ ] Enable `allowTvmConstantinople=1`, set receiver account type to Contract
  - [ ] Expect: `"Do not allow delegate resources to contract addresses"`.

Phase 3 — UnfreezeBalanceContract (12) Missing Fixtures (delegation OFF, new resource model OFF)

Owner/address/account branches
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`
  - [ ] Expect: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Use valid-looking address not in `AccountStore`
  - [ ] Expect: `"Account[" + owner + "] does not exist"`.

BANDWIDTH expiration boundary + multiple entries
- [ ] Add `edge_expire_time_equals_now_is_unfreezable_bandwidth`:
  - [ ] Seed a frozen entry with `expireTime == latestBlockHeaderTimestamp` before execution
  - [ ] Expect: `SUCCESS` and unfreezes that entry (validate uses `<= now`).
- [ ] Add `edge_partial_unfreeze_one_expired_one_not`:
  - [ ] Seed two frozen entries: one `expireTime < now`, one `expireTime > now`
  - [ ] Expect: `SUCCESS`, unfreeze amount equals only the expired entry, and one frozen entry remains.
- [ ] Add `edge_multiple_expired_entries_unfreeze_sum`:
  - [ ] Seed two expired frozen entries
  - [ ] Expect: `SUCCESS` and unfreeze amount equals the sum.

ENERGY resource coverage
- [ ] Add `happy_path_unfreeze_energy_v1`:
  - [ ] Seed `AccountResource.frozenBalanceForEnergy` with `expireTime < now`
  - [ ] Expect: `SUCCESS`.
- [ ] Add `validate_fail_unfreeze_energy_not_expired`:
  - [ ] `expireTime > now`
  - [ ] Expect: `"It's not time to unfreeze(Energy)."` (exact string).
- [ ] Add `validate_fail_unfreeze_energy_no_frozen`:
  - [ ] `frozenBalanceForEnergy == 0`
  - [ ] Expect: `"no frozenBalance(Energy)"`.

Invalid resource code
- [ ] Add `validate_fail_unfreeze_tron_power_when_new_resource_model_off`:
  - [ ] Ensure `ALLOW_NEW_RESOURCE_MODEL = 0`, set `resource = TRON_POWER`
  - [ ] Expect: `"ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]"`.

Receiver set while delegation is OFF (edge: ignored by Java-tron)
- [ ] Add `edge_receiver_address_ignored_when_delegation_off`:
  - [ ] Ensure `ALLOW_DELEGATE_RESOURCE = 0`, set non-empty `receiverAddress`
  - [ ] Expect: behaves like self-unfreeze (no delegated DBs touched).

V2-open compatibility (important cross-impl behavior)
- [ ] Add `edge_unfreeze_v1_succeeds_when_v2_open`:
  - [ ] Set `unfreezeDelayDays > 0` (V2 open) but seed a legacy V1 frozen entry
  - [ ] Expect: V1 unfreeze `SUCCESS` (since `UnfreezeBalanceActuator` doesn’t reject V2-open).

Phase 4 — UnfreezeBalanceContract (12) Delegation-Enabled Fixtures (optional but high value)

Enable delegation mode for this phase
- [ ] Set `DynamicPropertiesStore.saveAllowDelegateResource(1)`
- [ ] Include DBs in metadata:
  - [ ] `account`, `dynamic-properties`, `votes`, `DelegatedResource`, `DelegatedResourceAccountIndex`

Delegated unfreeze happy path
- [ ] Add `happy_path_unfreeze_delegated_bandwidth`:
  - [ ] Seed `DelegatedResource` (owner->receiver) with expired BANDWIDTH delegation
  - [ ] Expect: `SUCCESS`, delegated entry cleared (and deleted if both resources are 0).
- [ ] Add `happy_path_unfreeze_delegated_energy`:
  - [ ] Same for ENERGY.

Delegated unfreeze validation failures
- [ ] Add `validate_fail_receiver_same_as_owner`:
  - [ ] Expect: `"receiverAddress must not be the same as ownerAddress"`.
- [ ] Add `validate_fail_receiver_invalid_address`:
  - [ ] Expect: `"Invalid receiverAddress"`.
- [ ] Add `validate_fail_delegated_resource_not_exist`:
  - [ ] Receiver set, but no `DelegatedResource` entry
  - [ ] Expect: `"delegated Resource does not exist"`.
- [ ] Add `validate_fail_no_delegated_frozen_balance`:
  - [ ] DelegatedResource exists but frozen balance for the resource is `0`
  - [ ] Expect: `"no delegatedFrozenBalance(BANDWIDTH)"` / `"no delegateFrozenBalance(Energy)"`.
- [ ] Add `validate_fail_delegated_not_expired`:
  - [ ] DelegatedResource expireTime > now
  - [ ] Expect: `"It's not time to unfreeze."`.

Phase 5 — Validate Fixture Output
- [ ] Run the test class and regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.FreezeV1FixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
- [ ] Spot-check generated `metadata.json` files for exact error messages and correct `databasesTouched`.

