# TransferFixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/TransferFixtureGeneratorTest.java`
  so generated conformance fixtures cover the major validation branches and boundary conditions for:
  - `TransferContract` (1)
  - `TransferAssetContract` (2)

Non-Goals
- Do not change Java-tron validation/execute rules; only add/adjust fixtures to reflect current behavior.
- Do not refactor `FixtureGenerator` infrastructure (keep changes localized to the test class).
- Do not add new runtime assertions in the fixture generator tests unless needed to prevent silent drift.

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, and `expected/post_db/`.
- For validation failures: `metadata.json.expectedStatus == "VALIDATION_FAILED"` and `expectedErrorMessage`
  matches the thrown `ContractValidateException` message.
- For successful executions: `metadata.json.expectedStatus == "SUCCESS"` and `expected/post_db/` reflects the state
  mutation (including account creation where applicable).
- `caseCategory` aligns with the actual produced status (avoid “validate_fail but SUCCESS”).
- Fixtures remain deterministic across runs (timestamps/block context derived from `ConformanceFixtureTestSupport`).

Checklist / TODO

Phase 0 — Confirm Oracle Behavior
- [ ] Skim validate/execute paths to map exact messages and branches:
  - [ ] `actuator/src/main/java/org/tron/core/actuator/TransferActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/TransferAssetActuator.java`
- [ ] Use existing unit tests as message/branch reference:
  - [ ] `framework/src/test/java/org/tron/core/actuator/TransferActuatorTest.java`
  - [ ] `framework/src/test/java/org/tron/core/actuator/TransferAssetActuatorTest.java`
- [ ] Run fixture generation once to sanity-check current outputs:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.TransferFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
  - [ ] Spot-check a couple of generated `metadata.json` files for status + error message.

Phase 1 — TransferContract (1) Missing Fixtures

Invalid address validation
- [ ] Add `validate_fail_owner_address_invalid`:
  - [ ] `ownerAddress = ByteString.EMPTY` (or wrong-length bytes).
  - [ ] Expect error: `"Invalid ownerAddress!"`.
  - [ ] DBs: `account`, `dynamic-properties`.
- [ ] Add `validate_fail_to_address_invalid`:
  - [ ] `toAddress = ByteString.EMPTY` (or wrong-length bytes).
  - [ ] Expect error: `"Invalid toAddress!"`.
  - [ ] DBs: `account`, `dynamic-properties`.

Owner account existence
- [ ] Add `validate_fail_owner_account_not_found`:
  - [ ] Use a valid-looking address not inserted into `AccountStore`.
  - [ ] Expect error: `"Validate TransferContract error, no OwnerAccount."`.

Amount boundary
- [ ] Add `validate_fail_amount_negative`:
  - [ ] `amount = -1`.
  - [ ] Expect error: `"Amount must be greater than 0."`.

Overflow branch
- [ ] Add `validate_fail_recipient_balance_overflow`:
  - [ ] Set existing recipient’s TRX balance to `Long.MAX_VALUE`.
  - [ ] Transfer `amount = 1`.
  - [ ] Expect error: `"long overflow"`.

Create-recipient fee boundary (fee-added branch)
- [ ] Add `validate_fail_create_recipient_insufficient_for_fee`:
  - [ ] Recipient does not exist (fresh address).
  - [ ] Owner balance set to exactly `amount` (or `amount + fee - 1`), so `amount + CREATE_NEW_ACCOUNT_FEE` fails.
  - [ ] Expect error contains: `"balance is not sufficient"`.
  - [ ] Include dynamic property in metadata: `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`.

Boundary-success semantics
- [ ] Add `edge_perfect_transfer_drains_owner`:
  - [ ] Seed owner balance to exactly `amount` (recipient exists, so fee path is 0) and transfer full balance.
  - [ ] Expect `SUCCESS` and owner balance becomes 0.

Transfer-to-contract restrictions
- [ ] Add `validate_fail_forbid_transfer_to_contract`:
  - [ ] Seed recipient account with `AccountType.Contract`.
  - [ ] Set `forbidTransferToContract = 1` in dynamic properties.
  - [ ] Expect error: `"Cannot transfer TRX to a smartContract."`.
  - [ ] DBs: at least `account`, `dynamic-properties` (consider `contract` if later extended).
- [ ] (Optional, higher effort) Add `validate_fail_evm_compatible_version_one_contract`:
  - [ ] Set `allowTvmCompatibleEvm = 1` and recipient `AccountType.Contract`.
  - [ ] Seed a `ContractCapsule` for `toAddress` with `contractVersion = 1` in `ContractStore`.
  - [ ] Expect error mentions: `"contract which version is one"`.
  - [ ] DBs: include `contract` in addition to `account`, `dynamic-properties`.

Account creation permission initialization
- [ ] Add `edge_create_recipient_allow_multisign`:
  - [ ] Set `allowMultiSign = 1`.
  - [ ] Transfer to a non-existent recipient so account is created.
  - [ ] Verify via fixture inspection that new account permission fields match Java’s created account shape.

Fee disposition / blackhole optimization
- [ ] Add `edge_create_recipient_burn_fee`:
  - [ ] Set `allowBlackHoleOptimization = 1`.
  - [ ] Transfer to a non-existent recipient so `CREATE_NEW_ACCOUNT_FEE` is applied.
  - [ ] Verify fixture shows burn (dynamic-properties change) vs blackhole credit (account change).

Phase 2 — TransferAssetContract (2) Missing Fixtures

Invalid address validation
- [ ] Add `validate_fail_owner_address_invalid`:
  - [ ] `ownerAddress = ByteString.EMPTY` (or wrong-length bytes).
  - [ ] Expect error: `"Invalid ownerAddress"`.
  - [ ] DBs: `account`, `asset-issue-v2`, `dynamic-properties`.
- [ ] Add `validate_fail_to_address_invalid`:
  - [ ] `toAddress = ByteString.EMPTY` (or wrong-length bytes).
  - [ ] Expect error: `"Invalid toAddress"`.

Owner account existence
- [ ] Add `validate_fail_owner_account_not_found`:
  - [ ] Use a valid-looking owner address not inserted into `AccountStore`.
  - [ ] Expect error: `"No owner account!"`.

Amount boundary
- [ ] Add `validate_fail_amount_zero`:
  - [ ] `amount = 0`.
  - [ ] Expect error: `"Amount must be greater than 0."`.
- [ ] Add `validate_fail_amount_negative`:
  - [ ] `amount = -1`.
  - [ ] Expect error: `"Amount must be greater than 0."`.

Asset balance insufficient-but-nonzero
- [ ] Add `validate_fail_insufficient_asset_balance_nonzero`:
  - [ ] Seed owner with a small positive token balance (e.g., 5).
  - [ ] Attempt transfer with `amount = 6`.
  - [ ] Expect error: `"assetBalance is not sufficient."`.

Recipient asset overflow
- [ ] Add `validate_fail_recipient_asset_balance_overflow`:
  - [ ] Seed recipient with asset balance `Long.MAX_VALUE`.
  - [ ] Transfer `amount = 1`.
  - [ ] Expect error: `"long overflow"`.

Create-recipient fee failure (distinct message)
- [ ] Add `validate_fail_create_recipient_insufficient_trx_fee`:
  - [ ] Recipient does not exist (fresh address).
  - [ ] Owner has enough tokens but `owner.balance < CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`.
  - [ ] Expect error: `"Validate TransferAssetActuator error, insufficient fee."`.
  - [ ] Include dynamic property in metadata: `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`.

Transfer-to-contract restriction
- [ ] Add `validate_fail_forbid_transfer_asset_to_contract`:
  - [ ] Seed recipient as `AccountType.Contract`.
  - [ ] Set `forbidTransferToContract = 1`.
  - [ ] Expect error: `"Cannot transfer asset to smartContract."`.

Token-name mode split (optional, only if conformance needs legacy)
- [ ] Add a legacy-mode fixture set with `allowSameTokenName = 0`:
  - [ ] Seed asset into `asset-issue` (V1) and set `assetName` to token name bytes.
  - [ ] Duplicate at least: happy path + asset not found + insufficient balance.
  - [ ] Keep these fixtures clearly named (e.g., `legacy_*`) to avoid mixing eras.

Phase 3 — Metadata / DB Capture Hygiene
- [ ] Ensure `FixtureMetadata.database(...)` includes all stores required to validate parity for the case:
  - [ ] Always: `account`, `dynamic-properties`.
  - [ ] TRC-10: include `asset-issue-v2` (and optionally `asset-issue` for legacy-mode fixtures).
  - [ ] Contract restriction tests: include `contract` if validation touches `ContractStore`.
- [ ] Add `dynamicProperty(...)` entries for any toggled flags (`forbidTransferToContract`, `allowMultiSign`,
      `allowBlackHoleOptimization`, `allowTvmCompatibleEvm`) so fixtures are self-describing.

Phase 4 — Regenerate and Verify
- [ ] Regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.TransferFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
- [ ] Spot-check:
  - [ ] `metadata.json.expectedStatus` matches the outcome implied by `caseCategory`.
  - [ ] Error message text matches Java-tron thrown messages exactly (no substring placeholders).
  - [ ] Pre/post DBs reflect expected changes (new-account creation and/or fee sink behavior).
