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
- `caseCategory` and `description` match the observed result (avoid “validate_fail but SUCCESS”).

Checklist / TODO

Phase 0 — Confirm Baselines
- [ ] Re-skim validate paths to enumerate missing branches precisely:
  - [ ] `actuator/src/main/java/org/tron/core/actuator/UpdateSettingContractActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/UpdateEnergyLimitContractActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/ClearABIContractActuator.java`
  - [ ] `chainbase/src/main/java/org/tron/core/capsule/ReceiptCapsule.java` (`checkForEnergyLimit`)
- [ ] Run once to regenerate fixtures and confirm the current set is stable:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ContractMetadataFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

Phase 1 — Add “common” invalid ownerAddress fixtures (all three contracts)
- [ ] Add `validate_fail_owner_address_empty` per contract:
  - [ ] Build contract with `.setOwnerAddress(ByteString.EMPTY)`.
  - [ ] Expect validate error: `"Invalid address"`.
- [ ] Add `validate_fail_owner_address_wrong_length` per contract:
  - [ ] Use `ByteString.copyFrom(new byte[10])` (or any non-21-byte length).
  - [ ] Expect validate error: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist` per contract:
  - [ ] Use a valid address not present in `AccountStore`.
  - [ ] Expect validate error contains: `"Account["` and `"does not exist"` / `"not exist"` (message varies by actuator).

Phase 2 — UpdateSettingContract (33) missing branch
- [ ] Add `validate_fail_negative_percent`:
  - [ ] Set `consume_user_resource_percent = -1`.
  - [ ] Expect validate error: `"percent not in [0, 100]"`.

Phase 3 — UpdateEnergyLimitContract (45) fork-gated branch
- [ ] Add `validate_fail_energy_limit_fork_not_enabled`:
  - [ ] Ensure `latestBlockHeaderNumber < CommonParameter.blockNumForEnergyLimit`:
    - [ ] Option A: set `CommonParameter.getInstance().setBlockNumForEnergyLimit(100)` and keep latest block = 10.
    - [ ] Option B: keep fork height at default and set `saveLatestBlockHeaderNumber(0)` (if safe).
  - [ ] Expect validate error: `"contract type error, unexpected type [UpdateEnergyLimitContract]"`.
- [ ] (Optional) Add `edge_large_energy_limit`:
  - [ ] Use a very large positive `origin_energy_limit` (e.g. `Long.MAX_VALUE`).
  - [ ] Expect `SUCCESS` and contract stored value matches (if no downstream limits exist).

Phase 4 — Malformed payload / contract-type mismatch (optional, but high-value)
- [ ] Add one fixture per contract (or at least one representative) that uses:
  - [ ] Transaction `ContractType = UpdateSettingContract` but `parameter = Any.pack(AssetIssueContract)` (or similar).
  - [ ] Expect validate error starts with `"contract type error"` and mentions `expected type [...]`.

Phase 5 — Verify generated fixtures
- [ ] Regenerate fixtures after adding cases:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ContractMetadataFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
- [ ] Spot-check a few produced `metadata.json` files to ensure:
  - [ ] correct `expectedStatus`
  - [ ] stable `expectedErrorMessage`
  - [ ] `databasesTouched` includes `account` + `contract` (and `abi` for ClearABI) as appropriate.

