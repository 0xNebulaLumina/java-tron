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
- For “happy” fixtures: `metadata.json.expectedStatus == "SUCCESS"` and the expected DBs reflect issuance
  (account changes, asset store writes, and dynamic property updates like `TOKEN_ID_NUM`).

Checklist / TODO

Phase 0 — Confirm Baselines
- [ ] Skim the validation and input rules to align fixtures with real branches:
  - [ ] `actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java` (`validate`)
  - [ ] `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java` (`validAssetName`, `validUrl`, `validAssetDescription`)
- [ ] Run once to confirm existing fixtures produce the expected statuses:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.AssetIssueFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

Phase 1 — Owner / Address / Account Branches
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] Build contract with `owner_address = ByteString.EMPTY` (or wrong-length bytes).
  - [ ] Expect validate error: `"Invalid ownerAddress"`.
  - [ ] Databases: `account`, `asset-issue-v2`, `dynamic-properties`.
- [ ] Add `validate_fail_owner_account_not_exists`:
  - [ ] Use a valid-looking address not present in `AccountStore`.
  - [ ] Expect validate error: `"Account not exists"`.
- [ ] Add balance boundary fixtures:
  - [ ] `happy_path_balance_equals_fee`:
    - [ ] Set owner balance exactly `ASSET_ISSUE_FEE`.
    - [ ] Expect `SUCCESS`.
  - [ ] `validate_fail_balance_fee_minus_1`:
    - [ ] Set owner balance exactly `ASSET_ISSUE_FEE - 1`.
    - [ ] Expect validate error: `"No enough balance for fee!"`.

Phase 2 — Asset Name / Abbreviation Validation
- [ ] Add `validate_fail_asset_name_empty`:
  - [ ] `name = ByteString.EMPTY`.
  - [ ] Expect validate error: `"Invalid assetName"`.
- [ ] Add `validate_fail_asset_name_too_long_33`:
  - [ ] name length 33 bytes (all in 0x21..0x7E).
  - [ ] Expect validate error: `"Invalid assetName"`.
- [ ] Add `validate_fail_asset_name_contains_space`:
  - [ ] include byte 0x20 (space) so `TransactionUtil.validAssetName` fails.
  - [ ] Expect validate error: `"Invalid assetName"`.
- [ ] Add `validate_fail_asset_name_non_ascii`:
  - [ ] include a byte > 0x7E (e.g. `(char) 128`) so readability check fails.
  - [ ] Expect validate error: `"Invalid assetName"`.
- [ ] Reserved-name case-insensitivity:
  - [ ] Add `validate_fail_asset_name_reserved_trx_uppercase` (e.g. `"TRX"`).
  - [ ] Expect validate error: `"assetName can't be trx"`.
- [ ] Abbreviation edge cases:
  - [ ] Add `happy_path_abbr_empty`:
    - [ ] omit `.setAbbr(...)` or set `abbr = ByteString.EMPTY`.
    - [ ] Expect `SUCCESS` (abbr is optional).
  - [ ] Add `validate_fail_abbr_invalid_contains_space`:
    - [ ] abbr contains 0x20.
    - [ ] Expect validate error: `"Invalid abbreviation for token"`.
  - [ ] Add `validate_fail_abbr_too_long_33`:
    - [ ] abbr length 33.
    - [ ] Expect validate error: `"Invalid abbreviation for token"`.

Phase 3 — URL and Description Branches
- [ ] Add `validate_fail_url_empty`:
  - [ ] `url = ByteString.EMPTY`.
  - [ ] Expect validate error: `"Invalid url"`.
- [ ] Add `validate_fail_url_too_long_257`:
  - [ ] url length 257.
  - [ ] Expect validate error: `"Invalid url"`.
- [ ] Add `happy_path_description_empty`:
  - [ ] `description = ByteString.EMPTY`.
  - [ ] Expect `SUCCESS` (description allows empty).
- [ ] Add `validate_fail_description_too_long_201`:
  - [ ] description length 201.
  - [ ] Expect validate error: `"Invalid description"`.

Phase 4 — Time Field Branches
- [ ] Add `validate_fail_start_time_zero`:
  - [ ] `.setStartTime(0)`.
  - [ ] Expect validate error: `"Start time should be not empty"`.
- [ ] Add `validate_fail_end_time_zero`:
  - [ ] `.setEndTime(0)`.
  - [ ] Expect validate error: `"End time should be not empty"`.
- [ ] Add `validate_fail_end_time_equals_start_time`:
  - [ ] `end_time = start_time`.
  - [ ] Expect validate error: `"End time should be greater than start time"`.
- [ ] Add `validate_fail_end_time_before_start_time`:
  - [ ] `end_time = start_time - 1`.
  - [ ] Expect validate error: `"End time should be greater than start time"`.
- [ ] Add head-time boundary fixtures:
  - [ ] `validate_fail_start_time_equals_head_block_time`:
    - [ ] Ensure `start_time == dynamicStore.getLatestBlockHeaderTimestamp()` at validation time.
    - [ ] Expect validate error: `"Start time should be greater than HeadBlockTime"`.
  - [ ] `happy_path_start_time_just_after_head_block_time`:
    - [ ] Set `start_time = head_time + 1` and `end_time > start_time`.
    - [ ] Expect `SUCCESS`.

Phase 5 — Numeric Fields / Flags
- [ ] Add `validate_fail_trx_num_zero`:
  - [ ] `.setTrxNum(0)`.
  - [ ] Expect validate error: `"TrxNum must greater than 0!"`.
- [ ] Add `validate_fail_num_zero`:
  - [ ] `.setNum(0)`.
  - [ ] Expect validate error: `"Num must greater than 0!"`.
- [ ] Precision range (only enforced when `ALLOW_SAME_TOKEN_NAME=1` and `precision != 0`):
  - [ ] Add `validate_fail_precision_high_7`:
    - [ ] `.setPrecision(7)`.
    - [ ] Expect validate error: `"precision cannot exceed 6"`.
  - [ ] Add `validate_fail_precision_negative_1`:
    - [ ] `.setPrecision(-1)`.
    - [ ] Expect validate error: `"precision cannot exceed 6"`.
  - [ ] Add `happy_path_precision_zero`:
    - [ ] `.setPrecision(0)`.
    - [ ] Expect `SUCCESS`.
- [ ] Add `validate_fail_public_free_asset_net_usage_non_zero`:
  - [ ] `.setPublicFreeAssetNetUsage(1)`.
  - [ ] Expect validate error: `"PublicFreeAssetNetUsage must be 0!"`.
- [ ] Net limit bounds (oneDayNetLimit from dynamic props, default `300_000_000`):
  - [ ] Add `validate_fail_free_asset_net_limit_negative`:
    - [ ] `free_asset_net_limit = -1`.
    - [ ] Expect validate error: `"Invalid FreeAssetNetLimit"`.
  - [ ] Add `validate_fail_free_asset_net_limit_equal_one_day_net_limit`:
    - [ ] `free_asset_net_limit = oneDayNetLimit`.
    - [ ] Expect validate error: `"Invalid FreeAssetNetLimit"`.
  - [ ] Add `validate_fail_public_free_asset_net_limit_negative`:
    - [ ] `public_free_asset_net_limit = -1`.
    - [ ] Expect validate error: `"Invalid PublicFreeAssetNetLimit"`.
  - [ ] Add `validate_fail_public_free_asset_net_limit_equal_one_day_net_limit`:
    - [ ] `public_free_asset_net_limit = oneDayNetLimit`.
    - [ ] Expect validate error: `"Invalid PublicFreeAssetNetLimit"`.
  - [ ] (Optional) Add boundary-happy values `0` and `oneDayNetLimit - 1`.

Phase 6 — Frozen Supply List
- [ ] Add `happy_path_with_valid_frozen_supply`:
  - [ ] Provide 1–2 `FrozenSupply` entries with:
    - [ ] `frozen_amount > 0`, and sum <= `total_supply`.
    - [ ] `frozen_days` within `[minFrozenSupplyTime, maxFrozenSupplyTime]`.
  - [ ] Verify post-state includes:
    - [ ] `Account.frozenSupply` entries.
    - [ ] asset balance credited as `remainSupply` (totalSupply - sum(frozenAmount)).
- [ ] Add `validate_fail_frozen_supply_list_too_long`:
  - [ ] Add `maxFrozenSupplyNumber + 1` entries.
  - [ ] Expect validate error: `"Frozen supply list length is too long"`.
- [ ] Add `validate_fail_frozen_amount_zero`:
  - [ ] `frozen_amount = 0`.
  - [ ] Expect validate error: `"Frozen supply must be greater than 0!"`.
- [ ] Add `validate_fail_frozen_amount_exceeds_total_supply`:
  - [ ] `frozen_amount = totalSupply + 1`.
  - [ ] Expect validate error: `"Frozen supply cannot exceed total supply"`.
- [ ] Add `validate_fail_frozen_amount_sum_exceeds_total_supply`:
  - [ ] Two entries where the second pushes the cumulative sum above `totalSupply`.
  - [ ] Expect validate error: `"Frozen supply cannot exceed total supply"`.
- [ ] Add `validate_fail_frozen_days_below_min`:
  - [ ] `frozen_days = minFrozenSupplyTime - 1`.
  - [ ] Expect validate error contains: `"frozenDuration must be less than"`.
- [ ] Add `validate_fail_frozen_days_above_max`:
  - [ ] `frozen_days = maxFrozenSupplyTime + 1`.
  - [ ] Expect validate error contains: `"frozenDuration must be less than"`.

Phase 7 — Mode Differences: `ALLOW_SAME_TOKEN_NAME=0` (V1)
- [ ] Add a V1-mode setup path for this test class:
  - [ ] Start from `initCommonDynamicPropsV1(...)` and then set required asset-issue properties:
    - [ ] `ASSET_ISSUE_FEE`, `TOKEN_ID_NUM`
    - [ ] `MAX_FROZEN_SUPPLY_NUMBER`, `ONE_DAY_NET_LIMIT`
    - [ ] `MIN_FROZEN_SUPPLY_TIME`, `MAX_FROZEN_SUPPLY_TIME`
  - [ ] Ensure `ALLOW_SAME_TOKEN_NAME = 0`.
- [ ] Add `happy_path_issue_asset_v1`:
  - [ ] Expect writes to both `asset-issue` and `asset-issue-v2`.
  - [ ] Expect v2 stored precision forced to `0` (execute path sets `assetIssueCapsuleV2.setPrecision(0)`).
- [ ] Add `validate_fail_token_exists_v1`:
  - [ ] Seed `AssetIssueStore` with an existing token name, then issue the same name.
  - [ ] Expect validate error: `"Token exists"`.
  - [ ] Databases: `asset-issue`, `asset-issue-v2`, `account`, `dynamic-properties`.

Phase 8 — Optional Execution Branch: Fee Sink (burn vs blackhole)
- [ ] Decide whether conformance needs fixtures for `supportBlackHoleOptimization()`:
  - [ ] If yes, add one fixture with burning enabled and one with burning disabled.
  - [ ] Verify expected post-state differs (blackhole account credit vs dynamic burn counter).

Phase 9 — Hygiene / Determinism
- [ ] Ensure time computations account for `createBlockContext(dbManager, ...)` mutating the dynamic head time/height.
- [ ] Keep each fixture test self-contained; avoid dependence on test execution order.
- [ ] Include correct `FixtureMetadata.database(...)` set per case (`asset-issue` in V1 cases).

