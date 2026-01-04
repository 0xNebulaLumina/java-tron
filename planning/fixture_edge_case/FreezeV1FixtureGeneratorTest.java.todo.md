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
- [x] Skim validate/execute paths and record exact error messages to lock in:
  - [x] `actuator/src/main/java/org/tron/core/actuator/FreezeBalanceActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceActuator.java`
  - [x] `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java` (defaults for min/max frozen time)
- [x] Confirm which flags actually gate delegation:
  - [x] `DynamicPropertiesStore.supportDR()` (delegated resources) is controlled by `ALLOW_DELEGATE_RESOURCE`.
  - [x] `DynamicPropertiesStore.allowChangeDelegation()` (reward delegation) is controlled by `CHANGE_DELEGATION`.
- [ ] Run the current generator test to see baseline fixture outputs:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.FreezeV1FixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

Phase 1 — FreezeBalanceContract (11) Missing Fixtures (delegation OFF, new resource model OFF)

Owner/address/account branches
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`
  - [x] Expect: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking address not in `AccountStore`
  - [x] Expect: contains `"Account["` and `"not exist"` (exact string per actuator).

Frozen balance branches
- [x] Add `validate_fail_frozen_balance_zero`:
  - [x] `frozenBalance = 0`
  - [x] Expect: `"frozenBalance must be positive"`.
- [x] Add `happy_path_frozen_balance_exact_1_trx`:
  - [x] `frozenBalance = 1 * ONE_TRX`
  - [x] Expect: `SUCCESS`.
- [x] Add `happy_path_frozen_balance_equal_account_balance`:
  - [x] Seed account with balance `X`, freeze `X`
  - [x] Expect: `SUCCESS` and post balance is `0`.

Frozen duration branches (checkFrozenTime=1)
- [x] Add `validate_fail_frozen_duration_too_short`:
  - [x] Use `frozenDuration = minFrozenTime - 1` (default min is `3`)
  - [x] Expect: message starts with `"frozenDuration must be less than"`.
- [x] Add `validate_fail_frozen_duration_too_long`:
  - [x] Use `frozenDuration = maxFrozenTime + 1` (default max is `3`)
  - [x] Expect: same duration error message (but distinct input).

Pre-state guard (rare but explicitly validated)
- [x] Add `validate_fail_frozen_count_not_0_or_1`:
  - [x] Seed account with 2 `Account.Frozen` entries (use `AccountCapsule#setFrozen(...)` test helper)
  - [x] Expect: `"frozenCount must be 0 or 1"`.

Resource code validation
- [x] Add `validate_fail_resource_tron_power_when_new_resource_model_off`:
  - [x] Ensure `ALLOW_NEW_RESOURCE_MODEL = 0`
  - [x] `resource = TRON_POWER`
  - [x] Expect: `"ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]"`.

Receiver set while delegation is OFF (edge: ignored by Java-tron)
- [x] Add `edge_receiver_address_ignored_when_delegation_off`:
  - [x] Ensure `ALLOW_DELEGATE_RESOURCE = 0`
  - [x] Set a non-empty `receiverAddress`
  - [x] Expect: `SUCCESS` and behavior matches self-freeze (no delegated DBs touched).

Multi-freeze execution semantics
- [x] Add `edge_freeze_bandwidth_twice_accumulates`:
  - [x] Execute two freeze transactions for the same owner (separate fixtures or a single fixture that seeds
        pre-state with an existing frozen entry).
  - [x] Expect: post frozen balance is the sum; expireTime reflects the second freeze's `now + duration`.

Phase 2 — FreezeBalanceContract (11) Delegation-Enabled Fixtures (optional but high value)

Enable delegation mode for this phase
- [x] In per-test setup, set `DynamicPropertiesStore.saveAllowDelegateResource(1)` (and optionally
      `saveChangeDelegation(1)` if reward delegation side effects matter).
- [x] Include DBs in metadata:
  - [x] `account`, `dynamic-properties`, `DelegatedResource`, `DelegatedResourceAccountIndex`

Happy delegation
- [x] Add `happy_path_delegate_freeze_bandwidth`:
  - [x] `receiverAddress` is a different, existing account
  - [x] Expect: `SUCCESS` and delegated stores updated.
- [x] Add `happy_path_delegate_freeze_energy`:
  - [x] Same but `resource = ENERGY`.

Delegation validation failures
- [x] Add `validate_fail_receiver_same_as_owner`:
  - [x] Expect: `"receiverAddress must not be the same as ownerAddress"`.
- [x] Add `validate_fail_receiver_invalid_address`:
  - [x] Expect: `"Invalid receiverAddress"`.
- [x] Add `validate_fail_receiver_account_not_exist`:
  - [x] Expect: `"Account[" + receiver + "] not exists"` (exact string).
- [x] (Optional) Add `validate_fail_delegate_to_contract_address`:
  - [x] Enable `allowTvmConstantinople=1`, set receiver account type to Contract
  - [x] Expect: `"Do not allow delegate resources to contract addresses"`.

Phase 3 — UnfreezeBalanceContract (12) Missing Fixtures (delegation OFF, new resource model OFF)

Owner/address/account branches
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`
  - [x] Expect: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use valid-looking address not in `AccountStore`
  - [x] Expect: `"Account[" + owner + "] does not exist"`.

BANDWIDTH expiration boundary + multiple entries
- [x] Add `edge_expire_time_equals_now_is_unfreezable_bandwidth`:
  - [x] Seed a frozen entry with `expireTime == latestBlockHeaderTimestamp` before execution
  - [x] Expect: `SUCCESS` and unfreezes that entry (validate uses `<= now`).
- [x] Add `edge_partial_unfreeze_one_expired_one_not`:
  - [x] Seed two frozen entries: one `expireTime < now`, one `expireTime > now`
  - [x] Expect: `SUCCESS`, unfreeze amount equals only the expired entry, and one frozen entry remains.
- [x] Add `edge_multiple_expired_entries_unfreeze_sum`:
  - [x] Seed two expired frozen entries
  - [x] Expect: `SUCCESS` and unfreeze amount equals the sum.

ENERGY resource coverage
- [x] Add `happy_path_unfreeze_energy_v1`:
  - [x] Seed `AccountResource.frozenBalanceForEnergy` with `expireTime < now`
  - [x] Expect: `SUCCESS`.
- [x] Add `validate_fail_unfreeze_energy_not_expired`:
  - [x] `expireTime > now`
  - [x] Expect: `"It's not time to unfreeze(Energy)."` (exact string).
- [x] Add `validate_fail_unfreeze_energy_no_frozen`:
  - [x] `frozenBalanceForEnergy == 0`
  - [x] Expect: `"no frozenBalance(Energy)"`.

Invalid resource code
- [x] Add `validate_fail_unfreeze_tron_power_when_new_resource_model_off`:
  - [x] Ensure `ALLOW_NEW_RESOURCE_MODEL = 0`, set `resource = TRON_POWER`
  - [x] Expect: `"ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]"`.

Receiver set while delegation is OFF (edge: ignored by Java-tron)
- [x] Add `edge_receiver_address_ignored_when_delegation_off`:
  - [x] Ensure `ALLOW_DELEGATE_RESOURCE = 0`, set non-empty `receiverAddress`
  - [x] Expect: behaves like self-unfreeze (no delegated DBs touched).

V2-open compatibility (important cross-impl behavior)
- [x] Add `edge_unfreeze_v1_succeeds_when_v2_open`:
  - [x] Set `unfreezeDelayDays > 0` (V2 open) but seed a legacy V1 frozen entry
  - [x] Expect: V1 unfreeze `SUCCESS` (since `UnfreezeBalanceActuator` doesn't reject V2-open).

Phase 4 — UnfreezeBalanceContract (12) Delegation-Enabled Fixtures (optional but high value)

Enable delegation mode for this phase
- [x] Set `DynamicPropertiesStore.saveAllowDelegateResource(1)`
- [x] Include DBs in metadata:
  - [x] `account`, `dynamic-properties`, `votes`, `DelegatedResource`, `DelegatedResourceAccountIndex`

Delegated unfreeze happy path
- [x] Add `happy_path_unfreeze_delegated_bandwidth`:
  - [x] Seed `DelegatedResource` (owner->receiver) with expired BANDWIDTH delegation
  - [x] Expect: `SUCCESS`, delegated entry cleared (and deleted if both resources are 0).
- [x] Add `happy_path_unfreeze_delegated_energy`:
  - [x] Same for ENERGY.

Delegated unfreeze validation failures
- [x] Add `validate_fail_receiver_same_as_owner`:
  - [x] Expect: `"receiverAddress must not be the same as ownerAddress"`.
- [x] Add `validate_fail_receiver_invalid_address`:
  - [x] Expect: `"Invalid receiverAddress"`.
- [x] Add `validate_fail_delegated_resource_not_exist`:
  - [x] Receiver set, but no `DelegatedResource` entry
  - [x] Expect: `"delegated Resource does not exist"`.
- [x] Add `validate_fail_no_delegated_frozen_balance`:
  - [x] DelegatedResource exists but frozen balance for the resource is `0`
  - [x] Expect: `"no delegatedFrozenBalance(BANDWIDTH)"` / `"no delegateFrozenBalance(Energy)"`.
- [x] Add `validate_fail_delegated_not_expired`:
  - [x] DelegatedResource expireTime > now
  - [x] Expect: `"It's not time to unfreeze."`.

Phase 5 — Validate Fixture Output
- [ ] Run the test class and regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.FreezeV1FixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
- [ ] Spot-check generated `metadata.json` files for exact error messages and correct `databasesTouched`.

