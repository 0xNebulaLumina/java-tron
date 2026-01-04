# ContractMetadataFixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/ContractMetadataFixtureGeneratorTest.java`
  fixture generation so conformance covers the main validation branches and feature gates for:
  - `UpdateSettingContract` (33)
  - `UpdateEnergyLimitContract` (45)
  - `ClearABIContract` (48)

Non-Goals
- Do not change validation rules or actuator logic; only add fixtures that reflect current Java-tron behavior.
- Do not refactor fixture generator infrastructure; keep changes localized to this test class.

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, and `expected/post_db/`.
- Validation failures: `metadata.json.expectedStatus == "VALIDATION_FAILED"` and `expectedErrorMessage`
  matches the thrown `ContractValidateException` message.
- Happy fixtures: `metadata.json.expectedStatus == "SUCCESS"` and expected DBs reflect state updates.
- `caseCategory` and `description` match the observed result (avoid "validate_fail but SUCCESS").

Checklist / TODO

Phase 0 — Confirm Baselines
- [x] Re-skim validate paths to enumerate missing branches precisely:
  - [x] `actuator/src/main/java/org/tron/core/actuator/UpdateSettingContractActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/UpdateEnergyLimitContractActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/ClearABIContractActuator.java`
  - [x] `chainbase/src/main/java/org/tron/core/capsule/ReceiptCapsule.java` (`checkForEnergyLimit`)
- [ ] Run once to regenerate fixtures and confirm the current set is stable:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ContractMetadataFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

Phase 1 — Add "common" invalid ownerAddress fixtures (all three contracts)
- [x] Add `validate_fail_owner_address_empty` per contract:
  - [x] Build contract with `.setOwnerAddress(ByteString.EMPTY)`.
  - [x] Expect validate error: `"Invalid address"`.
- [x] Add `validate_fail_owner_address_wrong_length` per contract:
  - [x] Use `ByteString.copyFrom(new byte[10])` (or any non-21-byte length).
  - [x] Expect validate error: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist` per contract:
  - [x] Use a valid address not present in `AccountStore`.
  - [x] Expect validate error contains: `"Account["` and `"does not exist"` / `"not exist"` (message varies by actuator).
- [x] Add `validate_fail_contract_address_empty` per contract:
  - [x] Use `ByteString.EMPTY` for contract address.
  - [x] Expect validate error: `"Contract does not exist"` / `"Contract not exists"`.

Phase 2 — UpdateSettingContract (33) missing branch
- [x] Add `validate_fail_negative_percent`:
  - [x] Set `consume_user_resource_percent = -1`.
  - [x] Expect validate error: `"percent not in [0, 100]"`.

Phase 3 — UpdateEnergyLimitContract (45) fork-gated branch
- [x] Add `validate_fail_fork_not_enabled`:
  - [x] Ensure `latestBlockHeaderNumber < CommonParameter.blockNumForEnergyLimit`:
    - [x] Option A: set `CommonParameter.getInstance().setBlockNumForEnergyLimit(100)` and keep latest block = 10.
  - [x] Expect validate error: `"contract type error, unexpected type [UpdateEnergyLimitContract]"`.
- [x] Add `edge_large_energy_limit`:
  - [x] Use a very large positive `origin_energy_limit` (e.g. `Long.MAX_VALUE`).
  - [x] Expect `SUCCESS` and contract stored value matches (if no downstream limits exist).

Phase 4 — Malformed payload / contract-type mismatch (optional, but high-value)
- [x] Add one fixture per contract that uses:
  - [x] Transaction `ContractType = UpdateSettingContract` but `parameter = Any.pack(AssetIssueContract)`.
  - [x] Transaction `ContractType = UpdateEnergyLimitContract` but `parameter = Any.pack(AssetIssueContract)`.
  - [x] Transaction `ContractType = ClearABIContract` but `parameter = Any.pack(AssetIssueContract)`.
  - [x] Expect validate error starts with `"contract type error"` and mentions `expected type [...]`.

Phase 5 — Verify generated fixtures
- [ ] Regenerate fixtures after adding cases:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ContractMetadataFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
- [ ] Spot-check a few produced `metadata.json` files to ensure:
  - [ ] correct `expectedStatus`
  - [ ] stable `expectedErrorMessage`
  - [ ] `databasesTouched` includes `account` + `contract` (and `abi` for ClearABI) as appropriate.

## Summary of Added Fixtures

### UpdateSettingContract (33) - 6 new fixtures
1. `validate_fail_negative_percent` - percent < 0
2. `validate_fail_owner_address_empty` - ByteString.EMPTY owner
3. `validate_fail_owner_address_wrong_length` - 10-byte owner address
4. `validate_fail_owner_account_not_exist` - valid address not in AccountStore
5. `validate_fail_contract_address_empty` - empty contract address
6. `validate_fail_type_mismatch` - AssetIssueContract payload with UpdateSettingContract type

### UpdateEnergyLimitContract (45) - 8 new fixtures
1. `validate_fail_fork_not_enabled` - blockNum < blockNumForEnergyLimit
2. `edge_large_energy_limit` - Long.MAX_VALUE energy limit
3. `validate_fail_owner_address_empty` - ByteString.EMPTY owner
4. `validate_fail_owner_address_wrong_length` - 10-byte owner address
5. `validate_fail_owner_account_not_exist` - valid address not in AccountStore
6. `validate_fail_contract_address_empty` - empty contract address
7. `validate_fail_type_mismatch` - AssetIssueContract payload with UpdateEnergyLimitContract type

### ClearABIContract (48) - 5 new fixtures
1. `validate_fail_owner_address_empty` - ByteString.EMPTY owner
2. `validate_fail_owner_address_wrong_length` - 10-byte owner address
3. `validate_fail_owner_account_not_exist` - valid address not in AccountStore
4. `validate_fail_contract_address_empty` - empty contract address
5. `validate_fail_type_mismatch` - AssetIssueContract payload with ClearABIContract type

