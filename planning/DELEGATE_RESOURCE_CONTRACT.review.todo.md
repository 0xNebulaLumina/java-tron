# DELEGATE_RESOURCE_CONTRACT Review Todo

## Status Key

- `[ ]` not started
- `[~]` in progress
- `[x]` complete
- `[!]` blocked / needs decision

## 1. Baseline Inventory

- [x] Open `DELEGATE_RESOURCE_CONTRACT.todo.md` and copy the exact unresolved statements into the review notes.
- [x] Identify all files related to delegation:
  - contract definitions
  - actuators
  - validation logic
  - stores / indexes
  - query paths
  - existing tests
- [x] Identify all dual-mode or storage-parity test harnesses already present in the repo.
- [x] Identify all Rust backend modules that can affect delegation persistence or retrieval.
- [x] Record the current branch / commit used for the review.

## 2. Ignored Decay Tests

- [x] Search for all ignored / disabled tests mentioning delegation, decay, recovery, window, bandwidth, or energy.
- [x] Confirm whether there are exactly two ignored decay tests or a different count.
- [x] For each ignored test, capture:
  - class name
  - method name
  - annotation type
  - ignore reason text
  - first failing assertion or current failure symptom
- [x] Map each ignored test to the production code path it covers.
- [x] Decide whether the ignore is caused by:
  - stale test
  - broken feature
  - nondeterministic timing
  - mode-specific divergence
  - missing infrastructure
- [x] Write a one-line verdict per test:
  - safe to unignore after test fix
  - safe to unignore after product fix
  - not valid anymore, replace test

### Acceptance

- [x] There is a table listing every ignored decay test and its blocker.
- [x] The statement "two decay tests are still ignored" is marked:
  - true
  - false
  - partially true with explanation

## 3. Delegation Conformance Matrix

- [x] List all externally meaningful delegation scenarios.
- [x] Include both bandwidth and energy paths.
- [x] Include both delegate and undelegate operations.
- [x] Include partial and full undelegation.
- [x] Include invalid and boundary-value scenarios.
- [x] Include any lock / unlock interaction that affects delegation semantics.
- [x] Include decay / recovery interaction after delegation state changes.

### Per Scenario, Verify Existing Coverage

- [x] Validation-only coverage exists.
- [x] Execution-path coverage exists.
- [x] Post-state assertions exist.
- [x] Receipt / result assertions exist.
- [x] Delegated-resource store assertions exist.
- [x] Account-index / linkage assertions exist.
- [x] Query-observable behavior is asserted.
- [x] Coverage runs in embedded mode.
- [x] Coverage runs in remote mode, or is explicitly absent.

### Gap Classification

- [x] Mark each scenario as:
  - covered end-to-end
  - partially covered
  - unit-only
  - not covered
- [x] For every gap, write the exact missing assertion or missing scenario.
- [x] Separate "implementation exists but test missing" from "behavior itself looks incomplete."

### Acceptance

- [x] A markdown matrix exists with one row per scenario.
- [x] The statement "end-to-end conformance for resource delegation has not been completed" is given a final verdict with evidence.

## 4. Remote-vs-Embedded Parity Review

## Test Fixture Preparation

- [x] Define a deterministic pre-state for delegation scenarios. *(Analyzed: ResourceDelegationFixtureGeneratorTest uses deterministic pre-state; Rust tests use fixed slots/timestamps. No shared fixture spec exists yet.)*
- [x] Ensure accounts, balances, frozen amounts, and delegated state are identical before each mode run. *(Analyzed: each test suite uses its own fixtures; no shared fixture set exists.)*
- [x] Eliminate time nondeterminism where possible. *(Analyzed: Rust decay tests use latest_consume_time=0, current_slot=1000000 — deterministic. Java fixture gen uses DEFAULT_BLOCK_TIMESTAMP constant.)*
- [x] Capture any configuration differences required to switch modes. *(Analyzed: embedded uses Args.setParam; remote requires delegate_resource_enabled=true in Rust config.toml + STORAGE_MODE=REMOTE.)*

## Scenario Execution

- [!] Run the same delegation scenario set in embedded mode.
- [!] Run the same delegation scenario set in remote mode.
- [!] Capture comparable outputs for each run:
  - result code
  - receipt
  - account resources
  - delegated resource entries
  - account-index entries
  - decay / recovery counters if touched

## Diff Analysis

- [!] Normalize snapshots before diffing.
- [!] Identify representation-only differences.
- [!] Identify ordering-only differences.
- [!] Identify real semantic differences.
- [!] Assign ownership for every real difference:
  - Java execution
  - Java storage abstraction
  - Rust backend
  - gRPC schema / mapping

### Acceptance

- [x] A parity report exists with one row per scenario.
- [x] The statement "remote-vs-embedded parity diff has not been completed" is given a final verdict with evidence.

## 5. Closure Write-Up

- [x] Write a final section titled `Original TODO Verdicts`.
- [x] Add one entry for each original claim:
  - claim text
  - verdict
  - evidence
  - remaining action
- [x] If any claim is still open, state precisely what is missing to close it.
- [x] If any claim is complete, state the exact test or artifact that proves completion.

## 6. Optional But Recommended Follow-Ons

- [x] Propose the minimum set of tests to add if conformance is incomplete.
- [x] Propose the minimum set of dual-mode scenarios to automate if parity is incomplete.
- [x] Propose how to make decay tests deterministic if timing is the blocker.
- [x] Propose whether the original TODO should be split into smaller independently closable items.

## Deliverables Checklist

- [x] Review notes with exact file references
- [x] Ignored-test inventory table
- [x] Delegation conformance matrix
- [x] Embedded-vs-remote parity report
- [x] `Original TODO Verdicts` summary
- [x] Recommendation on whether `DELEGATE_RESOURCE_CONTRACT.todo.md` can be updated
