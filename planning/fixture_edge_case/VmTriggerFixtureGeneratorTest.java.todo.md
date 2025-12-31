# VM TriggerSmartContract Fixtures — Edge-Case TODO

Target file: `framework/src/test/java/org/tron/core/conformance/VmTriggerFixtureGeneratorTest.java`

Status: draft

Goal: expand TriggerSmartContract (type 31) fixture coverage so the Rust backend can be validated
against the meaningful Java validation + execution branches (not just “storage happy-path + missing
contract + OOG”).

Key Java references
- `actuator/src/main/java/org/tron/core/actuator/VMActuator.java` (`call()`, `checkTokenValueAndId(...)`,
  `getTotalEnergyLimit*`)
- `actuator/src/main/java/org/tron/core/vm/VMUtils.java` (`validateForSmartContract(...)` for TRX/TRC-10 transfers)
- `chainbase/src/main/java/org/tron/common/utils/StorageUtils.java` (`ENERGY_LIMIT_HARD_FORK` gating)
- Determinism helpers used elsewhere: `framework/src/test/java/org/tron/core/conformance/ConformanceFixtureTestSupport.java`

---

## Acceptance Criteria

- Each new case writes a complete fixture directory:
  - `pre_db/*.kv`
  - `request.pb`
  - `expected/post_db/*.kv`
  - `expected/result.pb` when receipt generation occurs
  - `metadata.json` with correct `expectedStatus` + `expectedErrorMessage`
- `caseCategory` matches intent:
  - `happy` → `SUCCESS`
  - `validate_fail` → `VALIDATION_FAILED` (ContractValidateException-style failures)
  - `edge` → deterministic runtime `REVERT`/`OUT_OF_ENERGY` parity cases
- DB list is coherent:
  - If `storage-row.kv` is generated, include `storage-row` in `databasesTouched` (or document why not).
- Avoid non-deterministic inputs that change txid/addresses (fixed timestamps, stable block context).

---

## Phase 0 — Baseline + determinism (do first)

- [ ] Decide whether to keep the VMTestBase-style generator or migrate to the shared `FixtureGenerator`
  pipeline used by other conformance tests (preferred for consistent status/error handling).
- [ ] Make `ExecutionContext` deterministic:
  - [ ] Replace `System.currentTimeMillis()` timestamps in request context with a fixed constant.
  - [ ] Set `metadata.json.blockNumber` / `blockTimestamp` to match the request context.
- [ ] Make contract deployment deterministic:
  - [ ] Avoid time-based tx fields that change `WalletUtil.generateContractAddress(tx)` across runs.
  - [ ] If you must keep current deployment helper, at least ensure the fixture’s “request” uses the
        *same* contract address that the Java run used.
- [ ] Add minimal sanity assertions so mislabeled fixtures can’t silently drift:
  - [ ] `Assert.assertNull(runtimeError)` for `happy_*`
  - [ ] `Assert.assertNotNull(errorMessage)` and substring checks for `validate_fail_*`

---

## Phase 1 — Add missing `VALIDATION_FAILED` fixtures (TriggerSmartContract)

VM enabled/disabled
- [ ] `validate_fail_vm_disabled`
  - Setup: force `supportVM == false` via dynamic properties.
  - Expect: `"VM work is off, need to be opened by the committee"`.

FeeLimit bounds (VMActuator.call feeLimit guard)
- [ ] `validate_fail_fee_limit_negative`
  - feeLimit = `-1`
  - Expect: `"feeLimit must be >= 0 and <= ..."`
- [ ] `validate_fail_fee_limit_above_max`
  - Setup: set a known `maxFeeLimit` then use `feeLimit = maxFeeLimit + 1`
  - Expect: same message (stable `<maxFeeLimit>` string)

Owner address validity/existence
- [ ] `validate_fail_owner_address_invalid_empty`
  - `ownerAddress = ByteString.EMPTY` (or wrong-length bytes)
  - Expect: address-validity failure (exact message depends on which path is reached).
- [ ] `validate_fail_owner_account_missing`
  - Use a valid-looking address not present in `AccountStore`
  - Expect: deterministic validation failure (confirm Java message and lock it in).

Contract address validity/existence
- [ ] `validate_fail_contract_address_missing`
  - Build TriggerSmartContract without `contractAddress`
  - Expect: `"Cannot get contract address from TriggerContract"`
- [ ] `validate_fail_contract_address_invalid_bytes`
  - wrong-length bytes (e.g. 10 bytes)
  - Expect: address-validity error
- [ ] `validate_fail_contract_not_smart_contract`
  - Use a valid address that exists as a normal account but has no entry in `ContractStore`
  - Expect: `"No contract or not a smart contract"`

callValue validation and funding
- [ ] `validate_fail_call_value_insufficient_balance`
  - Create a low-balance caller and set `callValue > balance`
  - Expect: internal transfer validation error (confirm exact string in `VMUtils.validateForSmartContract(...)`)
- [ ] `validate_fail_call_value_negative` (gated-by-config)
  - Decide whether conformance should run with `ENERGY_LIMIT_HARD_FORK` enabled:
    - [ ] If enabled: expect `"callValue must be >= 0"`
    - [ ] If disabled: expect `"Amount must be greater than or equals 0."` (from internal transfer validation)

TRC-10 token argument validation (`checkTokenValueAndId`)
- [ ] `validate_fail_token_value_positive_token_id_zero`
  - `callTokenValue > 0`, `tokenId = 0`
  - Expect: `"invalid arguments with tokenValue = ..., tokenId = 0"`
- [ ] `validate_fail_token_id_too_small`
  - `tokenId = 1_000_000` (or any `<= MIN_TOKEN_ID` and `!= 0`)
  - Expect: `"tokenId must be > 1000000"`

TRC-10 token transfer validation (`VMUtils.validateForSmartContract(..., tokenId, ...)`)
- [ ] `validate_fail_token_asset_missing`
  - `callTokenValue > 0`, `tokenId = 1_000_001`, do not create the asset
  - Expect: `"No asset !"`
- [ ] `validate_fail_token_balance_insufficient`
  - Create asset `1_000_001` and give caller a smaller token balance than `callTokenValue`
  - Expect: `"assetBalance is not sufficient."`

---

## Phase 2 — Add deterministic runtime parity fixtures (`REVERT` / `OUT_OF_ENERGY`)

Unknown selector / empty calldata
- [ ] `edge_empty_calldata_revert`
  - `data = ByteString.EMPTY` (or 0-length)
  - Expect: revert-style runtime error (confirm exact message + receipt fields)
- [ ] `edge_unknown_selector_revert`
  - Use a 4-byte selector that does not match any function
  - Expect: same as above (or a distinct “no function” path depending on compiler)

Nonpayable + callValue
- [ ] `edge_nonpayable_with_call_value_revert`
  - Call `testPut(...)` with `callValue > 0` (contract/function is nonpayable)
  - Expect: runtime revert (Solidity auto-reverts on nonpayable value transfer)
  - Verify: caller balance/contract balance rollback semantics match Java.

Explicit revert with reason (new minimal contract)
- [ ] `edge_revert_with_reason`
  - Deploy a contract with `require(false, "reason")` / `revert("reason")`
  - Expect: revert status and non-empty `return_data` (reason ABI) if exposed in receipt/result

Rollback after write
- [ ] `edge_write_then_revert_rollback`
  - Contract writes to storage then reverts
  - Verify post_db has no storage mutation (and no transfer side-effects)

Out-of-energy beyond the “feeLimit=1” trivial case
- [ ] `edge_out_of_energy_memory_expansion`
  - Large calldata + operations that expand memory (or a loop contract)
  - Goal: stress energy accounting rather than failing at the first opcode

---

## Phase 3 — StorageDemo boundary fixtures (high value for storage layout parity)

31-byte boundary (short-string vs long-string storage layout)
- [ ] `edge_long_string_gt_31_store_and_read`
  - Store a string length 32+ bytes and read back
- [ ] `edge_overwrite_long_to_short`
  - Store long string then overwrite with ≤31 bytes
- [ ] `edge_overwrite_short_to_long`
  - Store short string then overwrite with >31 bytes
- [ ] `edge_delete_long_string_refund`
  - Store long string then delete, verify storage cleanup/refund behavior

Empty/nonexistent entries
- [ ] `edge_delete_nonexistent_key_noop`
- [ ] `edge_read_nonexistent_key_returns_empty`
- [ ] `edge_put_empty_string`

---

## Phase 4 — Verification checklist

- [ ] Run: `./gradlew :framework:test --tests "org.tron.core.conformance.VmTriggerFixtureGeneratorTest" --dependency-verification=off`
- [ ] Confirm fixtures emitted under `conformance/fixtures/trigger_smart_contract/<caseName>/...`
- [ ] Spot-check a few `metadata.json` files:
  - [ ] `expectedStatus` matches intent and is supported by the consumer
  - [ ] `expectedErrorMessage` is stable (avoid environment-dependent prefixes)
  - [ ] `databasesTouched` aligns with the actual `pre_db/` + `expected/post_db/` contents
- [ ] (If available) run the Rust backend conformance runner over the new fixtures and compare:
  - [ ] status + error_message
  - [ ] receipt passthrough bytes (if used)
  - [ ] post-state key/value equality for touched DBs

