# Review: `Trc10ExtensionFixtureGeneratorTest.java`

File under review: `framework/src/test/java/org/tron/core/conformance/Trc10ExtensionFixtureGeneratorTest.java`

Purpose: generate conformance fixtures for TRC-10 “extension” system contracts:
- `ParticipateAssetIssueContract` (type 9)
- `UnfreezeAssetContract` (type 14)
- `UpdateAssetContract` (type 15)

This note focuses on *edge-case coverage gaps* (validation/execution paths that exist in Java actuators but have no fixture case here).

---

## What the file already covers

### ParticipateAssetIssueContract (9)
- `happy_path`
- `validate_fail_insufficient_balance`
- `validate_fail_asset_not_found`
- `validate_fail_sale_ended` (expired asset sale window)
- `validate_fail_self_participate` (owner == toAddress)

### UnfreezeAssetContract (14)
- `happy_path` (expired frozen supply exists)
- `validate_fail_no_frozen` (frozenSupplyCount == 0)
- `validate_fail_not_expired` (frozen supply exists but none expired)

### UpdateAssetContract (15)
- `happy_path`
- `validate_fail_not_owner` (but see “sanity notes” below)
- `validate_fail_invalid_url` (empty URL)
- `validate_fail_description_too_long` (>200 bytes)

---

## Missing edge cases (by contract)

The “missing” list below is driven by Java `validate()` logic in:
- `actuator/src/main/java/org/tron/core/actuator/ParticipateAssetIssueActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UnfreezeAssetActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UpdateAssetActuator.java`

### ParticipateAssetIssueContract (9)
Missing validations with distinct behavior/error strings:
- **Owner account missing**: `accountStore.get(ownerAddress) == null` → `"Account does not exist!"`
- **To account missing**: `accountStore.get(toAddress) == null` → `"To account does not exist!"`
- **To address is not the issuer of the asset**: `toAddress != assetIssue.ownerAddress` → `"The asset is not issued by ..."`
- **Sale not started yet**: `now < asset.startTime` → `"No longer valid period!"` (different root cause from “ended”; same message)
- **Amount <= 0**: amount `0` and negative values → `"Amount must greater than 0!"`
- **Issuer token balance insufficient**: `!toAccount.assetBalanceEnoughV2(...)` → `"Asset balance is not enough !"`
- **Exchange amount rounds down to 0**: `floor((amount * num) / trxNum) <= 0` → `"Can not process the exchange!"`
- **Overflow / ArithmeticException paths**:
  - `addExact(amount, fee)` overflow, `multiplyExact(amount, num)` overflow → validation fails with the Java `ArithmeticException` message.

Not covered today: the fixture set only includes “balance”, “asset missing”, “period ended”, and “self”.

### UnfreezeAssetContract (14)
Missing validations with distinct behavior/error strings:
- **Account exists, has frozen supply, but has not issued an asset**:
  - allowSameTokenName=1: `account.getAssetIssuedID().isEmpty()` → `"this account has not issued any asset"`
  - (allowSameTokenName=0 branch also exists for `assetIssuedName`)
- **Owner account missing**: → `ACCOUNT_EXCEPTION_STR + "... does not exist"` (readable address in message)
- **Invalid owner address bytes**: → `"Invalid address"`

Additional “realistic edge” worth a fixture:
- **Partial unfreeze success**: multiple frozen entries where *some* are expired and some are not; `validate()` passes (`allowedUnfreezeCount > 0`) and `execute()` removes only expired entries while keeping the rest.

### UpdateAssetContract (15)
Missing validations with distinct behavior/error strings:
- **Owner account missing**: → `"Account does not exist"`
- **Invalid owner address bytes**: → `"Invalid ownerAddress"`
- **Account has not issued any asset** (allowSameTokenName=1: `assetIssuedID` empty): → `"Account has not issued any asset"`
- **Account claims it issued an asset, but asset record missing in store**:
  - allowSameTokenName=1: `assetIssueV2Store.get(assetIssuedID) == null` → `"Asset is not existed in AssetIssueV2Store"`
  - (allowSameTokenName=0 uses `AssetIssueStore`)
- **URL too long** (>256 bytes): → `"Invalid url"` (same message as empty URL, but different boundary)
- **Invalid `newLimit` bounds**: `newLimit < 0` or `newLimit >= oneDayNetLimit` → `"Invalid FreeAssetNetLimit"`
- **Invalid `newPublicLimit` bounds**: `newPublicLimit < 0` or `newPublicLimit >= oneDayNetLimit` → `"Invalid PublicFreeAssetNetLimit"`
- (Optional boundary coverage) `newLimit == oneDayNetLimit - 1` and `newPublicLimit == oneDayNetLimit - 1` should validate successfully.

---

## Sanity notes (affects whether current “happy path” fixtures are actually happy)

These are not “missing edge cases”, but they can silently degrade the fixture set (because this generator logs but doesn’t assert).

- **Dynamic properties are only partially initialized.** The file only sets `allowSameTokenName` and latest block timestamp/number. `UpdateAssetActuator.validate()` compares `newLimit`/`newPublicLimit` against `dynamicStore.getOneDayNetLimit()`. Other conformance generators call `ConformanceFixtureTestSupport.initTrc10DynamicProps(...)` which sets `oneDayNetLimit` and other TRC-10 defaults.
- **OWNER is not marked as an issuer.** For allowSameTokenName=1, `UpdateAssetActuator` requires `account.getAssetIssuedID()` to be non-empty; `initializeTestData()` never sets it for `OWNER_ADDRESS`. (The Unfreeze happy path *does* set `assetIssuedID`, but only inside `setupAssetWithFrozenSupply()`.)
- **Non-deterministic timestamps.** Uses `System.currentTimeMillis()` for:
  - asset start/end times
  - dynamic property latest block timestamp
  - tx raw timestamp + expiration
  This makes the generated fixtures vary across runs, unlike other conformance fixture generators which use fixed timestamps from `ConformanceFixtureTestSupport`.
- **No assertions on `FixtureResult`.** If a case named `happy_path` actually fails validation, the fixture still gets written with expected status derived from the failure.

---

## Recommendation (coverage priority)

If the goal is “minimal but high-signal” coverage for conformance parity, the most valuable missing fixtures are:
- ParticipateAssetIssue (9): `to_not_issuer`, `to_account_missing`, `not_started`, `not_enough_asset`, `amount_zero`, `exchange_amount_zero`
- UnfreezeAsset (14): `not_issued_asset`, `partial_unfreeze_success`
- UpdateAsset (15): `no_asset_issued`, `asset_missing_in_store`, `invalid_new_limit`, `invalid_new_public_limit` (plus ensure `oneDayNetLimit` is set so a true happy path exists)

