# Review: `VmTriggerFixtureGeneratorTest.java`

File under review: `framework/src/test/java/org/tron/core/conformance/VmTriggerFixtureGeneratorTest.java`

Purpose: generate conformance fixtures for `TriggerSmartContract` (type 31) by deploying a simple
contract (`StorageDemo`) and capturing `pre_db/` + `expected/post_db/` snapshots plus request/receipt
protobufs.

---

## What the file already covers

TriggerSmartContract (31)
- `happy_path`: `testPut(1, "hello")` (first write into a mapping).
- `storage_overwrite`: `testPut(1, "abc")` then overwrite with another short string.
- `view_function`: read via `int2str(66)` after writing `testPut(66, "test")`.
- `delete_storage`: `testDelete(1)` after writing `testPut(1, "hello")`.
- `edge_nonexistent_contract`: trigger a valid-looking but missing contract address.
- `edge_out_of_energy`: trigger with an extremely low `feeLimit` to force OOG.

That’s a reasonable “minimum viable” trigger fixture set, but it only exercises a narrow slice of
the validation + execution space for smart contract calls.

---

## Missing edge cases (high-signal gaps)

These gaps are driven mainly by TriggerSmartContract paths in:
- `actuator/src/main/java/org/tron/core/actuator/VMActuator.java` (`call()` + `checkTokenValueAndId(...)`)
- `actuator/src/main/java/org/tron/core/vm/VMUtils.java` (internal TRX/TRC-10 transfer validation)

### A) Validation branches not covered (should be deterministic)

VM enabled/disabled
- `supportVM == false` should fail with: `"VM work is off, need to be opened by the committee"`.

Fee limit bounds
- `feeLimit < 0` and `feeLimit > DynamicPropertiesStore.maxFeeLimit` should fail with:
  `"feeLimit must be >= 0 and <= <maxFeeLimit>"`.
  - Current fixtures only cover “too small energy” at runtime, not invalid feeLimit validation.

Trigger proto shape / address validity
- Missing/empty `contractAddress` should hit `"Cannot get contract address from TriggerContract"`.
- Invalid address bytes (wrong length/prefix) for `ownerAddress` / `contractAddress` should fail via
  `DecodeUtil.addressValid(...)` (exact error depends on which validation path is hit).
- Owner account does not exist (valid bytes but absent in `AccountStore`) should be explicitly
  covered (today only the pre-seeded owner is used).

CallValue validation + internal transfer checks
- `callValue < 0` (proto allows int64) is an important gated-by-config branch:
  - when `StorageUtils.getEnergyLimitHardFork()` is enabled: `"callValue must be >= 0"`;
  - otherwise: it can fail later via `VMUtils.validateForSmartContract(...)` with
    `"Amount must be greater than or equals 0."`.
- `callValue > balance` should fail deterministically (insufficient balance on internal transfer).

TRC-10 transfer arguments + validations (not covered at all)
- `callTokenValue > 0 && tokenId == 0` → `"invalid arguments with tokenValue = X, tokenId = 0"`.
- `0 < tokenId <= MIN_TOKEN_ID` → `"tokenId must be > 1000000"`.
- Missing asset / insufficient token balance errors from `VMUtils.validateForSmartContract(..., tokenId, ...)`:
  `"No asset !"`, `"assetBalance must greater than 0."`, `"assetBalance is not sufficient."`.

### B) Runtime / VM-execution parity gaps (high risk for cross-VM divergence)

Unknown selector / empty calldata
- Triggering with empty `data` or a non-existent function selector should deterministically hit the
contract dispatcher “no match” path (typically REVERT with empty reason), and is a common parity
issue (return_data, receipt fields, and error strings).

Explicit contract `REVERT` (with and without reason)
- There is no fixture that forces a Solidity `require(false, "...")` / `revert("...")` path, which
is where return-data and error-message parity often breaks.

Rollback semantics
- No fixture asserts rollback behavior when execution fails after state changes:
  - storage writes followed by REVERT (storage must be rolled back);
  - `callValue` / TRC-10 transfers combined with REVERT (transfer rollback should be verified).

Logs/events
- Current contract emits no events; no fixture checks `LOG*` op behavior (topics/data encoding).

Internal calls / precompiles
- No fixtures for contracts that `CALL` another contract or touch precompiles; both are frequent
sources of subtle differences (gas/energy, revert bubbling, logs, internal tx list).

### C) StorageDemo-specific gaps (important because current fixtures only hit “short string” layout)

String length boundary at 31 bytes
- All stored strings are ≤ 5 bytes, so the fixtures only cover the **short-string-in-one-slot**
storage layout. Missing but very high value:
  - store/read strings **> 31 bytes** (multi-slot layout);
  - overwrite long→long with different lengths;
  - overwrite long→short and short→long (clearing/refund behavior differs).

Empty/nonexistent entries
- `testDelete(key)` where key is not set (no-op) isn’t covered.
- `int2str(key)` where key is not set / has been deleted (returns empty string) isn’t covered.
- `testPut(key, "")` (empty string) isn’t covered (can differ from delete semantics in storage layout).

### D) Fixture quality risks (can mask intended “edge” coverage)

- Non-determinism: `buildTriggerRequest(...)` uses `System.currentTimeMillis()` for
  `ExecutionContext.block_timestamp` and the metadata omits `blockNumber/blockTimestamp`.
- `databasesTouched` in `metadata.json` omits `storage-row` even though snapshots include it; if the
  consumer uses `databasesTouched` as the source of truth, this can silently drop an important DB.
- Status schema drift: one fixture writes `expectedStatus = "OUT_OF_ENERGY"` even though
  `FixtureMetadata.Builder` only validates `SUCCESS|REVERT|VALIDATION_FAILED`.

---

## Recommended minimal additions (if you only add a few)

High-signal TriggerSmartContract fixtures to add next:
- `validate_fail_fee_limit_negative` and `validate_fail_fee_limit_above_max`
- `validate_fail_owner_account_missing`
- `validate_fail_call_value_insufficient_balance`
- `validate_fail_token_value_positive_token_id_zero` and `validate_fail_token_id_too_small`
- `edge_unknown_selector_or_empty_data_revert`
- `edge_revert_with_reason` + `edge_write_then_revert_rollback`
- `edge_long_string_storage_layout_gt_31` (read + overwrite across the 31-byte boundary)

