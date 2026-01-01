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
- `caseCategory` aligns with the actual produced status (avoid "validate_fail but SUCCESS").
- Fixtures remain deterministic across runs (timestamps/block context derived from `ConformanceFixtureTestSupport`).

Checklist / TODO

Phase 0 — Confirm Oracle Behavior
- [x] Skim validate/execute paths to map exact messages and branches:
  - [x] `actuator/src/main/java/org/tron/core/actuator/TransferActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/TransferAssetActuator.java`
- [ ] Use existing unit tests as message/branch reference:
  - [ ] `framework/src/test/java/org/tron/core/actuator/TransferActuatorTest.java`
  - [ ] `framework/src/test/java/org/tron/core/actuator/TransferAssetActuatorTest.java`
- [ ] Run fixture generation once to sanity-check current outputs:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.TransferFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
  - [ ] Spot-check a couple of generated `metadata.json` files for status + error message.

Phase 1 — TransferContract (1) Missing Fixtures

Invalid address validation
- [x] Add `validate_fail_owner_address_invalid`:
  - [x] `ownerAddress = ByteString.EMPTY` (or wrong-length bytes).
  - [x] Expect error: `"Invalid ownerAddress!"`.
  - [x] DBs: `account`, `dynamic-properties`.
- [x] Add `validate_fail_to_address_invalid`:
  - [x] `toAddress = ByteString.EMPTY` (or wrong-length bytes).
  - [x] Expect error: `"Invalid toAddress!"`.
  - [x] DBs: `account`, `dynamic-properties`.

Owner account existence
- [x] Add `validate_fail_owner_account_not_found`:
  - [x] Use a valid-looking address not inserted into `AccountStore`.
  - [x] Expect error: `"Validate TransferContract error, no OwnerAccount."`.

Amount boundary
- [x] Add `validate_fail_amount_negative`:
  - [x] `amount = -1`.
  - [x] Expect error: `"Amount must be greater than 0."`.

Overflow branch
- [x] Add `validate_fail_recipient_balance_overflow`:
  - [x] Set existing recipient's TRX balance to `Long.MAX_VALUE`.
  - [x] Transfer `amount = 1`.
  - [x] Expect error: `"long overflow"`.

Create-recipient fee boundary (fee-added branch)
- [x] Add `validate_fail_create_recipient_insufficient_for_fee`:
  - [x] Recipient does not exist (fresh address).
  - [x] Owner balance set to exactly `amount` (or `amount + fee - 1`), so `amount + CREATE_NEW_ACCOUNT_FEE` fails.
  - [x] Expect error contains: `"balance is not sufficient"`.
  - [x] Include dynamic property in metadata: `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`.

Boundary-success semantics
- [x] Add `edge_perfect_transfer_drains_owner`:
  - [x] Seed owner balance to exactly `amount` (recipient exists, so fee path is 0) and transfer full balance.
  - [x] Expect `SUCCESS` and owner balance becomes 0.

Transfer-to-contract restrictions
- [x] Add `validate_fail_forbid_transfer_to_contract`:
  - [x] Seed recipient account with `AccountType.Contract`.
  - [x] Set `forbidTransferToContract = 1` in dynamic properties.
  - [x] Expect error: `"Cannot transfer TRX to a smartContract."`.
  - [x] DBs: at least `account`, `dynamic-properties` (consider `contract` if later extended).
- [x] (Optional, higher effort) Add `validate_fail_evm_compatible_version_one_contract`:
  - [x] Set `allowTvmCompatibleEvm = 1` and recipient `AccountType.Contract`.
  - [x] Seed a `ContractCapsule` for `toAddress` with `contractVersion = 1` in `ContractStore`.
  - [x] Expect error mentions: `"contract which version is one"`.
  - [x] DBs: include `contract` in addition to `account`, `dynamic-properties`.

Account creation permission initialization
- [x] Add `edge_create_recipient_allow_multisign`:
  - [x] Set `allowMultiSign = 1`.
  - [x] Transfer to a non-existent recipient so account is created.
  - [x] Verify via fixture inspection that new account permission fields match Java's created account shape.

Fee disposition / blackhole optimization
- [x] Add `edge_create_recipient_burn_fee`:
  - [x] Set `allowBlackHoleOptimization = 1`.
  - [x] Transfer to a non-existent recipient so `CREATE_NEW_ACCOUNT_FEE` is applied.
  - [x] Verify fixture shows burn (dynamic-properties change) vs blackhole credit (account change).

Phase 2 — TransferAssetContract (2) Missing Fixtures

Invalid address validation
- [x] Add `validate_fail_owner_address_invalid`:
  - [x] `ownerAddress = ByteString.EMPTY` (or wrong-length bytes).
  - [x] Expect error: `"Invalid ownerAddress"`.
  - [x] DBs: `account`, `asset-issue-v2`, `dynamic-properties`.
- [x] Add `validate_fail_to_address_invalid`:
  - [x] `toAddress = ByteString.EMPTY` (or wrong-length bytes).
  - [x] Expect error: `"Invalid toAddress"`.

Owner account existence
- [x] Add `validate_fail_owner_account_not_found`:
  - [x] Use a valid-looking owner address not inserted into `AccountStore`.
  - [x] Expect error: `"No owner account!"`.

Amount boundary
- [x] Add `validate_fail_amount_zero`:
  - [x] `amount = 0`.
  - [x] Expect error: `"Amount must be greater than 0."`.
- [x] Add `validate_fail_amount_negative`:
  - [x] `amount = -1`.
  - [x] Expect error: `"Amount must be greater than 0."`.

Asset balance insufficient-but-nonzero
- [x] Add `validate_fail_insufficient_asset_balance_nonzero`:
  - [x] Seed owner with a small positive token balance (e.g., 5).
  - [x] Attempt transfer with `amount = 6`.
  - [x] Expect error: `"assetBalance is not sufficient."`.

Recipient asset overflow
- [x] Add `validate_fail_recipient_asset_balance_overflow`:
  - [x] Seed recipient with asset balance `Long.MAX_VALUE`.
  - [x] Transfer `amount = 1`.
  - [x] Expect error: `"long overflow"`.

Create-recipient fee failure (distinct message)
- [x] Add `validate_fail_create_recipient_insufficient_trx_fee`:
  - [x] Recipient does not exist (fresh address).
  - [x] Owner has enough tokens but `owner.balance < CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`.
  - [x] Expect error: `"Validate TransferAssetActuator error, insufficient fee."`.
  - [x] Include dynamic property in metadata: `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`.

Transfer-to-contract restriction
- [x] Add `validate_fail_forbid_transfer_asset_to_contract`:
  - [x] Seed recipient as `AccountType.Contract`.
  - [x] Set `forbidTransferToContract = 1`.
  - [x] Expect error: `"Cannot transfer asset to smartContract."`.

Token-name mode split (optional, only if conformance needs legacy)
- [ ] Add a legacy-mode fixture set with `allowSameTokenName = 0`:
  - [ ] Seed asset into `asset-issue` (V1) and set `assetName` to token name bytes.
  - [ ] Duplicate at least: happy path + asset not found + insufficient balance.
  - [ ] Keep these fixtures clearly named (e.g., `legacy_*`) to avoid mixing eras.

Phase 3 — Metadata / DB Capture Hygiene
- [x] Ensure `FixtureMetadata.database(...)` includes all stores required to validate parity for the case:
  - [x] Always: `account`, `dynamic-properties`.
  - [x] TRC-10: include `asset-issue-v2` (and optionally `asset-issue` for legacy-mode fixtures).
  - [x] Contract restriction tests: include `contract` if validation touches `ContractStore`.
- [x] Add `dynamicProperty(...)` entries for any toggled flags (`forbidTransferToContract`, `allowMultiSign`,
      `allowBlackHoleOptimization`, `allowTvmCompatibleEvm`) so fixtures are self-describing.

Phase 4 — Regenerate and Verify
- [ ] Regenerate fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.TransferFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
- [ ] Spot-check:
  - [ ] `metadata.json.expectedStatus` matches the outcome implied by `caseCategory`.
  - [ ] Error message text matches Java-tron thrown messages exactly (no substring placeholders).
  - [ ] Pre/post DBs reflect expected changes (new-account creation and/or fee sink behavior).
