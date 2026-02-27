# TODO / Fix Plan: `PROPOSAL_APPROVE_CONTRACT` parity gaps

This checklist assumes we want to eliminate the edge-case parity gaps identified in `planning/review_again/PROPOSAL_APPROVE_CONTRACT.planning.md`.

## 0) Decide "parity target" (do this first)

- [x] Confirm desired scope:
  - [x] **Actuator-only parity** (match `ProposalApproveActuator` + `ProposalCapsule`)
  - [x] **Forward-compat parity** (also preserve unknown protobuf fields on stored `Proposal`)
- [x] Confirm whether we care about "corrupted DB" behavior:
  - [ ] treat duplicate approvals as impossible (no fix needed)
  - [x] treat duplicate approvals as possible (match Java's first-occurrence removal)

## 1) Duplicate approval removal semantics (strict parity)

Goal: match `ProposalCapsule.removeApproval`, which removes only one matching approval entry.

- [x] Update `execute_proposal_approve_contract()` in `rust-backend/crates/core/src/service/mod.rs`:
  - [x] Replace `retain(|a| a != &owner_address_bytes)` with "remove first occurrence only".
  - [x] Keep ordering of remaining approvals identical to Java.
- [x] Add a Rust regression test (unit or conformance-like):
  - [x] Pre-state proposal has approvals `[A, A, B]`
  - [x] Execute remove-approval for `A`
  - [x] Expected post approvals are `[A, B]` (not `[B]`)

Implementation details:
- Changed from `proposal.approvals.retain(|a| a != &owner_address_bytes)` to
  `if let Some(idx) = proposal.approvals.iter().position(|a| a == &owner_address_bytes) { proposal.approvals.remove(idx); }`
- Added unit test `test_proposal_approve_remove_first_occurrence_only` in
  `rust-backend/crates/core/src/service/tests/contracts/proposal_approve.rs`

## 2) Preserve unknown protobuf fields on `Proposal` (forward-compat parity)

Goal: ensure approving/removing approvals does not delete unknown fields that Java would preserve.

- [x] **Option B (strict)**: round-trip unknown fields by storing and re-emitting raw bytes.
  - [x] Change proposal read path to retain original serialized bytes alongside decoded fields.
    - Added `get_proposal_with_raw()` method to storage adapter
  - [x] Implement a "surgical update" to approvals that preserves all other fields/unknown fields:
    - [x] parse full message as a protobuf stream, rewrite only field `6` (approvals)
    - [x] keep all other fields byte-for-byte, including unknown fields
    - Added `surgical_update_proposal_approvals()` method to storage adapter
    - Added `put_proposal_raw()` method to storage adapter
  - [x] Add tests:
    - [x] Pre-state proposal bytes include an unknown field (e.g., field 99 length-delimited)
    - [x] After approve/remove, unknown field bytes still present and identical
    - Added `test_proposal_approve_preserves_unknown_protobuf_fields` test

## 3) Contract parameter presence/type-url parity (strict)

Goal: Rust backend requires `metadata.contract_parameter` for this contract (Java-like behavior).

- [x] Decide policy:
  - [x] Strict: missing `contract_parameter` returns Java-like `"No contract!"` (ActuatorConstant.CONTRACT_NOT_EXIST)
  - [ ] ~~Flexible (current): parse from `transaction.data` when Any is absent~~
- [x] Implementation:
  - [x] Update `execute_proposal_approve_contract()` to error with "No contract!" when `contract_parameter` is None
  - [x] Add test `test_proposal_approve_missing_contract_parameter_fails` for missing Any

## 4) Verification steps

- [x] Run Rust conformance fixtures:
  - [x] `./scripts/ci/run_fixture_conformance.sh --rust-only`
  - [x] Ensure all `proposal_approve_contract/*` cases pass:
    - [x] happy_path_approve
    - [x] happy_path_remove_approval
    - [x] validate_fail_* cases (error messages + no writes)
- [ ] Sanity-check Java reference behavior:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ProposalFixtureGeneratorTest"`

## Summary of changes

### Files modified:

1. **`rust-backend/crates/execution/src/storage_adapter/engine.rs`**:
   - Added `get_proposal_with_raw()` - returns both decoded Proposal and raw bytes
   - Added `put_proposal_raw()` - stores raw bytes directly
   - Added `surgical_update_proposal_approvals()` - updates only field 6 (approvals) in raw bytes, preserving unknown fields
   - Added `read_varint_from_slice()` helper

2. **`rust-backend/crates/core/src/service/mod.rs`**:
   - Updated `execute_proposal_approve_contract()`:
     - Added strict contract parameter check: returns "No contract!" when missing
     - Changed to use `get_proposal_with_raw()` instead of `get_proposal()`
     - Changed to use `surgical_update_proposal_approvals()` + `put_proposal_raw()` instead of `put_proposal()`
     - Uses "remove first occurrence only" for duplicate approval removal

3. **`rust-backend/crates/core/src/service/tests/contracts/proposal_approve.rs`** (new file):
   - `test_proposal_approve_missing_contract_parameter_fails` - Tests "No contract!" error
   - `test_proposal_approve_remove_first_occurrence_only` - Tests duplicate removal parity
   - `test_proposal_approve_add_approval_happy_path` - Tests basic approval addition
   - `test_proposal_approve_remove_not_approved_fails` - Tests validation error
   - `test_proposal_approve_repeat_approval_fails` - Tests validation error
   - `test_proposal_approve_preserves_unknown_protobuf_fields` - Tests unknown field preservation

### Test results:
- All 6 unit tests pass
- All 12 PROPOSAL_APPROVE_CONTRACT conformance fixtures pass
- Full conformance test suite passes
