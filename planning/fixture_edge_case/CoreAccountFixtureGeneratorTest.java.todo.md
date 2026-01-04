# CoreAccountFixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/CoreAccountFixtureGeneratorTest.java`
  fixture generation so conformance covers the key missing validation branches and feature-flag
  dependent execution behavior for:
  - `AccountCreateContract` (0)
  - `AccountUpdateContract` (10)

Non-Goals
- Do not change actuator validation/execute rules; only add/adjust fixtures to reflect current
  java-tron behavior.
- Do not refactor fixture generator infrastructure (keep changes localized to the test class).

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, and `expected/post_db/`.
- For validation failures: `metadata.json.expectedStatus == "VALIDATION_FAILED"` and
  `expectedErrorMessage` matches the thrown `ContractValidateException` message.
- For happy paths: `metadata.json.expectedStatus == "SUCCESS"` and post-state DBs reflect the intended
  state transitions.
- `caseCategory` and `description` are consistent with the observed result (avoid "validate_fail but SUCCESS").

Checklist / TODO

Phase 0 — Confirm Baselines
- [x] Skim the validate paths to align fixtures with real branches:
  - [x] `actuator/src/main/java/org/tron/core/actuator/CreateAccountActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/UpdateAccountActuator.java`
  - [x] `common/src/main/java/org/tron/common/utils/DecodeUtil.java` (`addressValid`)
  - [x] `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java` (`validAccountName`)
- [ ] Run once to confirm which existing cases match their intent:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.CoreAccountFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
  - [x] Verify whether `generateAccountUpdate_validateFailInvalidName` produces `SUCCESS`.
        **CONFIRMED:** Empty name is allowed by `validAccountName`, replaced with 201-byte test.

Phase 1 — AccountCreateContract (0) Fixtures

Missing validation branch: invalid owner address
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`.
  - [x] Expect validate error substring: `"Invalid ownerAddress"`.
  - [x] DBs: `account`, `dynamic-properties`.
- [x] Add `validate_fail_owner_address_invalid_prefix` (optional but nice):
  - [x] 21-byte address with wrong prefix byte.
  - [x] Expect validate error substring: `"Invalid ownerAddress"`.
- [x] Add `validate_fail_owner_address_wrong_length`:
  - [x] 20-byte address (wrong length).
  - [x] Expect validate error substring: `"Invalid ownerAddress"`.

Missing validation branch: invalid target account address
- [x] Add `validate_fail_account_address_invalid_empty`:
  - [x] `account_address = ByteString.EMPTY`.
  - [x] Expect validate error substring: `"Invalid account address"`.
- [x] Add `validate_fail_account_address_invalid_length`:
  - [x] 22-byte address (wrong length).
  - [x] Expect validate error substring: `"Invalid account address"`.

Fee boundary conditions
- [x] Add `edge_happy_balance_equals_fee`:
  - [x] Seed owner with `balance == CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`.
  - [x] Expect `SUCCESS`.
- [x] Add `validate_fail_balance_fee_minus_one`:
  - [x] Seed owner with `balance == fee - 1`.
  - [x] Expect validate error contains: `"insufficient fee"`.

Feature-flag dependent execute behavior
- [x] Add `edge_happy_allow_multi_sign_enabled_default_permissions`:
  - [x] Set `ALLOW_MULTI_SIGN = 1`.
  - [x] Expect `SUCCESS`.
  - [x] Verify new account contains default `ownerPermission` and `activePermission`.
  - [x] Ensure `ACTIVE_DEFAULT_OPERATIONS` is initialized in dynamic props (getter throws if unset).
- [x] Add `edge_happy_blackhole_optimization_burns_fee`:
  - [x] Set `ALLOW_BLACKHOLE_OPTIMIZATION = 1`.
  - [x] Expect `SUCCESS`.
  - [x] Verify `dynamic-properties` reflects `BURN_TRX_AMOUNT` increment and blackhole account balance
        is not credited.
  - [x] Ensure `BURN_TRX_AMOUNT` is initialized (getter throws if unset).

Fee parameterization (optional)
- [x] Add `edge_happy_fee_zero`:
  - [x] Set `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT = 0`.
  - [x] Seed owner with 0 balance; expect `SUCCESS`.
  - [x] Verify no balance changes (or only deterministic no-ops) beyond account creation itself.

Phase 2 — AccountUpdateContract (10) Fixtures

Fix "invalid name" to hit the real validation rule
- [x] Replace/adjust `generateAccountUpdate_validateFailInvalidName`:
  - [x] Renamed to `generateAccountUpdate_validateFailInvalidNameTooLong`.
  - [x] Use `accountName` length 201 bytes (e.g., 201 `'a'` bytes).
  - [x] Expect validate error: `"Invalid accountName"`.

Owner address validity
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`.
  - [x] Expect validate error substring: `"Invalid ownerAddress"`.

Account name boundary success
- [x] Add `edge_happy_account_name_len_200`:
  - [x] `accountName` length exactly 200 bytes.
  - [x] Expect `SUCCESS`.

Missing allowUpdateAccountName branch: owner already has a name
- [x] Add `validate_fail_owner_already_named_updates_disabled`:
  - [x] Seed owner with a non-empty existing name in `AccountStore`.
  - [x] Ensure `ALLOW_UPDATE_ACCOUNT_NAME = 0`.
  - [x] Attempt to set a different (unique) name.
  - [x] Expect validate error: `"This account name is already existed"`.

Update-enabled behavior (`ALLOW_UPDATE_ACCOUNT_NAME = 1`)
- [x] Add `happy_update_existing_name_updates_enabled`:
  - [x] Seed owner with a non-empty existing name.
  - [x] Set `ALLOW_UPDATE_ACCOUNT_NAME = 1`.
  - [x] Update to a new name; expect `SUCCESS`.
- [x] Add `edge_happy_duplicate_name_updates_enabled_overwrites_index`:
  - [x] Seed two accounts.
  - [x] Set `ALLOW_UPDATE_ACCOUNT_NAME = 1`.
  - [x] Set account A name to `"dup"`, then set account B name to `"dup"`.
  - [x] Expect both `SUCCESS`; verify `account-index["dup"]` points to the last writer (B).

Phase 3 — Metadata / DB Capture Hygiene
- [x] Ensure `FixtureMetadata.database(...)` lists match stores actually mutated:
  - [x] CreateAccount: `account`, `dynamic-properties` (+ include `account-index` only if needed by new cases).
  - [x] UpdateAccount: `account`, `account-index`, `dynamic-properties`.
- [x] Include `dynamicProperty(...)` in metadata for behavior-driving flags in new cases:
  - [x] `ALLOW_MULTI_SIGN`, `ALLOW_BLACKHOLE_OPTIMIZATION`, `ALLOW_UPDATE_ACCOUNT_NAME`,
        `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`.
- [x] Use consistent `caseCategory` names:
  - [x] `happy` for success cases, `validate_fail` for validation failures, `edge` for boundary-success cases.
- [x] Fixed `expectedError` to match actual error messages from actuators.

Phase 4 — Regenerate & Validate Fixtures
- [ ] Run the class and regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.CoreAccountFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
- [ ] Spot-check representative `metadata.json` files for expectedStatus and error messages.
- [ ] (Optional) Run the conformance runner that consumes these fixtures to ensure no schema regressions.

---

## Implementation Summary

### New AccountCreateContract (0) Fixtures Added:
1. `generateAccountCreate_validateFailOwnerAddressEmpty` - Empty owner address
2. `generateAccountCreate_validateFailOwnerAddressWrongPrefix` - Wrong prefix byte (0x00 vs 0x41)
3. `generateAccountCreate_validateFailOwnerAddressWrongLength` - 20-byte address (should be 21)
4. `generateAccountCreate_validateFailAccountAddressEmpty` - Empty target account address
5. `generateAccountCreate_validateFailAccountAddressWrongLength` - 22-byte address (should be 21)
6. `generateAccountCreate_edgeHappyBalanceEqualsFee` - Balance exactly equals fee (SUCCESS)
7. `generateAccountCreate_validateFailBalanceFeeMinus1` - Balance = fee - 1 (FAIL)
8. `generateAccountCreate_edgeHappyAllowMultiSignEnabled` - ALLOW_MULTI_SIGN=1 with default permissions
9. `generateAccountCreate_edgeHappyBlackholeOptimizationBurnsFee` - ALLOW_BLACKHOLE_OPTIMIZATION=1
10. `generateAccountCreate_edgeHappyFeeZero` - Fee=0, zero balance owner can create

### New/Modified AccountUpdateContract (10) Fixtures:
1. `generateAccountUpdate_validateFailInvalidNameTooLong` - **FIXED:** 201 bytes instead of empty
2. `generateAccountUpdate_validateFailOwnerAddressEmpty` - Empty owner address
3. `generateAccountUpdate_edgeHappyAccountNameLen200` - Exactly 200 bytes (max allowed)
4. `generateAccountUpdate_validateFailOwnerAlreadyNamedUpdatesDisabled` - Owner has name + updates disabled
5. `generateAccountUpdate_happyUpdateExistingNameUpdatesEnabled` - ALLOW_UPDATE_ACCOUNT_NAME=1
6. `generateAccountUpdate_edgeHappyDuplicateNameUpdatesEnabledOverwritesIndex` - Duplicate name overwrites index

### Fixes to Existing Fixtures:
- `generateAccountCreate_validateFailInsufficientFee`: Updated `expectedError` from `"balance"` to `"insufficient fee"`
- `generateAccountUpdate_validateFailDuplicateNameUpdatesDisabled`: Updated `expectedError` from `"exist"` to `"This name is existed"`

