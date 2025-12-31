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
- `caseCategory` and `description` are consistent with the observed result (avoid “validate_fail but SUCCESS”).

Checklist / TODO

Phase 0 — Confirm Baselines
- [ ] Skim the validate paths to align fixtures with real branches:
  - [ ] `actuator/src/main/java/org/tron/core/actuator/CreateAccountActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/UpdateAccountActuator.java`
  - [ ] `common/src/main/java/org/tron/common/utils/DecodeUtil.java` (`addressValid`)
  - [ ] `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java` (`validAccountName`)
- [ ] Run once to confirm which existing cases match their intent:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.CoreAccountFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
  - [ ] Verify whether `generateAccountUpdate_validateFailInvalidName` produces `SUCCESS`.

Phase 1 — AccountCreateContract (0) Fixtures

Missing validation branch: invalid owner address
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`.
  - [ ] Expect validate error substring: `"Invalid ownerAddress"`.
  - [ ] DBs: `account`, `dynamic-properties`.
- [ ] Add `validate_fail_owner_address_invalid_prefix` (optional but nice):
  - [ ] 21-byte address with wrong prefix byte.
  - [ ] Expect validate error substring: `"Invalid ownerAddress"`.

Missing validation branch: invalid target account address
- [ ] Add `validate_fail_account_address_invalid_empty`:
  - [ ] `account_address = ByteString.EMPTY`.
  - [ ] Expect validate error substring: `"Invalid account address"`.
- [ ] Add `validate_fail_account_address_invalid_length` (optional):
  - [ ] 20-byte or 22-byte address.
  - [ ] Expect validate error substring: `"Invalid account address"`.

Fee boundary conditions
- [ ] Add `edge_happy_balance_equals_fee`:
  - [ ] Seed owner with `balance == CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`.
  - [ ] Expect `SUCCESS`.
- [ ] Add `validate_fail_balance_fee_minus_one`:
  - [ ] Seed owner with `balance == fee - 1`.
  - [ ] Expect validate error contains: `"insufficient fee"`.

Feature-flag dependent execute behavior
- [ ] Add `edge_happy_allow_multi_sign_enabled_default_permissions`:
  - [ ] Set `ALLOW_MULTI_SIGN = 1`.
  - [ ] Expect `SUCCESS`.
  - [ ] Verify new account contains default `ownerPermission` and `activePermission`.
  - [ ] Ensure `ACTIVE_DEFAULT_OPERATIONS` is initialized in dynamic props (getter throws if unset).
- [ ] Add `edge_happy_blackhole_optimization_burns_fee`:
  - [ ] Set `ALLOW_BLACKHOLE_OPTIMIZATION = 1`.
  - [ ] Expect `SUCCESS`.
  - [ ] Verify `dynamic-properties` reflects `BURN_TRX_AMOUNT` increment and blackhole account balance
        is not credited.
  - [ ] Ensure `BURN_TRX_AMOUNT` is initialized (getter throws if unset).

Fee parameterization (optional)
- [ ] Add `edge_happy_fee_zero`:
  - [ ] Set `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT = 0`.
  - [ ] Seed owner with 0 balance; expect `SUCCESS`.
  - [ ] Verify no balance changes (or only deterministic no-ops) beyond account creation itself.

Phase 2 — AccountUpdateContract (10) Fixtures

Fix “invalid name” to hit the real validation rule
- [ ] Replace/adjust `generateAccountUpdate_validateFailInvalidName`:
  - [ ] Use `accountName` length 201 bytes (e.g., 201 `'a'` bytes).
  - [ ] Expect validate error: `"Invalid accountName"`.

Owner address validity
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`.
  - [ ] Expect validate error substring: `"Invalid ownerAddress"`.

Account name boundary success
- [ ] Add `edge_happy_account_name_len_200`:
  - [ ] `accountName` length exactly 200 bytes.
  - [ ] Expect `SUCCESS`.

Missing allowUpdateAccountName branch: owner already has a name
- [ ] Add `validate_fail_owner_already_named_updates_disabled`:
  - [ ] Seed owner with a non-empty existing name in `AccountStore`.
  - [ ] Ensure `ALLOW_UPDATE_ACCOUNT_NAME = 0`.
  - [ ] Attempt to set a different (unique) name.
  - [ ] Expect validate error: `"This account name is already existed"`.

Update-enabled behavior (`ALLOW_UPDATE_ACCOUNT_NAME = 1`)
- [ ] Add `happy_update_existing_name_updates_enabled`:
  - [ ] Seed owner with a non-empty existing name.
  - [ ] Set `ALLOW_UPDATE_ACCOUNT_NAME = 1`.
  - [ ] Update to a new name; expect `SUCCESS`.
- [ ] Add `edge_happy_duplicate_name_updates_enabled_overwrites_index`:
  - [ ] Seed two accounts.
  - [ ] Set `ALLOW_UPDATE_ACCOUNT_NAME = 1`.
  - [ ] Set account A name to `"dup"`, then set account B name to `"dup"`.
  - [ ] Expect both `SUCCESS`; verify `account-index["dup"]` points to the last writer (B).

Phase 3 — Metadata / DB Capture Hygiene
- [ ] Ensure `FixtureMetadata.database(...)` lists match stores actually mutated:
  - [ ] CreateAccount: `account`, `dynamic-properties` (+ include `account-index` only if needed by new cases).
  - [ ] UpdateAccount: `account`, `account-index`, `dynamic-properties`.
- [ ] Include `dynamicProperty(...)` in metadata for behavior-driving flags in new cases:
  - [ ] `ALLOW_MULTI_SIGN`, `ALLOW_BLACKHOLE_OPTIMIZATION`, `ALLOW_UPDATE_ACCOUNT_NAME`,
        `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`.
- [ ] Use consistent `caseCategory` names:
  - [ ] `happy` for success cases, `validate_fail` for validation failures, `edge` for boundary-success cases.

Phase 4 — Regenerate & Validate Fixtures
- [ ] Run the class and regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.CoreAccountFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
- [ ] Spot-check representative `metadata.json` files for expectedStatus and error messages.
- [ ] (Optional) Run the conformance runner that consumes these fixtures to ensure no schema regressions.

