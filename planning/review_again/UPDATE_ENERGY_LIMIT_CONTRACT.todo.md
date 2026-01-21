# UPDATE_ENERGY_LIMIT_CONTRACT (type 45) — parity hardening TODO

## When a fix is “needed”
- Do this work if:
  - you plan to enable remote execution for contract metadata (`-Dremote.exec.contract.enabled=true` on Java, `execution.remote.update_energy_limit_enabled=true` on Rust), or
  - you want conformance fixtures that cover the **fork-enabled** validation branches (invalid address, missing account, not-owner, etc.).

## Checklist / plan

### 1) Establish the current baseline (pre-fix)
- [ ] Run the Rust conformance runner for `conformance/fixtures/update_energy_limit_contract/*`.
- [ ] Confirm what branch is currently covered (today it’s mostly “fork not enabled” early-fail due to the fork threshold).
- [ ] Record any fixtures that unexpectedly pass/fail and their observed error strings.

### 2) Generate fork-enabled fixtures (so we can lock in real parity)
Goal: stop masking all validation branches behind `checkForEnergyLimit == false`.

- [ ] In `framework/src/test/java/org/tron/core/conformance/ContractMetadataFixtureGeneratorTest.java`, for the UpdateEnergyLimitContract fixture methods:
  - [ ] Set `CommonParameter.getInstance().setBlockNumForEnergyLimit(0)` (or any value `<= latestBlockHeaderNumber`) for all cases that are meant to exercise normal validation/execute behavior.
  - [ ] Keep `generateUpdateEnergyLimit_forkNotEnabled()` as the only case where `blockNumForEnergyLimit` is set higher than the latest block number.
  - [ ] Restore the original `blockNumForEnergyLimit` in a `finally` block (avoid cross-test leakage).
- [ ] Re-run fixture generation and verify `metadata.json` now contains:
  - [ ] `happy_path` with `expectedStatus = SUCCESS`
  - [ ] `validate_fail_owner_address_empty|wrong_length` with `expectedErrorMessage = "Invalid address"`
  - [ ] `validate_fail_owner_account_not_exist` with `expectedErrorMessage = "Account[<hex>] does not exist"`
  - [ ] `validate_fail_contract_address_empty` with `expectedErrorMessage = "Contract does not exist"`
  - [ ] `validate_fail_not_owner` with `expectedErrorMessage = "Account[<hex>] is not the owner of the contract"`
  - [ ] `validate_fail_zero_limit|negative_limit` with `expectedErrorMessage = "origin energy limit must be > 0"`
  - [ ] `validate_fail_type_mismatch` with the exact java string:
    - [ ] `"contract type error, expected type [UpdateEnergyLimitContract],real type[class com.google.protobuf.Any]"`

### 3) Fix the fork gate threshold in Rust (config parity)
Goal: `check_for_energy_limit()` should behave like:
`latestBlockHeaderNumber >= CommonParameter.blockNumForEnergyLimit`.

- [ ] Add a Rust config knob (suggested) in `rust-backend/config.toml` + config structs:
  - [ ] `execution.forks.block_num_for_energy_limit = 4727890` (default mainnet value).
- [ ] Plumb this value into `EngineBackedEvmStateStore` (store it as a field) and make
  `check_for_energy_limit()` use it instead of the hard-coded constant.
- [ ] Decide how conformance runs should set this:
  - Option A: keep fixtures at low block numbers and set the fork threshold to `0` in the runner config.
  - Option B: set latest block number in fixtures above the mainnet threshold.

### 4) Align Rust validation logic + messages with java-tron
Goal: match `UpdateEnergyLimitContractActuator.validate()` ordering and error strings.

- [ ] Update `rust-backend/crates/core/src/service/mod.rs`:
  - [ ] Change `parse_update_energy_limit_contract` to return `owner_address` as well:
    - [ ] `owner_address` (bytes, field 1)
    - [ ] `contract_address` (bytes, field 2)
    - [ ] `origin_energy_limit` (int64, field 3)
  - [ ] Remove the early `"contract_address is required"` check so empty contract address falls through to:
    - [ ] `"Contract does not exist"` (matches Java).
- [ ] In `execute_update_energy_limit_contract`, implement Java-parity validation flow:
  - [ ] EnergyLimit fork gate → `"contract type error, unexpected type [UpdateEnergyLimitContract]"`
  - [ ] Any type_url check → `"contract type error, expected type [UpdateEnergyLimitContract],real type[class com.google.protobuf.Any]"`
  - [ ] Owner address validity (payload owner first; fallback to `from_raw` only for Any-less compatibility):
    - [ ] `len == 21` and prefix matches `storage_adapter.address_prefix()`
    - [ ] else `"Invalid address"`
  - [ ] Owner account existence:
    - [ ] on miss → `"Account[<hex 21-byte owner>] does not exist"`
  - [ ] `origin_energy_limit > 0`:
    - [ ] else `"origin energy limit must be > 0"`
  - [ ] Contract existence:
    - [ ] on miss → `"Contract does not exist"`
  - [ ] Ownership (`owner == smart_contract.origin_address`):
    - [ ] on mismatch → `"Account[<hex 21-byte owner>] is not the owner of the contract"`
- [ ] Ensure “readable owner” formatting uses **hex** of the full 21-byte tron address (matches `StringUtil.createReadableString`).
- [ ] Double-check punctuation/spaces in the type-mismatch string (`],real type[` has no extra spaces in Java).

### 5) Validate with fixtures + (optional) unit tests
- [ ] Run the updated conformance fixtures in Rust and confirm exact string matches with `metadata.json.expectedErrorMessage`.
- [ ] Optionally add a small Rust unit test for `parse_update_energy_limit_contract`:
  - [ ] empty contract address should not error in parsing; it should fail at “contract does not exist”.
  - [ ] negative energy limit encodes/decodes correctly.

### 6) Rollout / safety
- [ ] Keep `execution.remote.update_energy_limit_enabled` defaulted to false until parity fixtures pass.
- [ ] After parity, enable `-Dremote.exec.contract.enabled=true` + Rust flag in a controlled environment and monitor:
  - [ ] validation failures (error strings)
  - [ ] ContractStore updates (origin_energy_limit changes only)

