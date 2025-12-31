Review Target

- `framework/src/test/java/org/tron/core/conformance/ContractMetadataFixtureGeneratorTest.java`

Scope

- Fixture generation for Smart Contract metadata contracts:
  - `UpdateSettingContract` (type 33)
  - `UpdateEnergyLimitContract` (type 45)
  - `ClearABIContract` (type 48)

Current Coverage (as written)

UpdateSettingContract (33)

- Happy: update `consume_user_resource_percent` to 75.
- Happy: set percent to 0.
- Happy: set percent to 100.
- Validate-fail: caller is not the contract owner (`origin_address` mismatch).
- Validate-fail: target contract does not exist in `ContractStore`.
- Validate-fail: percent > 100.

UpdateEnergyLimitContract (45)

- Happy: update `origin_energy_limit` to 20_000_000.
- Validate-fail: caller is not the contract owner.
- Validate-fail: target contract does not exist.
- Validate-fail: `origin_energy_limit == 0`.
- Validate-fail: `origin_energy_limit < 0`.

ClearABIContract (48)

- Happy: clear ABI for an existing contract (writes default ABI into `AbiStore`).
- Happy: clear ABI on a contract with no ABI (idempotent; ABI store entry may not exist yet).
- Validate-fail: caller is not the contract owner.
- Validate-fail: target contract does not exist.
- Validate-fail: TVM Constantinople disabled.

Missing Edge Cases (high value for conformance)

Common across all three contracts (validate paths are in `actuator/src/main/java/org/tron/core/actuator/*`)

- Invalid `ownerAddress` bytes (fails `DecodeUtil.addressValid`, message: `"Invalid address"`):
  - `ByteString.EMPTY`
  - wrong-length bytes (not 21 bytes)
  - wrong prefix (not `Wallet.getAddressPreFixByte()`)
- Owner account does not exist (valid-looking address absent in `AccountStore`):
  - Distinct from “not owner”: should fail earlier on account existence.
- Malformed contract payload / type mismatch:
  - Transaction `ContractType` is `UpdateSettingContract`/`UpdateEnergyLimitContract`/`ClearABIContract`
    but `parameter` packs a different protobuf message (covers the `"contract type error, expected type [...]"`
    branch).
- Empty / missing `contractAddress` (`ByteString.EMPTY`):
  - Actuators do not validate contract address format; this falls through to `"Contract does not exist"`
    (or `"Contract not exists"` for ClearABI) but it’s useful to pin down the exact error message/behavior.

UpdateSettingContract-specific

- Negative `consume_user_resource_percent` (< 0) should fail:
  - Validate checks `newPercent > 100 || newPercent < 0` and throws `"percent not in [0, 100]"`.

UpdateEnergyLimitContract-specific

- Fork-gated validation: energy-limit feature not enabled:
  - `UpdateEnergyLimitContractActuator.validate` fails early when
    `ReceiptCapsule.checkForEnergyLimit(dynamicPropertiesStore)` is false, with
    `"contract type error, unexpected type [UpdateEnergyLimitContract]"`.
  - This requires a fixture that makes `latestBlockHeaderNumber < CommonParameter.blockNumForEnergyLimit`.
- (Optional) extreme-but-valid values:
  - Very large positive `origin_energy_limit` (no max check exists in validate; useful to ensure no
    serialization/overflow surprises in consumers).

ClearABIContract-specific

- No additional high-value branches beyond the common invalid-owner / owner-missing cases (Constantinople gating
  and idempotent behavior are already covered).

Notes / Potential Fragility (worth confirming)

- `UpdateEnergyLimitContract` “happy” cases depend on fork height:
  - `ReceiptCapsule.checkForEnergyLimit` is `latestBlockHeaderNumber >= CommonParameter.blockNumForEnergyLimit`.
  - The fixture setup pins `latestBlockHeaderNumber = 10` but does not explicitly set
    `CommonParameter.blockNumForEnergyLimit`; if the test config sets it > 10, “happy_path” becomes validate-fail.
- `FixtureGenerator.generate(...)` overwrites `expectedStatus/expectedErrorMessage` from actual actuator outcome.
  If a test doesn’t hit its intended branch, fixture `caseCategory`/description can drift from the generated status.

