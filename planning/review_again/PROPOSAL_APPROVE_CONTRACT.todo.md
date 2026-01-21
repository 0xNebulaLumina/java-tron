# TODO / Fix Plan: `PROPOSAL_APPROVE_CONTRACT` parity gaps

This checklist assumes we want to eliminate the edge-case parity gaps identified in `planning/review_again/PROPOSAL_APPROVE_CONTRACT.planning.md`.

## 0) Decide “parity target” (do this first)

- [ ] Confirm desired scope:
  - [ ] **Actuator-only parity** (match `ProposalApproveActuator` + `ProposalCapsule`)
  - [ ] **Forward-compat parity** (also preserve unknown protobuf fields on stored `Proposal`)
- [ ] Confirm whether we care about “corrupted DB” behavior:
  - [ ] treat duplicate approvals as impossible (no fix needed)
  - [ ] treat duplicate approvals as possible (match Java’s first-occurrence removal)

## 1) Duplicate approval removal semantics (optional but strict parity)

Goal: match `ProposalCapsule.removeApproval`, which removes only one matching approval entry.

- [ ] Update `execute_proposal_approve_contract()` in `rust-backend/crates/core/src/service/mod.rs`:
  - [ ] Replace `retain(|a| a != &owner_address_bytes)` with “remove first occurrence only”.
  - [ ] Keep ordering of remaining approvals identical to Java.
- [ ] Add a Rust regression test (unit or conformance-like):
  - [ ] Pre-state proposal has approvals `[A, A, B]`
  - [ ] Execute remove-approval for `A`
  - [ ] Expected post approvals are `[A, B]` (not `[B]`)

## 2) Preserve unknown protobuf fields on `Proposal` (forward-compat parity)

Goal: ensure approving/removing approvals does not delete unknown fields that Java would preserve.

Options (pick one):

- [ ] **Option A (pragmatic)**: accept current behavior; document that unknown fields may be dropped (no code change).
- [ ] **Option B (strict)**: round-trip unknown fields by storing and re-emitting raw bytes.
  - [ ] Change proposal read path to retain original serialized bytes alongside decoded fields.
  - [ ] Implement a “surgical update” to approvals that preserves all other fields/unknown fields:
    - [ ] parse full message as a protobuf stream, rewrite only field `6` (approvals)
    - [ ] keep all other fields byte-for-byte, including unknown fields
  - [ ] Add tests:
    - [ ] Pre-state proposal bytes include an unknown field (e.g., field 99 length-delimited)
    - [ ] After approve/remove, unknown field bytes still present and identical

## 3) Contract parameter presence/type-url parity (optional)

Goal: decide whether the Rust backend should require `metadata.contract_parameter` for this contract (Java-like), or keep the current “fallback to transaction.data” behavior.

- [ ] Decide policy:
  - [ ] Strict: missing `contract_parameter` returns Java-like `"No contract!"` (or equivalent)
  - [ ] Flexible (current): parse from `transaction.data` when Any is absent
- [ ] If strict:
  - [ ] Update the contract parsing entry path for `ProposalApproveContract` to error when Any is missing.
  - [ ] Add tests for both: missing Any and wrong type_url.

## 4) Verification steps

- [ ] Run Rust conformance fixtures:
  - [ ] `cd rust-backend && cargo test -p core test_run_real_fixtures -- --ignored`
  - [ ] Ensure all `proposal_approve_contract/*` cases pass:
    - [ ] happy_path_approve
    - [ ] happy_path_remove_approval
    - [ ] validate_fail_* cases (error messages + no writes)
- [ ] Sanity-check Java reference behavior:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ProposalFixtureGeneratorTest"`

