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
- “Happy” fixtures execute successfully and mutate the expected DBs (notably `delegation`).
- `caseCategory`/`description` remain consistent with the observed result (avoid “validate_fail but SUCCESS”).

Checklist / TODO

Phase 0 — Confirm Baselines
- [ ] Confirm validation order and exact messages in:
  - [ ] `actuator/src/main/java/org/tron/core/actuator/UpdateBrokerageActuator.java`
  - [ ] `framework/src/test/java/org/tron/core/actuator/UpdateBrokerageActuatorTest.java` (expected messages)
- [ ] Run current fixture generator once and inspect generated `metadata.json` for intent vs reality:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.BrokerageFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
  - [ ] Verify `generateUpdateBrokerage_accountNotExist` currently fails with `"Not existed witness:..."`.

Phase 1 — Add Missing: Invalid Owner Address Fixtures
- [ ] Add `validate_fail_owner_address_empty`:
  - [ ] Build contract with `owner_address = ByteString.EMPTY`, `brokerage = 20`.
  - [ ] Expect validate error: `"Invalid ownerAddress"`.
  - [ ] Databases: `account`, `witness`, `delegation`, `dynamic-properties`.
- [ ] Add `validate_fail_owner_address_wrong_length`:
  - [ ] Build contract with `owner_address = ByteString.copyFrom(new byte[20])` (wrong-length bytes).
  - [ ] Expect validate error: `"Invalid ownerAddress"`.
- [ ] Add `validate_fail_owner_address_wrong_prefix`:
  - [ ] Construct a 21-byte address with a non-network prefix byte and otherwise valid-looking bytes.
  - [ ] Expect validate error: `"Invalid ownerAddress"`.

Phase 2 — Add Missing: “Account Does Not Exist” (Witness Exists) Fixture
- [ ] Add `validate_fail_account_missing_witness_exists`:
  - [ ] Choose a valid new address `MISSING_ACCOUNT_ADDRESS`.
  - [ ] Seed `WitnessStore` with a witness for `MISSING_ACCOUNT_ADDRESS`.
  - [ ] Do **not** seed `AccountStore` with an account for `MISSING_ACCOUNT_ADDRESS`.
  - [ ] Build `UpdateBrokerageContract(owner=MISSING_ACCOUNT_ADDRESS, brokerage=20)`.
  - [ ] Expect validate error: `"Account does not exist"`.
  - [ ] Databases: `account`, `witness`, `delegation`, `dynamic-properties`.

Phase 3 — Fix/Clarify Existing “Account Not Exist” Case
- [ ] Decide whether to keep two distinct fixtures:
  - [ ] “witness missing” → expect `"Not existed witness:<hex>"`
  - [ ] “account missing (witness exists)” → expect `"Account does not exist"`
- [ ] If keeping both:
  - [ ] Rename the existing `generateUpdateBrokerage_accountNotExist` fixture to reflect witness-missing intent.
  - [ ] Keep the new Phase 2 fixture as the true account-missing branch.

Phase 4 — Optional: Contract Encoding / Type Mismatch Fixtures
- [ ] Add `validate_fail_contract_parameter_wrong_type`:
  - [ ] Create a transaction whose contract `type` is `UpdateBrokerageContract` but `parameter` packs a different message.
  - [ ] Expect validate error like:
        `contract type error, expected type [UpdateBrokerageContract], real type[class com.google.protobuf.Any]`
  - [ ] Note: this covers the `!any.is(UpdateBrokerageContract.class)` branch.
- [ ] (Optional, if stable) Add `validate_fail_invalid_protobuf_bytes`:
  - [ ] Manually build `Any` with `type_url` for `UpdateBrokerageContract` but invalid `value` bytes.
  - [ ] Expect an `InvalidProtocolBufferException`-derived message.
  - [ ] Treat as optional because protobuf error strings can change across versions.

Phase 5 — Fixture Determinism / Consistency (Optional Improvement)
- [ ] Switch to `ConformanceFixtureTestSupport.createTransaction(...)` and `createBlockContext(dbManager, ...)`:
  - [ ] Deterministic timestamps/expiration
  - [ ] Populated `feeLimit/refBlock*` fields
  - [ ] Dynamic head block fields aligned with the block context used in `request.pb`

Phase 6 — Validate Outputs
- [ ] Regenerate fixtures and spot-check:
  - [ ] new invalid-owner fixtures show `"Invalid ownerAddress"`
  - [ ] new account-missing fixture shows `"Account does not exist"`
  - [ ] happy fixtures show `SUCCESS` and delegation DB changes
