Review Target

- `framework/src/test/java/org/tron/core/conformance/CoreAccountFixtureGeneratorTest.java`

Scope

- Fixture generation for:
  - `AccountCreateContract` (type 0)
  - `AccountUpdateContract` (type 10)

Current Coverage (as written)

AccountCreateContract (0)

- Happy: create a new account when owner exists, has balance, and target is absent.
- Validate-fail: owner account missing.
- Validate-fail: target account already exists.
- Validate-fail: owner balance is below the create-account fee.

AccountUpdateContract (10)

- Happy: set account name for the first time (`ALLOW_UPDATE_ACCOUNT_NAME = 0`, current name empty).
- Validate-fail (intended): “invalid name” (currently uses empty name).
- Validate-fail: owner account missing.
- Validate-fail: duplicate name when updates are disabled (name exists in `account-index`,
  `ALLOW_UPDATE_ACCOUNT_NAME = 0`).

Missing Edge Cases (high value for conformance)

Validation paths:

- `actuator/src/main/java/org/tron/core/actuator/CreateAccountActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UpdateAccountActuator.java`
- `common/src/main/java/org/tron/common/utils/DecodeUtil.java` (`addressValid`)
- `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java` (`validAccountName`)

AccountCreateContract (0)

- Invalid `ownerAddress` (fails `DecodeUtil.addressValid`):
  - empty bytes.
  - wrong length (not 21 bytes).
  - wrong prefix byte (first byte != `Constant.ADD_PRE_FIX_BYTE_MAINNET`).
- Invalid `accountAddress` (fails `DecodeUtil.addressValid`), same shapes as above.
- Fee boundary conditions (validate uses `balance < fee`):
  - `balance == fee` should succeed.
  - `balance == fee - 1` should fail.
- Feature-flag dependent execute paths (currently fixed to `0` by `initCommonDynamicPropsV1`):
  - `ALLOW_MULTI_SIGN = 1` creates default owner+active permissions on the new account; `0` does not.
  - `ALLOW_BLACKHOLE_OPTIMIZATION = 1` burns fees via `BURN_TRX_AMOUNT` instead of crediting the
    blackhole account.
- Parameterization of fee source:
  - Use a non-default `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT` value (e.g., 0 or 2 TRX) to ensure
    the fee is read from the correct dynamic property and reflected in post-state.

AccountUpdateContract (10)

- Invalid `ownerAddress` (fails `DecodeUtil.addressValid`).
- Invalid `accountName` (fails `TransactionUtil.validAccountName`):
  - length > 200 bytes (e.g., 201) should fail `"Invalid accountName"`.
  - boundary success: exactly 200 bytes should succeed.
- Missing `ALLOW_UPDATE_ACCOUNT_NAME` validation branch:
  - owner already has a non-empty name and `ALLOW_UPDATE_ACCOUNT_NAME = 0` should fail with
    `"This account name is already existed"`.
- Update-enabled behavior (`ALLOW_UPDATE_ACCOUNT_NAME = 1`) is uncovered:
  - updating a non-empty name should succeed.
  - duplicate-name updates should succeed and will overwrite `account-index` mapping (name -> last writer).

Notes / Potential “false coverage” in current tests (worth confirming)

- `generateAccountUpdate_validateFailInvalidName` likely does not fail as intended:
  - `TransactionUtil.validAccountName(...)` only checks `len <= 200` and allows empty.
  - `ByteString.EMPTY` should validate, so the fixture can end up `SUCCESS` while `caseCategory` says
    `validate_fail`.
- `generateAccountCreate_validateFailInsufficientFee` uses `expectedError("balance")`, but the actual
  validation error text is `"Validate CreateAccountActuator error, insufficient fee."`.
  `FixtureGenerator` overwrites `expectedErrorMessage` from the observed error, but `caseCategory` and
  description can still become misleading if a case hits an unexpected branch.

