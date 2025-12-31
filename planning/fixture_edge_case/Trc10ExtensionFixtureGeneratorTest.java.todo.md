# TRC-10 Extension Fixtures — Edge-Case TODO

Target file: `framework/src/test/java/org/tron/core/conformance/Trc10ExtensionFixtureGeneratorTest.java`

Status: draft

Goal: expand conformance fixture coverage for TRC-10 extension contracts (9/14/15) so the Rust backend can be validated against *all* meaningful Java actuator validation paths, not just the current “happy + a few validate_fail”.

Key Java references:
- `actuator/src/main/java/org/tron/core/actuator/ParticipateAssetIssueActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UnfreezeAssetActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UpdateAssetActuator.java`
- Dynamic props baseline: `framework/src/test/java/org/tron/core/conformance/ConformanceFixtureTestSupport.java` (`initTrc10DynamicProps`)

---

## 0) Preconditions / Fixture Quality (do first)

- [ ] Switch test setup to a deterministic timestamp source (match other conformance generators); eliminate `System.currentTimeMillis()` drift in:
  - asset start/end times
  - latest block timestamp/number
  - tx raw timestamp + expiration
- [ ] Initialize TRC-10 dynamic properties via `ConformanceFixtureTestSupport.initTrc10DynamicProps(...)` so `oneDayNetLimit` and other required defaults are present (needed for UpdateAsset limits).
- [ ] Mark the TRC-10 issuer account as having issued the asset:
  - allowSameTokenName=1: set `Account.assetIssuedID = ByteString.copyFromUtf8(ASSET_ID)` for `OWNER_ADDRESS`.
- [ ] Add minimal assertions for “happy_path” cases:
  - `assertTrue(result.isSuccess())` for happy fixtures
  - `assertNotNull(result.getValidationError())` for validate_fail fixtures
  This prevents silently writing mislabeled fixtures.

---

## 1) ParticipateAssetIssueContract (9) — Add missing validate_fail fixtures

Base reference errors (from actuator):
- `"Invalid ownerAddress"`, `"Invalid toAddress"`
- `"Amount must greater than 0!"`
- `"Account does not exist!"`
- `"To account does not exist!"`
- `"The asset is not issued by ..."`
- `"No longer valid period!"`
- `"Can not process the exchange!"`
- `"Asset balance is not enough !"`

TODOs
- [ ] `validate_fail_owner_account_missing`
  - Setup: use a fresh valid owner address not present in `AccountStore`.
  - Contract: ownerAddress=missing, toAddress=issuer, assetName=ASSET_ID, amount>0.
- [ ] `validate_fail_to_account_missing`
  - Setup: use a fresh valid toAddress not present in `AccountStore`.
  - Contract: ownerAddress=participant, toAddress=missing, assetName=ASSET_ID, amount>0.
- [ ] `validate_fail_to_not_issuer`
  - Setup: keep asset issued by `OWNER_ADDRESS`, but set `toAddress=OTHER_ADDRESS` (exists).
  - Contract: ownerAddress=participant, toAddress=OTHER_ADDRESS, assetName=ASSET_ID.
- [ ] `validate_fail_sale_not_started`
  - Setup: create an asset with `startTime` in the future (`now + 1 day`), store it under a new asset id, and ensure issuer owns enough tokens.
  - Contract: buy that future asset.
- [ ] `validate_fail_amount_zero`
  - Contract: amount=0 (everything else valid).
- [ ] `validate_fail_amount_negative`
  - Contract: amount = -1 (proto allows signed long).
- [ ] `validate_fail_not_enough_asset`
  - Setup: ensure issuer’s `assetV2[ASSET_ID]` is smaller than computed exchangeAmount.
  - Contract: amount big enough to require more tokens than issuer holds.
- [ ] `validate_fail_exchange_amount_zero`
  - Setup: create an asset where `trxNum` is very large vs `num`, and use a small `amount` so `floor(amount * num / trxNum) == 0`.
  - Contract: amount>0 but tiny, triggers `"Can not process the exchange!"`.
- [ ] (optional/deep) `validate_fail_overflow_add_exact` / `validate_fail_overflow_multiply_exact`
  - Setup: craft `amount` close to `Long.MAX_VALUE` to overflow `multiplyExact(amount, num)` or `addExact(amount, fee)`.
  - Goal: lock down Java’s overflow error message behavior in fixtures.

---

## 2) UnfreezeAssetContract (14) — Add missing validate_fail + edge fixtures

Base reference errors:
- `"Invalid address"`
- `"no frozen supply balance"`
- `"this account has not issued any asset"`
- `"It's not time to unfreeze asset supply"`
- account-missing message includes readable address and “does not exist”

TODOs
- [ ] `validate_fail_not_issued_asset`
  - Setup: account exists, has `frozenSupplyCount > 0`, but `assetIssuedID` empty (allowSameTokenName=1).
  - Expected: `"this account has not issued any asset"`.
- [ ] `validate_fail_owner_account_missing`
  - Setup: ownerAddress is valid but absent from `AccountStore`.
- [ ] `validate_fail_invalid_owner_address`
  - Setup: ownerAddress is invalid bytes (wrong length / invalid prefix).
- [ ] `edge_partial_unfreeze_success`
  - Setup: one account with two frozen entries:
    - Frozen A: `expireTime <= now` (should unfreeze)
    - Frozen B: `expireTime > now` (should remain)
  - Assert: fixture captures post-state where only B remains in `frozenSupplyList`.
- [ ] (optional) Add a second run-mode fixture with `allowSameTokenName=0` to cover the `assetIssuedName` branch (only if conformance wants parity across both modes).

---

## 3) UpdateAssetContract (15) — Add missing validate_fail fixtures

Base reference errors:
- `"Invalid ownerAddress"`
- `"Account does not exist"`
- `"Account has not issued any asset"`
- `"Asset is not existed in AssetIssueV2Store"`
- `"Invalid url"`
- `"Invalid description"`
- `"Invalid FreeAssetNetLimit"`
- `"Invalid PublicFreeAssetNetLimit"`

TODOs
- [ ] Fix/confirm a true `happy_path` precondition:
  - ensure `OWNER_ADDRESS` has `assetIssuedID=ASSET_ID`
  - ensure `asset-issue-v2` contains `ASSET_ID`
  - ensure `oneDayNetLimit` is set (via `initTrc10DynamicProps`)
- [ ] `validate_fail_owner_account_missing`
  - Setup: ownerAddress valid but absent from `AccountStore`.
- [ ] `validate_fail_invalid_owner_address`
  - Setup: invalid ownerAddress bytes.
- [ ] `validate_fail_no_asset_issued`
  - Setup: account exists, but `assetIssuedID` empty.
- [ ] `validate_fail_asset_missing_in_store`
  - Setup: account has `assetIssuedID` set to some id, but no corresponding entry in `AssetIssueV2Store`.
- [ ] `validate_fail_url_too_long`
  - Setup: URL is 257 bytes (non-empty) → `"Invalid url"`.
- [ ] `validate_fail_new_limit_negative`
  - Contract: `newLimit = -1`.
- [ ] `validate_fail_new_limit_too_large`
  - Contract: `newLimit = oneDayNetLimit` (boundary) or larger.
- [ ] `validate_fail_new_public_limit_negative`
  - Contract: `newPublicLimit = -1`.
- [ ] `validate_fail_new_public_limit_too_large`
  - Contract: `newPublicLimit = oneDayNetLimit` (boundary) or larger.
- [ ] (optional) `edge_limit_max_ok`
  - Contract: `newLimit = oneDayNetLimit - 1`, `newPublicLimit = oneDayNetLimit - 1` should succeed.

---

## 4) Naming / Output conventions

- [ ] Keep `caseCategory` aligned with existing convention:
  - `happy` for successful execution fixtures
  - `validate_fail` for actuator.validate failures
  - `edge` only when it’s a success path exercising a boundary (e.g., partial unfreeze, max-limit success)
- [ ] Use stable, descriptive `caseName`s (no timestamps), and keep them unique per contract type directory.

---

## 5) Verification checklist (when implementing)

- [ ] Run: `./gradlew :framework:test --tests "Trc10ExtensionFixtureGeneratorTest" --dependency-verification=off`
- [ ] Confirm fixtures are written under `conformance/fixtures/<contract_type>/<caseName>/...`
- [ ] Spot-check `metadata.json` for each new case:
  - expectedStatus matches the intended outcome
  - expectedErrorMessage includes the actual Java error string (auto-filled by `FixtureGenerator`)
- [ ] (Optional) Run Rust conformance runner against the new fixtures (if available) to confirm parity.

