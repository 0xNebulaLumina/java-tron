# Review: `VmFixtureGeneratorTest.java`

File under review: `framework/src/test/java/org/tron/core/conformance/VmFixtureGeneratorTest.java`

Purpose: generate conformance fixtures for TVM contract execution parity (Java ↔ Rust backend). In
practice, this file currently generates fixtures only for:
- `CreateSmartContract` (type 30)

Note: `TriggerSmartContract` (type 31) fixtures are generated in
`framework/src/test/java/org/tron/core/conformance/VmTriggerFixtureGeneratorTest.java`. This file
still contains helper code for trigger fixtures, but it has **no** `@Test` coverage for triggers.

---

## What the file already covers

CreateSmartContract (30)
- `happy_path`: deploy `StorageDemo` (no callValue).
- `happy_path_with_value`: deploy `PayableContract` with `callValue = 1 TRX`.
- `validate_fail_insufficient_balance`: deploy with `callValue > balance` to force a deterministic
  internal-transfer validation failure.
- `edge_invalid_bytecode`: deploy with init bytecode that should fail at runtime (invalid opcode).

That’s a good start, but it only hits a small subset of the validation and execution branches that
matter for parity.

---

## Missing edge cases (high-signal gaps)

The “missing” list is driven by `CreateSmartContract` paths in:
- `actuator/src/main/java/org/tron/core/actuator/VMActuator.java` (`create()` + `execute()` hooks)
- `actuator/src/main/java/org/tron/core/vm/VMUtils.java` (`validateForSmartContract(...)`)
- `actuator/src/main/java/org/tron/core/vm/VMConstant.java` (limits like `CONTRACT_NAME_LENGTH`)

### A) Validation branches not covered (should produce `VALIDATION_FAILED`)

VM enabled / disabled
- **VM disabled**: `DynamicPropertiesStore.supportVM()` (aka `ALLOW_CREATION_OF_CONTRACTS=0`) →
  `"vm work is off, need to be opened by the committee"`.

Owner / origin consistency
- **Owner != origin**: `contract.ownerAddress != newContract.originAddress` →
  `"OwnerAddress is not equals OriginAddress"`.

Name / percent bounds
- **Contract name too long**: `name.getBytes().length > 32` →
  `"contractName's length cannot be greater than 32"`.
  - Important sub-case: multibyte UTF-8 characters can push `byte` length > 32 even if the visible
    char count is ≤ 32.
- **consumeUserResourcePercent out of range**: `< 0` or `> 100` →
  `"percent must be >= 0 and <= 100"`.
  - Boundary-success fixtures (`0`, `100`) also add value.

Fee limit bounds
- **feeLimit negative / above max**:
  - `"feeLimit must be >= 0 and <= <maxFeeLimit>"`
  - Current fixtures always use `DEFAULT_FEE_LIMIT` and never exercise the `maxFeeLimit` boundary.

Contract address collision
- **contract address already exists**:
  - Pre-create an account at `WalletUtil.generateContractAddress(tx)` and then deploy with that tx →
    `"Trying to create a contract with existing contract address: <base58>"`

TRC-10 token argument validation (especially important because this test explicitly enables it)
- **tokenId too small** (`tokenId <= 1_000_000 && tokenId != 0`) →
  `"tokenId must be > 1000000"`.
- **tokenValue > 0 but tokenId == 0** →
  `"invalid arguments with tokenValue = X, tokenId = 0"`.

TRC-10 token transfer validations (happen later via `MUtil.transferToken(...)` → `VMUtils`)
- **tokenId points to a non-existent asset** → `"No asset !"`
- **owner has no token balance** → `"assetBalance must greater than 0."`
- **owner token balance insufficient** → `"assetBalance is not sufficient."`

Account existence / internal transfer validations
- **owner account missing** (or invalid owner bytes) should be captured explicitly; today’s fixtures
  only cover “owner exists but balance insufficient”.

Gated-by-config validations worth calling out
- Some validations in `VMActuator.create()` only run when
  `CommonParameter.ENERGY_LIMIT_HARD_FORK` / `StorageUtils.getEnergyLimitHardFork()` is enabled:
  - `callValue >= 0`
  - `tokenValue >= 0`
  - `originEnergyLimit > 0`
  If conformance targets mainnet-like behavior, these fixtures should exist (and the test harness
  should explicitly enable the flag).

### B) Execution / runtime parity gaps (should produce `REVERT` or `SUCCESS` deterministically)

Constructor REVERT vs invalid opcode
- **Constructor `REVERT` opcode path**: distinct from “invalid bytecode” because it should hit the
  explicit revert handling (`runtimeError = "REVERT opcode executed"`).

Out-of-energy during constructor
- **OOG in init code**: low `feeLimit` + infinite loop / heavy op sequence. This is a common source
  of cross-VM divergence (energy accounting, error strings, receipt fields).

Not enough energy to save returned runtime code
- In `VMActuator.execute()`, after init returns, Java charges
  `saveCodeEnergy = code.length * EnergyCost.getCreateData()`. If that exceeds remaining energy,
  creation should fail with a “not enough energy” exception. There is no fixture for this.

London invalid code prefix (EIP-3541 style)
- If `VMConfig.allowTvmLondon()` is enabled, Java rejects runtime code whose first byte is `0xEF`.
  This is a high-risk parity point and should have a dedicated fixture.

Constructor with state initialization
- Current “happy” deployment doesn’t exercise constructor storage writes, so `contract-state` often
  remains trivial. A constructor that `SSTORE`s (and optionally emits an event) would increase
  confidence that state roots + receipts match between Java and Rust.

### C) Fixture quality / determinism issues (not edge cases, but can mask them)

- **Non-deterministic timestamps**: uses `System.currentTimeMillis()` for tx timestamps/expiration,
  which changes txid and derived contract address across runs. Other conformance generators use
  `ConformanceFixtureTestSupport` fixed timestamps for reproducibility.
- **No assertions**: cases named `happy_path` / `validate_fail_*` can silently drift if the setup
  doesn’t hit the intended branch; the generator will still write fixtures.

---

## Recommended “minimal but high-value” additions

If you only add a handful of new CreateSmartContract fixtures, the most valuable gaps to close are:
- `validate_fail_vm_disabled` (supportVM off)
- `validate_fail_owner_origin_mismatch`
- `validate_fail_contract_name_too_long` (+ boundary `name_len_32_ok`)
- `validate_fail_percent_out_of_range` (+ boundary `percent_0_ok`, `percent_100_ok`)
- `validate_fail_fee_limit_above_max`
- `validate_fail_contract_address_already_exists`
- `validate_fail_token_value_positive_token_id_zero`
- `validate_fail_token_id_too_small`
- `validate_fail_token_asset_missing` (TRC-10)
- `edge_constructor_revert`
- `edge_constructor_out_of_energy`
- `edge_not_enough_energy_to_save_code`
- (optional but very high signal) `edge_london_invalid_code_prefix_0xef`

