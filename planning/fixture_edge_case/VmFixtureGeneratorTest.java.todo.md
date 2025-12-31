# VM CreateSmartContract Fixtures — Edge-Case TODO

Target file: `framework/src/test/java/org/tron/core/conformance/VmFixtureGeneratorTest.java`

Status: draft

Goal: expand fixture coverage for `CreateSmartContract` (type 30) so Rust backend execution can be
validated against the full set of meaningful Java validation/execution branches (not just “happy +
insufficient balance + invalid opcode”).

Key Java references
- `actuator/src/main/java/org/tron/core/actuator/VMActuator.java` (create validation + execute-time
  behaviors like save-code energy + London 0xEF reject)
- `actuator/src/main/java/org/tron/core/vm/VMUtils.java` (internal TRX/TRC-10 transfer validation)
- `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java` (`supportVM`,
  `maxFeeLimit`, feature toggles)
- Determinism helpers: `framework/src/test/java/org/tron/core/conformance/ConformanceFixtureTestSupport.java`

---

## Acceptance Criteria

- Each new case writes a complete fixture directory:
  - `pre_db/*.kv`
  - `request.pb`
  - `expected/post_db/*.kv`
  - `expected/result.pb` when execution reaches receipt generation
  - `metadata.json` with correct `expectedStatus` + `expectedErrorMessage`
- `caseCategory` matches observed status:
  - `happy` → `SUCCESS`
  - `validate_fail` → `VALIDATION_FAILED`
  - `edge` → boundary-success or deterministic `REVERT` parity cases

---

## Phase 0 — Fixture hygiene / determinism (do first)

- [ ] Replace `System.currentTimeMillis()` timestamps with deterministic values (match other
  conformance fixture generators via `ConformanceFixtureTestSupport.createTransaction(...)`).
- [ ] Use deterministic block context (`ConformanceFixtureTestSupport.createBlockContext(dbManager, ...)`)
  so block number/timestamp/hash are coherent with dynamic properties.
- [ ] Explicitly initialize VM-related dynamic properties needed for deterministic outcomes:
  - [ ] `saveAllowCreationOfContracts(1)` (and optionally toggle to `0` for the VM-disabled case)
  - [ ] `saveMaxFeeLimit(...)` with a known value (so `feeLimit_above_max` is stable)
  - [ ] `saveAllowTvmConstantinople(1)`
  - [ ] `saveAllowTvmTransferTrc10(1)` (already done today)
  - [ ] `saveAllowMultiSign(1)` (already done today; required for `checkTokenValueAndId`)
- [ ] Add minimal assertions to prevent mislabeled fixtures:
  - [ ] `Assert.assertTrue(...)` for `happy_*`
  - [ ] `Assert.assertFalse(...)` + error substring checks for `validate_fail_*`

---

## Phase 1 — Add missing `VALIDATION_FAILED` fixtures (CreateSmartContract)

VM enabled / disabled
- [ ] `validate_fail_vm_disabled`
  - Setup: `dbManager.getDynamicPropertiesStore().saveAllowCreationOfContracts(0)`
  - Expect: `"vm work is off, need to be opened by the committee"`

Owner / origin mismatch
- [ ] `validate_fail_owner_origin_mismatch`
  - Build the protobuf manually (don’t use `TvmTestUtils.buildCreateSmartContract`), so:
    - `CreateSmartContract.ownerAddress = OWNER`
    - `newContract.originAddress = OTHER`
  - Expect: `"OwnerAddress is not equals OriginAddress"`

Name length bounds
- [ ] `validate_fail_contract_name_too_long`
  - Setup: name whose `getBytes().length == 33` (use ASCII for deterministic byte length).
  - Expect: `"contractName's length cannot be greater than 32"`
- [ ] `edge_contract_name_len_32_ok`
  - Setup: exactly 32 bytes.
  - Expect: `SUCCESS`
- [ ] (optional) `validate_fail_contract_name_multibyte_over_32_bytes`
  - Setup: fewer than 32 chars but >32 bytes in UTF-8.

consumeUserResourcePercent bounds
- [ ] `validate_fail_percent_negative`
  - percent = `-1` → `"percent must be >= 0 and <= 100"`
- [ ] `validate_fail_percent_gt_100`
  - percent = `101` → same message
- [ ] `edge_percent_0_ok` and `edge_percent_100_ok`

FeeLimit bounds
- [ ] `validate_fail_fee_limit_negative`
  - feeLimit = `-1` → `"feeLimit must be >= 0 and <= ..."`
- [ ] `validate_fail_fee_limit_above_max`
  - Setup: `saveMaxFeeLimit(1_000_000_000L)` and feeLimit = `1_000_000_001L`

Contract address collision
- [ ] `validate_fail_contract_address_already_exists`
  - Setup:
    - Build tx first (deterministic timestamps so txid is stable)
    - Compute `contractAddress = WalletUtil.generateContractAddress(tx)`
    - Pre-create an account at `contractAddress` in `AccountStore`
  - Expect: `"Trying to create a contract with existing contract address: ..."`

TRC-10 token argument validation (`checkTokenValueAndId`)
- [ ] `validate_fail_token_id_too_small`
  - tokenId = `1_000_000` → `"tokenId must be > 1000000"`
- [ ] `validate_fail_token_value_positive_token_id_zero`
  - tokenValue > 0, tokenId = `0` → `"invalid arguments with tokenValue = ..., tokenId = 0"`

TRC-10 token transfer validation (`MUtil.transferToken` → `VMUtils.validateForSmartContract`)
- [ ] `validate_fail_token_asset_missing`
  - tokenValue > 0, tokenId = `1_000_001`
  - Do *not* create the asset in asset stores
  - Expect: `"No asset !"`
- [ ] `validate_fail_token_balance_insufficient`
  - Create asset `1_000_001` in the store
  - Owner balance in that token smaller than tokenValue
  - Expect: `"assetBalance is not sufficient."`

Gated-by-config validations (only if enabling `CommonParameter.ENERGY_LIMIT_HARD_FORK`)
- [ ] Decide if conformance should run with energy-limit hard fork enabled (mainnet parity):
  - [ ] If yes, set `CommonParameter.setENERGY_LIMIT_HARD_FORK(true)` in test setup and add:
    - [ ] `validate_fail_origin_energy_limit_zero` → `"The originEnergyLimit must be > 0"`
    - [ ] `validate_fail_call_value_negative` → `"callValue must be >= 0"`
    - [ ] `validate_fail_token_value_negative` → `"tokenValue must be >= 0"`

---

## Phase 2 — Add deterministic `REVERT` / execution parity fixtures

Constructor revert path
- [ ] `edge_constructor_revert`
  - Use init code that executes `REVERT` (not invalid opcode).
  - Expect: runtime error `"REVERT opcode executed"` and no committed contract state.

Constructor out-of-energy
- [ ] `edge_constructor_out_of_energy`
  - Low `feeLimit` + init bytecode with infinite loop / heavy ops.
  - Expect: OOG-style runtime error; confirm Java’s exact message and encode it in fixture metadata.

Not enough energy to save returned runtime code
- [ ] `edge_not_enough_energy_to_save_code`
  - Construct init code that returns a non-trivial runtime code size.
  - Set `feeLimit` just high enough to run init but too low for
    `code.length * EnergyCost.getCreateData()`.
  - Expect: “not enough energy” exception string (from Java).

London invalid code prefix (0xEF)
- [ ] `edge_london_invalid_code_prefix_0xef`
  - Preconditions: `dbManager.getDynamicPropertiesStore().saveAllowTvmLondon(1)` (or equivalent VMConfig init).
  - Init code returns runtime code whose first byte is `0xEF`.
  - Expect: invalid-code runtime error (Java’s `invalidCodeException` message).

Optional success edge
- [ ] `edge_empty_runtime_code_success`
  - Init code returns empty runtime (`RETURN(0,0)`).
  - Expect: `SUCCESS` and contract exists but code store entry is empty/missing (document actual Java behavior).

---

## Phase 3 — Verification checklist

- [ ] Run: `./gradlew :framework:test --tests "org.tron.core.conformance.VmFixtureGeneratorTest" --dependency-verification=off`
- [ ] Confirm fixtures emitted under `conformance/fixtures/create_smart_contract/<caseName>/...`
- [ ] Spot-check a few `metadata.json` outputs:
  - `expectedStatus` matches intent
  - `expectedErrorMessage` is stable and meaningful (avoid `null`)
- [ ] If available, run the Rust backend conformance runner against the new fixtures to validate
  parity across status, receipt fields, and post-state.

