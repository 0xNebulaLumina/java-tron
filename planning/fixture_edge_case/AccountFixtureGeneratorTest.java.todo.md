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
- “Happy” fixtures execute successfully and mutate the expected DBs.
- `caseCategory`/`description` remain consistent with the observed result (avoid “validate_fail but SUCCESS”).

Checklist / TODO

Phase 0 — Confirm Baselines
- [ ] Skim the actuator validate paths to ensure the fixtures align with real branches:
  - [ ] `actuator/src/main/java/org/tron/core/actuator/SetAccountIdActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/AccountPermissionUpdateActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java` (`validAccountId`)
- [ ] Run (once) to see which existing fixtures are actually produced as intended:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.AccountFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
  - [ ] Verify `generateSetAccountId_invalidCharacters` and `generateAccountPermissionUpdate_witnessPermission` outcomes.

Phase 1 — SetAccountIdContract (19) Fixtures

Missing validation branch: invalid owner address
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] Build contract with `owner_address = ByteString.EMPTY` (or wrong-length bytes).
  - [ ] Expect validate error substring: `"Invalid ownerAddress"`.
  - [ ] Databases: `account`, `accountid-index`, `dynamic-properties`.

Missing validation branch: unreadable / non-printable bytes in accountId
- [ ] Add `validate_fail_account_id_contains_space`:
  - [ ] Use accountId with a space (0x20), e.g. `"ab  cdefgh"`.
  - [ ] Expect validate error: `"Invalid accountId"`.
- [ ] Add `validate_fail_account_id_non_ascii`:
  - [ ] Include a byte > 0x7E (e.g. `(char) 128`) in the accountId string.
  - [ ] Expect validate error: `"Invalid accountId"`.

Boundary-happy fixtures
- [ ] Add `happy_path_min_len_8`:
  - [ ] accountId length exactly 8 (all bytes in 0x21..0x7E).
  - [ ] Expect `SUCCESS`.
- [ ] Add `happy_path_max_len_32`:
  - [ ] accountId length exactly 32.
  - [ ] Expect `SUCCESS`.

Clarity: explicit empty accountId
- [ ] Add `validate_fail_account_id_empty`:
  - [ ] `account_id = ByteString.EMPTY`.
  - [ ] Expect validate error: `"Invalid accountId"`.

Phase 2 — AccountPermissionUpdateContract (46) Fixtures

Owner address validation and existence
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`.
  - [ ] Expect validate error: `"invalidate ownerAddress"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Use a valid-looking address not present in `AccountStore`.
  - [ ] Expect validate error: `"ownerAddress account does not exist"`.

Missing required fields
- [ ] Add `validate_fail_owner_permission_missing`:
  - [ ] Build contract without `.setOwner(...)`.
  - [ ] Include at least one active permission so the failure is specifically “owner permission is missed”.
  - [ ] Expect validate error: `"owner permission is missed"`.
- [ ] Add `validate_fail_active_permission_missing`:
  - [ ] Build contract with `.setOwner(...)` but no `.addActives(...)`.
  - [ ] Ensure multi-sign enabled.
  - [ ] Expect validate error: `"active permission is missed"`.
- [ ] Add `validate_fail_active_permission_too_many`:
  - [ ] Provide 9 active permissions (all valid).
  - [ ] Expect validate error: `"active permission is too many"`.

Witness-specific required field (account is witness)
- [ ] Ensure witness account is actually marked witness for these fixtures:
  - [ ] In setup or per-test, call `AccountCapsule#setIsWitness(true)` for `WITNESS_ADDRESS`
        before storing in `AccountStore`.
- [ ] Add `validate_fail_witness_permission_missing`:
  - [ ] Owner is witness account, omit `.setWitness(...)`.
  - [ ] Expect validate error: `"witness permission is missed"`.

Wrong permission types
- [ ] Add `validate_fail_owner_permission_type_wrong`:
  - [ ] Set owner permission `type = Active` (or Witness).
  - [ ] Expect validate error: `"owner permission type is error"`.
- [ ] Add `validate_fail_active_permission_type_wrong`:
  - [ ] Provide an “active” permission entry with `type = Owner` (or Witness).
  - [ ] Expect validate error: `"active permission type is error"`.
- [ ] (Witness account) Add `validate_fail_witness_permission_type_wrong`:
  - [ ] Set witness permission `type = Owner` (or Active).
  - [ ] Expect validate error: `"witness permission type is error"`.

checkPermission(...) missing branches
- [ ] Add `validate_fail_keys_count_zero`:
  - [ ] Any permission with zero keys (e.g. owner keys list empty).
  - [ ] Expect validate error: `"key's count should be greater than 0"`.
- [ ] (Witness) Add `validate_fail_witness_keys_count_not_one`:
  - [ ] Witness permission with 2 keys.
  - [ ] Expect validate error: `"Witness permission's key count should be 1"`.
- [ ] Add `validate_fail_threshold_zero`:
  - [ ] Set threshold = 0 for a permission.
  - [ ] Expect validate error: `"permission's threshold should be greater than 0"`.
- [ ] Add `validate_fail_permission_name_too_long`:
  - [ ] permissionName length 33.
  - [ ] Expect validate error: `"permission's name is too long"`.
- [ ] Add `validate_fail_parent_id_not_owner`:
  - [ ] permission.parentId = 1.
  - [ ] Expect validate error: `"permission's parent should be owner"`.
- [ ] Add `validate_fail_key_address_invalid`:
  - [ ] Set a key address to an invalid byte array (e.g. 10 bytes).
  - [ ] Expect validate error: `"key is not a validate address"`.
- [ ] Add `validate_fail_key_weight_zero`:
  - [ ] Set a key weight to 0 (or negative).
  - [ ] Expect validate error: `"key's weight should be greater than 0"`.
- [ ] Add `validate_fail_non_active_has_operations`:
  - [ ] Set `operations` on an owner (or witness) permission to non-empty.
  - [ ] Expect validate error contains: `"permission needn't operations"`.

Active operations validation
- [ ] Add `validate_fail_active_operations_empty`:
  - [ ] Active permission with `operations = ByteString.EMPTY`.
  - [ ] Expect validate error: `"operations size must 32"`.
- [ ] Add `validate_fail_active_operations_wrong_size`:
  - [ ] Active permission with 31-byte operations.
  - [ ] Expect validate error: `"operations size must 32"`.
- [ ] Add `validate_fail_active_operations_invalid_contract_type_bit`:
  - [ ] Set `DynamicPropertiesStore.AVAILABLE_CONTRACT_TYPE` to all zeros (32 bytes).
  - [ ] Create active operations with bit 0 set.
  - [ ] Expect validate error: `"0 isn't a validate ContractType"`.

Phase 3 — Hygiene and Consistency
- [ ] Ensure each new fixture includes correct DBs in `FixtureMetadata` (`account`, `dynamic-properties`, plus `witness` when relevant).
- [ ] Prefer `caseCategory = "edge"` for boundary-success fixtures (min/max lengths), keep `"validate_fail"` for real validation failures.
- [ ] Avoid relying on test execution order; treat each test as isolated (BaseTest setup should re-init state).

Phase 4 — Validate Output
- [ ] Run the single test class and regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.AccountFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
- [ ] Spot-check a few generated `metadata.json` files for expectedStatus and error messages.
- [ ] (Optional) Run the conformance runner that consumes these fixtures to ensure no schema regressions.

