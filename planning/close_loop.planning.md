# Close Loop Planning

## Objective

Close the execution + storage semantic gap so the Rust backend can become a trustworthy state-transition core for java-tron.

This phase is about:

- correctness
- ownership
- parity
- verification

This phase is not about:

- Rust P2P replacement
- Rust full sync pipeline
- Rust consensus replacement
- replacing the Java node shell in one jump

## Mode Decision

We should simplify the project model and stop treating every mode combination as equally meaningful.

Product / target modes:

- `EE`: embedded execution + embedded storage
- `RR`: remote execution + remote storage

Not a target mode for this planning:

- current in-process `SHADOW`
- mixed combinations such as remote execution + embedded storage

Reason:

- The current `SHADOW` design is not the validation model we actually want.
- The migration comparison we want requires clean, trustworthy isolation between the Java baseline and the Rust target.
- In practice, shared JVM singletons / global state make the current shadow approach a poor primary acceptance mechanism.
- So `SHADOW` should be treated as legacy / optional tooling, not as the core migration strategy.

Validation strategy going forward:

- run `EE` as the baseline
- run `RR` as the target
- compare outputs, state, replay results, and metrics outside the in-process shadow path

## Current State

### Storage

- Storage hot-path operations are already real: CRUD, batch write, batch get.
- Storage semantics are not closed:
  - snapshot is not a true snapshot
  - transaction support is structurally incomplete
  - storage crate still lacks direct test coverage

Judgement:

- storage is "basically usable on hot paths"
- storage is not "basically complete as a semantic replacement"

### Smart Contract / Execution

- The write-path is already substantial:
  - Java maps many contract types to Rust
  - Rust executes many system contracts and VM flows
  - Java can apply Rust-produced side effects back into local state
- But node-level execution is still not closed:
  - Java remote read/query APIs still have placeholders
  - Rust gRPC read/query APIs are only partially implemented
  - `energy_limit` wire semantics are not locked
  - `TriggerSmartContract` TRC-10 pre-transfer is still incomplete

Judgement:

- if the question is "is the write execution path already substantial?", the answer is yes
- if the question is "is remote execution already a closed node-level replacement?", the answer is no

## Why The Next Step Is Not P2P

P2P is not a thin edge module here.

- Networking startup wires many subsystems together.
- Message dispatch fans out to handshake, inventory, block, sync, PBFT, relay, and fetch flows.
- Block sync is tightly coupled to block processing.
- Block processing is tightly coupled to execution, maintenance, rewards, and consensus application.

So doing P2P first would combine:

- the noisiest edge of the system
- the most stateful core of the system

That is the highest-risk path.

## Recommended Roadmap

### Phase 1: Close Execution + Storage Semantics

This is the current phase.

Workstreams:

1. semantic freeze
2. execution read-path closure
3. storage transaction/snapshot closure
4. execution edge-case parity
5. EE-vs-RR verification pipeline

### Phase 2: Rust Block Importer / Block Executor Readiness

After Phase 1, the next milestone should be:

- Rust block importer
- Rust block executor
- Rust-owned state transition boundary

Not:

- Rust P2P first

### Phase 3: Consensus / Maintenance Ownership

Only after block execution ownership is stable should we move more logic such as:

- maintenance cycle logic
- witness scheduling logic
- consensus-related state transition rules

### Phase 4: P2P / Sync Replacement

Only after the Rust side can be trusted as the execution + block-processing core should networking become the next replacement target.

## Phase 1 Workstreams

### A. Semantic Freeze

Decide and document:

- canonical write ownership in `EE` and `RR`
- `energy_limit` wire contract
- storage transaction semantics
- snapshot semantics
- contract support readiness matrix

This must happen first.

Without this step, all later tests are ambiguous.

### B. Execution Read-Path Closure

Close the gap between:

- "remote write execution exists"
- and "remote execution is actually node-usable"

Priorities:

1. Java `callContract`
2. Java `estimateEnergy`
3. Rust `getCode/getStorageAt/getNonce/getBalance`
4. snapshot/revert semantics
5. health semantics

Acceptance:

- no placeholder read/query APIs remain in the active RR path
- `EE` and `RR` can be compared on real query behavior

### C. Storage Semantic Hardening

Close the semantic holes in:

- transaction IDs
- buffered transactional writes
- commit / rollback behavior
- snapshot correctness
- direct storage test coverage

Recommended approach:

- do not try to build a generic distributed database product
- implement the minimum transaction and snapshot semantics required for execution and future block importer work

### D. Execution Edge-Case Parity

Close high-risk semantic gaps such as:

- `TriggerSmartContract` TRC-10 pre-transfer
- resource / fee / sidecar parity
- config flag drift between code defaults and checked-in config

This is not the first task, but it must be finished inside Phase 1.

### E. EE-vs-RR Verification Pipeline

Replace shadow-centric thinking with explicit baseline-vs-target verification.

The comparison path should be:

- golden vectors: run once in `EE`, once in `RR`, compare
- replay: run once in `EE`, once in `RR`, compare
- CI smoke gates: `EE`, `RR`, and `EE-vs-RR diff`

This should become the main migration dashboard.

## Phase 1 Exit Criteria

We should only leave this phase when all of the following are true:

- Java remote execution read/query APIs are no longer placeholders
- Rust remote execution read/query APIs are implemented or explicitly unsupported
- storage transaction semantics are real enough for execution and future block importer needs
- snapshot semantics are real, or unsupported explicitly, but never fake
- `energy_limit` wire semantics are locked
- write ownership is unambiguous in `EE` and `RR`
- a first contract whitelist reaches stable `EE-vs-RR` parity
- storage crate has real tests
- replay and CI continuously report parity status

## Planning Implication

Going forward, planning language should use:

- `EE` for baseline
- `RR` for target
- `EE-vs-RR comparison` for validation

And should avoid relying on:

- `SHADOW` as the primary proof mechanism
- "dual mode" as if mixed mode combinations are still the strategic target
