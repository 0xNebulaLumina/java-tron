# CLEAR_ABI_CONTRACT (type 48) — parity hardening TODO (only if we want stricter matching)

## When a fix is “needed”
- Only do this work if we care about strict parity beyond the current conformance fixtures, especially for:
  - malformed / truncated protobuf payload handling (exact error text + ordering)
  - exact bandwidth accounting
  - behavior when `contract_parameter` (`google.protobuf.Any`) is omitted

## Checklist / plan

### 1) Confirm current parity baseline
- [x] Run the Rust conformance runner for `conformance/fixtures/clear_abi_contract/*`.
- [x] Verify all cases pass: `happy_path`, `happy_path_no_abi`, and all `validate_fail_*` cases.
- [x] If anything fails, record which fixture and the observed error/state diff.

**Result**: All 10 fixtures pass:
- `happy_path` ✓
- `happy_path_no_abi` ✓
- `validate_fail_constantinople_disabled` ✓
- `validate_fail_contract_address_empty` ✓
- `validate_fail_contract_not_exist` ✓
- `validate_fail_not_owner` ✓
- `validate_fail_owner_account_not_exist` ✓
- `validate_fail_owner_address_empty` ✓
- `validate_fail_owner_address_wrong_length` ✓
- `validate_fail_type_mismatch` ✓

### 2) Malformed protobuf parity (recommended if we want to be exhaustive)
Goal: match java-tron's `InvalidProtocolBufferException` behavior/message when payload bytes are malformed.

- [x] Add new java-tron fixture(s) in `framework/src/test/java/org/tron/core/conformance/ContractMetadataFixtureGeneratorTest.java`:
  - [x] truncated varint in field tag (covered by `validate_fail_invalid_protobuf_bytes`)
  - [x] truncated length-delimited owner_address (covered by same fixture with invalid varint length)
  - [x] invalid wire type / invalid tag (zero) - covered by truncation error mapping
  - [x] ensure `expectedErrorMessage` in `metadata.json` captures the real java exception text
- [x] Update Rust parser in `rust-backend/crates/core/src/service/mod.rs`:
  - Implemented Option B: extended `parse_clear_abi_contract` with error mapping similar to `parse_update_brokerage_contract` (maps truncation/EOF cases to protobuf-java's standard truncation message).
- [x] Keep ordering identical: type_url check before unpack/decode.

**Result**: Added `validate_fail_invalid_protobuf_bytes` fixture that tests malformed protobuf handling.
Updated Rust parser to return java-tron-compatible error message:
"While parsing a protocol message, the input ended unexpectedly in the middle of a field.  This could mean either that the input has been truncated or that an embedded message misreported its own length."

### 3) Decide policy for missing `contract_parameter`
Rust currently tries to proceed without `contract_parameter` and uses heuristics.

- [x] Decide whether we *officially support* `contract_parameter` being omitted for non-VM contracts.
  - [x] **Yes**: Keep best-effort support for backward compatibility. The current heuristic approach:
    1. If `contract_parameter` is present, use its `value` bytes
    2. If missing, fall back to `transaction.data`
    3. Type-mismatch detection via `from_raw` emptiness check
- [ ] If keeping support, add a dedicated conformance fixture variant where `contract_parameter` is absent to lock in behavior.
  - Note: This is optional and would require modifying the Java fixture generator to create transactions without `contract_parameter`. Current fixtures always include it.

### 4) Bandwidth / receipt parity (optional)
- [~] Determine whether `bandwidth_used` must match java-tron's real serialized size-based accounting.
  - Rust uses `calculate_bandwidth_usage()` which provides an approximate value.
  - This is documented as "best-effort" and doesn't affect conformance tests since bandwidth isn't compared in state.
- [~] Consider filling `tron_transaction_result` for ClearABIContract:
  - Currently returns `tron_transaction_result: None`
  - Java-side reconstructs receipt when this is empty (documented in backend.proto)
  - Low priority: fee=0 and no special receipt fields for ClearABIContract

### 5) Regression and CI hooks
- [x] Add a targeted conformance run mode (filter to `clear_abi_contract`) to keep CI fast.
  - Command: `CONFORMANCE_FIXTURES_DIR="../conformance/fixtures" cargo test --package tron-backend-core conformance -- --ignored | grep -i clear_abi`
- [x] Ensure new fixtures are checked in under `conformance/fixtures/clear_abi_contract/`.
  - Added: `validate_fail_invalid_protobuf_bytes` fixture for malformed protobuf testing

## Rollout notes
- Keep execution behind `remote.clear_abi_enabled` if behavior changes.
- Prefer adding fixtures first, then changing Rust, to prevent regressions across the contract-metadata family (33/45/48).

