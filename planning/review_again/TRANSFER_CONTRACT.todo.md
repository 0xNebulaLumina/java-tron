# TODO / Fix Plan: `TRANSFER_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/TRANSFER_CONTRACT.planning.md`.

## 0) Decide the “parity target” (do this first)

- [ ] Confirm desired scope:
  - [ ] **Actuator-only parity** (match `TransferActuator.validate + execute`)
  - [ ] **End-to-end parity** (also match BandwidthProcessor/AEXT outcomes in remote mode)
- [ ] Confirm network expectations:
  - [ ] enforce strict `DecodeUtil.addressValid` semantics for both owner/to (21 bytes, prefix == configured prefix)
  - [ ] decide whether the Rust side must accept fixtures with malformed addresses and still match Java’s *error ordering*
- [ ] Confirm which feature flags/dynamic properties are assumed in the target environment:
  - [ ] `ALLOW_MULTI_SIGN`
  - [ ] `supportBlackHoleOptimization`
  - [ ] `FORBID_TRANSFER_TO_CONTRACT`
  - [ ] `ALLOW_TVM_COMPATIBLE_EVM`

## 1) Fix `toAddress` validation to match `DecodeUtil.addressValid`

Goal: match Java validation for `toAddress`:

- non-empty
- exactly 21 bytes
- prefix byte == `storage_adapter.address_prefix()`
- error message: `"Invalid toAddress!"`

Checklist:

- [ ] Extend `tron_backend_execution::TxMetadata` (`rust-backend/crates/execution/src/tron_evm.rs`) to carry `to_raw: Option<Vec<u8>>` (analogous to `from_raw`).
- [ ] Populate `to_raw` in gRPC conversion (`rust-backend/crates/core/src/service/grpc/conversion.rs`) from `tx.to`.
- [ ] In `execute_transfer_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [ ] Validate `to_raw` using the same rules as Java (21 bytes + prefix match).
  - [ ] Only after validation, derive the 20-byte EVM `to` address (strip prefix) for storage access and comparisons.
  - [ ] Keep error ordering: owner-address validation must happen before to-address validation.

## 2) Avoid early conversion failure for malformed `to` addresses (error-order parity)

Goal: allow contract-level validation to produce Java’s error messages (and ordering) instead of failing during protobuf→Address conversion.

- [ ] Update `convert_protobuf_transaction()` (`rust-backend/crates/core/src/service/grpc/conversion.rs`):
  - [ ] Add an “allow malformed `to`” path for NON_VM system contracts that validate raw `to` bytes themselves (at minimum `TransferContract`; possibly others).
  - [ ] If `strip_tron_address_prefix(tx.to)` fails, do not return an error immediately:
    - [ ] store `to_raw` anyway
    - [ ] set `transaction.to` to `Some(Address::ZERO)` or `None` (pick the variant that lets executor return `"Invalid toAddress!"` consistently)
- [ ] Add/extend conformance tests or fixtures for:
  - [ ] invalid owner + invalid to (must return `"Invalid ownerAddress!"`)
  - [ ] valid owner + invalid to (must return `"Invalid toAddress!"`)
  - [ ] wrong-prefix to (must return `"Invalid toAddress!"`)

## 3) Clarify/remove the extra flat fee (`fee_amount`) if strict actuator parity is required

Goal: avoid Rust-only fee semantics that don’t exist in `TransferActuator`.

Options:

- [ ] **Option A (strict parity)**: remove `fee_config.non_vm_blackhole_credit_flat` from `TRANSFER_CONTRACT` execution (keep create-account-fee only).
- [ ] **Option B (keep feature, but make it Java-compatible)**:
  - [ ] Define what it represents (bandwidth fee? memo fee? something else).
  - [ ] If the fee is “burned”, call `storage_adapter.burn_trx(fee_amount)` (and ensure dynamic keys are dirtied/returned if required).
  - [ ] If it is “blackholed”, source the blackhole address the same way Java does (dynamic property), and respect `supportBlackHoleOptimization()` instead of a separate config mode.

## 4) Bandwidth/AEXT parity work (only if end-to-end parity is required)

Goal: align Rust’s `bandwidth_used`/AEXT side effects with Java’s `BandwidthProcessor`.

- [ ] Replace `calculate_bandwidth_usage(...)` with a Java-equivalent size computation:
  - [ ] Base it on protobuf serialization size for the TRON transaction (or reproduce Java’s `trx.getInstance().toBuilder().clearRet()...getSerializedSize()` behavior).
- [ ] Implement CREATE_ACCOUNT bandwidth path for `TransferContract` that creates a recipient:
  - [ ] use `CREATE_NEW_ACCOUNT_BANDWIDTH_RATE` ratio (`bytesCost = bytesSize * ratio`)
  - [ ] apply windowed usage updates using `now = headSlot` (not block number)
- [ ] Implement ACCOUNT_NET logic (requires net limit derived from freezes and `TOTAL_NET_WEIGHT/TOTAL_NET_LIMIT`).
- [ ] Implement FREE_NET logic with global public net pool updates (as Java does).
- [ ] Implement FEE path (charge `TRANSACTION_FEE * bytesSize` and update total counters).
- [ ] Ensure `now` matches Java’s `headSlot`:
  - [ ] either compute slot from `(block_timestamp - genesis_timestamp) / BLOCK_PRODUCED_INTERVAL`, or
  - [ ] have Java pass `headSlot` explicitly in `ExecutionContext`.

## 5) Verification

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] Add unit tests specifically for TransferContract validation edge cases (invalid to, wrong prefix, etc.).
- [ ] Java:
  - [ ] `./gradlew :framework:test --tests \"org.tron.core.actuator.TransferActuatorTest\"`
  - [ ] If validating remote parity: run the relevant remote/dual-mode integration tests and compare CSV state digests.

