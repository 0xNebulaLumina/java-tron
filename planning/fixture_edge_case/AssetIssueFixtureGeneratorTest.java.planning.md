Review Target

- `framework/src/test/java/org/tron/core/conformance/AssetIssueFixtureGeneratorTest.java`

Scope

- Fixture generation for:
  - `AssetIssueContract` (type 6)
- Baseline mode in this test class:
  - `ALLOW_SAME_TOKEN_NAME = 1` (TRC-10 V2 / id-based mode via `initTrc10DynamicProps`)

Current Coverage (as written)

- Happy: issue a TRC-10 asset (V2 mode, `precision=6`, no frozen supply).
- Validate-fail: `start_time` before head block time.
- Validate-fail: `total_supply == 0`.
- Validate-fail: owner already issued (`Account.assetIssuedName` non-empty).
- Validate-fail: insufficient balance for asset issue fee.
- Validate-fail: reserved asset name `"trx"` when `ALLOW_SAME_TOKEN_NAME=1`.

Missing Edge Cases (high value for conformance)

Validation path is in `actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java` and
byte/length rules are mostly in `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java`.

Owner/address/account branches

- Invalid `ownerAddress` (fails `DecodeUtil.addressValid`):
  - empty / wrong length / wrong prefix bytes.
- Owner account does not exist (fails `Account not exists`).
- Balance boundaries:
  - `balance == ASSET_ISSUE_FEE` should succeed (validate uses `< calcFee()`).
  - `balance == ASSET_ISSUE_FEE - 1` should fail with `"No enough balance for fee!"`.

Asset name / abbreviation branches (`TransactionUtil.validAssetName`)

`validAssetName` requires: non-empty, <= 32 bytes, and all bytes in printable ASCII range 0x21..0x7E.

- Invalid `name`:
  - empty (`ByteString.EMPTY`) → `"Invalid assetName"`.
  - length 33 → `"Invalid assetName"`.
  - contains space/control byte (e.g. `0x20`, `\n`) → `"Invalid assetName"`.
  - contains non-ASCII bytes (`> 0x7E`, e.g. UTF-8 for non-latin chars) → `"Invalid assetName"`.
- Reserved `"trx"` variants:
  - The current test covers lowercase `"trx"`; it does not explicitly confirm case-insensitive behavior
    (`"TRX"`, `"Trx"`). (Validation lowercases before comparing.)
- Abbreviation edge cases:
  - Empty `abbr` is allowed (validate only checks when non-empty) — missing happy-path fixture.
  - Invalid non-empty `abbr` (same readability/length rules as `name`) — missing validate-fail fixture(s).

URL / description branches

- URL is required (validate calls `TransactionUtil.validUrl(..., allowEmpty=false)`):
  - empty URL should fail `"Invalid url"`.
  - URL length > 256 should fail `"Invalid url"`.
- Description allows empty but has a max length:
  - empty description should succeed (missing boundary-happy fixture).
  - description length > 200 should fail `"Invalid description"`.

Time branches

- Missing-field checks:
  - `start_time == 0` → `"Start time should be not empty"`.
  - `end_time == 0` → `"End time should be not empty"`.
- Ordering:
  - `end_time <= start_time` (equal and less-than) → `"End time should be greater than start time"`.
- Boundary vs head block time:
  - `start_time == latestBlockHeaderTimestamp` should fail `"Start time should be greater than HeadBlockTime"`.
  - `start_time == latestBlockHeaderTimestamp + 1` should succeed (boundary-happy).

Numeric fields / flags

- Precision range (only enforced when `ALLOW_SAME_TOKEN_NAME=1` and `precision != 0`):
  - `precision = 7` or `precision = -1` should fail `"precision cannot exceed 6"`.
  - `precision = 0` is allowed (boundary-happy fixture missing).
- Exchange ratio:
  - `trx_num == 0` → `"TrxNum must greater than 0!"`.
  - `num == 0` → `"Num must greater than 0!"`.
- Net usage / limits:
  - `public_free_asset_net_usage != 0` → `"PublicFreeAssetNetUsage must be 0!"`.
  - `free_asset_net_limit < 0` or `>= oneDayNetLimit` → `"Invalid FreeAssetNetLimit"`.
  - `public_free_asset_net_limit < 0` or `>= oneDayNetLimit` → `"Invalid PublicFreeAssetNetLimit"`.

FrozenSupply list branches

- Happy: at least one valid frozen supply entry to cover execute-side mutation of
  `Account.frozenSupply` and supply split (`remainSupply` vs frozen amounts).
- Validate-fail:
  - list length > `maxFrozenSupplyNumber` → `"Frozen supply list length is too long"`.
  - frozenAmount <= 0 → `"Frozen supply must be greater than 0!"`.
  - sum of frozenAmount exceeds totalSupply (including multi-entry cumulative case)
    → `"Frozen supply cannot exceed total supply"`.
  - frozenDays outside `[minFrozenSupplyTime, maxFrozenSupplyTime]`
    → `"frozenDuration must be less than ... and more than ... days"`.

Mode differences (`ALLOW_SAME_TOKEN_NAME=0` / V1 mode)

- No fixture covers the V1 uniqueness check:
  - if `ALLOW_SAME_TOKEN_NAME=0` and the token name exists in `asset-issue`, validate fails `"Token exists"`.
- No happy-path V1 issuance fixture to capture execute behavior differences:
  - writes to both `asset-issue` and `asset-issue-v2`.
  - v2 precision forced to `0` in stored asset (`assetIssueCapsuleV2.setPrecision(0)`).

Execution-only branch worth considering

- Fee sink branch: `supportBlackHoleOptimization()` determines whether the issuance fee is burned
  or credited to the blackhole account; the test class does not generate fixtures for both modes.

Fixture-generation pitfalls (for new edge fixtures)

- `createBlockContext(dbManager, ...)` mutates the dynamic head block time/number; time-boundary fixtures
  must compute `start_time`/`end_time` relative to the updated head timestamp actually used in validation.
- `FixtureGenerator` derives `expectedStatus/expectedErrorMessage` from the observed actuator outcome; if a
  case doesn’t hit the intended branch, `caseCategory`/description can become misleading.

