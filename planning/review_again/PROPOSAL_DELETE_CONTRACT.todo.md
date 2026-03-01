# PROPOSAL_DELETE_CONTRACT (18) — Fix plan / TODO checklist

Goal: if we need strict parity (including error strings and persisted proposal bytes), close the identified gaps.

## A) Confirm the mismatch with targeted reproductions
- [x] Add a java-tron conformance fixture: `ProposalDeleteContract` with `proposal_id = 0` and assert java-side error is `Proposal[0] not exists` (or the exact string produced by `ProposalDeleteActuator.validate()` in that case).
  - Added `generateProposalDelete_proposalIdZero` in `ProposalFixtureGeneratorTest.java` (case: `validate_fail_proposal_id_zero`).
- [x] Add a java-tron conformance fixture where the stored proposal has **multiple parameters with a deliberately non-sorted insertion order** (use `LinkedHashMap` to force order, e.g. keys inserted `[16, 0, 1]`) and then run ProposalDelete:
  - [x] Assert java-tron's persisted proposal bytes keep that map entry order and only add/update field 7 (`state = CANCELED`).
  - Added `generateProposalDelete_multiParamNonSortedOrder` in `ProposalFixtureGeneratorTest.java` (case: `happy_path_delete_multi_param_nonsorted`).
- [x] (Optional) Add an "invalid protobuf bytes" fixture for ProposalDeleteContract truncation to pin error-string behavior if you care about it.
  - Added `generateProposalDelete_invalidProtobufBytes` in `ProposalFixtureGeneratorTest.java` (case: `validate_fail_invalid_protobuf_bytes`).
  - Updated `parse_proposal_delete_contract` to use `read_varint_typed` + `skip_protobuf_field_checked` with Java-compatible error normalization (maps truncation/EOF to `PROTOBUF_TRUNCATED_MESSAGE`, malformed varint to `PROTOBUF_MALFORMED_VARINT`).
  - Added Rust tests: `test_proposal_delete_truncated_bytes_error_string` and `test_proposal_delete_truncated_varint_error_string`.

## B) Fix 1: ProposalDeleteContract parsing should match proto3 defaulting
- [x] Change `parse_proposal_delete_contract` (`rust-backend/crates/core/src/service/mod.rs`) to treat a missing field `proposal_id` as `0` (proto3 default) instead of returning `Missing proposal_id`.
  - Changed `proposal_id.ok_or_else(...)` to `proposal_id.unwrap_or(0)` at line 3387.
- [x] Ensure the resulting validation error matches java-tron for `proposal_id == 0`:
  - [x] It should flow into the existing "proposal exists" checks and end up as `Proposal[0] not exists` (assuming `LATEST_PROPOSAL_NUM >= 0`).
  - Verified: `proposal_id=0` flows through validation and produces `Proposal[0] not exists`.
- [x] Add a Rust-side unit/integration test (or conformance test) that locks the exact error string for this case.
  - Added `test_proposal_delete_missing_proposal_id_defaults_to_zero` and `test_proposal_delete_explicit_proposal_id_zero` in `proposal_delete.rs`.

## C) Fix 2: Don't reorder proposal `parameters` on delete (byte-level parity)
Decision point:
- [x] Decide whether your parity requirement is *semantic* (state transitions only) or *byte-exact persistence* (DB value equality).
  - Decision: **byte-exact persistence** — implemented Option C1.

### Option C1 (recommended for delete): preserve raw proposal bytes and patch only `state` ✅ IMPLEMENTED
- [x] Change storage adapter to provide access to the raw proposal bytes (e.g., `get_proposal_raw(id) -> Vec<u8>`).
  - Already existed: `get_proposal_with_raw()` returns `(Proposal, Vec<u8>)`.
- [x] For ProposalDelete, parse only what's needed from raw bytes (proposer, expiration_time, state, approvals if needed), *without* canonicalizing the map.
  - Changed `execute_proposal_delete_contract` to use `get_proposal_with_raw()` for decoded validation fields + raw bytes for persistence.
- [x] When persisting:
  - [x] If `state` field (7) exists: replace its varint value with `3` while keeping other bytes untouched.
  - [x] If `state` field is absent (common for PENDING=0): append `tag(7)` + `varint(3)` in the same place java-tron would serialize it (after approvals and before unknown fields).
  - Added `surgical_update_proposal_state()` method in `engine.rs` that handles both cases.
- [x] Add tests that compare the post-delete raw bytes against java-tron fixtures for non-sorted parameter order.
  - Added `test_proposal_delete_preserves_parameter_order`, `test_surgical_update_proposal_state_replaces_existing`, and `test_surgical_update_proposal_state_inserts_when_absent`.

### Option C2: preserve parameter insertion order in the decoded representation
- N/A (Option C1 chosen instead — simpler and more robust for delete-only use case).

## D) Validation / regression coverage
- [ ] Run Java tests: `./gradlew :framework:test --tests "org.tron.core.actuator.ProposalDeleteActuatorTest"`
- [ ] Run any conformance generation you rely on (if applicable): `./gradlew :framework:test --tests "org.tron.core.conformance.ProposalFixtureGeneratorTest"`
- [x] Run Rust tests for backend execution: `cd rust-backend && cargo test` (or the narrow crate(s) that cover proposal execution/encoding).
  - All 5 new proposal_delete tests pass. 252/255 workspace tests pass (3 pre-existing vote_witness failures unrelated).
- [x] Run conformance tests: `./scripts/ci/run_fixture_conformance.sh --rust-only`
  - All conformance fixtures pass, including all existing proposal_delete_contract fixtures.
- [ ] Verify both embedded and remote modes if this codepath is gated by config flags.
