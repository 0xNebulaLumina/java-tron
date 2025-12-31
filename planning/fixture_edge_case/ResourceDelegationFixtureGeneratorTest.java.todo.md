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
- For time-sensitive behaviors, embedded execution “now” (dynamic props) and remote context timestamp are aligned.

Checklist / TODO

Phase 0 — Confirm Baselines + Make Fixture Output Deterministic
- [ ] Record exact validate error messages and gating conditions:
  - [ ] `actuator/src/main/java/org/tron/core/actuator/WithdrawExpireUnfreezeActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/DelegateResourceActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/UnDelegateResourceActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/CancelAllUnfreezeV2Actuator.java`
  - [ ] `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java`:
    - [ ] `supportUnfreezeDelay()`
    - [ ] `supportDR()`
    - [ ] `supportAllowCancelAllUnfreezeV2()`
    - [ ] `supportMaxDelegateLockPeriod()`
  - [ ] `chainbase/src/main/java/org/tron/core/store/DelegatedResourceStore.java` (`unLockExpireResource` uses `< now`)
- [ ] Align block context + dynamic props (avoid “now” mismatches):
  - [ ] Prefer `ConformanceFixtureTestSupport.createBlockContext(dbManager, witnessAddr)` to keep
    `latestBlockHeaderTimestamp/Number/Hash` consistent with `blockCap`.
- [ ] Make timestamps deterministic:
  - [ ] Prefer `ConformanceFixtureTestSupport.createTransaction(...)` (fixed tx timestamp/expiration/feeLimit).
- [ ] Run the generator once to confirm current baseline output:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ResourceDelegationFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`

Phase 1 — WithdrawExpireUnfreezeContract (56) Missing Fixtures

Feature gating
- [ ] Add `validate_fail_feature_not_enabled`:
  - [ ] Set `DynamicPropertiesStore.saveUnfreezeDelayDays(0)`
  - [ ] Expect: `"Not support WithdrawExpireUnfreeze transaction, need to be opened by the committee"`.

Owner/address/account validation
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`
  - [ ] Expect: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Use a valid-looking address not in `AccountStore`
  - [ ] Expect: `"Account[... ] not exists"`.

Time/list handling
- [ ] Add `edge_mixed_expired_and_unexpired_entries`:
  - [ ] Seed one `unfrozenV2` with `expireTime < now` and one with `expireTime > now`
  - [ ] Expect: `SUCCESS`, `withdrawExpireAmount == sum(expired)`, and unexpired entry remains in `unfrozenV2List`.
- [ ] Add `edge_expire_time_equals_now_is_withdrawable`:
  - [ ] Seed entry with `expireTime == now`
  - [ ] Expect: treated as expired (`<= now`) and withdrawn.

Overflow protection (optional but high value)
- [ ] Add `validate_fail_balance_overflow_on_withdraw`:
  - [ ] Seed account balance near `Long.MAX_VALUE` and an expired unfreeze amount that overflows add
  - [ ] Expect: validation failure with `ArithmeticException` message.

Phase 2 — DelegateResourceContract (57) Missing Fixtures

Feature gating
- [ ] Add `validate_fail_delegate_disabled_supportDR`:
  - [ ] Set `DynamicPropertiesStore.saveAllowDelegateResource(0)`
  - [ ] Expect: `"No support for resource delegate"`.
- [ ] Add `validate_fail_unfreeze_delay_disabled`:
  - [ ] Set `DynamicPropertiesStore.saveUnfreezeDelayDays(0)` (keep allowDelegateResource enabled)
  - [ ] Expect: `"Not support Delegate resource transaction, need to be opened by the committee"`.

Owner/address/account validation
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`
  - [ ] Expect: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Valid-looking owner not in `AccountStore`
  - [ ] Expect: `"Account[... ] not exists"`.

Delegate amount boundaries
- [ ] Add `validate_fail_delegate_balance_lt_1_trx`:
  - [ ] `balance = ONE_TRX - 1`
  - [ ] Expect: `"delegateBalance must be greater than or equal to 1 TRX"`.
- [ ] Add `happy_path_delegate_balance_exact_1_trx`:
  - [ ] `balance = ONE_TRX`
  - [ ] Expect: `SUCCESS`.

Resource code validation
- [ ] Add `validate_fail_resource_unrecognized_value`:
  - [ ] Use `DelegateResourceContract.Builder#setResourceValue(999)`
  - [ ] Expect: `"ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]"`.

Receiver/address/account validation
- [ ] Add `validate_fail_receiver_address_invalid_empty`:
  - [ ] `receiver_address = ByteString.EMPTY`
  - [ ] Expect: `"Invalid receiverAddress"`.
- [ ] Add `validate_fail_receiver_account_not_exist`:
  - [ ] Receiver address not in `AccountStore`
  - [ ] Expect: `"Account[... ] not exists"`.
- [ ] Add `validate_fail_receiver_is_contract_account`:
  - [ ] Seed receiver as `AccountType.Contract`
  - [ ] Expect: `"Do not allow delegate resources to contract addresses"`.

Lock semantics (max-lock feature)
- [ ] Enable `supportMaxDelegateLockPeriod()` for lock-period validation:
  - [ ] Set `DynamicPropertiesStore.saveMaxDelegateLockPeriod(X)` where `X > DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL`
- [ ] Add `validate_fail_lock_period_negative`:
  - [ ] `lock=true`, `lockPeriod=-1`
  - [ ] Expect: `"The lock period of delegate resource cannot be less than 0..."`.
- [ ] Add `validate_fail_lock_period_exceeds_max`:
  - [ ] `lock=true`, `lockPeriod=max+1`
  - [ ] Expect: `"The lock period of delegate resource cannot be less than 0 and cannot exceed <max>!"`.
- [ ] Add `validate_fail_lock_period_less_than_remaining_previous_lock`:
  - [ ] Seed an existing locked delegation with remaining time `R`
  - [ ] Attempt new delegate with `lockPeriod * interval < R`
  - [ ] Expect: `validRemainTime(...)` error string.
- [ ] Add `edge_lock_period_zero_defaults`:
  - [ ] `lock=true`, `lockPeriod=0` (supportMaxDelegateLockPeriod enabled)
  - [ ] Expect: `SUCCESS` and expireTime uses default `DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL`.

Phase 3 — UnDelegateResourceContract (58) Missing Fixtures

Feature gating
- [ ] Add `validate_fail_undelegate_disabled_supportDR`:
  - [ ] Set `DynamicPropertiesStore.saveAllowDelegateResource(0)`
  - [ ] Expect: `"No support for resource delegate"`.
- [ ] Add `validate_fail_unfreeze_delay_disabled`:
  - [ ] Set `DynamicPropertiesStore.saveUnfreezeDelayDays(0)`
  - [ ] Expect: `"Not support unDelegate resource transaction, need to be opened by the committee"`.

Address + amount validation
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`
  - [ ] Expect: `"Invalid address"`.
- [ ] Add `validate_fail_receiver_address_invalid_empty`:
  - [ ] `receiver_address = ByteString.EMPTY`
  - [ ] Expect: `"Invalid receiverAddress"`.
- [ ] Add `validate_fail_receiver_equals_owner`:
  - [ ] `receiver_address == owner_address`
  - [ ] Expect: `"receiverAddress must not be the same as ownerAddress"`.
- [ ] Add `validate_fail_unDelegate_balance_zero`:
  - [ ] `balance = 0`
  - [ ] Expect: `"unDelegateBalance must be more than 0 TRX"`.
- [ ] Add `validate_fail_resource_unrecognized_value`:
  - [ ] Use `UnDelegateResourceContract.Builder#setResourceValue(999)`
  - [ ] Expect: `"ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]"`.

Locked delegation boundary (strict `< now`)
- [ ] Add `validate_fail_only_locked_delegation_not_expired`:
  - [ ] Seed `DelegatedResourceStore` lock-key (`createDbKeyV2(..., true)`) with `expireTime >= now`
  - [ ] Attempt undelegate
  - [ ] Expect: `"insufficient delegatedFrozenBalance(...)"`.
- [ ] Add `validate_fail_locked_expire_time_equals_now`:
  - [ ] Same as above but `expireTime == now`
  - [ ] Expect: still locked and fails (since unlock logic requires `< now`).

Execution path gaps
- [ ] Add `happy_path_full_undelegate_deletes_store_and_index`:
  - [ ] Seed delegation and `DelegatedResourceAccountIndexStore` entry (or create via `DelegateResourceContract`)
  - [ ] Undelegate the full amount
  - [ ] Expect: `DelegatedResourceStore` entry removed and index store updated via `unDelegateV2(...)`.
- [ ] Add `happy_path_receiver_account_missing`:
  - [ ] Do not create receiver account, but seed delegation + owner balances
  - [ ] Expect: `SUCCESS` and no NPE; receiver-side updates are skipped safely.

Phase 4 — CancelAllUnfreezeV2Contract (59) Missing Fixtures

Feature gating nuance
- [ ] Add `validate_fail_unfreeze_delay_disabled`:
  - [ ] Set `DynamicPropertiesStore.saveUnfreezeDelayDays(0)` while keeping `ALLOW_CANCEL_ALL_UNFREEZE_V2 = 1`
  - [ ] Expect: `"Not support CancelAllUnfreezeV2 transaction, need to be opened by the committee"`.

Owner/address/account validation
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`
  - [ ] Expect: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Valid-looking owner not in `AccountStore`
  - [ ] Expect: `"Account[... ] not exists"`.

TRON_POWER coverage
- [ ] Add `happy_path_tron_power_unexpired_refreezes`:
  - [ ] Seed `unfrozenV2(type=TRON_POWER, expireTime > now, amount > 0)`
  - [ ] Expect: `SUCCESS`, TRON power V2 frozen increases, `totalTronPowerWeight` increases by floor(amount/ONE_TRX).
- [ ] Add `happy_path_tron_power_expired_withdraws`:
  - [ ] Seed `unfrozenV2(type=TRON_POWER, expireTime <= now)`
  - [ ] Expect: `SUCCESS`, amount contributes to `withdrawExpireAmount`, no TRON power refreeze.

Time boundary
- [ ] Add `edge_expire_time_equals_now_treated_as_expired`:
  - [ ] Seed entry with `expireTime == now`
  - [ ] Expect: withdrawn (not refrozen).

List composition + rounding
- [ ] Add `edge_all_entries_expired_withdraw_only`:
  - [ ] Seed only expired entries across multiple resource types
  - [ ] Expect: withdraw-only behavior; cancel amounts map entries are `0`.
- [ ] Add `edge_multiple_entries_same_resource_sums`:
  - [ ] Seed multiple unexpired entries of the same resource (e.g. BANDWIDTH twice)
  - [ ] Expect: amounts sum and weight delta matches flooring rules.
- [ ] Add `edge_amount_not_multiple_of_trx_precision_rounding`:
  - [ ] Use an unexpired `unfreezeAmount = ONE_TRX + 1` (or `1`) to pin floor-division behavior in weight updates.

Phase 5 — Validate Fixture Output
- [ ] Regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ResourceDelegationFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
- [ ] Spot-check `metadata.json` + `expected/post_db`:
  - [ ] `expectedStatus` matches (SUCCESS vs VALIDATION_FAILED)
  - [ ] `expectedErrorMessage` matches actuator string for validate-fail
  - [ ] `dynamic-properties` deltas present for `CancelAllUnfreezeV2Contract`
  - [ ] `DelegatedResourceAccountIndex` deltas present for full undelegate fixtures (if seeded)
