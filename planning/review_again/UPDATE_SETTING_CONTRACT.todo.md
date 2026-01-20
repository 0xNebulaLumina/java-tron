# UPDATE_SETTING_CONTRACT (type 33) — parity hardening TODO

## When a fix is “needed”
- No fix is required for current fixtures / `RemoteExecutionSPI` (it always sends `contract_parameter`, and the Rust handler matches java-tron on that path).
- Do this work if you need stricter parity for:
  - Any-less requests (older clients / synthetic tests), or
  - malformed/truncated protobuf payloads where exact `InvalidProtocolBufferException` text matters, or
  - exact bandwidth accounting (if anything depends on exact net usage).

## Checklist / plan

### 1) Lock in the current baseline
- [ ] Run Rust conformance for `conformance/fixtures/update_setting_contract/*` and confirm all cases pass with exact `metadata.json.expectedErrorMessage`.
- [ ] Verify that `RemoteExecutionSPI` always sets `TronTransaction.contract_parameter` for UpdateSettingContract (it should).

### 2) Decide what “parity” means for Any-less requests
Goal: either (a) explicitly require `contract_parameter` for this contract, or (b) define a stable fallback that still mirrors java-tron as closely as possible.

- [ ] Confirm whether Any-less requests can happen in production (older Java builds / custom callers).
- [ ] If Any-less requests are not supported:
  - [ ] In Rust, fail fast when `contract_parameter` is missing with a clear message (or keep current behavior but document it).
- [ ] If Any-less requests must be supported:
  - [ ] Avoid using `from_raw` as a semantic fallback for a missing payload `owner_address` (Java validates only the payload owner field).
  - [ ] Keep the type-mismatch heuristic only as a fixture-compat branch, and gate it narrowly (so valid UpdateSetting payloads can’t be misclassified).

### 3) Align malformed-protobuf error strings (optional strict parity)
Goal: make Rust parse failures match Java’s `InvalidProtocolBufferException` messages (only if you truly need exact text parity).

- [ ] Replace `parse_update_setting_contract`’s lightweight parser with a prost decode of the real message type, and map decode failures to a stable error string that matches Java (or update fixture expectations accordingly).
- [ ] Add targeted Rust unit tests around decode failures if you implement this (to prevent regressions).

### 4) Bandwidth accounting strictness (optional)
- [ ] If required, compute `bandwidth_used` based on the exact serialized transaction size Java uses for net usage, not a simplified estimator.
- [ ] Add/extend fixtures to assert net usage if/when Java’s fixture generator starts emitting it for this contract type.

