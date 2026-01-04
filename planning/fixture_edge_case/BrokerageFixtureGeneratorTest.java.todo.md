# BrokerageFixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/BrokerageFixtureGeneratorTest.java` fixture generation
  so conformance covers all major validation branches and boundary conditions for:
  - `UpdateBrokerageContract` (49)

Non-Goals
- Do not change `UpdateBrokerageActuator` validation rules; only add/adjust fixtures to reflect current behavior.
- Do not refactor fixture generator infrastructure broadly (keep changes localized to this test class).

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, and `expected/post_db/`.
- For validation failures: `metadata.json.expectedStatus == "VALIDATION_FAILED"` and `expectedErrorMessage`
  matches the thrown `ContractValidateException` message.
- "Happy" fixtures execute successfully and mutate the expected DBs (notably `delegation`).
- `caseCategory`/`description` remain consistent with the observed result (avoid "validate_fail but SUCCESS").

Checklist / TODO

Phase 0 — Confirm Baselines
- [x] Confirm validation order and exact messages in:
  - [x] `actuator/src/main/java/org/tron/core/actuator/UpdateBrokerageActuator.java`
  - [x] `framework/src/test/java/org/tron/core/actuator/UpdateBrokerageActuatorTest.java` (expected messages)
- [ ] Run current fixture generator once and inspect generated `metadata.json` for intent vs reality:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.BrokerageFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
  - [ ] Verify `generateUpdateBrokerage_accountNotExist` currently fails with `"Not existed witness:..."`.

Phase 1 — Add Missing: Invalid Owner Address Fixtures
- [x] Add `validate_fail_owner_address_empty`:
  - [x] Build contract with `owner_address = ByteString.EMPTY`, `brokerage = 20`.
  - [x] Expect validate error: `"Invalid ownerAddress"`.
  - [x] Databases: `account`, `witness`, `delegation`, `dynamic-properties`.
- [x] Add `validate_fail_owner_address_wrong_length`:
  - [x] Build contract with `owner_address = ByteString.copyFrom(new byte[20])` (wrong-length bytes).
  - [x] Expect validate error: `"Invalid ownerAddress"`.
- [x] Add `validate_fail_owner_address_wrong_prefix`:
  - [x] Construct a 21-byte address with a non-network prefix byte and otherwise valid-looking bytes.
  - [x] Expect validate error: `"Invalid ownerAddress"`.

Phase 2 — Add Missing: "Account Does Not Exist" (Witness Exists) Fixture
- [x] Add `validate_fail_account_missing_witness_exists`:
  - [x] Choose a valid new address `WITNESS_ONLY_ADDRESS`.
  - [x] Seed `WitnessStore` with a witness for `WITNESS_ONLY_ADDRESS`.
  - [x] Do **not** seed `AccountStore` with an account for `WITNESS_ONLY_ADDRESS`.
  - [x] Build `UpdateBrokerageContract(owner=WITNESS_ONLY_ADDRESS, brokerage=20)`.
  - [x] Expect validate error: `"Account does not exist"`.
  - [x] Databases: `account`, `witness`, `delegation`, `dynamic-properties`.

Phase 3 — Fix/Clarify Existing "Account Not Exist" Case
- [x] Decide whether to keep two distinct fixtures:
  - [x] "witness missing" → expect `"Not existed witness:<hex>"` (renamed to `generateUpdateBrokerage_witnessNotExist`)
  - [x] "account missing (witness exists)" → expect `"Account does not exist"` (new `generateUpdateBrokerage_accountMissingWitnessExists`)
- [x] If keeping both:
  - [x] Rename the existing `generateUpdateBrokerage_accountNotExist` fixture to reflect witness-missing intent.
  - [x] Keep the new Phase 2 fixture as the true account-missing branch.

Phase 4 — Optional: Contract Encoding / Type Mismatch Fixtures
- [x] Add `validate_fail_contract_parameter_wrong_type`:
  - [x] Create a transaction whose contract `type` is `UpdateBrokerageContract` but `parameter` packs a different message (TransferContract).
  - [x] Expect validate error like:
        `contract type error, expected type [UpdateBrokerageContract], real type[class com.google.protobuf.Any]`
  - [x] Note: this covers the `!any.is(UpdateBrokerageContract.class)` branch.
- [x] (Optional, if stable) Add `validate_fail_invalid_protobuf_bytes`:
  - [x] Manually build `Any` with `type_url` for `UpdateBrokerageContract` but invalid `value` bytes.
  - [x] Expect an `InvalidProtocolBufferException`-derived message.
  - [x] Treat as optional because protobuf error strings can change across versions.

Phase 5 — Fixture Determinism / Consistency (Optional Improvement)
- [x] Switch to `ConformanceFixtureTestSupport.createTransaction(...)` and `createBlockContext(dbManager, ...)`:
  - [x] Deterministic timestamps/expiration (uses `DEFAULT_TX_TIMESTAMP` = 1700000000000L)
  - [x] Populated `feeLimit/refBlock*` fields (via support class)
  - [x] Dynamic head block fields aligned with the block context used in `request.pb`
  - [x] Simplified `initializeTestData()` using `putAccount()`, `putWitness()`, `initCommonDynamicPropsV1()`

Phase 6 — Validate Outputs
- [x] Regenerate fixtures and spot-check:
  - [x] All 14 tests pass (includes new `invalidProtobufBytes` test)
  - [x] new invalid-owner fixtures show `"Invalid ownerAddress"`
  - [x] new account-missing fixture shows `"Account does not exist"`
  - [x] happy fixtures show `SUCCESS` and delegation DB changes

## Implementation Summary

### New Test Methods Added:
1. `generateUpdateBrokerage_ownerAddressEmpty()` - Tests empty owner address (Phase 1)
2. `generateUpdateBrokerage_ownerAddressWrongLength()` - Tests 20-byte address (Phase 1)
3. `generateUpdateBrokerage_ownerAddressWrongPrefix()` - Tests 0xa0 prefix instead of 0x41 (Phase 1)
4. `generateUpdateBrokerage_accountMissingWitnessExists()` - Tests witness exists but account doesn't (Phase 2)
5. `generateUpdateBrokerage_contractParameterWrongType()` - Tests mismatched contract type (Phase 4)
6. `generateUpdateBrokerage_invalidProtobufBytes()` - Tests corrupted protobuf bytes (Phase 4)

### Renamed Test Methods:
- `generateUpdateBrokerage_accountNotExist` → `generateUpdateBrokerage_witnessNotExist` (Phase 3)
  - Updated `caseName` to `validate_fail_witness_not_exist`
  - Updated `expectedError` to `"Not existed witness"`

### New Test Data:
- Added `WITNESS_ONLY_ADDRESS` constant for address with witness but no account
- Added witness entry for `WITNESS_ONLY_ADDRESS` in `initializeTestData()` without corresponding account

### Specialized Helper Methods (kept local):
- `createTransactionWithMismatchedType()` - Creates transaction with declared type different from actual parameter type
- `createTransactionWithRawAny()` - Creates transaction with pre-built Any parameter (for invalid protobuf bytes testing)

### Phase 5 Refactoring (ConformanceFixtureTestSupport):
- Switched to static import from `ConformanceFixtureTestSupport`
- Replaced local `createTransaction()` with support class version (deterministic timestamps)
- Replaced local `createBlockContext()` with `createBlockContext(dbManager, WITNESS_ADDRESS)`
- Simplified `initializeTestData()` using `putAccount()`, `putWitness()`, `initCommonDynamicPropsV1()`
- Updated specialized helper methods to use `DEFAULT_TX_TIMESTAMP` and `DEFAULT_TX_EXPIRATION`
