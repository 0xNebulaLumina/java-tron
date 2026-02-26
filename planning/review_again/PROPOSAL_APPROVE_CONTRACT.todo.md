# TODO / Fix Plan: `PROPOSAL_APPROVE_CONTRACT` parity gaps

This checklist assumes we want to eliminate the edge-case parity gaps identified in `planning/review_again/PROPOSAL_APPROVE_CONTRACT.planning.md`.

## 0) Decide "parity target" (do this first)

- [x] Confirm desired scope:
  - [x] **Actuator-only parity** (match `ProposalApproveActuator` + `ProposalCapsule`)
  - [ ] **Forward-compat parity** (also preserve unknown protobuf fields on stored `Proposal`) - DEFERRED (Option A: pragmatic)
- [x] Confirm whether we care about "corrupted DB" behavior:
  - [ ] treat duplicate approvals as impossible (no fix needed)
  - [x] treat duplicate approvals as possible (match Java's first-occurrence removal)

## 1) Duplicate approval removal semantics (optional but strict parity)

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

Options (pick one):

- [x] **Option A (pragmatic)**: accept current behavior; document that unknown fields may be dropped (no code change).
- [ ] **Option B (strict)**: round-trip unknown fields by storing and re-emitting raw bytes.
  - [ ] Change proposal read path to retain original serialized bytes alongside decoded fields.
  - [ ] Implement a "surgical update" to approvals that preserves all other fields/unknown fields:
    - [ ] parse full message as a protobuf stream, rewrite only field `6` (approvals)
    - [ ] keep all other fields byte-for-byte, including unknown fields
  - [ ] Add tests:
    - [ ] Pre-state proposal bytes include an unknown field (e.g., field 99 length-delimited)
    - [ ] After approve/remove, unknown field bytes still present and identical

**Decision**: Option A selected. Unknown fields are not a concern for current conformance testing,
and the extra complexity of Option B is not justified. This is documented in the planning.md file.

## 3) Contract parameter presence/type-url parity (optional)

Goal: decide whether the Rust backend should require `metadata.contract_parameter` for this contract (Java-like), or keep the current "fallback to transaction.data" behavior.

- [x] Decide policy:
  - [ ] Strict: missing `contract_parameter` returns Java-like `"No contract!"` (or equivalent)
  - [x] Flexible (current): parse from `transaction.data` when Any is absent
- [ ] If strict:
  - [ ] Update the contract parsing entry path for `ProposalApproveContract` to error when Any is missing.
  - [ ] Add tests for both: missing Any and wrong type_url.

**Decision**: Keep flexible behavior. The current implementation provides flexibility for non-Java
callers while still validating the type_url when `contract_parameter` is present. This is acceptable
for conformance testing purposes.

## 4) Verification steps

- [x] Run Rust conformance fixtures:
  - [x] `./scripts/ci/run_fixture_conformance.sh --rust-only`
  - [x] Ensure all `proposal_approve_contract/*` cases pass:
    - [x] happy_path_approve
    - [x] happy_path_remove_approval
    - [x] validate_fail_* cases (error messages + no writes)
- [ ] Sanity-check Java reference behavior:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ProposalFixtureGeneratorTest"`

## Additional tests added

- `test_proposal_approve_add_approval_happy_path` - Tests basic approval addition
- `test_proposal_approve_remove_first_occurrence_only` - Tests Java parity for duplicate removal
- `test_proposal_approve_remove_not_approved_fails` - Tests validation error when removing non-existent approval
- `test_proposal_approve_repeat_approval_fails` - Tests validation error when adding duplicate approval
