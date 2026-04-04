# DELEGATE_RESOURCE_CONTRACT Review Todo

## Status Key

- `[ ]` not started
- `[~]` in progress
- `[x]` complete
- `[!]` blocked / needs decision

## 1. Baseline Inventory

- [ ] Open `DELEGATE_RESOURCE_CONTRACT.todo.md` and copy the exact unresolved statements into the review notes.
- [ ] Identify all files related to delegation:
  - contract definitions
  - actuators
  - validation logic
  - stores / indexes
  - query paths
  - existing tests
- [ ] Identify all dual-mode or storage-parity test harnesses already present in the repo.
- [ ] Identify all Rust backend modules that can affect delegation persistence or retrieval.
- [ ] Record the current branch / commit used for the review.

## 2. Ignored Decay Tests

- [ ] Search for all ignored / disabled tests mentioning delegation, decay, recovery, window, bandwidth, or energy.
- [ ] Confirm whether there are exactly two ignored decay tests or a different count.
- [ ] For each ignored test, capture:
  - class name
  - method name
  - annotation type
  - ignore reason text
  - first failing assertion or current failure symptom
- [ ] Map each ignored test to the production code path it covers.
- [ ] Decide whether the ignore is caused by:
  - stale test
  - broken feature
  - nondeterministic timing
  - mode-specific divergence
  - missing infrastructure
- [ ] Write a one-line verdict per test:
  - safe to unignore after test fix
  - safe to unignore after product fix
  - not valid anymore, replace test

### Acceptance

- [ ] There is a table listing every ignored decay test and its blocker.
- [ ] The statement "two decay tests are still ignored" is marked:
  - true
  - false
  - partially true with explanation

## 3. Delegation Conformance Matrix

- [ ] List all externally meaningful delegation scenarios.
- [ ] Include both bandwidth and energy paths.
- [ ] Include both delegate and undelegate operations.
- [ ] Include partial and full undelegation.
- [ ] Include invalid and boundary-value scenarios.
- [ ] Include any lock / unlock interaction that affects delegation semantics.
- [ ] Include decay / recovery interaction after delegation state changes.

### Per Scenario, Verify Existing Coverage

- [ ] Validation-only coverage exists.
- [ ] Execution-path coverage exists.
- [ ] Post-state assertions exist.
- [ ] Receipt / result assertions exist.
- [ ] Delegated-resource store assertions exist.
- [ ] Account-index / linkage assertions exist.
- [ ] Query-observable behavior is asserted.
- [ ] Coverage runs in embedded mode.
- [ ] Coverage runs in remote mode, or is explicitly absent.

### Gap Classification

- [ ] Mark each scenario as:
  - covered end-to-end
  - partially covered
  - unit-only
  - not covered
- [ ] For every gap, write the exact missing assertion or missing scenario.
- [ ] Separate "implementation exists but test missing" from "behavior itself looks incomplete."

### Acceptance

- [ ] A markdown matrix exists with one row per scenario.
- [ ] The statement "end-to-end conformance for resource delegation has not been completed" is given a final verdict with evidence.

## 4. Remote-vs-Embedded Parity Review

## Test Fixture Preparation

- [ ] Define a deterministic pre-state for delegation scenarios.
- [ ] Ensure accounts, balances, frozen amounts, and delegated state are identical before each mode run.
- [ ] Eliminate time nondeterminism where possible.
- [ ] Capture any configuration differences required to switch modes.

## Scenario Execution

- [ ] Run the same delegation scenario set in embedded mode.
- [ ] Run the same delegation scenario set in remote mode.
- [ ] Capture comparable outputs for each run:
  - result code
  - receipt
  - account resources
  - delegated resource entries
  - account-index entries
  - decay / recovery counters if touched

## Diff Analysis

- [ ] Normalize snapshots before diffing.
- [ ] Identify representation-only differences.
- [ ] Identify ordering-only differences.
- [ ] Identify real semantic differences.
- [ ] Assign ownership for every real difference:
  - Java execution
  - Java storage abstraction
  - Rust backend
  - gRPC schema / mapping

### Acceptance

- [ ] A parity report exists with one row per scenario.
- [ ] The statement "remote-vs-embedded parity diff has not been completed" is given a final verdict with evidence.

## 5. Closure Write-Up

- [ ] Write a final section titled `Original TODO Verdicts`.
- [ ] Add one entry for each original claim:
  - claim text
  - verdict
  - evidence
  - remaining action
- [ ] If any claim is still open, state precisely what is missing to close it.
- [ ] If any claim is complete, state the exact test or artifact that proves completion.

## 6. Optional But Recommended Follow-Ons

- [ ] Propose the minimum set of tests to add if conformance is incomplete.
- [ ] Propose the minimum set of dual-mode scenarios to automate if parity is incomplete.
- [ ] Propose how to make decay tests deterministic if timing is the blocker.
- [ ] Propose whether the original TODO should be split into smaller independently closable items.

## Deliverables Checklist

- [ ] Review notes with exact file references
- [ ] Ignored-test inventory table
- [ ] Delegation conformance matrix
- [ ] Embedded-vs-remote parity report
- [ ] `Original TODO Verdicts` summary
- [ ] Recommendation on whether `DELEGATE_RESOURCE_CONTRACT.todo.md` can be updated
