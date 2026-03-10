# UPDATE_SETTING_CONTRACT (type 33) — parity hardening TODO

## When a fix is "needed"
- No fix is required for current fixtures / `RemoteExecutionSPI` (it always sends `contract_parameter`, and the Rust handler matches java-tron on that path).
- Do this work if you need stricter parity for:
  - Any-less requests (older clients / synthetic tests), or
  - malformed/truncated protobuf payloads where exact `InvalidProtocolBufferException` text matters, or
  - exact bandwidth accounting (if anything depends on exact net usage).

## Checklist / plan

### 1) Lock in the current baseline
- [x] Run Rust conformance for `conformance/fixtures/update_setting_contract/*` and confirm all cases pass with exact `metadata.json.expectedErrorMessage`.
  - All 12 fixtures pass: 3 happy-path (happy_path, happy_path_100, happy_path_zero) + 9 validation-fail cases.
- [x] Verify that `RemoteExecutionSPI` always sets `TronTransaction.contract_parameter` for UpdateSettingContract (it should).
  - Confirmed: `RemoteExecutionSPI.java:788-804` always unpacks `contractParameter` and sets it via `.setContractParameter(contractParameter)` at line 1074.

### 2) Decide what "parity" means for Any-less requests
Goal: either (a) explicitly require `contract_parameter` for this contract, or (b) define a stable fallback that still mirrors java-tron as closely as possible.

- [x] Confirm whether Any-less requests can happen in production (older Java builds / custom callers).
  - **Decision**: Any-less requests CANNOT happen in production. `RemoteExecutionSPI` always sends `contract_parameter`. The Any-less fallback path is retained only for backward compatibility with older test clients.
- [x] If Any-less requests are not supported:
  - [x] In Rust, fail fast when `contract_parameter` is missing with a clear message (or keep current behavior but document it).
    - **Implemented**: Added a `warn!` log when `contract_parameter` is absent, documenting that the Any-less path is not used by `RemoteExecutionSPI` and may not achieve full Java parity. Kept existing fallback behavior for backward compatibility rather than hard-failing.
- [N/A] If Any-less requests must be supported:
  - [N/A] Avoid using `from_raw` as a semantic fallback for a missing payload `owner_address` (Java validates only the payload owner field).
  - [N/A] Keep the type-mismatch heuristic only as a fixture-compat branch, and gate it narrowly (so valid UpdateSetting payloads can't be misclassified).

### 3) Align malformed-protobuf error strings (optional strict parity)
Goal: make Rust parse failures match Java's `InvalidProtocolBufferException` messages (only if you truly need exact text parity).

- [x] Replace `parse_update_setting_contract`'s lightweight parser with a prost decode of the real message type, and map decode failures to a stable error string that matches Java (or update fixture expectations accordingly).
  - **Decision**: Kept the lightweight parser. The current parser correctly handles all 12 conformance fixtures. Java's `InvalidProtocolBufferException` messages are specific to the Java protobuf library; replicating exact strings would be fragile. No conformance fixtures test malformed-protobuf error text. If needed in the future, prost decode can be added with error message mapping.
- [x] Add targeted Rust unit tests around decode failures if you implement this (to prevent regressions).
  - **Implemented**: Added 14 unit tests in `rust-backend/crates/core/src/service/tests/contracts/update_setting.rs` covering:
    - `test_type_url_mismatch` — wrong type_url returns correct error
    - `test_invalid_owner_address_empty` — empty owner → "Invalid address"
    - `test_invalid_owner_address_wrong_length` — short owner → "Invalid address"
    - `test_owner_account_not_exist` — missing account → "Account[...] does not exist"
    - `test_percent_over_100` — percent > 100 → "percent not in [0, 100]"
    - `test_negative_percent` — percent < 0 → "percent not in [0, 100]"
    - `test_contract_not_exist` — missing contract → "Contract does not exist"
    - `test_empty_contract_address_falls_through` — empty address → "Contract does not exist"
    - `test_not_owner_of_contract` — wrong origin → "Account[...] is not the owner of the contract"
    - `test_happy_path_update_percent` — updates to 75, verifies stored value
    - `test_happy_path_update_to_zero` — updates to 0, verifies stored value
    - `test_happy_path_update_to_100` — updates to 100, verifies stored value
    - `test_disabled_config_falls_back` — feature gate rejects when disabled
    - `test_parse_empty_data` — empty protobuf bytes → "Invalid address"

### 4) Bandwidth accounting strictness
- [x] If required, compute `bandwidth_used` based on the exact serialized transaction size Java uses for net usage, not a simplified estimator.
  - **Implemented**: `calculate_bandwidth_usage` already prefers `transaction_bytes_size` (sent by Java via gRPC field 4 of `ExecuteTransactionRequest`) when available. Java computes the exact value: `clearRet().getSerializedSize() + numContracts * MAX_RESULT_SIZE_IN_TX` (where MAX_RESULT_SIZE_IN_TX = 64). This makes the production path byte-exact. The fallback approximation (base 60 + data_len + 65) is only used when the field is missing (e.g., conformance fixtures that predate the field).
  - **Tests added**: 3 dedicated bandwidth tests in `update_setting.rs`:
    - `test_bandwidth_uses_java_computed_bytes_size` — verifies exact Java value (280) is returned
    - `test_bandwidth_fallback_without_bytes_size` — verifies fallback formula when field is absent
    - `test_bandwidth_zero_bytes_size_uses_fallback` — verifies 0 triggers fallback (not literal 0)
  - Happy-path test `test_happy_path_update_percent` also asserts `bandwidth_used == transaction_bytes_size`.
- [x] Add/extend fixtures to assert net usage if/when Java's fixture generator starts emitting it for this contract type.
  - **Implemented**: Updated `FixtureGenerator.buildRequest()` to compute and set `transaction_bytes_size` using the same formula as `RemoteExecutionSPI` and `BandwidthProcessor.consume()`: `clearRet().getSerializedSize() + numContracts * MAX_RESULT_SIZE_IN_TX`. All 12 regenerated fixtures now include non-zero `transaction_bytes_size` values (e.g., happy_path=188, validate_fail_negative_percent=198). The Rust conformance runner reads this value and passes it through `calculate_bandwidth_usage`, ensuring the exact Java-computed bandwidth is used during conformance testing.
