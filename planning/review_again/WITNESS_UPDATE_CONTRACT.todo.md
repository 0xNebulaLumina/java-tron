# TODO / Fix Plan: `WITNESS_UPDATE_CONTRACT` parity gaps

This checklist assumes we want Rust remote execution to match java-tron’s `WitnessUpdateActuator` behavior, especially the “update URL only” state transition.

## 0) Confirm the parity target (do this first)

- [ ] Confirm the Java→Rust mapping contract:
  - [ ] `RemoteExecutionSPI` sends `tx.from = WitnessUpdateContract.owner_address` (raw bytes)
  - [ ] `RemoteExecutionSPI` sends `tx.data = WitnessUpdateContract.update_url` bytes (payload only)
  - [ ] `tx.contract_parameter` carries the original `Any` (`type_url` + `value`) for `any.is(...)` parity when desired
- [ ] Confirm what we want for Rust `TronExecutionResult.state_changes`:
  - [ ] Embedded java-tron does not expose witness-store writes as EVM state changes; returning `[]` is likely correct for conformance logging.

## 1) Fix witness persistence (core fix)

Goal: mirror Java `WitnessUpdateActuator.updateWitness()` — update only the witness URL and preserve all other `protocol.Witness` fields.

Recommended approach (preserve full protobuf):

- [ ] Add a storage-layer helper that updates `protocol.Witness.url` without losing other fields, e.g.:
  - [ ] `EngineBackedEvmStateStore::update_witness_url(owner: &Address, new_url: String) -> Result<()>`
  - [ ] Implementation outline:
    - [ ] Load raw witness bytes from the witness DB using the witness key (`prefix + owner`).
    - [ ] Decode as `crate::protocol::Witness` (prost).
    - [ ] Replace only `witness.url`.
    - [ ] Re-encode and write back under the same key.
    - [ ] Ensure address prefix handling is unchanged (don’t rewrite the address field unless needed).

Alternative approach (expand `WitnessInfo` to be lossless):

- [ ] Extend `tron_backend_execution::WitnessInfo` to carry all relevant `protocol.Witness` fields:
  - [ ] `pub_key`, `total_produced`, `total_missed`, `latest_block_num`, `latest_slot_num`, `is_jobs`
- [ ] Update `WitnessInfo::deserialize()` to populate those fields.
- [ ] Update `WitnessInfo::serialize_with_prefix()` to serialize using stored values (not defaults).
- [ ] In `execute_witness_update_contract()`, update by cloning the loaded witness and changing only `url` (avoid constructing a new instance with defaults).

Decision points:

- [ ] Decide whether to keep or remove the “no-op write” optimization:
  - [ ] If strict behavioral parity is desired, always write (Java always calls `put`).
  - [ ] If keeping optimization, document why touched-key differences are acceptable.

## 2) Add missing `Any.is(...)` parity check (recommended)

Goal: mirror Java `validate()` contract-type behavior when `tx.contract_parameter` is present.

- [ ] In `execute_witness_update_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [ ] If `transaction.metadata.contract_parameter` is present, verify `type_url` matches `protocol.WitnessUpdateContract`.
  - [ ] On mismatch, return the Java-like message:
    - [ ] `contract type error, expected type [WitnessUpdateContract],real type[class com.google.protobuf.Any]`
- [ ] (Optional) Consider validating that `contract_parameter.value` is decodable as `WitnessUpdateContract` to mirror `any.unpack(...)` failure modes.

## 3) Review Any-unwrapping of `tx.data` (edge case hardening)

- [ ] Consider gating `unwrap_any_value_if_present(tx.data)` by contract type:
  - [ ] Exclude payload-style contracts (including `WITNESS_CREATE_CONTRACT` and `WITNESS_UPDATE_CONTRACT`) where `tx.data` is *not* the contract bytes.
  - [ ] Add a test for a crafted URL that matches Any wire-format to ensure it is stored as-is.

## 4) Tests (prevent regression)

Rust unit tests to add/adjust:

- [ ] **Preserve witness fields test**:
  - [ ] Store a witness record with non-default fields (e.g. `total_produced=7`, `latest_block_num=123`, etc.).
  - [ ] Execute witness update with a new URL.
  - [ ] Assert URL changed and all other witness fields are unchanged.
- [ ] **Any type_url mismatch test** (if `contract_parameter` is provided in transactions):
  - [ ] `contract_type = WitnessUpdateContract` but `contract_parameter.type_url` != `protocol.WitnessUpdateContract`
  - [ ] Assert the `contract type error...` message.

Java-side sanity:

- [ ] Run `./gradlew :framework:test --tests "org.tron.core.actuator.WitnessUpdateActuatorTest"`

## 5) Verification / conformance

- [ ] Run Rust tests: `cd rust-backend && cargo test`
- [ ] If dual-mode/conformance fixtures are used, rerun witness voting fixtures and ensure witness update does not change witness stats:
  - [ ] `framework/src/test/java/org/tron/core/conformance/WitnessVotingFixtureGeneratorTest.java`

