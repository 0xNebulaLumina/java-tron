# UPDATE_ENERGY_LIMIT_CONTRACT (type 45) — parity hardening TODO

## When a fix is "needed"
- Do this work if:
  - you plan to enable remote execution for contract metadata (`-Dremote.exec.contract.enabled=true` on Java, `execution.remote.update_energy_limit_enabled=true` on Rust), or
  - you want conformance fixtures that cover the **fork-enabled** validation branches (invalid address, missing account, not-owner, etc.).

## Checklist / plan

### 1) Establish the current baseline (pre-fix)
- [x] Run the Rust conformance runner for `conformance/fixtures/update_energy_limit_contract/*`.
- [x] Confirm what branch is currently covered (today it's mostly "fork not enabled" early-fail due to the fork threshold).
- [x] Record any fixtures that unexpectedly pass/fail and their observed error strings.

### 2) Generate fork-enabled fixtures (so we can lock in real parity)
Goal: stop masking all validation branches behind `checkForEnergyLimit == false`.

- [x] In `framework/src/test/java/org/tron/core/conformance/ContractMetadataFixtureGeneratorTest.java`, for the UpdateEnergyLimitContract fixture methods:
  - [x] Set `CommonParameter.getInstance().setBlockNumForEnergyLimit(0)` (or any value `<= latestBlockHeaderNumber`) for all cases that are meant to exercise normal validation/execute behavior.
  - [x] Keep `generateUpdateEnergyLimit_forkNotEnabled()` as the only case where `blockNumForEnergyLimit` is set higher than the latest block number.
  - [x] Restore the original `blockNumForEnergyLimit` in a `finally` block (avoid cross-test leakage).
- [x] Re-run fixture generation and verify `metadata.json` now contains:
  - [x] `happy_path` with `expectedStatus = SUCCESS`
  - [x] `validate_fail_owner_address_empty|wrong_length` with `expectedErrorMessage = "Invalid address"`
  - [x] `validate_fail_owner_account_not_exist` with `expectedErrorMessage = "Account[<hex>] does not exist"`
  - [x] `validate_fail_contract_address_empty` with `expectedErrorMessage = "Contract does not exist"`
  - [x] `validate_fail_not_owner` with `expectedErrorMessage = "Account[<hex>] is not the owner of the contract"`
  - [x] `validate_fail_zero_limit|negative_limit` with `expectedErrorMessage = "origin energy limit must be > 0"`
  - [x] `validate_fail_type_mismatch` with the exact java string:
    - [x] `"contract type error, expected type [UpdateEnergyLimitContract],real type[class com.google.protobuf.Any]"`

### 3) Fix the fork gate threshold in Rust (config parity)
Goal: `check_for_energy_limit()` should behave like:
`latestBlockHeaderNumber >= CommonParameter.blockNumForEnergyLimit`.

- [x] Add a Rust config knob (suggested) in `rust-backend/config.toml` + config structs:
  - [x] `block_num_for_energy_limit` field on `EngineBackedEvmStateStore` (default: 4727890 mainnet value).
- [x] Plumb this value into `EngineBackedEvmStateStore` (store it as a field) and make
  `check_for_energy_limit()` use it instead of the hard-coded constant.
- [x] Decide how conformance runs should set this:
  - Option A: keep fixtures at low block numbers and set the fork threshold to `0` in the runner config. ✅ (Used Option A: conformance runner reads `blockNumForEnergyLimit` from fixture metadata.dynamicProperties, defaulting to 0)

### 4) Align Rust validation logic + messages with java-tron
Goal: match `UpdateEnergyLimitContractActuator.validate()` ordering and error strings.

- [x] Update `rust-backend/crates/core/src/service/mod.rs`:
  - [x] Change `parse_update_energy_limit_contract` to return `owner_address` as well:
    - [x] `owner_address` (bytes, field 1)
    - [x] `contract_address` (bytes, field 2)
    - [x] `origin_energy_limit` (int64, field 3)
  - [x] Remove the early `"contract_address is required"` check so empty contract address falls through to:
    - [x] `"Contract does not exist"` (matches Java).
- [x] In `execute_update_energy_limit_contract`, implement Java-parity validation flow:
  - [x] EnergyLimit fork gate → `"contract type error, unexpected type [UpdateEnergyLimitContract]"`
  - [x] Any type_url check → `"contract type error, expected type [UpdateEnergyLimitContract],real type[class com.google.protobuf.Any]"`
  - [x] Owner address validity (payload owner first; fallback to `from_raw` only for Any-less compatibility):
    - [x] `len == 21` and prefix matches `storage_adapter.address_prefix()`
    - [x] else `"Invalid address"`
  - [x] Owner account existence:
    - [x] on miss → `"Account[<hex 21-byte owner>] does not exist"`
  - [x] `origin_energy_limit > 0`:
    - [x] else `"origin energy limit must be > 0"`
  - [x] Contract existence:
    - [x] on miss → `"Contract does not exist"`
  - [x] Ownership (`owner == smart_contract.origin_address`):
    - [x] on mismatch → `"Account[<hex 21-byte owner>] is not the owner of the contract"`
- [x] Ensure "readable owner" formatting uses **hex** of the full 21-byte tron address (matches `StringUtil.createReadableString`).
- [x] Double-check punctuation/spaces in the type-mismatch string (`],real type[` has no extra spaces in Java).

### 5) Validate with fixtures + (optional) unit tests
- [x] Run the updated conformance fixtures in Rust and confirm exact string matches with `metadata.json.expectedErrorMessage`.
- [x] Optionally add a small Rust unit test for `parse_update_energy_limit_contract`:
  - [x] empty contract address should not error in parsing; it should fail at "contract does not exist".
  - [x] negative energy limit encodes/decodes correctly.

### 6) Rollout / safety
- [x] Keep `execution.remote.update_energy_limit_enabled` defaulted to false until parity fixtures pass.
- [ ] After parity, enable `-Dremote.exec.contract.enabled=true` + Rust flag in a controlled environment and monitor:
  - [ ] validation failures (error strings)
  - [ ] ContractStore updates (origin_energy_limit changes only)
