# TODO: execution/storage close loop before block importer

Objective: close the execution + storage semantics gap so the Rust backend can be treated as a trustworthy state-transition core, instead of a partial remote accelerator.

This phase is intentionally about correctness, ownership, and verification.

This phase is intentionally not about:

- Rust P2P replacement
- Rust full sync pipeline
- Rust consensus replacement
- Replacing the Java node shell in one jump

Current judgement:

- Storage hot-path DB operations are already real, but transaction/snapshot semantics are not closed.
- Smart-contract write-path support is already substantial, but node-level execution capability is not closed because read/query/snapshot/health paths are still incomplete.
- The next phase after this one should be `block importer / block executor readiness`, not `P2P`.

Known baseline signals before starting:

- `cargo test -p tron-backend-core create_smart_contract -- --nocapture` passes.
- `cargo test -p tron-backend-core update_setting -- --nocapture` passes.
- `cargo test -p tron-backend-storage -- --nocapture` passes trivially but currently has `0 tests`.

Primary references:

- `framework/src/main/java/org/tron/core/storage/spi/StorageSpiFactory.java`
- `framework/src/main/java/org/tron/core/execution/spi/ExecutionSpiFactory.java`
- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java`
- `framework/src/main/proto/backend.proto`
- `rust-backend/crates/storage/src/engine.rs`
- `rust-backend/crates/core/src/service/grpc/mod.rs`
- `rust-backend/crates/core/src/service/mod.rs`
- `rust-backend/crates/execution/src/lib.rs`
- `rust-backend/crates/common/src/config.rs`
- `rust-backend/config.toml`

---

## 0. Phase boundaries and exit criteria

- [ ] Freeze Phase 1 scope as: execution semantics + storage semantics + parity verification only
- [ ] Freeze explicit non-goals for this phase:
  - [ ] No Rust P2P networking rewrite
  - [ ] No Rust sync scheduler / peer manager rewrite
  - [ ] No Rust consensus rewrite
  - [ ] No attempt to remove the Java node shell in this phase
- [ ] Freeze the intended next milestone as `Rust block importer / block executor readiness`
- [ ] Publish a short "why not P2P yet" note inside this file or a sibling planning note so the roadmap does not drift
- [ ] Define Phase 1 exit criteria:
  - [ ] Java execution read/query APIs are no longer placeholders
  - [ ] Rust execution read/query APIs are either implemented or explicitly unsupported
  - [ ] Storage transaction semantics are real enough for execution needs
  - [ ] Storage snapshot semantics are real, or snapshot is explicitly unavailable and not silently fake
  - [ ] `energy_limit` wire semantics are locked
  - [ ] Write ownership is unambiguous
  - [ ] A first contract whitelist reaches stable shadow parity
  - [ ] Storage crate has real tests
  - [ ] Replay + CI can continuously report parity state

---

## 1. Semantic freeze and architectural decisions

Goal: stop the project from moving forward on top of ambiguous semantics.

### 1.1 Canonical write ownership

Primary touchpoints:

- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- `framework/src/main/java/org/tron/core/execution/spi/ShadowExecutionSPI.java`
- `rust-backend/config.toml`
- `rust-backend/crates/common/src/config.rs`

- [ ] Write down the authoritative write-path matrix for:
  - [ ] `EMBEDDED`
  - [ ] `REMOTE`
  - [ ] `SHADOW`
- [ ] Define whether `RuntimeSpiImpl` Java-side apply is canonical, transitional, or legacy-only
- [ ] Define whether `rust_persist_enabled=true` is allowed in:
  - [ ] development only
  - [ ] shadow only
  - [ ] remote canonical mode
  - [ ] never, until later phase
- [ ] Align code defaults, checked-in config, and comments
- [ ] Add a future implementation item to fail fast when an unsafe mode combination is detected:
  - [ ] `SHADOW + rust_persist_enabled=true`
  - [ ] any other double-write or state-polluting combination
- [ ] Document one recommended safe rollout profile and one experimental profile

Acceptance:

- [ ] Any engineer can answer "who writes the final state in this mode?" without ambiguity
- [ ] `config.toml`, `config.rs`, and planning docs no longer contradict each other

### 1.2 Lock `energy_limit` wire semantics

Primary touchpoints:

- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- `framework/src/main/proto/backend.proto`
- `rust-backend/crates/execution/src/lib.rs`
- fixture/conformance generators and readers

- [ ] Audit current Java sender behavior for VM txs:
  - [ ] `CreateSmartContract`
  - [ ] `TriggerSmartContract`
  - [ ] any other path that sets `ExecutionContext.energy_limit`
- [ ] Audit current Rust receiver behavior and conversion logic
- [ ] Audit conformance fixture assumptions for `energy_limit`
- [ ] Choose one canonical wire contract:
  - [ ] send SUN, convert in Rust
  - [ ] send energy units, do not reconvert in Rust
  - [ ] introduce an explicit unit field/flag if neither is safe enough
- [ ] Record migration impact for:
  - [ ] Java bridge
  - [ ] Rust execution
  - [ ] fixtures
  - [ ] shadow comparisons
  - [ ] replay tooling
- [ ] Update comments in `backend.proto`
- [ ] Add a follow-up implementation item to prevent mixed old/new interpretations during transition

Acceptance:

- [ ] No remaining ambiguity on whether Java sends fee-limit SUN or already-computed energy units
- [ ] Java, Rust, and conformance tooling target the same unit contract

### 1.3 Lock storage transaction semantics

Primary touchpoints:

- `framework/src/main/proto/backend.proto`
- `framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java`
- `framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java`
- `rust-backend/crates/storage/src/engine.rs`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

- [ ] Decide the required semantics for `beginTransaction/commit/rollback`
- [ ] Decide whether transaction scope is:
  - [ ] per DB
  - [ ] cross DB
  - [ ] only "execution-local enough", not generic DB transaction
- [ ] Decide whether transaction-scoped reads need read-your-writes visibility for:
  - [ ] `get`
  - [ ] `has`
  - [ ] `batchGet`
  - [ ] iterators / prefix / range reads
- [ ] Decide what execution actually needs versus what can be deferred
- [ ] Explicitly reject turning `StorageSPI` into a generic database product if that is not needed now
- [ ] Write down behavior when `transaction_id` is absent on a write call

Acceptance:

- [ ] There is a clear "minimum transaction semantics required by execution/block importer" statement
- [ ] No one needs to infer semantics from partial code paths

### 1.4 Lock snapshot semantics

Primary touchpoints:

- `framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java`
- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- `rust-backend/crates/storage/src/engine.rs`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

- [ ] Decide whether storage snapshot must be a true RocksDB point-in-time snapshot
- [ ] Decide whether EVM snapshot/revert must be built on top of:
  - [ ] storage snapshot
  - [ ] execution-local journaling
  - [ ] both
- [ ] Decide whether temporary "unsupported" is safer than fake success
- [ ] Write explicit unsupported behavior for any API not implemented in this phase

Acceptance:

- [ ] Fake snapshot success is no longer an accepted state
- [ ] Snapshot-dependent APIs either have real guarantees or fail explicitly

### 1.5 Build a contract support matrix

Primary touchpoints:

- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- `rust-backend/crates/core/src/service/mod.rs`
- `rust-backend/config.toml`

- [ ] Enumerate all contract types currently seen by `RemoteExecutionSPI`
- [ ] Mark each as:
  - [ ] Java only
  - [ ] shadow only
  - [ ] remote canonical candidate
- [ ] For each contract type, record:
  - [ ] depends on read-path closure
  - [ ] depends on TRC-10 semantics
  - [ ] depends on freeze/resource sidecars
  - [ ] depends on dynamic-property strictness
  - [ ] has fixture coverage
  - [ ] has Rust unit coverage
  - [ ] has replay coverage
- [ ] Split the matrix into:
  - [ ] core high-value contracts for Phase 1 acceptance
  - [ ] secondary contracts that can remain shadow-only longer

Acceptance:

- [ ] Remote enablement is no longer driven only by config convenience
- [ ] There is an explicit whitelist target for the end of this phase

---

## 2. Execution read-path closure

Goal: close the biggest gap between "write-path already exists" and "node-level execution capability is still incomplete".

### 2.1 Java execution bridge tasks

Primary touchpoints:

- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- `framework/src/main/java/org/tron/core/execution/spi/ShadowExecutionSPI.java`

- [ ] Replace placeholder `callContract(...)` with a real RPC-backed path
- [ ] Replace placeholder `estimateEnergy(...)` with a real RPC-backed path
- [ ] Replace placeholder `getCode(...)`
- [ ] Replace placeholder `getStorageAt(...)`
- [ ] Replace placeholder `getNonce(...)`
- [ ] Replace placeholder `getBalance(...)`
- [ ] Replace placeholder `createSnapshot(...)`
- [ ] Replace placeholder `revertToSnapshot(...)`
- [ ] Replace placeholder `healthCheck(...)`
- [ ] Normalize timeout handling across all remote execution APIs
- [ ] Normalize error mapping across all remote execution APIs
- [ ] Decide when Java should:
  - [ ] fail hard
  - [ ] fall back to embedded
  - [ ] return explicit remote unsupported
- [ ] Make sure `SHADOW` mode compares real remote results instead of placeholder outputs

### 2.2 Rust execution gRPC tasks

Primary touchpoints:

- `rust-backend/crates/core/src/service/grpc/mod.rs`
- `rust-backend/crates/execution/src/lib.rs`
- storage adapter code used by execution query paths

- [ ] Implement `get_code`
- [ ] Implement `get_storage_at`
- [ ] Implement `get_nonce`
- [ ] Implement `get_balance`
- [ ] Decide whether `create_evm_snapshot` is in scope this phase:
  - [ ] if yes, define storage/journal backing model
  - [ ] if no, return explicit unsupported
- [ ] Decide whether `revert_to_evm_snapshot` is in scope this phase:
  - [ ] if yes, define rollback semantics
  - [ ] if no, return explicit unsupported
- [ ] Make `health` reflect actual module readiness, not a placeholder
- [ ] Add logging/diagnostics that can explain which remote query path failed

### 2.3 Request/response semantic alignment

- [ ] Verify Java request builders carry all fields required by Rust query APIs
- [ ] Verify snapshot identifiers, if kept, have stable cross-side meaning
- [ ] Verify query responses distinguish:
  - [ ] not found
  - [ ] unsupported
  - [ ] internal error
  - [ ] transport error
- [ ] Verify `estimateEnergy` comparison rules in shadow mode:
  - [ ] exact match
  - [ ] tolerated delta
  - [ ] per-contract exception list if needed

### 2.4 Execution read-path tests

Java-focused:

- [ ] Add focused Java tests for each remote execution read/query API
- [ ] Add `ShadowExecutionSPI` tests that compare real `callContract` results
- [ ] Add `ShadowExecutionSPI` tests that compare real `estimateEnergy` results

Rust-focused:

- [ ] Add gRPC service tests for each query API
- [ ] Add execution-level tests for common EOA/contract states
- [ ] Add negative tests for unsupported snapshot/revert if that is the chosen temporary behavior

Acceptance:

- [ ] Node-level remote execution no longer depends on placeholder query APIs
- [ ] `callContract` and `estimateEnergy` are usable in remote/shadow mode
- [ ] Query APIs either work or fail explicitly, never with fake success payloads

---

## 3. Storage semantic hardening

Goal: upgrade storage from "hot-path operations work" to "execution can safely rely on the semantics it claims to expose".

### 3.1 `transaction_id` end-to-end plumbing

Primary touchpoints:

- `framework/src/main/proto/backend.proto`
- `framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

- [ ] Audit all Java write calls that could carry `transaction_id`
- [ ] Define where transaction identifiers are created and owned
- [ ] Pass `transaction_id` through Java `put/delete/batchWrite`
- [ ] Make Rust gRPC handlers branch on `transaction_id` instead of always writing directly
- [ ] Document default behavior for non-transaction-scoped writes
- [ ] Add tracing/logging that makes it obvious whether a write was transactional or direct

### 3.2 Transaction buffer semantics in Rust storage

Primary touchpoints:

- `rust-backend/crates/storage/src/engine.rs`

- [ ] Add real per-transaction operation buffers
- [ ] Route transactional `put` into the buffer
- [ ] Route transactional `delete` into the buffer
- [ ] Route transactional `batch_write` into the buffer
- [ ] Apply buffered operations atomically on `commit`
- [ ] Discard buffered operations on `rollback`
- [ ] Decide read-your-writes behavior for transaction-scoped reads
- [ ] If read-your-writes is required, design layered read behavior over buffered writes
- [ ] Decide whether transaction-scoped iterators/range queries are in scope or explicitly unsupported

### 3.3 Snapshot correctness

Primary touchpoints:

- `rust-backend/crates/storage/src/engine.rs`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

- [ ] Replace current "snapshot reads current DB" behavior with real point-in-time semantics
- [ ] If real snapshot is not implemented this phase, remove fake behavior and surface explicit unsupported
- [ ] Define snapshot lifecycle:
  - [ ] creation
  - [ ] read paths allowed
  - [ ] deletion
  - [ ] cleanup on process shutdown
- [ ] Define interaction rules between transactions and snapshots
- [ ] Decide whether iterator APIs against snapshot are needed now or later

### 3.4 Storage tests and dual-mode checks

Rust-focused:

- [ ] Add unit tests for CRUD
- [ ] Add unit tests for batch writes
- [ ] Add unit tests for transaction commit
- [ ] Add unit tests for transaction rollback
- [ ] Add unit tests for snapshot correctness
- [ ] Add tests for absent `transaction_id`
- [ ] Add tests for transaction not found / snapshot not found
- [ ] Add tests for concurrent transaction IDs and cleanup paths

Java-focused:

- [ ] Extend or add integration coverage around `RemoteStorageSPI`
- [ ] Add tests proving Java actually carries `transaction_id` into remote writes
- [ ] Add dual-mode tests that verify remote semantics, not just factory wiring

Acceptance:

- [ ] Storage transaction APIs are no longer structural placeholders
- [ ] Snapshot is either real or explicitly unavailable
- [ ] Storage crate test suite has meaningful coverage and is no longer `0 tests`

---

## 4. Close state-ownership gaps and bridge debt

Goal: reduce the number of "temporary bridge" pieces that hide split ownership between Java and Rust.

Primary touchpoints:

- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java`
- any code paths that pre-sync Java-side mutations before remote execution

- [ ] Audit every place where Java mutates state and then pushes/synchronizes it to Rust
- [ ] Classify each bridge as:
  - [ ] required in Phase 1
  - [ ] removable once write ownership is frozen
  - [ ] must survive into block importer phase
- [ ] Document whether `ResourceSyncService` is:
  - [ ] a transitional patch
  - [ ] a medium-term integration layer
  - [ ] fundamentally incompatible with final ownership goals
- [ ] Write an explicit "bridge removal sequence" note for after Phase 1
- [ ] Confirm no new bridge mechanism should be added without first checking ownership implications

Acceptance:

- [ ] The project has an explicit list of temporary bridge mechanisms
- [ ] Temporary bridge debt is visible and sequenced, not hidden

---

## 5. Execution parity edge cases and semantic closure

Goal: stop calling execution "basically done" while known semantic holes still exist on important branches.

### 5.1 `TriggerSmartContract` TRC-10 pre-execution transfer

Primary touchpoints:

- `rust-backend/crates/execution/src/lib.rs`
- related trigger/VM tests and conformance fixtures
- `planning/review_again/TRIGGER_SMART_CONTRACT.todo.md`

- [ ] Keep the existing explicit reject path until the replacement semantics are designed
- [ ] Design Java-parity pre-exec token transfer semantics for trigger calls
- [ ] Define rollback behavior on VM failure
- [ ] Define interaction with energy accounting and side effects
- [ ] Add targeted tests for:
  - [ ] happy path token pre-transfer
  - [ ] insufficient balance
  - [ ] missing asset
  - [ ] VM revert after pre-transfer
  - [ ] no token transfer when `tokenValue == 0`

Acceptance:

- [ ] The current known gap is either closed or explicitly kept out of the canonical whitelist

### 5.2 Resource / fee / sidecar parity

Primary touchpoints:

- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- freeze/resource/delegation/withdraw/apply sidecar paths
- relevant Rust execution service code and tests

- [ ] Enumerate all state sidecars emitted by Rust and applied by Java
- [ ] Identify which sidecars are still incomplete or fragile
- [ ] Build a parity checklist for:
  - [ ] freeze ledger changes
  - [ ] global resource total changes
  - [ ] TRC-10 changes
  - [ ] vote changes
  - [ ] withdraw changes
- [ ] Decide which contract families can be declared stable only after these sidecars are verified

Acceptance:

- [ ] Sidecar completeness is part of contract readiness, not treated as an afterthought

### 5.3 Config and feature-flag convergence

Primary touchpoints:

- `rust-backend/config.toml`
- `rust-backend/crates/common/src/config.rs`

- [ ] Audit every `execution.remote.*` flag currently enabled in checked-in config
- [ ] Compare against code defaults
- [ ] Mark each flag as:
  - [ ] experimental
  - [ ] shadow-ready
  - [ ] canonical-ready
  - [ ] legacy / should be removed
- [ ] Produce one recommended conservative config for parity work
- [ ] Produce one experimental config for targeted validation only

Acceptance:

- [ ] The repo no longer looks "stable by config file, experimental by code comment" at the same time

---

## 6. Verification, replay, and release gates

Goal: turn parity from a subjective feeling into an observable gate.

### 6.1 Storage verification

- [ ] Add storage crate tests until `tron-backend-storage` has meaningful direct coverage
- [ ] Add at least one Java integration path that validates remote storage semantics, not only factory creation
- [ ] Track storage regressions separately from execution regressions

### 6.2 Execution read-path vs write-path verification

- [ ] Split verification into two lanes:
  - [ ] write-path / execute tx parity
  - [ ] read-path / query parity
- [ ] Avoid using strong write-path results to imply read-path closure
- [ ] Publish separate pass/fail state for both lanes

### 6.3 Golden vectors

Primary touchpoints:

- `framework/src/test/java/org/tron/core/execution/spi/GoldenVectorTestSuite.java`

- [ ] Make golden vectors execute real remote/shadow paths instead of mostly validating framework structure
- [ ] Add vectors for remote read/query APIs where appropriate
- [ ] Add vectors for known mismatch-prone branches:
  - [ ] trigger smart contract
  - [ ] create smart contract
  - [ ] update setting / metadata
  - [ ] resource/freeze paths

### 6.4 Historical replay

Primary touchpoints:

- `framework/src/test/java/org/tron/core/execution/spi/HistoricalReplayTool.java`

- [ ] Pick a small fixed replay range for routine work
- [ ] Pick a larger replay range for milestone validation
- [ ] Record mismatch categories by contract type
- [ ] Record mismatch categories by read-path vs write-path
- [ ] Record whether mismatch is:
  - [ ] result-code only
  - [ ] energy only
  - [ ] return-data only
  - [ ] state-change / sidecar difference

### 6.5 Contract readiness dashboard

- [ ] Turn the support matrix into a living readiness table
- [ ] For each contract type, record:
  - [ ] remote support status
  - [ ] fixture coverage
  - [ ] Rust unit coverage
  - [ ] replay status
  - [ ] major known gaps
- [ ] Use the readiness table as the only source of truth for enabling canonical remote support

### 6.6 CI smoke gates

- [ ] Define a minimal remote storage smoke set
- [ ] Define a minimal remote execution smoke set
- [ ] Define a minimal shadow mismatch smoke set
- [ ] Make CI output mismatches in a readable, triageable form

Acceptance:

- [ ] The project can answer "what is safe to enable today?" from tests and dashboards, not from memory

---

## 7. Sequencing and parallel work

Goal: keep the critical path clear and avoid starting expensive but premature work.

### 7.1 Critical path

- [ ] Phase 1 critical path is:
  - [ ] semantic freeze
  - [ ] execution read-path closure
  - [ ] storage transaction/snapshot closure
  - [ ] parity verification
  - [ ] block importer readiness planning
- [ ] Explicitly keep `P2P / sync / consensus rewrite` off the critical path

### 7.2 Suggested first batch

- [ ] Start with these items first:
  - [ ] 1.1 Canonical write ownership
  - [ ] 1.2 `energy_limit` wire contract
  - [ ] 1.3 storage transaction semantics
  - [ ] 1.5 contract support matrix
  - [ ] 2.1 Java `callContract/estimateEnergy`
  - [ ] 3.1 `transaction_id` plumbing

### 7.3 Parallelization opportunities

- [ ] Run Java execution bridge work in parallel with Rust storage semantics work
- [ ] Run Rust execution query implementation in parallel with verification harness improvements
- [ ] Keep one owner responsible for semantic freeze so implementation work does not diverge

---

## 8. Explicit non-goals and defer list

These items should remain out of scope until the exit criteria above are met.

- [ ] Do not start Rust P2P handshake work
- [ ] Do not start Rust peer/session manager work
- [ ] Do not start Rust sync scheduler / inventory pipeline work
- [ ] Do not start Rust consensus scheduling rewrite
- [ ] Do not treat "many system contracts already run remotely" as proof that the full execution problem is solved
- [ ] Do not treat "storage CRUD works" as proof that storage semantics are solved

---

## 9. Handoff to next phase

Only after this file's exit criteria are met:

- [ ] Open `BLOCK-01` planning for Rust block importer / block executor
- [ ] Decompose `Manager.processBlock(...)` into Rust-owned responsibilities
- [ ] Re-evaluate whether consensus should follow block importer or stay on Java longer
- [ ] Re-evaluate whether P2P should remain Java-owned until after importer stability

Success condition for this handoff:

- [ ] The next roadmap discussion starts from "Rust state-transition engine ownership", not from "networking looks exciting"
