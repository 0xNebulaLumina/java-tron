# DELEGATE_RESOURCE_CONTRACT Review Plan

## Current Assessment

The statement in `DELEGATE_RESOURCE_CONTRACT.todo.md` should be treated as **still open until disproven by evidence**:

1. Two decay tests are still ignored.
2. End-to-end conformance for resource delegation is not complete.
3. Remote-vs-embedded parity diff for delegation flows is not complete.

This review plan is written to confirm or falsify each claim with repository evidence, test evidence, and a closure record. A claim is not closed merely because the relevant code exists; it is only closed when there is a reproducible test path and a documented parity outcome.

## Review Goals

1. Identify exactly which two decay tests are ignored, why they are ignored, and what must be true to unignore them safely.
2. Prove whether resource delegation behavior is covered end-to-end across the supported execution paths.
3. Prove whether embedded storage and remote storage produce equivalent externally observable outcomes for delegation flows.
4. Produce a closure artifact that states, per claim, one of:
   - confirmed still open
   - completed and evidenced
   - partially completed with a residual gap

## Scope

### In Scope

- `DelegateResourceContract` and `UnDelegateResourceContract` execution behavior
- Stake 2.0 resource delegation semantics relevant to bandwidth and energy
- Decay or recovery behavior that affects delegated resource accounting
- Java embedded-storage path
- Rust remote-storage path over gRPC
- Cross-mode parity for:
  - transaction result
  - receipts and resource usage
  - account resource state
  - delegated resource indexes / stores
  - balance / frozen / delegated accounting

### Out of Scope

- Unrelated Stake 2.0 contracts unless they are prerequisites for delegation parity
- Performance tuning unless it blocks deterministic parity validation
- Large refactors not required to complete verification

## Evidence Standard

Each open item must be closed with all of the following:

1. A precise file or test reference showing the implemented behavior.
2. A reproducible command or test target that exercises it.
3. An observed outcome recorded in a review note or markdown table.
4. A residual-risk note if the behavior is still only partially covered.

## Workstreams

## 1. Decay Tests Review

### Objective

Determine whether exactly two decay-related tests remain ignored, whether they are still valid, and what conditions are blocking them from being enabled.

### Tasks

1. Search for ignored tests related to delegation, decay, recovery window, or resource usage window logic.
2. Record the exact test names, class names, annotations, and ignore reasons.
3. Trace each ignored test back to the production code path it is meant to protect.
4. Determine whether the ignore is caused by:
   - nondeterministic timing
   - incomplete implementation
   - storage-mode divergence
   - stale test assumptions after feature changes
5. Define the minimum remediation needed to enable each test.

### Required Output

- A table with columns:
  - test class
  - test method
  - ignore annotation / reason
  - feature protected
  - blocker type
  - action to unignore

### Exit Criteria

- The repo contains an explicit list of the two ignored tests.
- There is a documented explanation for why each one is ignored.
- There is a clear enablement path for both tests.

## 2. End-to-End Conformance Review

### Objective

Validate that delegation behavior is covered from contract construction through persisted post-state, not just at unit or actuator level.

### Required Coverage Matrix

The review should verify coverage for each row below:

1. Delegate bandwidth to another account
2. Delegate energy to another account
3. Undelegate bandwidth
4. Undelegate energy
5. Partial undelegation
6. Full undelegation
7. Invalid receiver / self-delegation / missing account cases
8. Insufficient delegated amount / invalid amount boundaries
9. Lock / unlock or unfreeze interaction if applicable in current implementation
10. Decay / recovery interaction after delegation state changes

### For Each Flow, Confirm All Layers

1. Contract validation path
2. Execution path
3. Receipt / result code
4. Account state changes
5. Delegated-resource store changes
6. Index or linkage updates used for lookups
7. Follow-up query observability

### Required Output

- A conformance matrix with columns:
  - flow
  - existing unit coverage
  - existing integration coverage
  - existing dual-mode coverage
  - missing assertions
  - missing scenario

### Exit Criteria

- Every required delegation flow is classified as covered or missing.
- Missing scenarios are specific enough to turn directly into tests.
- The review distinguishes unit coverage from true end-to-end coverage.

## 3. Remote-vs-Embedded Parity Diff Review

### Objective

Determine whether delegation flows produce the same behavior in embedded mode and remote mode.

### Parity Dimensions

For the same input transaction and chain pre-state, compare:

1. Validation result
2. Execution result / exception behavior
3. Receipt status and resource usage fields
4. Account balances
5. Frozen / delegated / acquired delegated resource fields
6. Delegated resource store entries
7. Delegated resource account-index entries
8. Any time-window or decay-related counters touched by delegation flows
9. Query-time observable state after execution

### Method

1. Build a deterministic test fixture shared by both storage modes.
2. Run the same delegation scenarios in embedded mode and remote mode.
3. Serialize comparable post-state snapshots.
4. Diff the snapshots field-by-field.
5. Classify each difference as:
   - expected representation-only difference
   - ordering-only noise
   - real behavioral divergence

### Required Output

- A parity diff report containing:
  - scenario
  - mode A result
  - mode B result
  - diff summary
  - severity
  - disposition

### Exit Criteria

- There is an explicit parity verdict for each delegation scenario.
- Any real divergence is tied to a concrete owner module:
  - Java runtime
  - Java storage adapter
  - Rust backend
  - gRPC mapping

## Execution Order

1. Establish the inventory:
   - locate TODO claims
   - locate ignored tests
   - locate current delegation tests
   - locate dual-mode test harnesses
2. Review decay-test status first.
   - It is the fastest way to validate part of the TODO note.
3. Build the conformance matrix second.
   - This determines whether the problem is missing tests, missing implementation coverage, or both.
4. Run parity review third.
   - Parity work depends on knowing which scenarios must be compared.
5. Write the closure note last.
   - One row per original TODO claim.

## Suggested Artifact Set

The reviewer should leave behind:

1. A short evidence log with file references and command lines used.
2. A conformance matrix markdown file.
3. A parity diff markdown file or checked-in fixture output.
4. An updated TODO status note stating which original claims remain open.

## Ownership Suggestions

### Java Reviewer

- Actuator behavior
- state transition assertions
- existing JUnit / integration coverage

### Storage Reviewer

- dual-mode harness
- snapshot extraction
- embedded vs remote post-state comparison

### Rust Reviewer

- remote backend storage semantics
- gRPC mapping parity
- store/index behavior under delegation flows

## Risks To Watch

1. A unit test may look complete while never asserting persisted delegated-resource indexes.
2. A dual-mode test may exercise delegation indirectly without asserting parity-critical fields.
3. Decay behavior may rely on block time manipulation and hide nondeterminism.
4. Embedded and remote modes may both succeed but still mutate different backing state.
5. Existing ignore annotations may no longer match the true failure mode.

## Closure Rule

Do not mark the original TODO fully closed unless all three statements have a final evidence-backed verdict. If any one of them lacks proof, the correct status is still open or partially open, not done.
