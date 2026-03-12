# TODO / Fix Plan: `WITNESS_UPDATE_CONTRACT` parity gaps

This checklist assumes we want Rust remote execution to match java-tron's `WitnessUpdateActuator` behavior, especially the "update URL only" state transition.

## 0) Confirm the parity target (do this first)

- [x] Confirm the Java→Rust mapping contract:
  - [x] `RemoteExecutionSPI` sends `tx.from = WitnessUpdateContract.owner_address` (raw bytes)
  - [x] `RemoteExecutionSPI` sends `tx.data = WitnessUpdateContract.update_url` bytes (payload only)
  - [x] `tx.contract_parameter` carries the original `Any` (`type_url` + `value`) for `any.is(...)` parity when desired
- [x] Confirm what we want for Rust `TronExecutionResult.state_changes`:
  - [x] Embedded java-tron does not expose witness-store writes as EVM state changes; returning `[]` is likely correct for conformance logging.

## 1) Fix witness persistence (core fix)

Goal: mirror Java `WitnessUpdateActuator.updateWitness()` — update only the witness URL and preserve all other `protocol.Witness` fields.

Recommended approach (preserve full protobuf):

- [x] Add a storage-layer helper that updates `protocol.Witness.url` without losing other fields, e.g.:
  - [x] `EngineBackedEvmStateStore::update_witness_url(owner: &Address, new_url: &str) -> Result<()>`
  - [x] Implementation outline:
    - [x] Load raw witness bytes from the witness DB using `buffered_get` (respects write buffer).
    - [x] Decode as `crate::protocol::Witness` (prost).
    - [x] Replace only `witness.url`.
    - [x] Re-encode and write back under the same key via `buffered_put`.
    - [x] Ensure address prefix handling is unchanged (don't rewrite the address field unless needed).

Alternative approach (expand `WitnessInfo` to be lossless):

- N/A — Chose the recommended approach above instead.

Decision points:

- [x] Decide whether to keep or remove the "no-op write" optimization:
  - [x] Removed: Java always calls `witnessStore.put(...)` so we always write for strict behavioral parity.

## 2) Add missing `Any.is(...)` parity check (recommended)

Goal: mirror Java `validate()` contract-type behavior when `tx.contract_parameter` is present.

- [x] In `execute_witness_update_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [x] If `transaction.metadata.contract_parameter` is present, verify `type_url` matches `protocol.WitnessUpdateContract`.
  - [x] On mismatch, return the Java-like message:
    - [x] `contract type error, expected type [WitnessUpdateContract],real type[class com.google.protobuf.Any]`
- [ ] (Optional) Consider validating that `contract_parameter.value` is decodable as `WitnessUpdateContract` to mirror `any.unpack(...)` failure modes.

## 3) Review Any-unwrapping of `tx.data` (edge case hardening)

- [x] Consider gating `unwrap_any_value_if_present(tx.data)` by contract type:
  - [x] Exclude payload-style contracts (including `WITNESS_CREATE_CONTRACT` and `WITNESS_UPDATE_CONTRACT`) where `tx.data` is *not* the contract bytes.
  - [x] Add a test for a crafted URL that matches Any wire-format to ensure it is stored as-is (`test_witness_update_crafted_any_url_not_unwrapped`).

## 4) Tests (prevent regression)

Rust unit tests to add/adjust:

- [x] **Preserve witness fields test** (`test_witness_update_preserves_all_witness_fields`):
  - [x] Store a witness record with non-default fields (e.g. `total_produced=7`, `latest_block_num=123456`, etc.).
  - [x] Execute witness update with a new URL.
  - [x] Assert URL changed and all other witness fields are unchanged (verified via raw protobuf decode).
- [x] **Any type_url mismatch test** (`test_witness_update_any_type_url_mismatch`):
  - [x] `contract_type = WitnessUpdateContract` but `contract_parameter.type_url` != `protocol.WitnessUpdateContract`
  - [x] Assert the `contract type error...` message.
- [x] **Any type_url correct test** (`test_witness_update_any_type_url_correct`):
  - [x] Correct `type_url` passes validation and update succeeds.
- [x] **Always-write test** (`test_witness_update_always_writes_even_same_url`):
  - [x] Same URL update still succeeds (no-op optimization removed).

Java-side sanity:

- [ ] Run `./gradlew :framework:test --tests "org.tron.core.actuator.WitnessUpdateActuatorTest"`

## 5) Verification / conformance

- [x] Run Rust tests: `cd rust-backend && cargo test` — 430 passed, 3 pre-existing failures (vote_witness tests unrelated to this change)
- [x] Run fixture conformance: `./scripts/ci/run_fixture_conformance.sh --rust-only` — All passed including all WITNESS_UPDATE_CONTRACT fixtures
- [ ] If dual-mode/conformance fixtures are used, rerun witness voting fixtures and ensure witness update does not change witness stats:
  - [ ] `framework/src/test/java/org/tron/core/conformance/WitnessVotingFixtureGeneratorTest.java`
