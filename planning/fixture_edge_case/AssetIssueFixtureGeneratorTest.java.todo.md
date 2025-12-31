# AssetIssueFixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/AssetIssueFixtureGeneratorTest.java` fixture generation
  so conformance covers major validation branches, boundary conditions, and mode differences for:
  - `AssetIssueContract` (type 6)

Non-Goals
- Do not change contract validation rules; only add/adjust fixtures to reflect current Java-tron behavior.
- Do not refactor fixture generator infrastructure (keep changes localized to the test class).

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, and `expected/post_db/`.
- For validation failures: `metadata.json.expectedStatus == "VALIDATION_FAILED"` and `expectedErrorMessage`
  matches the thrown `ContractValidateException` message (or a stable substring).
- For "happy" fixtures: `metadata.json.expectedStatus == "SUCCESS"` and the expected DBs reflect issuance
  (account changes, asset store writes, and dynamic property updates like `TOKEN_ID_NUM`).

Checklist / TODO

Phase 0 — Confirm Baselines
- [x] Skim the validation and input rules to align fixtures with real branches:
  - [x] `actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java` (`validate`)
  - [x] `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java` (`validAssetName`, `validUrl`, `validAssetDescription`)
- [ ] Run once to confirm existing fixtures produce the expected statuses:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.AssetIssueFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

Phase 1 — Owner / Address / Account Branches
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] Build contract with `owner_address = ByteString.EMPTY` (or wrong-length bytes).
  - [x] Expect validate error: `"Invalid ownerAddress"`.
  - [x] Databases: `account`, `asset-issue-v2`, `dynamic-properties`.
- [x] Add `validate_fail_owner_account_not_exists`:
  - [x] Use a valid-looking address not present in `AccountStore`.
  - [x] Expect validate error: `"Account not exists"`.
- [x] Add balance boundary fixtures:
  - [x] `happy_path_balance_equals_fee`:
    - [x] Set owner balance exactly `ASSET_ISSUE_FEE`.
    - [x] Expect `SUCCESS`.
  - [x] `validate_fail_balance_fee_minus_1`:
    - [x] Set owner balance exactly `ASSET_ISSUE_FEE - 1`.
    - [x] Expect validate error: `"No enough balance for fee!"`.

Phase 2 — Asset Name / Abbreviation Validation
- [x] Add `validate_fail_asset_name_empty`:
  - [x] `name = ByteString.EMPTY`.
  - [x] Expect validate error: `"Invalid assetName"`.
- [x] Add `validate_fail_asset_name_too_long_33`:
  - [x] name length 33 bytes (all in 0x21..0x7E).
  - [x] Expect validate error: `"Invalid assetName"`.
- [x] Add `validate_fail_asset_name_contains_space`:
  - [x] include byte 0x20 (space) so `TransactionUtil.validAssetName` fails.
  - [x] Expect validate error: `"Invalid assetName"`.
- [x] Add `validate_fail_asset_name_non_ascii`:
  - [x] include a byte > 0x7E (e.g. `(char) 128`) so readability check fails.
  - [x] Expect validate error: `"Invalid assetName"`.
- [x] Reserved-name case-insensitivity:
  - [x] Add `validate_fail_asset_name_reserved_trx_uppercase` (e.g. `"TRX"`).
  - [x] Expect validate error: `"assetName can't be trx"`.
- [x] Abbreviation edge cases:
  - [x] Add `happy_path_abbr_empty`:
    - [x] omit `.setAbbr(...)` or set `abbr = ByteString.EMPTY`.
    - [x] Expect `SUCCESS` (abbr is optional).
  - [x] Add `validate_fail_abbr_invalid_contains_space`:
    - [x] abbr contains 0x20.
    - [x] Expect validate error: `"Invalid abbreviation for token"`.
  - [x] Add `validate_fail_abbr_too_long_33`:
    - [x] abbr length 33.
    - [x] Expect validate error: `"Invalid abbreviation for token"`.

Phase 3 — URL and Description Branches
- [x] Add `validate_fail_url_empty`:
  - [x] `url = ByteString.EMPTY`.
  - [x] Expect validate error: `"Invalid url"`.
- [x] Add `validate_fail_url_too_long_257`:
  - [x] url length 257.
  - [x] Expect validate error: `"Invalid url"`.
- [x] Add `happy_path_description_empty`:
  - [x] `description = ByteString.EMPTY`.
  - [x] Expect `SUCCESS` (description allows empty).
- [x] Add `validate_fail_description_too_long_201`:
  - [x] description length 201.
  - [x] Expect validate error: `"Invalid description"`.

Phase 4 — Time Field Branches
- [x] Add `validate_fail_start_time_zero`:
  - [x] `.setStartTime(0)`.
  - [x] Expect validate error: `"Start time should be not empty"`.
- [x] Add `validate_fail_end_time_zero`:
  - [x] `.setEndTime(0)`.
  - [x] Expect validate error: `"End time should be not empty"`.
- [x] Add `validate_fail_end_time_equals_start_time`:
  - [x] `end_time = start_time`.
  - [x] Expect validate error: `"End time should be greater than start time"`.
- [x] Add `validate_fail_end_time_before_start_time`:
  - [x] `end_time = start_time - 1`.
  - [x] Expect validate error: `"End time should be greater than start time"`.
- [x] Add head-time boundary fixtures:
  - [x] `validate_fail_start_time_equals_head_block_time`:
    - [x] Ensure `start_time == dynamicStore.getLatestBlockHeaderTimestamp()` at validation time.
    - [x] Expect validate error: `"Start time should be greater than HeadBlockTime"`.
  - [x] `happy_path_start_time_just_after_head_block_time`:
    - [x] Set `start_time = head_time + 1` and `end_time > start_time`.
    - [x] Expect `SUCCESS`.

Phase 5 — Numeric Fields / Flags
- [x] Add `validate_fail_trx_num_zero`:
  - [x] `.setTrxNum(0)`.
  - [x] Expect validate error: `"TrxNum must greater than 0!"`.
- [x] Add `validate_fail_num_zero`:
  - [x] `.setNum(0)`.
  - [x] Expect validate error: `"Num must greater than 0!"`.
- [x] Precision range (only enforced when `ALLOW_SAME_TOKEN_NAME=1` and `precision != 0`):
  - [x] Add `validate_fail_precision_high_7`:
    - [x] `.setPrecision(7)`.
    - [x] Expect validate error: `"precision cannot exceed 6"`.
  - [x] Add `validate_fail_precision_negative_1`:
    - [x] `.setPrecision(-1)`.
    - [x] Expect validate error: `"precision cannot exceed 6"`.
  - [x] Add `happy_path_precision_zero`:
    - [x] `.setPrecision(0)`.
    - [x] Expect `SUCCESS`.
- [x] Add `validate_fail_public_free_asset_net_usage_non_zero`:
  - [x] `.setPublicFreeAssetNetUsage(1)`.
  - [x] Expect validate error: `"PublicFreeAssetNetUsage must be 0!"`.
- [x] Net limit bounds (oneDayNetLimit from dynamic props, default `300_000_000`):
  - [x] Add `validate_fail_free_asset_net_limit_negative`:
    - [x] `free_asset_net_limit = -1`.
    - [x] Expect validate error: `"Invalid FreeAssetNetLimit"`.
  - [x] Add `validate_fail_free_asset_net_limit_equal_one_day_net_limit`:
    - [x] `free_asset_net_limit = oneDayNetLimit`.
    - [x] Expect validate error: `"Invalid FreeAssetNetLimit"`.
  - [x] Add `validate_fail_public_free_asset_net_limit_negative`:
    - [x] `public_free_asset_net_limit = -1`.
    - [x] Expect validate error: `"Invalid PublicFreeAssetNetLimit"`.
  - [x] Add `validate_fail_public_free_asset_net_limit_equal_one_day_net_limit`:
    - [x] `public_free_asset_net_limit = oneDayNetLimit`.
    - [x] Expect validate error: `"Invalid PublicFreeAssetNetLimit"`.
  - [ ] (Optional) Add boundary-happy values `0` and `oneDayNetLimit - 1`.

Phase 6 — Frozen Supply List
- [x] Add `happy_path_with_valid_frozen_supply`:
  - [x] Provide 1–2 `FrozenSupply` entries with:
    - [x] `frozen_amount > 0`, and sum <= `total_supply`.
    - [x] `frozen_days` within `[minFrozenSupplyTime, maxFrozenSupplyTime]`.
  - [x] Verify post-state includes:
    - [x] `Account.frozenSupply` entries.
    - [x] asset balance credited as `remainSupply` (totalSupply - sum(frozenAmount)).
- [x] Add `validate_fail_frozen_supply_list_too_long`:
  - [x] Add `maxFrozenSupplyNumber + 1` entries.
  - [x] Expect validate error: `"Frozen supply list length is too long"`.
- [x] Add `validate_fail_frozen_amount_zero`:
  - [x] `frozen_amount = 0`.
  - [x] Expect validate error: `"Frozen supply must be greater than 0!"`.
- [x] Add `validate_fail_frozen_amount_exceeds_total_supply`:
  - [x] `frozen_amount = totalSupply + 1`.
  - [x] Expect validate error: `"Frozen supply cannot exceed total supply"`.
- [x] Add `validate_fail_frozen_amount_sum_exceeds_total_supply`:
  - [x] Two entries where the second pushes the cumulative sum above `totalSupply`.
  - [x] Expect validate error: `"Frozen supply cannot exceed total supply"`.
- [x] Add `validate_fail_frozen_days_below_min`:
  - [x] `frozen_days = minFrozenSupplyTime - 1`.
  - [x] Expect validate error contains: `"frozenDuration must be less than"`.
- [x] Add `validate_fail_frozen_days_above_max`:
  - [x] `frozen_days = maxFrozenSupplyTime + 1`.
  - [x] Expect validate error contains: `"frozenDuration must be less than"`.

Phase 7 — Mode Differences: `ALLOW_SAME_TOKEN_NAME=0` (V1)
- [x] Add a V1-mode setup path for this test class:
  - [x] Start from `initCommonDynamicPropsV1(...)` and then set required asset-issue properties:
    - [x] `ASSET_ISSUE_FEE`, `TOKEN_ID_NUM`
    - [x] `MAX_FROZEN_SUPPLY_NUMBER`, `ONE_DAY_NET_LIMIT`
    - [x] `MIN_FROZEN_SUPPLY_TIME`, `MAX_FROZEN_SUPPLY_TIME`
  - [x] Ensure `ALLOW_SAME_TOKEN_NAME = 0`.
- [x] Add `happy_path_issue_asset_v1`:
  - [x] Expect writes to both `asset-issue` and `asset-issue-v2`.
  - [x] Expect v2 stored precision forced to `0` (execute path sets `assetIssueCapsuleV2.setPrecision(0)`).
- [x] Add `validate_fail_token_exists_v1`:
  - [x] Seed `AssetIssueStore` with an existing token name, then issue the same name.
  - [x] Expect validate error: `"Token exists"`.
  - [x] Databases: `asset-issue`, `asset-issue-v2`, `account`, `dynamic-properties`.

Phase 8 — Optional Execution Branch: Fee Sink (burn vs blackhole)
- [ ] Decide whether conformance needs fixtures for `supportBlackHoleOptimization()`:
  - [ ] If yes, add one fixture with burning enabled and one with burning disabled.
  - [ ] Verify expected post-state differs (blackhole account credit vs dynamic burn counter).

Phase 9 — Hygiene / Determinism
- [x] Ensure time computations account for `createBlockContext(dbManager, ...)` mutating the dynamic head time/height.
- [x] Keep each fixture test self-contained; avoid dependence on test execution order.
- [x] Include correct `FixtureMetadata.database(...)` set per case (`asset-issue` in V1 cases).

