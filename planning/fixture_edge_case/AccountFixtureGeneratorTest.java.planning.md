Review Target

- `framework/src/test/java/org/tron/core/conformance/AccountFixtureGeneratorTest.java`

Scope

- Fixture generation for:
  - `SetAccountIdContract` (type 19)
  - `AccountPermissionUpdateContract` (type 46)

Current Coverage (as written)

SetAccountIdContract (19)

- Happy: sets a new, valid account ID on an existing account.
- Validate-fail: ID too short (< 8).
- Validate-fail: ID too long (> 32).
- Validate-fail: “invalid characters”.
- Validate-fail: duplicate ID (ID already present in `accountid-index`).
- Validate-fail: owner already has an ID set.
- Validate-fail: owner account does not exist.

AccountPermissionUpdateContract (46)

- Happy: owner + one active permission.
- Happy: multi-sig owner threshold (2-of-2).
- Happy: includes witness permission (intended).
- Validate-fail: multi-sign disabled (dynamic property).
- Revert/execute-fail: insufficient balance to pay `UPDATE_ACCOUNT_PERMISSION_FEE`.
- Validate-fail: too many keys (exceeds `TOTAL_SIGN_NUM`).
- Validate-fail: duplicate key addresses in a permission.
- Validate-fail: threshold higher than sum(weights).
- Validate-fail: set witness permission on a non-witness account.

Missing Edge Cases (high value for conformance)

SetAccountIdContract (validate path is in `actuator/src/main/java/org/tron/core/actuator/SetAccountIdActuator.java`)

- Invalid `ownerAddress` (fails `DecodeUtil.addressValid`):
  - empty / wrong length / wrong prefix bytes.
- Invalid `accountId` due to “unreadable” bytes (fails `TransactionUtil.validAccountId`):
  - contains space (0x20) or control bytes (< 0x21), e.g. `"ab  cdefgh"`, `"abc\n12345"`.
  - contains non-ASCII bytes (> 0x7E), e.g. `(char) 128` in the string.
- Boundary-valid lengths:
  - exactly 8 bytes (minimum) should succeed.
  - exactly 32 bytes (maximum) should succeed.
- Explicit empty `accountId` (0 bytes) fixture (separate from “too short” for clarity in consumers).

AccountPermissionUpdateContract (validate path is in `actuator/src/main/java/org/tron/core/actuator/AccountPermissionUpdateActuator.java`)

- Invalid `ownerAddress` (fails `DecodeUtil.addressValid`) and “owner account does not exist”.
- Missing fields:
  - `hasOwner == false` (“owner permission is missed”).
  - `activesCount == 0` (“active permission is missed”) with multi-sign enabled.
  - `activesCount > 8` (“active permission is too many”).
  - witness account (`account.is_witness=true`) but `hasWitness == false` (“witness permission is missed”).
- Wrong permission types:
  - owner permission `type != Owner`.
  - any active permission `type != Active`.
  - witness permission `type != Witness` (when account is witness).
- Permission validation branches in `checkPermission(...)` that are currently uncovered:
  - `keysCount == 0`.
  - witness permission `keysCount != 1`.
  - `threshold <= 0`.
  - permission name length > 32.
  - `parentId != 0`.
  - invalid key address bytes (fails `DecodeUtil.addressValid`).
  - key weight `<= 0`.
  - non-active permission has non-empty `operations` (“... permission needn't operations”).
  - active permission `operations` empty or size != 32 (“operations size must 32”).
  - active permission `operations` sets a bit for a contract type not enabled in
    `DynamicPropertiesStore.AVAILABLE_CONTRACT_TYPE` (error like `"X isn't a validate ContractType"`).

Notes / Potential “false coverage” in current tests (worth confirming)

- `generateSetAccountId_invalidCharacters` likely does not fail as intended:
  - Java-tron’s `TransactionUtil.validAccountId` only enforces length and printable ASCII (0x21..0x7E).
    Characters like `@#$%` are printable and should validate.
  - If it validates, the produced fixture would be `SUCCESS` while `caseCategory` is `validate_fail`.
- `generateAccountPermissionUpdate_witnessPermission` may not produce a real happy-path witness fixture:
  - Witness-ness is determined by `AccountCapsule.is_witness` (not just presence in `WitnessStore`).
  - The setup creates a witness entry but never sets `witnessAccount.setIsWitness(true)`, so validation
    can reject witness permission with “account isn't witness can't set witness permission”.
- `FixtureGenerator` overwrites `expectedStatus/expectedErrorMessage` from the actual actuator outcome.
  If a test case doesn’t hit its intended validation branch, the generated fixture metadata will drift
  from the test’s `caseCategory`/description.

