Review Target

- `framework/src/test/java/org/tron/core/conformance/TransferFixtureGeneratorTest.java`

Scope

- Fixture generation for:
  - `TransferContract` (type 1)
  - `TransferAssetContract` (type 2)

Current Coverage (as written)

TransferContract (1)

- Happy: TRX transfer to an existing recipient.
- Happy: TRX transfer that creates the recipient account (pays `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`).
- Validate-fail: to self.
- Validate-fail: amount = 0.
- Validate-fail: owner has insufficient TRX balance (recipient exists).

TransferAssetContract (2)

- Happy: TRC-10 transfer to an existing recipient.
- Happy: TRC-10 transfer that creates the recipient account (pays `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT` in TRX).
- Validate-fail: asset not found in `asset-issue-v2`.
- Validate-fail: owner has no TRC-10 balance (asset balance null/0).
- Validate-fail: to self.

Missing Edge Cases (high value for conformance)

TransferContract validate path lives in `actuator/src/main/java/org/tron/core/actuator/TransferActuator.java`.

- Invalid addresses (currently uncovered):
  - Invalid `ownerAddress` (empty / wrong length / wrong prefix) → `"Invalid ownerAddress!"`.
  - Invalid `toAddress` → `"Invalid toAddress!"`.
- Owner account missing (currently uncovered):
  - Valid-looking owner address not present in `AccountStore` → `"Validate TransferContract error, no OwnerAccount."`.
- Negative amount (currently uncovered):
  - `amount < 0` is a distinct input class from `amount == 0` even if it shares the same message
    (`"Amount must be greater than 0."`).
- Overflow validation (currently uncovered):
  - Recipient exists, recipient balance near `Long.MAX_VALUE`, `amount = 1` should throw `"long overflow"`
    (from `addExact(toBalance, amount)`).
- Create-recipient fee boundary (currently uncovered):
  - Recipient does not exist so `fee += CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`, but owner only has
    enough for `amount` (not `amount + fee`) → should fail `"balance is not sufficient"` via the
    “fee added” branch (different from the existing insufficient-balance case where the amount alone is huge).
- Boundary-success semantics (currently uncovered):
  - “Perfect transfer” draining owner to exactly zero (`amount == balance - fee`) to ensure exact
    arithmetic and no off-by-one around fees.
- Transfer-to-contract restrictions (currently uncovered; dynamic-property driven):
  - `forbidTransferToContract = 1` and `toAccount.type == Contract` → `"Cannot transfer TRX to a smartContract."`
  - `allowTvmCompatibleEvm = 1` and `toAccount.type == Contract` with `contractVersion == 1` →
    `"Cannot transfer TRX to a smartContract which version is one..."` (needs `contract` store seeding).
- Account creation permission shape (currently uncovered):
  - `allowMultiSign = 1` changes how newly-created recipient accounts are initialized (default permission);
    a fixture should pin the created account fields for parity.
- Fee disposition path (currently uncovered):
  - `allowBlackHoleOptimization = 1` burns the fee instead of crediting the blackhole account; current fixtures
    only exercise the “credit blackhole” behavior (baseline dynamic props set `allowBlackHoleOptimization = 0`).

TransferAssetContract validate path lives in `actuator/src/main/java/org/tron/core/actuator/TransferAssetActuator.java`.

- Invalid addresses (currently uncovered):
  - Invalid `ownerAddress` → `"Invalid ownerAddress"`.
  - Invalid `toAddress` → `"Invalid toAddress"`.
- Owner account missing (currently uncovered):
  - Valid-looking owner address not present in `AccountStore` → `"No owner account!"`.
- Amount boundary (currently uncovered):
  - `amount = 0` and `amount < 0` → `"Amount must be greater than 0."`.
- Asset balance insufficient-but-nonzero (currently uncovered):
  - Owner has the asset, but `0 < ownerAssetBalance < amount` → `"assetBalance is not sufficient."`
    (different from the existing “no balance at all” case which hits `"assetBalance must be greater than 0."`).
- Recipient asset overflow (currently uncovered):
  - Recipient exists and already has asset balance near `Long.MAX_VALUE`, `amount = 1` should throw `"long overflow"`.
- Create-recipient fee failure (currently uncovered):
  - Recipient does not exist, owner has enough tokens but `owner.balance < CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
    → `"Validate TransferAssetActuator error, insufficient fee."`.
- Transfer-to-contract restriction (currently uncovered):
  - `forbidTransferToContract = 1` and `toAccount.type == Contract` → `"Cannot transfer asset to smartContract."`.
- Token-name mode split (not covered by current setup):
  - Fixtures only cover `allowSameTokenName = 1` (V2/id-based `assetName`); no coverage for legacy
    `allowSameTokenName = 0` (name-based) behavior, which may matter if conformance needs both eras.

Notes / Potential Fixture Blind Spots

- Database capture is limited to what each test lists in `FixtureMetadata`:
  - New-account creation and transfers can touch indexes (e.g., `account-index`) and/or fee sinks, but the
    current tests only capture `account` and `dynamic-properties` (plus `asset-issue-v2` for TRC-10).
  - If conformance expects parity in index stores or contract stores, the fixture DB list must be expanded.
- `FixtureGenerator` overwrites `expectedStatus`/`expectedErrorMessage` based on actual execution.
  If a test doesn’t hit its intended branch, the fixture won’t “fail the test”; instead, it will silently
  generate a fixture whose `caseCategory` no longer matches the actual outcome.
