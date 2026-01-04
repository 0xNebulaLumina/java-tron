# ResourceDelegationFixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/ResourceDelegationFixtureGeneratorTest.java`
  so conformance covers major validation branches and boundary behaviors for:
  - `WithdrawExpireUnfreezeContract` (56)
  - `DelegateResourceContract` (57)
  - `UnDelegateResourceContract` (58)
  - `CancelAllUnfreezeV2Contract` (59)

Non-Goals
- Do not change actuator validation/execution logic; only add/adjust fixtures to reflect current behavior.
- Avoid refactors unless needed for determinism (timestamps / block-context alignment).

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, `expected/post_db/`, and `metadata.json`.
- Validation failures produce `metadata.json.expectedStatus == "VALIDATION_FAILED"` with the exact
  `ContractValidateException` message captured.
- Happy fixtures execute successfully and mutate expected DB state (`account`, `DelegatedResource`, index stores,
  and `dynamic-properties` where applicable).
- For time-sensitive behaviors, embedded execution "now" (dynamic props) and remote context timestamp are aligned.

Checklist / TODO

Phase 0 — Confirm Baselines + Make Fixture Output Deterministic
- [x] Record exact validate error messages and gating conditions:
  - [x] `actuator/src/main/java/org/tron/core/actuator/WithdrawExpireUnfreezeActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/DelegateResourceActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/UnDelegateResourceActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/CancelAllUnfreezeV2Actuator.java`
  - [x] `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java`:
    - [x] `supportUnfreezeDelay()`
    - [x] `supportDR()`
    - [x] `supportAllowCancelAllUnfreezeV2()`
    - [x] `supportMaxDelegateLockPeriod()`
  - [x] `chainbase/src/main/java/org/tron/core/store/DelegatedResourceStore.java` (`unLockExpireResource` uses `< now`)
- [x] Align block context + dynamic props (avoid "now" mismatches):
  - [x] Updated `createBlockContext()` to call `saveLatestBlockHeaderTimestamp/Number/Hash` after
    creating block, ensuring actuators see consistent "now" values.
- [x] Make timestamps deterministic:
  - [x] Updated `initializeTestData()` to use `DEFAULT_BLOCK_TIMESTAMP` instead of `System.currentTimeMillis()`.
  - [x] Updated `createTransaction()` to use `DEFAULT_TX_TIMESTAMP` and `DEFAULT_TX_EXPIRATION`.
- [ ] Run the generator once to confirm current baseline output:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ResourceDelegationFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`

Phase 1 — WithdrawExpireUnfreezeContract (56) Missing Fixtures

Feature gating
- [x] Add `validate_fail_feature_not_enabled`:
  - [x] Set `DynamicPropertiesStore.saveUnfreezeDelayDays(0)`
  - [x] Expect: `"Not support WithdrawExpireUnfreeze transaction, need to be opened by the committee"`.

Owner/address/account validation
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`
  - [x] Expect: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking address not in `AccountStore`
  - [x] Expect: `"Account[... ] not exists"`.

Time/list handling
- [x] Add `edge_mixed_expired_and_unexpired_entries`:
  - [x] Seed one `unfrozenV2` with `expireTime < now` and one with `expireTime > now`
  - [x] Expect: `SUCCESS`, `withdrawExpireAmount == sum(expired)`, and unexpired entry remains in `unfrozenV2List`.
- [x] Add `edge_expire_time_equals_now_is_withdrawable`:
  - [x] Seed entry with `expireTime == now`
  - [x] Expect: treated as expired (`<= now`) and withdrawn.

Overflow protection (optional but high value)
- [x] Add `validate_fail_balance_overflow_on_withdraw`:
  - [x] Seed account balance near `Long.MAX_VALUE` and an expired unfreeze amount that overflows add
  - [x] Expect: validation failure with `ArithmeticException` message.

Phase 2 — DelegateResourceContract (57) Missing Fixtures

Feature gating
- [x] Add `validate_fail_delegate_disabled_supportDR`:
  - [x] Set `DynamicPropertiesStore.saveAllowDelegateResource(0)`
  - [x] Expect: `"No support for resource delegate"`.
- [x] Add `validate_fail_unfreeze_delay_disabled`:
  - [x] Set `DynamicPropertiesStore.saveUnfreezeDelayDays(0)` (keep allowDelegateResource enabled)
  - [x] Expect: `"Not support Delegate resource transaction, need to be opened by the committee"`.

Owner/address/account validation
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`
  - [x] Expect: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Valid-looking owner not in `AccountStore`
  - [x] Expect: `"Account[... ] not exists"`.

Delegate amount boundaries
- [x] Add `validate_fail_delegate_balance_lt_1_trx`:
  - [x] `balance = ONE_TRX - 1`
  - [x] Expect: `"delegateBalance must be greater than or equal to 1 TRX"`.
- [x] Add `happy_path_delegate_balance_exact_1_trx`:
  - [x] `balance = ONE_TRX`
  - [x] Expect: `SUCCESS`.

Resource code validation
- [x] Add `validate_fail_resource_unrecognized_value`:
  - [x] Use `DelegateResourceContract.Builder#setResourceValue(999)`
  - [x] Expect: `"ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]"`.

Receiver/address/account validation
- [x] Add `validate_fail_receiver_address_invalid_empty`:
  - [x] `receiver_address = ByteString.EMPTY`
  - [x] Expect: `"Invalid receiverAddress"`.
- [x] Add `validate_fail_receiver_account_not_exist`:
  - [x] Receiver address not in `AccountStore`
  - [x] Expect: `"Account[... ] not exists"`.
- [x] Add `validate_fail_receiver_is_contract_account`:
  - [x] Seed receiver as `AccountType.Contract`
  - [x] Expect: `"Do not allow delegate resources to contract addresses"`.

Lock semantics (max-lock feature)
- [x] Enable `supportMaxDelegateLockPeriod()` for lock-period validation:
  - [x] Set `DynamicPropertiesStore.saveMaxDelegateLockPeriod(X)` where `X > DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL`
- [x] Add `validate_fail_lock_period_negative`:
  - [x] `lock=true`, `lockPeriod=-1`
  - [x] Expect: `"The lock period of delegate resource cannot be less than 0..."`.
- [x] Add `validate_fail_lock_period_exceeds_max`:
  - [x] `lock=true`, `lockPeriod=max+1`
  - [x] Expect: `"The lock period of delegate resource cannot be less than 0 and cannot exceed <max>!"`.
- [x] Add `validate_fail_lock_period_less_than_remaining_previous_lock`:
  - [x] Seed an existing locked delegation with remaining time `R`
  - [x] Attempt new delegate with `lockPeriod * interval < R`
  - [x] Expect: `validRemainTime(...)` error string.
- [x] Add `edge_lock_period_zero_defaults`:
  - [x] `lock=true`, `lockPeriod=0` (supportMaxDelegateLockPeriod enabled)
  - [x] Expect: `SUCCESS` and expireTime uses default `DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL`.

Phase 3 — UnDelegateResourceContract (58) Missing Fixtures

Feature gating
- [x] Add `validate_fail_undelegate_disabled_supportDR`:
  - [x] Set `DynamicPropertiesStore.saveAllowDelegateResource(0)`
  - [x] Expect: `"No support for resource delegate"`.
- [x] Add `validate_fail_unfreeze_delay_disabled`:
  - [x] Set `DynamicPropertiesStore.saveUnfreezeDelayDays(0)`
  - [x] Expect: `"Not support unDelegate resource transaction, need to be opened by the committee"`.

Address + amount validation
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`
  - [x] Expect: `"Invalid address"`.
- [x] Add `validate_fail_receiver_address_invalid_empty`:
  - [x] `receiver_address = ByteString.EMPTY`
  - [x] Expect: `"Invalid receiverAddress"`.
- [x] Add `validate_fail_receiver_equals_owner`:
  - [x] `receiver_address == owner_address`
  - [x] Expect: `"receiverAddress must not be the same as ownerAddress"`.
- [x] Add `validate_fail_unDelegate_balance_zero`:
  - [x] `balance = 0`
  - [x] Expect: `"unDelegateBalance must be more than 0 TRX"`.
- [x] Add `validate_fail_resource_unrecognized_value`:
  - [x] Use `UnDelegateResourceContract.Builder#setResourceValue(999)`
  - [x] Expect: `"ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]"`.

Locked delegation boundary (strict `< now`)
- [x] Add `validate_fail_only_locked_delegation_not_expired`:
  - [x] Seed `DelegatedResourceStore` lock-key (`createDbKeyV2(..., true)`) with `expireTime >= now`
  - [x] Attempt undelegate
  - [x] Expect: `"insufficient delegatedFrozenBalance(...)"`.
- [x] Add `validate_fail_locked_expire_time_equals_now`:
  - [x] Same as above but `expireTime == now`
  - [x] Expect: still locked and fails (since unlock logic requires `< now`).

Execution path gaps
- [x] Add `happy_path_full_undelegate_deletes_store_and_index`:
  - [x] Seed delegation and `DelegatedResourceAccountIndexStore` entry (or create via `DelegateResourceContract`)
  - [x] Undelegate the full amount
  - [x] Expect: `DelegatedResourceStore` entry removed and index store updated via `unDelegateV2(...)`.
- [x] Add `happy_path_receiver_account_missing`:
  - [x] Do not create receiver account, but seed delegation + owner balances
  - [x] Expect: `SUCCESS` and no NPE; receiver-side updates are skipped safely.

Phase 4 — CancelAllUnfreezeV2Contract (59) Missing Fixtures

Feature gating nuance
- [x] Add `validate_fail_unfreeze_delay_disabled`:
  - [x] Set `DynamicPropertiesStore.saveUnfreezeDelayDays(0)` while keeping `ALLOW_CANCEL_ALL_UNFREEZE_V2 = 1`
  - [x] Expect: `"Not support CancelAllUnfreezeV2 transaction, need to be opened by the committee"`.

Owner/address/account validation
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`
  - [x] Expect: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Valid-looking owner not in `AccountStore`
  - [x] Expect: `"Account[... ] not exists"`.

TRON_POWER coverage
- [x] Add `happy_path_tron_power_unexpired_refreezes`:
  - [x] Seed `unfrozenV2(type=TRON_POWER, expireTime > now, amount > 0)`
  - [x] Expect: `SUCCESS`, TRON power V2 frozen increases, `totalTronPowerWeight` increases by floor(amount/ONE_TRX).
- [x] Add `happy_path_tron_power_expired_withdraws`:
  - [x] Seed `unfrozenV2(type=TRON_POWER, expireTime <= now)`
  - [x] Expect: `SUCCESS`, amount contributes to `withdrawExpireAmount`, no TRON power refreeze.

Time boundary
- [x] Add `edge_expire_time_equals_now_treated_as_expired`:
  - [x] Seed entry with `expireTime == now`
  - [x] Expect: withdrawn (not refrozen).

List composition + rounding
- [x] Add `edge_all_entries_expired_withdraw_only`:
  - [x] Seed only expired entries across multiple resource types
  - [x] Expect: withdraw-only behavior; cancel amounts map entries are `0`.
- [x] Add `edge_multiple_entries_same_resource_sums`:
  - [x] Seed multiple unexpired entries of the same resource (e.g. BANDWIDTH twice)
  - [x] Expect: amounts sum and weight delta matches flooring rules.
- [x] Add `edge_amount_not_multiple_of_trx_precision_rounding`:
  - [x] Use an unexpired `unfreezeAmount = ONE_TRX + 1` (or `1`) to pin floor-division behavior in weight updates.

Phase 5 — Validate Fixture Output
- [ ] Regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ResourceDelegationFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
- [ ] Spot-check `metadata.json` + `expected/post_db`:
  - [ ] `expectedStatus` matches (SUCCESS vs VALIDATION_FAILED)
  - [ ] `expectedErrorMessage` matches actuator string for validate-fail
  - [ ] `dynamic-properties` deltas present for `CancelAllUnfreezeV2Contract`
  - [ ] `DelegatedResourceAccountIndex` deltas present for full undelegate fixtures (if seeded)
