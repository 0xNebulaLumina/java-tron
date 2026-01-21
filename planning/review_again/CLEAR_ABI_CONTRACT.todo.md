# CLEAR_ABI_CONTRACT (type 48) — parity hardening TODO (only if we want stricter matching)

## When a fix is “needed”
- Only do this work if we care about strict parity beyond the current conformance fixtures, especially for:
  - malformed / truncated protobuf payload handling (exact error text + ordering)
  - exact bandwidth accounting
  - behavior when `contract_parameter` (`google.protobuf.Any`) is omitted

## Checklist / plan

### 1) Confirm current parity baseline
- [ ] Run the Rust conformance runner for `conformance/fixtures/clear_abi_contract/*`.
- [ ] Verify all cases pass: `happy_path`, `happy_path_no_abi`, and all `validate_fail_*` cases.
- [ ] If anything fails, record which fixture and the observed error/state diff.

### 2) Malformed protobuf parity (recommended if we want to be exhaustive)
Goal: match java-tron’s `InvalidProtocolBufferException` behavior/message when payload bytes are malformed.

- [ ] Add new java-tron fixture(s) in `framework/src/test/java/org/tron/core/conformance/ContractMetadataFixtureGeneratorTest.java`:
  - [ ] truncated varint in field tag
  - [ ] truncated length-delimited owner_address
  - [ ] invalid wire type / invalid tag (zero)
  - [ ] ensure `expectedErrorMessage` in `metadata.json` captures the real java exception text
- [ ] Update Rust parser in `rust-backend/crates/core/src/service/mod.rs`:
  - Option A (preferred for correctness): decode `protocol::ClearABIContract` via `prost::Message::decode` and map decode errors to java-tron’s messages when necessary.
  - Option B (minimal change): extend `parse_clear_abi_contract` with error mapping similar to `parse_update_brokerage_contract` (map truncation/EOF cases to protobuf-java’s standard truncation message).
- [ ] Keep ordering identical: type_url check before unpack/decode.

### 3) Decide policy for missing `contract_parameter`
Rust currently tries to proceed without `contract_parameter` and uses heuristics.

- [ ] Decide whether we *officially support* `contract_parameter` being omitted for non-VM contracts.
  - [ ] If **yes**: document the guarantees (best-effort parity) and keep heuristics consistent across system contracts.
  - [ ] If **no**: fail fast with a clear error (breaking change; likely not desired for backward compatibility).
- [ ] If keeping support, add a dedicated conformance fixture variant where `contract_parameter` is absent to lock in behavior.

### 4) Bandwidth / receipt parity (optional)
- [ ] Determine whether `bandwidth_used` must match java-tron’s real serialized size-based accounting.
  - [ ] If yes: compute bandwidth based on actual protobuf serialization size of the request/tx (or implement a java-tron-equivalent net usage computation).
- [ ] Consider filling `tron_transaction_result` for ClearABIContract:
  - [ ] Build a minimal `Protocol.Transaction.Result` with `fee=0` and `ret=SUCESS` using `TransactionResultBuilder`.
  - [ ] Confirm Java-side still behaves correctly if this field is populated.

### 5) Regression and CI hooks
- [ ] Add a targeted conformance run mode (filter to `clear_abi_contract`) to keep CI fast.
- [ ] Ensure new fixtures are checked in under `conformance/fixtures/clear_abi_contract/`.

## Rollout notes
- Keep execution behind `remote.clear_abi_enabled` if behavior changes.
- Prefer adding fixtures first, then changing Rust, to prevent regressions across the contract-metadata family (33/45/48).

