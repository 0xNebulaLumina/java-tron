# AccountFixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/AccountFixtureGeneratorTest.java` fixture generation
  so conformance covers all major validation branches and boundary conditions for:
  - `SetAccountIdContract` (19)
  - `AccountPermissionUpdateContract` (46)

Non-Goals
- Do not change contract validation rules; only add/adjust fixtures to reflect current Java-tron behavior.
- Do not refactor fixture generator infrastructure (keep changes localized to the test class).

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, and `expected/post_db/`.
- For validation failures: `metadata.json.expectedStatus == "VALIDATION_FAILED"` and `expectedErrorMessage`
  matches the thrown `ContractValidateException` message.
- For execute failures (reverts): `metadata.json.expectedStatus == "REVERT"` and `expectedErrorMessage`
  matches the thrown `ContractExeException` message.
- "Happy" fixtures execute successfully and mutate the expected DBs.
- `caseCategory`/`description` remain consistent with the observed result (avoid "validate_fail but SUCCESS").

Checklist / TODO

Phase 0 — Confirm Baselines
- [x] Skim the actuator validate paths to ensure the fixtures align with real branches:
  - [x] `actuator/src/main/java/org/tron/core/actuator/SetAccountIdActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/AccountPermissionUpdateActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java` (`validAccountId`)
- [ ] Run (once) to see which existing fixtures are actually produced as intended:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.AccountFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
  - [ ] Verify `generateSetAccountId_invalidCharacters` and `generateAccountPermissionUpdate_witnessPermission` outcomes.

Phase 1 — SetAccountIdContract (19) Fixtures

Missing validation branch: invalid owner address
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] Build contract with `owner_address = ByteString.EMPTY` (or wrong-length bytes).
  - [x] Expect validate error substring: `"Invalid ownerAddress"`.
  - [x] Databases: `account`, `accountid-index`, `dynamic-properties`.
- [x] Add `validate_fail_owner_address_wrong_length` (bonus):
  - [x] Build contract with 10-byte owner address.

Missing validation branch: unreadable / non-printable bytes in accountId
- [x] Add `validate_fail_account_id_contains_space`:
  - [x] Use accountId with a space (0x20), e.g. `"ab  cdefgh"`.
  - [x] Expect validate error: `"Invalid accountId"`.
- [x] Add `validate_fail_account_id_control_char` (bonus):
  - [x] Use accountId with newline character.
- [x] Add `validate_fail_account_id_non_ascii`:
  - [x] Include a byte > 0x7E (e.g. `(char) 128`) in the accountId string.
  - [x] Expect validate error: `"Invalid accountId"`.

Boundary-happy fixtures
- [x] Add `happy_path_min_len_8`:
  - [x] accountId length exactly 8 (all bytes in 0x21..0x7E).
  - [x] Expect `SUCCESS`.
- [x] Add `happy_path_max_len_32`:
  - [x] accountId length exactly 32.
  - [x] Expect `SUCCESS`.

Clarity: explicit empty accountId
- [x] Add `validate_fail_account_id_empty`:
  - [x] `account_id = ByteString.EMPTY`.
  - [x] Expect validate error: `"Invalid accountId"`.

Phase 2 — AccountPermissionUpdateContract (46) Fixtures

Owner address validation and existence
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`.
  - [x] Expect validate error: `"invalidate ownerAddress"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking address not present in `AccountStore`.
  - [x] Expect validate error: `"ownerAddress account does not exist"`.

Missing required fields
- [x] Add `validate_fail_owner_permission_missing`:
  - [x] Build contract without `.setOwner(...)`.
  - [x] Include at least one active permission so the failure is specifically "owner permission is missed".
  - [x] Expect validate error: `"owner permission is missed"`.
- [x] Add `validate_fail_active_permission_missing`:
  - [x] Build contract with `.setOwner(...)` but no `.addActives(...)`.
  - [x] Ensure multi-sign enabled.
  - [x] Expect validate error: `"active permission is missed"`.
- [x] Add `validate_fail_active_permission_too_many`:
  - [x] Provide 9 active permissions (all valid).
  - [x] Expect validate error: `"active permission is too many"`.

Witness-specific required field (account is witness)
- [x] Ensure witness account is actually marked witness for these fixtures:
  - [x] In setup or per-test, call `AccountCapsule#setIsWitness(true)` for `WITNESS_ADDRESS`
        before storing in `AccountStore`.
- [x] Add `validate_fail_witness_permission_missing`:
  - [x] Owner is witness account, omit `.setWitness(...)`.
  - [x] Expect validate error: `"witness permission is missed"`.

Wrong permission types
- [x] Add `validate_fail_owner_permission_type_wrong`:
  - [x] Set owner permission `type = Active` (or Witness).
  - [x] Expect validate error: `"owner permission type is error"`.
- [x] Add `validate_fail_active_permission_type_wrong`:
  - [x] Provide an "active" permission entry with `type = Owner` (or Witness).
  - [x] Expect validate error: `"active permission type is error"`.
- [x] (Witness account) Add `validate_fail_witness_permission_type_wrong`:
  - [x] Set witness permission `type = Owner` (or Active).
  - [x] Expect validate error: `"witness permission type is error"`.

checkPermission(...) missing branches
- [x] Add `validate_fail_keys_count_zero`:
  - [x] Any permission with zero keys (e.g. owner keys list empty).
  - [x] Expect validate error: `"key's count should be greater than 0"`.
- [x] (Witness) Add `validate_fail_witness_keys_count_not_one`:
  - [x] Witness permission with 2 keys.
  - [x] Expect validate error: `"Witness permission's key count should be 1"`.
- [x] Add `validate_fail_threshold_zero`:
  - [x] Set threshold = 0 for a permission.
  - [x] Expect validate error: `"permission's threshold should be greater than 0"`.
- [x] Add `validate_fail_permission_name_too_long`:
  - [x] permissionName length 33.
  - [x] Expect validate error: `"permission's name is too long"`.
- [x] Add `validate_fail_parent_id_not_owner`:
  - [x] permission.parentId = 1.
  - [x] Expect validate error: `"permission's parent should be owner"`.
- [x] Add `validate_fail_key_address_invalid`:
  - [x] Set a key address to an invalid byte array (e.g. 10 bytes).
  - [x] Expect validate error: `"key is not a validate address"`.
- [x] Add `validate_fail_key_weight_zero`:
  - [x] Set a key weight to 0 (or negative).
  - [x] Expect validate error: `"key's weight should be greater than 0"`.
- [x] Add `validate_fail_non_active_has_operations`:
  - [x] Set `operations` on an owner (or witness) permission to non-empty.
  - [x] Expect validate error contains: `"permission needn't operations"`.

Active operations validation
- [x] Add `validate_fail_active_operations_empty`:
  - [x] Active permission with `operations = ByteString.EMPTY`.
  - [x] Expect validate error: `"operations size must 32"`.
- [x] Add `validate_fail_active_operations_wrong_size`:
  - [x] Active permission with 31-byte operations.
  - [x] Expect validate error: `"operations size must 32"`.
- [x] Add `validate_fail_active_operations_invalid_contract_type_bit`:
  - [x] Set `DynamicPropertiesStore.AVAILABLE_CONTRACT_TYPE` to all zeros (32 bytes).
  - [x] Create active operations with bit 0 set.
  - [x] Expect validate error: `"0 isn't a validate ContractType"`.

Phase 3 — Hygiene and Consistency
- [x] Ensure each new fixture includes correct DBs in `FixtureMetadata` (`account`, `dynamic-properties`, plus `witness` when relevant).
- [x] Prefer `caseCategory = "edge"` for boundary-success fixtures (min/max lengths), keep `"validate_fail"` for real validation failures.
- [x] Avoid relying on test execution order; treat each test as isolated (BaseTest setup should re-init state).

Phase 4 — Validate Output
- [ ] Run the single test class and regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.AccountFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
- [ ] Spot-check a few generated `metadata.json` files for expectedStatus and error messages.
- [ ] (Optional) Run the conformance runner that consumes these fixtures to ensure no schema regressions.

