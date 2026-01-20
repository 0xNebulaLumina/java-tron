# PROPOSAL_DELETE_CONTRACT (18) — Fix plan / TODO checklist

Goal: if we need strict parity (including error strings and persisted proposal bytes), close the identified gaps.

## A) Confirm the mismatch with targeted reproductions
- [ ] Add a java-tron conformance fixture: `ProposalDeleteContract` with `proposal_id = 0` and assert java-side error is `Proposal[0] not exists` (or the exact string produced by `ProposalDeleteActuator.validate()` in that case).
- [ ] Add a java-tron conformance fixture where the stored proposal has **multiple parameters with a deliberately non-sorted insertion order** (use `LinkedHashMap` to force order, e.g. keys inserted `[16, 0, 1]`) and then run ProposalDelete:
  - [ ] Assert java-tron’s persisted proposal bytes keep that map entry order and only add/update field 7 (`state = CANCELED`).
- [ ] (Optional) Add an “invalid protobuf bytes” fixture for ProposalDeleteContract truncation to pin error-string behavior if you care about it.

## B) Fix 1: ProposalDeleteContract parsing should match proto3 defaulting
- [ ] Change `parse_proposal_delete_contract` (`rust-backend/crates/core/src/service/mod.rs`) to treat a missing field `proposal_id` as `0` (proto3 default) instead of returning `Missing proposal_id`.
- [ ] Ensure the resulting validation error matches java-tron for `proposal_id == 0`:
  - [ ] It should flow into the existing “proposal exists” checks and end up as `Proposal[0] not exists` (assuming `LATEST_PROPOSAL_NUM >= 0`).
- [ ] Add a Rust-side unit/integration test (or conformance test) that locks the exact error string for this case.

## C) Fix 2: Don’t reorder proposal `parameters` on delete (byte-level parity)
Decision point:
- [ ] Decide whether your parity requirement is *semantic* (state transitions only) or *byte-exact persistence* (DB value equality).

If byte-exact persistence is required, options:

### Option C1 (recommended for delete): preserve raw proposal bytes and patch only `state`
- [ ] Change storage adapter to provide access to the raw proposal bytes (e.g., `get_proposal_raw(id) -> Vec<u8>`).
- [ ] For ProposalDelete, parse only what’s needed from raw bytes (proposer, expiration_time, state, approvals if needed), *without* canonicalizing the map.
- [ ] When persisting:
  - [ ] If `state` field (7) exists: replace its varint value with `3` while keeping other bytes untouched.
  - [ ] If `state` field is absent (common for PENDING=0): append `tag(7)` + `varint(3)` in the same place java-tron would serialize it (after approvals and before unknown fields, if you preserve java’s field order; simplest is to re-encode in java field order while copying map entries in their original order).
- [ ] Add tests that compare the post-delete raw bytes against java-tron fixtures for non-sorted parameter order.

### Option C2: preserve parameter insertion order in the decoded representation
- [ ] Stop relying on `BTreeMap` for `.protocol.Proposal.parameters` if you need insertion order:
  - [ ] Revisit `rust-backend/crates/execution/build.rs` (currently forces `BTreeMap`).
  - [ ] Introduce a custom representation (e.g. `Vec<(i64,i64)>` or `IndexMap`) for `parameters` in the Rust “proposal model”, while keeping protobuf decode/encode compatibility.
- [ ] Update `encode_proposal_java_compatible` to emit map entries in the *original insertion order* (not sorted-by-key).
- [ ] Add fixtures/tests that lock the exact byte output for multi-parameter proposals.

## D) Validation / regression coverage
- [ ] Run Java tests: `./gradlew :framework:test --tests "org.tron.core.actuator.ProposalDeleteActuatorTest"`
- [ ] Run any conformance generation you rely on (if applicable): `./gradlew :framework:test --tests "org.tron.core.conformance.ProposalFixtureGeneratorTest"`
- [ ] Run Rust tests for backend execution: `cd rust-backend && cargo test` (or the narrow crate(s) that cover proposal execution/encoding).
- [ ] Verify both embedded and remote modes if this codepath is gated by config flags.

