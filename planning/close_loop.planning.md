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
- snapshot semantics are real, or **explicitly not-a-real-snapshot and loud about it** (LD-6 + LD-9: EVM snapshot/revert is hard-unsupported; storage `getFromSnapshot` is loud degrade to live-read with `tracing::warn!` on every call). Never silently fake.
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

## Locked Decisions

This section records semantic-freeze decisions made during Phase 1. It is
append-only: once a decision lands here, code/config/tests are expected to
honor it. Anything contradicting a locked decision is a bug, not a variant.

### LD-1: Canonical Write Ownership

Scope: who is allowed to mutate persistent state in each mode.

This decision is split into a **target state** (what `RR` is supposed to be
when Phase 1 closes) and a **current state** (what the implementation
actually does today). Anything in current state that diverges from target
state is tracked as a deferred follow-up and is not allowed to grow.

#### Target state (Phase 1 close)

**Mode `EE` (embedded execution + embedded storage)**

- Canonical writer: Java actuators / `RuntimeSpiImpl` via `chainbase` /
  embedded `StorageSPI`.
- Rust backend: not in the path. No Rust process is required for `EE`.
- Side-channel writes (e.g. shadow mirroring) are **forbidden** in `EE`.

**Mode `RR` (remote execution + remote storage)**

- Canonical writer: **Java `RuntimeSpiImpl` apply path**, fed by gRPC results
  from the Rust execution service. Specifically:
  - `applyStateChangesToLocalDatabase`
  - `applyFreezeLedgerChanges` — also applies the
    `GlobalResourceTotalsChange` sidecar emitted alongside freeze/unfreeze
  - `applyTrc10Changes`
  - `applyVoteChanges`
  - `applyWithdrawChanges`
- Rust execution service: **computes** state deltas and returns them. It does
  not directly persist into its own RocksDB on the production write path.
- Direct Rust persistence is reserved for **conformance / isolation tests
  only** (see LD-2). It must never be combined with the Java apply path on
  the same data, because that double-writes and breaks idempotency for
  delta-style changes such as TRC-10 transfers.
- The `WriteMode::PERSISTED` short-circuit in `RuntimeSpiImpl.execute(...)`
  must only be reachable from conformance/isolation lanes, never in `RR`.

**Status of `RuntimeSpiImpl`** (target)

- Classification: **canonical for `RR`** (not transitional, not legacy).
- It owns the apply step that turns Rust-computed deltas into local state.
- Until the Rust block importer phase, this apply step stays in Java.

**Status of current `SHADOW`**

- Classification: **legacy / optional tooling**, not a Phase 1 acceptance
  mode. See "Mode Decision" above.
- No Phase 1 task is allowed to depend on SHADOW being correct.

#### Current state (gap tracker)

The implementation has not yet caught up to the target state above. The
known gaps are:

- **NonVm contracts bypass `rust_persist_enabled`.** In
  `rust-backend/crates/core/src/service/grpc/mod.rs` (around the
  `execute_transaction` handler, see `use_buffered_writes` and the post-
  execution commit), every NonVm execution forces a buffered write that
  commits on success and reports `WriteMode::PERSISTED` regardless of the
  config flag. `RuntimeSpiImpl.execute(...)` then sets `skipApply = true`,
  so for NonVm contracts the Rust process — not the Java apply path — is
  the actual canonical writer today.
- **VM contracts largely follow target state, with one exception.** They
  respect `rust_persist_enabled` (defaulted to `false` in code), so the
  bulk of VM writes flow through the Java apply path. **Exception:** the
  `CreateSmartContract` metadata persist path in
  `rust-backend/crates/core/src/service/grpc/mod.rs` (around line 1442,
  which invokes `persist_smart_contract_metadata` in
  `rust-backend/crates/core/src/service/mod.rs`) calls the storage
  adapter's `buffered_put` with no write buffer attached in the default
  VM configuration, which falls through to a direct storage-engine
  write at `rust-backend/crates/execution/src/storage_adapter/engine.rs`
  around line 138. This is a Rust-owned VM-side write that lives
  **outside** the `WriteMode::PERSISTED` signal entirely — it is not
  gated by `rust_persist_enabled` and does not appear as a
  `state_change`. It is tracked as a distinct LD-1 gap in the § LD-11
  bridge-debt inventory, not here, because it is orthogonal to the
  `rust_persist_enabled` / `WriteMode` lock.

Implication for Phase 1:

- LD-1 and LD-2 describe the **direction of travel**, not what `RR`
  observably does today.
- The gap is closed by the deferred follow-ups in
  `close_loop.todo.md § 1.1 deferred follow-ups`. Until those land, no
  Phase 1 acceptance test should rely on "all RR writes go through Java
  apply" without qualifying VM vs NonVm.

### LD-2: `rust_persist_enabled` Allowed Usage (target state)

| Mode / context                              | Allowed? | Notes |
|---------------------------------------------|----------|-------|
| `EE` production                             | N/A      | Rust execution not in path |
| `RR` production                             | **No**   | Java apply is canonical; Rust must only compute deltas |
| Rust conformance / isolation runner         | Yes      | Required because there is no Java apply lane |
| Standalone Rust dev / experiments           | Yes      | But never against shared/production data |

This table reflects the target state. Today `rust_persist_enabled` only
gates the **VM** execution path; NonVm contracts always commit Rust-side
regardless of this flag (see LD-1 "Current state" gap tracker). Closing
that gap is a deferred follow-up.

Action items captured from LD-2:

- Code default in `rust-backend/crates/common/src/config.rs` is already
  `false` for `rust_persist_enabled`. Keep it that way.
- Checked-in `rust-backend/config.toml` is now aligned with the code
  default and the LD-2 lock: `rust_persist_enabled = false`. The
  historical override to `true` has been flipped as part of §1.1
  deferred follow-up #1. The conformance runner still forces `true`
  for itself at `rust-backend/crates/core/src/conformance/runner.rs`
  because Profile B requires it; that is the only remaining enabled
  path and it is internal to the runner, not driven by `config.toml`.
- **Runtime guard (VM half — landed).** `rust-backend/src/main.rs`
  now refuses to start the production binary when the loaded
  `ExecutionConfig` has `rust_persist_enabled = true`, emitting a loud
  `error!` that cites LD-1 / LD-2 and points the developer at the
  conformance runner as the supported force-`true` path. The check runs
  immediately after `Config::load()` and returns an `Err` from `main`,
  so the process exits non-zero before any gRPC handler runs. The
  conformance runner is unaffected because it builds its own
  `ExecutionConfig` in code and never calls `Config::load()`. This
  closes §1.1 deferred follow-up #3 ("Add a runtime guard in the Rust
  execution service that fails fast when `rust_persist_enabled = true`
  is observed alongside an active Java apply lane"). The "active Java
  apply lane" signal is deliberately proxied by "main.rs is the in-tree
  binary entrypoint for RR production", since Rust has no direct signal
  for whether a Java node is attached — see §1.1 follow-up #3 evidence
  for the full reasoning.
- **NonVm bypass (still open).** The above guard covers only the VM
  half of LD-2. NonVm contracts still use buffered writes
  unconditionally in `rust-backend/crates/core/src/service/grpc/mod.rs`
  regardless of `rust_persist_enabled`, so they still emit
  `WriteMode::PERSISTED` to Java in RR production. Bringing the NonVm
  bypass under the same LD-1 / LD-2 ownership lock is tracked as §1.1
  deferred follow-up #5 ("Reconcile the NonVm execution path with
  LD-1/LD-2") and requires a separate design decision (either drop the
  unconditional NonVm buffering, or formally extend LD-1/LD-2 to declare
  NonVm a Rust-canonical lane with an explicit idempotent re-apply
  story). The guard is intentionally scoped narrowly so it can land
  without waiting on that decision.
- **Java-side `WriteMode::PERSISTED` audit (landed).** §1.1 deferred
  follow-up #4 ("Audit Java-side `WriteMode::PERSISTED` usage") has
  been completed. The audit is scoped strictly to whether the Java
  `skipApply` short-circuit — the only behavioral consumer of
  `ExecutionSPI.WriteMode.PERSISTED` — can be reached in RR
  production. It is **not** a claim that all Rust-side writes in the
  VM path are Java-owned; there is a separate class of direct Rust
  writes that live outside the `WriteMode` signal entirely (see the
  third bullet below). In-tree consumers enumerated:
  - `RuntimeSpiImpl.execute(...)` — the `skipApply` short-circuit
    (only behavioral consumer).
  - `RemoteExecutionSPI.convertExecuteTransactionResponse(...)` — a
    logging branch that just notes the mode, no state effect.
  - `ShadowExecutionSPI` — out of scope per LD-6.

  In `RuntimeSpiImpl.execute(...)`, the `skipApply` branch is now
  documented inline with a `// LD-2 audit:` block that records:
  - VM path (buffered-write lane): safe post-flip + startup guard —
    Rust never emits `WriteMode::PERSISTED` for VM transactions in
    RR production, so the short-circuit is not reachable from VM in
    RR. Java's normal `applyStateChanges*` path always runs for VM.
  - NonVm path: outstanding gap — every successful NonVm
    transaction in RR still emits `WriteMode::PERSISTED` and
    triggers the short-circuit. Cross-linked to §1.1 deferred
    follow-up #5 so the code comment does not drift from the tracker.
  - Orthogonal VM-side Rust-owned writes (outside the `WriteMode`
    signal): the `CreateSmartContract` handler in
    `rust-backend/crates/core/src/service/grpc/mod.rs` calls
    `persist_smart_contract_metadata` on an adapter that writes
    straight through to the storage engine when `write_buffer` is
    absent (i.e. VM path with `rust_persist_enabled = false`). These
    writes never touch `WriteMode::PERSISTED`, so the short-circuit
    reachability argument above is unaffected, but they are still
    Rust-owned writes in RR production and therefore a distinct LD-1
    bridge-debt item. They are tracked under the § LD-11 bridge-debt
    inventory, not under this audit. The audit comment at
    `RuntimeSpiImpl.java` explicitly calls this out so readers do
    not misread the audit as "LD-1 fully enforced for VM".

  The audit result is recorded in code, not in a separate audit
  document, so future readers hit the gap callout at the same place
  they read the short-circuit.

### LD-3: Recommended Configuration Profiles

Two profiles are recognized by Phase 1. Anything else is unsupported.

**Profile A — Safe `RR` parity profile (recommended default for Phase 1 work)**

This profile is a set of recommended **explicit overrides** in
`rust-backend/config.toml`. It is **not** the same as the built-in
`RemoteExecutionConfig` defaults in `rust-backend/crates/common/src/config.rs`,
which are deliberately conservative (e.g. `accountinfo_aext_mode = "none"`,
`emit_freeze_ledger_changes = false`, `emit_global_resource_changes = false`,
`strict_dynamic_properties = false`). Profile A intentionally turns several
of these on for parity work; running an `RR` parity comparison without
them is unsupported.

Java side:

- `--execution-spi-enabled --execution-mode "REMOTE"`
- Storage SPI uses the remote (gRPC) backend.
- All Rust-produced sidecars are applied via `RuntimeSpiImpl`.

Rust side (`rust-backend/config.toml` overrides):

- `execution.remote.system_enabled = true` (matches code default)
- `execution.remote.rust_persist_enabled = false` (matches code default; **must** stay off in `RR`)
- `execution.remote.accountinfo_aext_mode = "hybrid"` (overrides default `"none"`)
- `execution.remote.emit_freeze_ledger_changes = true` (overrides default `false`)
- `execution.remote.emit_global_resource_changes = true` (overrides default `false`)
- `execution.remote.strict_dynamic_properties = true` (overrides default `false`)

> **Historical caveat (resolved):** the checked-in
> `rust-backend/config.toml` previously set
> `execution.remote.rust_persist_enabled = true`, which made the file
> an unsupported hybrid between Profile A and Profile B. That gap has
> been closed by §1.1 deferred follow-up #1: `config.toml` now matches
> the code default (`false`) and the LD-2 lock. The only remaining
> Profile-A deviation in `config.toml` is `market_strict_index_parity
> = true`, which is tracked under LD-10 cleanup #1 as a scoped
> deviation rather than a contradiction.

This is the profile we expect `EE-vs-RR` parity comparisons to use.

**Profile B — Experimental / conformance profile**

Used only by:

- `tron-backend-core` conformance runner
- targeted Rust isolation tests
- ad-hoc experiments that cannot share storage with a Java node

Rust side:

- `execution.remote.rust_persist_enabled = true`
- The Java apply path must be disabled, or there must be no Java node sharing
  this RocksDB at all.

Profile B is **not** the production `RR` mode. It exists so Rust tests can
exercise persistence end-to-end without a Java front-end.

### LD-4: `energy_limit` Wire Semantics

Scope: how the `energy_limit` field on `Transaction` and `ExecutionContext`
in `framework/src/main/proto/backend.proto` is interpreted across Java,
Rust, and conformance fixtures.

#### Audit (current state, before lock)

**Java sender (`framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`)**

- `buildExecuteTransactionRequest(...)` initializes a single local at line ~393:
  `long energyLimit = transaction.getRawData().getFeeLimit();` — i.e. raw
  **SUN**. This is the value that **non-VM** contract paths (TransferContract,
  freeze/unfreeze, vote, witness, etc.) keep until the request builder runs,
  because their switch arms never re-assign `energyLimit`. So the wire today
  carries SUN for non-VM contracts.
- `CreateSmartContract` (VM): re-assigns
  `energyLimit = computeEnergyLimitWithFixRatio(context, fromAddress, feeLimit, callValue)`.
  This method matches `VMActuator.getAccountEnergyLimitWithFixRatio()` and
  returns **energy units** =
  `min(leftFrozenEnergy + max(balance - callValue, 0)/sunPerEnergy, feeLimit/sunPerEnergy)`.
- `TriggerSmartContract` (VM): same `computeEnergyLimitWithFixRatio` call,
  also returning **energy units**. Note that `RemoteExecutionSPI.java#L555-L561`
  itself flags this as caller-side capping only — it does not include the
  contract-side `getTotalEnergyLimitWithFixRatio` clamp from
  `VMActuator.java#L697-L763`, which considers
  `origin_energy_limit` and `consume_user_resource_percent`. Closing that
  gap is its own item, not LD-4 scope.
- Whatever the local `energyLimit` ends up being is then written to BOTH:
  - `Transaction.energy_limit` (proto field 5) — the per-transaction cap
  - `ExecutionContext.energy_limit` (proto field 5 of `ExecutionContext`) —
    populated as the same value, treated by Rust as the block-level envelope
- **Hidden defect (VM path only):** `computeEnergyLimitWithFixRatio` falls
  back to returning the raw `feeLimit` (i.e. **SUN**, not energy units) on
  multiple error branches: missing `StoreFactory`, missing `ChainBaseManager`,
  missing `AccountStore`/`DynamicPropertiesStore`, missing owner account,
  or any caught exception. So even on the VM path the wire value silently
  changes units in degenerate cases. This is a Java-side defensive-recovery
  bug that LD-4 closes by failing loudly.
- **Non-VM caveat:** because non-VM contracts have no concept of an EVM
  energy cap, the right LD-4 wire value for them is `0` (or any agreed
  sentinel that means "this field is not meaningful"). Sending raw SUN is
  the legacy default and is incompatible with the locked unit. See LD-4
  scope below.

**Rust receiver (`rust-backend/crates/execution/src/lib.rs`)**

- VM execution path reads `tx.gas_limit` (which gRPC populates from `Transaction.energy_limit`) and divides it by `energy_fee_rate` from storage:
  `adjusted_tx.gas_limit = adjusted_tx.gas_limit / energy_fee_rate;`
- The same division is performed in `execute_transaction_with_storage`,
  `call_contract_with_storage`, and `estimate_energy_with_storage`. All three paths assume
  the wire value is **SUN** and convert to **energy units** themselves.
- An explicit `// KNOWN MISMATCH — needs spec lock` block previously
  documented this as inconsistent with the production Java sender; this
  iteration replaces that block with a short LD-4 cross-reference, but
  the divide itself is intentionally left in place until the deferred
  cutover lands.

**Conformance fixtures (`rust-backend/crates/core/src/conformance/runner.rs`)**

- The runner reads `tx.energy_limit` directly into `gas_limit` with no
  conversion, then hands the transaction to the same `lib.rs` execution
  path which divides by `energy_fee_rate`. Therefore fixtures **must**
  encode `energy_limit` as **SUN** (raw fee-limit-style) for the existing
  Rust path to produce the right energy-unit cap.
- This means production and fixtures currently disagree on the unit of
  the same proto field, and the Rust receiver is right for fixtures and
  wrong for production. That is the source of the parity drift.

#### Locked decision

**Scope of LD-4:** VM transactions only — i.e. `TxKind::Vm` /
`CreateSmartContract` / `TriggerSmartContract`. Non-VM contracts do not
have an EVM energy cap and must send `Transaction.energy_limit = 0` on
the wire (Rust ignores the field on the non-VM dispatch path). LD-4
**does not** require Rust to interpret a non-zero non-VM value.

**Wire contract (VM):** `Transaction.energy_limit` and
`ExecutionContext.energy_limit` carry **energy units** (already converted),
not SUN. Rust **must not** divide by `energy_fee_rate` again. Conformance
fixtures **must** encode the field as energy units.

Rationale for choosing this option over "send SUN, convert in Rust":

- Java's caller-side `computeEnergyLimitWithFixRatio` already implements
  the caller-side leg of the Java parity formula (frozen energy from
  `EnergyProcessor.getAccountLeftEnergyFromFreeze`, balance-derived energy
  via `(balance - callValue) / sunPerEnergy`, fee-limit-derived energy via
  `feeLimit / sunPerEnergy`, then `min`). Full trigger parity additionally
  requires the contract-side `getTotalEnergyLimitWithFixRatio` clamp from
  `actuator/src/main/java/org/tron/core/actuator/VMActuator.java#L697-L763`,
  which considers `origin_energy_limit` and `consume_user_resource_percent`.
  Reproducing the caller-side formula in Rust would already require porting
  `EnergyProcessor` and the `sunPerEnergy`/`getEnergyFee` lookup; reproducing
  the contract-side clamp on top of that doubles the surface and adds a
  high parity-bug risk. Java already has these implementations.
- The current Java code already produces energy units on the happy path.
  The real defect is the Rust double-conversion plus the Java fallback,
  not the unit choice.
- Fixtures are testing infrastructure under our control; updating the
  generator is cheap.
- "Add an explicit unit field" is rejected because the proto would carry
  permanent transition baggage for a fix-and-move-on cleanup.

#### Migration impact

| Surface                        | Change required |
|--------------------------------|-----------------|
| Java `RemoteExecutionSPI` (VM)        | Harden `computeEnergyLimitWithFixRatio` so error branches **fail loudly** (return `0` or throw a strict error) instead of falling back to raw `feeLimit` SUN. Also stop reusing the per-transaction value as `ExecutionContext.energy_limit` — the block envelope should be a separate, conservative number. |
| Java `RemoteExecutionSPI` (non-VM)    | Initialize `energyLimit = 0` for non-VM contract paths instead of seeding from `transaction.getRawData().getFeeLimit()`. Non-VM contracts have no EVM energy cap and `Transaction.energy_limit` is meaningless for them under LD-4. |
| Rust execution (`lib.rs`)             | Remove the `adjusted_tx.gas_limit / energy_fee_rate` division in `execute_transaction_with_storage`, `call_contract_with_storage`, and `estimate_energy_with_storage`. Trust the wire as energy units. Delete the comment block once removed. |
| Rust gRPC handler                     | Audit `convert_protobuf_transaction` and the conformance runner's `gas_limit` mapping to confirm no other code path silently converts SUN→energy. |
| Conformance fixtures                  | Update fixture generators (Java + Rust) to encode `energy_limit` as energy units. Audit and update SUN-assuming literals outside the generator path: `rust-backend/crates/core/src/conformance/runner.rs` (transaction & context constructors), `rust-backend/crates/core/src/service/tests/contracts/create_smart_contract.rs`, and the stale comment in `rust-backend/crates/execution/src/tron_evm.rs`. Add a one-time migration note in fixture changelogs so old fixtures cannot be replayed against the new Rust path without explicit regeneration. |
| `EE-vs-RR` comparison tooling         | If the comparator currently asserts on raw `energy_used` per branch, no change. If it asserts on the wire `energy_limit` field directly, regenerate baselines. |
| Replay tooling                        | The in-tree `framework/src/test/java/org/tron/core/execution/spi/HistoricalReplayTool.java` is test-only and `loadBlock()` is still TODO, so "no replay tool change" is a *target* statement, not verified today. Once the replay tool is finished, it should source `energy_limit` from the same locked Java path; for now this row is just a reminder, not a free pass. |
| `backend.proto` comment               | Already done in this iteration: the "KNOWN MISMATCH" warning on `Transaction.energy_limit` (field 5) has been replaced with the LD-4 contract (VM-only energy units, non-VM = `0`/sentinel). Remove the LD-4 cross-reference once the deferred Rust + Java + fixtures cutover lands and the parity dashboard stays green. |

#### Transition safety

- The cutover **cannot be staged** field-by-field: as soon as Rust stops
  dividing, fixtures must already be in energy units, and Java must
  already be guaranteed to send energy units (no SUN fallback).
- Therefore the deferred follow-ups for LD-4 must land as a single
  coherent change. A future runtime check should reject any wire value
  whose magnitude is "obviously SUN" until we are confident no
  stragglers remain. The threshold must be derived from runtime
  constants, not hardcoded: `MAX_FEE_LIMIT` is `10_000_000_000` SUN
  ([ProposalUtil.java#L395-L405](../actuator/src/main/java/org/tron/core/utils/ProposalUtil.java#L395-L405)),
  and existing Java parity tests use energy limits well above `10^7`
  units (e.g. [RuntimeImplTest.java#L157-L162](../framework/src/test/java/org/tron/common/runtime/RuntimeImplTest.java#L157-L162),
  [RuntimeImplTest.java#L247-L253](../framework/src/test/java/org/tron/common/runtime/RuntimeImplTest.java#L247-L253)),
  so a naive `> 10_000_000` cutoff would reject valid energy-unit
  values. The recommended formulation is "reject if
  `wire_value > MAX_FEE_LIMIT`" (which is `10^10` — orders of magnitude
  larger than any legitimate energy-unit count even at the lowest
  configured `sun_per_energy`), or, more precisely, "reject if
  `wire_value > MAX_FEE_LIMIT / max(1, current_sun_per_energy)`" once
  the receiver has access to the current chain parameter. This guard
  belongs in both Java pre-send and Rust on-receive.

### LD-5: Storage Transaction Semantics

#### Audit (current state, evidence-based)

This audit underwrites the locked decision below. Every claim is
file-grounded and reflects the code as it exists today, not aspirations.

**Java SPI surface.** [`StorageSPI.java#L53-L65`](../framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java#L53-L65)
declares six transaction/snapshot methods: `beginTransaction(dbName)`,
`commitTransaction(transactionId)`, `rollbackTransaction(transactionId)`,
`createSnapshot(dbName)`, `deleteSnapshot(snapshotId)`,
`getFromSnapshot(snapshotId, key)`. [`RemoteStorageSPI.java#L609-L728`](../framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java#L609-L728)
implements all six and issues real gRPC calls. [`EmbeddedStorageSPI.java#L287-L323`](../framework/src/main/java/org/tron/core/storage/spi/EmbeddedStorageSPI.java#L287-L323)
stubs all six (`beginTransaction` returns `"embedded-tx-" + currentTimeMillis`,
`commit`/`rollback` return immediately, `getFromSnapshot` reads the live
DB and ignores the snapshot ID).

**Production callers.** No Phase 1 production code path calls
`beginTransaction` / `commitTransaction` / `rollbackTransaction` on
`StorageSPI`. The only callers are in `framework/src/test/java/.../StorageSPIIntegrationTest.java`
(integration test). [`RuntimeSpiImpl.java`](../framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java)
uses `StorageSPI` only for read-side mirroring (`getMirrorRemoteStorageSPI()`
calls `batchGet` and `get` in `postExecMirror`); it never calls
`beginTransaction` / `commitTransaction` / `rollbackTransaction` and
never writes through `StorageSPI` at all. Sidecar application
(`applyStateChangesToLocalDatabase`, `applyFreezeLedgerChanges`, etc.)
writes to local Java stores via `ChainBaseManager` /
`TronStoreWithRevoking` — i.e. the **Java revoking layer is still the
real "transaction" boundary** for state mutation in `RR`, not the
storage SPI.

**Java SPI lacks transaction-aware write overloads.** The Java
`StorageSPI` write methods at
[`StorageSPI.java#L14-L25`](../framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java#L14-L25)
(`get`, `put`, `delete`, `has`, `batchWrite`, `batchGet`) take only
`(String dbName, ...)` — there is **no overload** that accepts a
`String transactionId` parameter alongside the write. So even if a
hypothetical Phase 1 caller wanted to issue a transaction-scoped
write, the Java SPI surface itself would not let them: they would
have to use the bare `put(dbName, key, value)` overload that issues a
`PutRequest` with no `transaction_id` set. This is an additional
structural reason "Phase 1 required semantics: none" is the only
honest lock — there is no in-tree path for a write to even reach the
gRPC handler with a `transaction_id` populated. Adding such overloads
is itself §3.1 deferred work (`transaction_id` end-to-end plumbing).

**Wire asymmetry.** From `framework/src/main/proto/backend.proto`:

| Message                     | `transaction_id` | `snapshot_id` |
|-----------------------------|------------------|---------------|
| `GetRequest`                | absent           | optional (field 3) |
| `HasRequest`                | absent           | optional (field 3) |
| `BatchGetRequest`           | absent           | optional (field 3) |
| `IteratorRequest`           | absent           | optional (field 4) |
| `PrefixQueryRequest`        | absent           | optional (field 3) |
| `PutRequest`                | optional (field 4) | absent      |
| `DeleteRequest`             | optional (field 3) | absent      |
| `BatchWriteRequest`         | optional (field 3) | absent      |
| `BeginTransactionRequest`   | absent (`database` only) | absent |
| `CommitTransactionRequest`  | required (field 1) | absent     |
| `RollbackTransactionRequest`| required (field 1) | absent     |

In other words, the wire format is **structurally incapable of
read-your-writes**: writes can carry a `transaction_id`, reads can carry
a `snapshot_id`, but neither can carry the other. A client cannot
currently issue a `Get` against an in-progress transaction even in
principle.

**Rust engine.** [`engine.rs#L387-L398`](../rust-backend/crates/storage/src/engine.rs#L387-L398)
defines `begin_transaction(db_name)` as: allocate a
`TransactionInfo { db_name, operations: Vec::new() }`, insert into
`self.transactions: HashMap<String, TransactionInfo>` keyed by a fresh
UUID, return the UUID. **No RocksDB transaction handle is opened.**
[`commit_transaction`](../rust-backend/crates/storage/src/engine.rs#L401-L425)
removes the entry and writes a `WriteBatch` from the buffered ops.
However the buffer is initialized empty and there is **no engine method
that ever appends to it** — the engine has no `put_in_transaction`
counterpart. The Vec is dead state. `rollback_transaction`
([engine.rs#L427-L433](../rust-backend/crates/storage/src/engine.rs#L427-L433))
just removes the entry.

**Rust gRPC handlers.** Every storage write handler in
`rust-backend/crates/core/src/service/grpc/mod.rs` ignores the
`transaction_id` field and writes directly to RocksDB:

- `put` ([mod.rs#L192-L215](../rust-backend/crates/core/src/service/grpc/mod.rs#L192-L215)) → `engine.put` → `db.put`
- `delete` ([mod.rs#L217-L243](../rust-backend/crates/core/src/service/grpc/mod.rs#L217-L243)) → `engine.delete` → `db.delete`
- `batch_write` ([mod.rs#L272-L398](../rust-backend/crates/core/src/service/grpc/mod.rs#L272-L398)) → `engine.batch_write` → `db.write`

Every read handler ignores `snapshot_id` and reads the live DB:

- `get` / `has` / `batch_get` / `iterator` / `prefix_query` — none of them
  read `req.snapshot_id` or `req.transaction_id`.

**Snapshot.** [`engine.create_snapshot`](../rust-backend/crates/storage/src/engine.rs#L436-L448)
allocates a `SnapshotInfo { db_name }` and inserts it into a HashMap
under a UUID. **No RocksDB snapshot handle is captured.**
[`get_from_snapshot`](../rust-backend/crates/storage/src/engine.rs#L460-L472)
looks up the `db_name` and calls `self.get(...)` — i.e. it reads the
live DB at the current state. The struct doc string at
[`engine.rs#L31-L33`](../rust-backend/crates/storage/src/engine.rs#L31-L33)
admits this with: *"In a real implementation, you'd need to handle
snapshot lifetimes more carefully."* (Snapshot is treated as `LD-6` /
§1.4 below; LD-5 only locks the transaction surface.)

**Cross-DB.** `BeginTransactionRequest` and the engine `TransactionInfo`
both carry exactly one `db_name`. There is no mechanism to associate a
single `transaction_id` with multiple named databases (`account`,
`contract-state`, `code`, `witness`, etc.). Cross-DB transactionality is
doubly absent: even if a single-DB buffer were populated, multi-DB
atomicity is not expressible on the wire.

**Iterators.** `engine.get_keys_next` / `get_values_next` / `get_next` /
`prefix_query` use `db.iterator(...)` directly against the live RocksDB
handle. None accept a transaction or snapshot context. The
`RemoteStorageIterator` returned by [`RemoteStorageSPI.iterator(...)`](../framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java#L343-L355)
is a local Java wrapper that does not issue a streaming RPC.

**Summary of the gap.** Today, the storage transaction surface is a
**structural placeholder**: the Java SPI exists, `RemoteStorageSPI`
serializes the calls, the Rust engine allocates IDs, and the gRPC
handlers happily return success. But no caller writes through the
transaction, no buffer is ever populated, no read can see in-flight
writes (the wire forbids it), and the only `commit` work that ever
happens is a no-op `WriteBatch::default().write()`. This is worse than
"unimplemented" — it is silently fake.

#### Locked decision

**LD-5 locks the storage transaction surface as follows.** The choices
are deliberately minimal: they reflect what Phase 1 execution actually
needs (per the audit above: nothing), what the next phase
(`block importer / block executor`) plausibly needs, and a refusal to
turn `StorageSPI` into a generic database product on the way there.

1. **Phase 1 required semantics: none.** No Phase 1 caller of
   `StorageSPI` (in either `EE` or `RR`) requires
   `beginTransaction` / `commitTransaction` / `rollbackTransaction` to
   do real work. The Java revoking layer
   (`TronStoreWithRevoking` / `ChainBaseManager`) remains the canonical
   Phase 1 transaction boundary for `RR` sidecar application, and
   `EE` is unchanged. The storage SPI's transaction methods are
   permitted to remain stubs **only if** they are made loud about it
   (see "Mandatory cleanup" below). Phase 1 acceptance does not require
   real per-DB transaction buffering.

2. **Transaction scope: per DB.** When real semantics land (next phase
   or later), `transaction_id` is scoped to **exactly one named
   database**. Cross-DB atomicity is **rejected** for the storage SPI
   contract — it is not expressible on the current wire and would
   require a different abstraction layer. Multi-store atomicity in
   `RR` is the responsibility of the layer above `StorageSPI` (the
   block importer's commit step, not the storage RPC).

3. **"Execution-local enough", not "generic database product".**
   `StorageSPI.beginTransaction` is **not** a general-purpose database
   transaction. It is a storage-side write batch with an explicit
   commit point, scoped to one DB, and only for the use cases that the
   block importer needs. It does **not** need to support nested
   transactions, savepoints, isolation levels, deadlock detection,
   long-running idle transactions, or distributed coordinator
   semantics. Any future work that wants those things must propose a
   different abstraction, not extend this one.

4. **Read-your-writes: not required in Phase 1.** Specifically:
   - `get` against a `transaction_id`: **not supported**, and the wire
     format already enforces this (no `transaction_id` field on
     `GetRequest`).
   - `has` against a `transaction_id`: **not supported**, same reason.
   - `batchGet` against a `transaction_id`: **not supported**, same
     reason.
   - iterators / prefix / range against a `transaction_id`: **not
     supported**, same reason.

   Justification: no Phase 1 caller needs to issue a read inside a
   write transaction. `RuntimeSpiImpl` does not write through
   `StorageSPI` at all (it uses `ChainBaseManager`), and the Rust
   execution service does not call back into Java-visible storage
   reads during `execute_transaction`. If the block importer needs
   read-your-writes later, that is a deliberate extension and gets
   its own LD entry; for Phase 1 we **lock the absence**.

5. **`transaction_id` absent on a write call: direct write to the
   durable store** — exactly what the code does today. There is no
   implicit auto-commit transaction context. This is the locked
   default and it is allowed in Phase 1 because (per item 1) every
   real production write today takes this path. Loud logging on this
   branch is nice-to-have; failing on it would break the entire
   current `RR` write path.

6. **Reject "generic database product" framing.** The `StorageSPI`
   contract is intentionally narrow: a key-value store with batched
   writes, scoped per-database, used by execution and (later) the
   block importer. We will **not** extend it to support arbitrary
   client transaction semantics, ad-hoc query languages, or
   multi-tenant isolation. Any pressure to broaden the contract is
   treated as scope creep and rejected at planning time.

#### Why these choices

- **The current API is silently fake.** Today every `commit_transaction`
  is a no-op `WriteBatch`, every `get_from_snapshot` is a live read,
  and the wire forbids read-your-writes anyway. Locking "Phase 1
  semantics = none + loud about it" is more honest than continuing
  to claim "transactions are partially implemented".
- **No production caller is harmed.** The audit shows zero Phase 1
  callers of the transaction methods, so taking them out of the
  contract costs nothing today.
- **Per-DB scope matches the wire.** The proto, the engine, and the
  handlers all already key on `db_name`. Locking per-DB scope makes
  the future implementation match the existing wire instead of
  forcing a wire migration.
- **"Execution-local enough" matches LD-1.** LD-1 establishes that
  `RR` write ownership is Java-side via `RuntimeSpiImpl.apply*`. Java
  apply uses `TronStoreWithRevoking` for atomicity. The storage SPI
  does not need to duplicate that — it sits *under* the revoking
  layer in `EE` and *beside* it in `RR`, not above it.
- **Rejecting cross-DB transactions early prevents wire churn.** Once
  any caller starts depending on cross-DB atomicity at the
  `StorageSPI` layer, removing it later would be a wire break across
  every database. Rejecting it now keeps the contract minimal.

#### What this lock does NOT cover

- **Snapshot semantics** are tracked separately in §1.4 / a future LD-6.
  LD-5 only addresses the transaction surface. The audit notes the
  current snapshot fakeness for context but does not lock a fix.
- **Real per-DB transaction implementation** is deferred to a future
  phase (almost certainly the block importer phase), and tracked in
  §3.1 / §3.2 of `close_loop.todo.md`. LD-5 does not require it for
  Phase 1 acceptance.
- **The transaction_id wire field** is **not** removed from `PutRequest`
  / `DeleteRequest` / `BatchWriteRequest`. Removing it would be a
  wire break; leaving it lets the future block-importer-era
  implementation light up without a proto migration. Until then it
  is documented (in proto comments — see the deferred follow-ups) as
  "ignored by the current handler; reserved for the block importer
  phase".

#### Mandatory cleanup (LD-5 deferred follow-ups)

These are the minimum changes that LD-5 demands so the codebase stops
silently lying about transaction support, even before real semantics
land:

1. **`backend.proto` comments**: two distinct annotations are required
   because the message-level and field-level honesty stories differ.
   - On the **field** `PutRequest.transaction_id`,
     `DeleteRequest.transaction_id`, and
     `BatchWriteRequest.transaction_id`: "LD-5: this `transaction_id`
     field is **silently ignored** by the current Rust gRPC write
     handlers; writes always go directly to the durable store. Per-DB
     scope only. Real semantics deferred to the block importer phase."
   - On the **messages** `BeginTransactionRequest`,
     `CommitTransactionRequest`, and `RollbackTransactionRequest`:
     "LD-5: structural placeholder. The gRPC handler **does** process
     these RPCs and forwards them to `engine.begin_transaction` /
     `engine.commit_transaction` / `engine.rollback_transaction`, but
     the engine never populates the per-transaction buffer (no
     `engine.put_in_transaction` exists), so `commit_transaction`
     observably writes an empty `WriteBatch` and `rollback_transaction`
     discards an already-empty buffer. Per-DB scope only. Real
     semantics deferred to the block importer phase."
   - On the **field** `snapshot_id` of `GetRequest`, `HasRequest`,
     `BatchGetRequest`, `IteratorRequest`, `PrefixQueryRequest`,
     `GetKeysNextRequest`, `GetValuesNextRequest`, and `GetNextRequest`:
     "LD-5/LD-6: silently ignored by the current handler; reads always
     hit the live durable store. Read-your-writes is intentionally not
     supported in Phase 1."
2. **Java SPI Javadoc**: add a `// LD-5:` block to
   `StorageSPI.beginTransaction` / `commitTransaction` /
   `rollbackTransaction` explicitly stating "Phase 1: structural
   placeholder. No production caller relies on real semantics. Will be
   implemented in the block importer phase. Per-DB scope only.
   Read-your-writes is intentionally not supported."
3. **`EmbeddedStorageSPI` honesty**: replace the silent no-op
   implementations with either (a) a logged warning on first call, or
   (b) explicit `UnsupportedOperationException` if any caller is ever
   discovered. Today no caller exists, so logged-warning is the
   minimal acceptable form.
4. **Rust engine honesty**: either (a) remove the dead `operations:
   Vec::new()` field and the no-op `commit_transaction` /
   `rollback_transaction` paths and have them return an explicit
   `unimplemented`-style error, or (b) keep them but log loudly that
   the transaction was a no-op. Today the safer choice is (b) with an
   explicit `tracing::warn!` and a doc comment cross-referencing
   LD-5, because some test paths exercise the round trip.
5. **gRPC handler honesty**: in `mod.rs`, log a warning when a
   `PutRequest` / `DeleteRequest` / `BatchWriteRequest` arrives with a
   non-empty `transaction_id`, since today that field is silently
   ignored and a future caller could mistakenly believe it is
   honored. Similarly log when a `GetRequest` / `HasRequest` /
   `BatchGetRequest` / `IteratorRequest` / `PrefixQueryRequest` /
   `GetKeysNextRequest` / `GetValuesNextRequest` / `GetNextRequest`
   arrives with a non-empty `snapshot_id`. Note that
   `BeginTransactionRequest` / `CommitTransactionRequest` /
   `RollbackTransactionRequest` are **not** in this "field silently
   ignored" category — those handlers do call into the engine, and
   their honesty cleanup belongs to item (4) above (Rust engine
   honesty: warn at engine level that the round trip is a no-op).

These cleanups are doc/log only — none of them changes runtime
behavior. They are sequenced to land ahead of any real implementation
work, and they are tracked as `1.3 deferred follow-ups` in
`close_loop.todo.md`.

### LD-6: Snapshot Semantics

#### Audit (current state, evidence-based)

LD-5's audit already established that **storage snapshot is fake** in
the Rust gRPC stack: [`engine.create_snapshot`](../rust-backend/crates/storage/src/engine.rs#L436-L448)
allocates a `SnapshotInfo { db_name }` with no RocksDB snapshot handle,
[`get_from_snapshot`](../rust-backend/crates/storage/src/engine.rs#L460-L472)
reads the live DB, and the gRPC handlers for
`Get` / `Has` / `BatchGet` / `Iterator` / `PrefixQuery` /
`GetKeysNext` / `GetValuesNext` / `GetNext` ignore `snapshot_id`
entirely. LD-6 takes that finding as given and adds the **EVM
snapshot/revert** half of the story.

**Java SPI (EVM snapshot).** [`ExecutionSPI.java#L86-L99`](../framework/src/main/java/org/tron/core/execution/spi/ExecutionSPI.java#L86-L99)
declares two methods:

- `CompletableFuture<String> createSnapshot()` — Javadoc *"Create EVM
  snapshot for state rollback."* No parameters.
- `CompletableFuture<Boolean> revertToSnapshot(String snapshotId)` —
  Javadoc *"Revert to EVM snapshot."*

**`RemoteExecutionSPI` does not even issue a gRPC call.**
[`RemoteExecutionSPI.java#L214-L225`](../framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java#L214-L225)
returns `"remote_snapshot_" + System.currentTimeMillis()` after logging
*"Remote createSnapshot not yet implemented - returning placeholder."*
[`RemoteExecutionSPI.java#L228-L239`](../framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java#L228-L239)
returns hardcoded `false` after logging *"Remote revertToSnapshot not
yet implemented - returning false."* Both contain a `// TODO: Implement
in Task 2 with ExecutionGrpcClient` comment. Neither method touches
the wire.

**Production callers: none.** A repo-wide search finds zero
production callers of `ExecutionSPI.createSnapshot` /
`revertToSnapshot`. The only callers are `ShadowExecutionSPI.java#L282-L329`
(the in-process shadow path that LD-1 and the §0 mode decision both
explicitly de-emphasize as the Phase 1 validator) and
`EmbeddedExecutionSPI.java#L193-L220` (which has its own placeholder
implementations). [`RuntimeSpiImpl.java`](../framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java)
never calls them — not in `execute()`, not in `postExecMirror()`, not
in `applyStateChangesToLocalDatabase()`. No actuator under
`framework/src/main/java` calls them either. They are **dead call
sites in the production transaction path**.

**Wire surface.** From `framework/src/main/proto/backend.proto`:

- `CreateEvmSnapshotRequest` (around line 949): **empty body**,
  zero fields. Comment: *"execution uses unified storage context."*
- `CreateEvmSnapshotResponse`: `string snapshot_id = 1`,
  `bool success = 2`, `string error_message = 3`.
- `RevertToEvmSnapshotRequest`: `string snapshot_id = 1`.
- `RevertToEvmSnapshotResponse`: `bool success = 1`,
  `string error_message = 2`.
- `ExecutionContext.snapshot_id = 7` is a **storage-level read
  snapshot identifier** for `get_code` / `get_storage_at` /
  `get_balance` / etc., **distinct** from `CreateEvmSnapshotResponse.snapshot_id`.
  In [`RemoteExecutionSPI.buildExecuteTransactionRequest`](../framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java#L381)
  the outgoing `ExecutionContext.snapshot_id` is never set.

**Rust gRPC handlers (EVM snapshot).** Both
[`create_evm_snapshot`](../rust-backend/crates/core/src/service/grpc/mod.rs#L1870-L1884)
and [`revert_to_evm_snapshot`](../rust-backend/crates/core/src/service/grpc/mod.rs#L1886-L1899)
are labeled "Placeholder implementation". `create_evm_snapshot`
allocates a fresh UUID, returns `success: true`, empty error message,
and **does not call any execution or storage subsystem**.
`revert_to_evm_snapshot` returns `success: true`, **ignores
`request.snapshot_id` entirely**, and likewise calls no subsystem. Both
return fake success unconditionally.

**Rust execution module.** [`rust-backend/crates/execution/src/lib.rs`](../rust-backend/crates/execution/src/lib.rs)
contains **no `create_evm_snapshot` / `revert_to_evm_snapshot` method
at all.** The `ExecutionModule` exposes only
`execute_transaction_with_storage`, `call_contract_with_storage`,
`estimate_energy_with_storage`, `get_code`, `get_storage_at`,
`get_nonce`, `get_balance`, and lifecycle methods. The gRPC handlers
above do not even attempt to delegate downward — there is nothing to
delegate to.

**Internal revm checkpointing (different layer — not LD-6 scope).**
[`rust-backend/crates/execution/src/tron_evm.rs`](../rust-backend/crates/execution/src/tron_evm.rs)
does use revm's own journaled state for **contract-internal**
rollback: line 188 reads `context.journaled_state.depth()`, line 266
checks call-stack depth, lines 306-315 call
`context.evm.journaled_state.create_account_checkpoint(...)` for
contract-creation frames and pass the resulting `checkpoint` into
`FrameOrResult::new_create_frame`. This is revm's own
single-`execute_transaction`-call journaled-state mechanism for
sub-call / `CREATE` rollback when a frame `REVERT`s. **It is not
connected to the external `CreateEvmSnapshot` / `RevertToEvmSnapshot`
RPCs and is not snapshot semantics in LD-6's sense.** It works fine
today and is out of LD-6 scope.

**No tests.** The repo has zero tests — Java or Rust — that exercise
`CreateEvmSnapshot` / `RevertToEvmSnapshot`. The Rust test tree has no
references to either RPC. The only `createSnapshot` calls in
`framework/src/test` are
[`StorageSPIIntegrationTest.java#L224`](../framework/src/test/java/org/tron/core/storage/spi/StorageSPIIntegrationTest.java#L224)
(storage-level snapshot, not EVM-level — already covered by LD-5).

**Summary of the gap.** Phase 1 has **two simultaneous fakeries**: the
storage `snapshot_id` field is silently ignored on every read RPC, and
the EVM `CreateEvmSnapshot` / `RevertToEvmSnapshot` RPCs return
`success: true` without performing any action. The Java
`RemoteExecutionSPI` does not even reach the wire for the EVM
half. Yet the Java SPI surface continues to advertise both as if they
work, and `ShadowExecutionSPI` continues to call them on every shadow
comparison path. This is structurally indistinguishable from "fake
snapshot success", which §1.4 acceptance explicitly forbids.

#### Locked decision

**LD-6 locks Phase 1 snapshot semantics as follows.** Both halves
(storage snapshot and EVM snapshot) are placed under one decision
because they share the same root cause (no production caller, no real
implementation, fake success at every layer) and the same Phase 1
remediation (be loud about it instead of silently lying).

1. **Storage snapshot in Phase 1 = no point-in-time guarantee, every
   call is warned.** The Rust storage `engine.create_snapshot` /
   `engine.get_from_snapshot` and the corresponding `RemoteStorageSPI`
   methods are **not** required to implement true RocksDB
   point-in-time snapshots in Phase 1. Real point-in-time snapshot is
   deferred to the block importer phase, where it has a concrete
   consumer (the importer's pre-block snapshot).

   The Phase 1 contract for `getFromSnapshot` is **locked by LD-9**
   (superseding the original LD-6 disjunction): keep the live-read
   fallback so `EmbeddedStorageSPI.getFromSnapshot` and the existing
   `StorageSPIIntegrationTest` round-trip continue to compile and
   run, but emit a loud `tracing::warn!` **on every call** (not
   first-call-only) cross-referencing LD-6/LD-9, loud enough to trip
   a CI grep. Option (a) `tonic::Status::unimplemented` is **no
   longer open** for Phase 1.

   What is **not** acceptable is a production `RR` code path that
   silently treats `getFromSnapshot(id, key)` as a point-in-time read
   without the warn. Today there is no such path (the audit confirms
   zero production callers), and LD-6/LD-9 locks that absence: any
   future addition of a production storage-snapshot caller is gated
   on the deferred block-importer-phase implementation landing first.

2. **EVM snapshot in Phase 1 = explicitly unsupported in `RR`.**
   `ExecutionSPI.createSnapshot` /
   `ExecutionSPI.revertToSnapshot` and their gRPC counterparts are
   **not** required to support cross-transaction or cross-call EVM
   state rollback in Phase 1. The Phase 1 stance is:
   - `RemoteExecutionSPI.createSnapshot` must change from "return
     fake placeholder string and log a warning" to **fail-loud**:
     `return CompletableFuture.failedFuture(new
     UnsupportedOperationException("EVM snapshot/revert is not
     supported in RR Phase 1 (LD-6/LD-8)"))`. Same for
     `revertToSnapshot`. **Locked by LD-8 §1** to the failed-future
     form only; an earlier phrasing that also allowed a synchronous
     `throw` has been superseded to align with all eight
     CompletableFuture-returning read-path methods.
   - The Rust gRPC handlers `create_evm_snapshot` /
     `revert_to_evm_snapshot` must change from "return
     `success: true` unconditionally" to returning
     `Err(tonic::Status::unimplemented("<rpc_method> not implemented
     in Phase 1 (LD-6/LD-8)"))` at the transport layer.
     **Locked by LD-8 §2**: the earlier LD-6 phrasing that also
     allowed application-level `{success: false, error_message: ...}`
     as a valid alternative has been superseded — LD-8 closes that
     choice in favor of transport `Status::unimplemented` only,
     aligned with the other four placeholder read-path RPCs. The
     deferred follow-up below acknowledges this is a behavior
     change for any consumer that today blindly trusts `success`.

   The wire messages themselves (`CreateEvmSnapshotRequest`,
   `RevertToEvmSnapshotRequest`, their responses) are **not removed**
   from the proto in Phase 1; only the handlers' behavior is locked.
   This preserves wire stability for the future implementation.

3. **EVM snapshot/revert is not built on storage snapshot in Phase 1.**
   Of the §1.4 sub-options:
   - storage snapshot — **rejected for Phase 1** (no real impl, no
     consumer)
   - execution-local journaling — **already in use, sufficient,
     unchanged** (revm's `journaled_state` / `create_account_checkpoint`
     in `tron_evm.rs` handles contract-internal rollback for the
     duration of a single `execute_transaction` call, which is
     everything Phase 1 actually needs)
   - both — **rejected** (premature; cross-tx EVM rollback has no
     Phase 1 consumer)

   In other words: Phase 1 EVM rollback semantics are 100% **internal
   to a single transaction's revm execution**, and that already
   works. Cross-transaction or external-RPC-driven EVM snapshot is
   deferred.

4. **`ShadowExecutionSPI` is not a production validator (LD-1
   reaffirmation).** The audit shows that the only Java callers of
   `ExecutionSPI.createSnapshot` / `revertToSnapshot` live in
   `ShadowExecutionSPI`. Per LD-1 and §0 mode decision, shadow is
   explicitly not the Phase 1 acceptance mechanism. LD-6 therefore
   does **not** treat `ShadowExecutionSPI` as a constraint when
   deciding to make `RemoteExecutionSPI.createSnapshot` /
   `revertToSnapshot` fail loud. If the change breaks shadow mode,
   that is the intended signal — shadow is on the deprecation path
   already.

5. **Temporary "unsupported" is safer than fake success.** This is
   the §1.4 sub-question and LD-6 answers it explicitly: **yes**.
   Returning fake `success: true` for an EVM revert (or fake reads
   for a storage snapshot) hides real divergence and contaminates
   any future `EE`-vs-`RR` parity verification. Explicit "unsupported"
   makes the gap observable and CI-trippable.

6. **Snapshot-dependent APIs either fail explicitly, loud-degrade
   with an every-call warn, or provide real guarantees.** This is
   the §1.4 second acceptance criterion. LD-6 locks Phase 1 to a
   two-halves form (with the storage half later refined by LD-9):
   (a) EVM snapshot/revert is **hard-unsupported** — `RemoteExecutionSPI`
   returns `CompletableFuture.failedFuture(UnsupportedOperationException)`
   and the Rust gRPC `create_evm_snapshot` / `revert_to_evm_snapshot`
   return `tonic::Status::unimplemented`; (b) storage `getFromSnapshot`
   is **loud-degrade** — keep the live-read fallback so
   `EmbeddedStorageSPI.getFromSnapshot` and the existing
   `StorageSPIIntegrationTest` round-trip compile, but emit a
   `tracing::warn!` on every call cross-referencing LD-6/LD-9.
   The "real guarantees" half is deferred to the block importer
   phase. Silently-fake success (no warn, no error) is still
   explicitly forbidden.

#### Why these choices

- **No production caller is harmed.** The audit shows zero production
  call sites for either flavor of snapshot. The only callers are in
  shadow (already de-emphasized) and in tests (which can be updated).
  Phase 1 can flip to fail-loud without breaking the live transaction
  path.
- **Fake success is structurally worse than unimplemented.** A
  consumer that calls `revert_to_evm_snapshot(id)` and gets back
  `success: true` cannot tell whether revert happened. Every future
  parity comparison that wires snapshot/revert will inherit silent
  divergence. Fail-loud is the only honest interim state.
- **revm's internal journaling is sufficient for Phase 1 contract
  execution.** Single-tx contract-internal rollback already works via
  `tron_evm.rs`, and that is the only EVM rollback semantics any
  Phase 1 contract actually exercises. Cross-tx snapshot/revert is a
  block-importer concern, not an execution concern.
- **Wire stability is preserved.** Not removing the proto messages
  means the future block-importer-phase implementation can light up
  without a wire migration, just as LD-5 preserves
  `transaction_id` on the write messages.
- **One LD covers both halves intentionally.** Storage snapshot and
  EVM snapshot share the same fake-success failure mode and the same
  Phase 1 remediation. Splitting them into LD-6a / LD-6b would just
  duplicate the rationale.

#### What this lock does NOT cover

- **Real per-RocksDB-handle snapshot implementation** is deferred to
  the block importer phase. LD-6 does not require it for Phase 1
  acceptance.
- **Cross-transaction EVM state rollback** is deferred to whichever
  phase first introduces a consumer (likely the block importer's
  failed-block recovery path, possibly later). LD-6 does not lock a
  design for it.
- **Internal revm `journaled_state` / `create_account_checkpoint`
  usage in `tron_evm.rs`** is **out of LD-6 scope** and continues to
  work as-is. LD-6 only locks the external snapshot RPCs.
- **Removing `ExecutionSPI.createSnapshot` /
  `revertToSnapshot` from the Java SPI surface** is **not** mandated
  by LD-6. The methods stay (so shadow mode keeps compiling) but
  their `RemoteExecutionSPI` implementations are flipped to
  fail-loud.
- **Removing `snapshot_id` from `ExecutionContext`** is not mandated.
  The field stays for forward compatibility with a future
  storage-snapshot-aware read path.

#### Mandatory cleanup (LD-6 deferred follow-ups)

These are the minimum changes that LD-6 demands so Phase 1 stops
silently lying about snapshot support. Like the LD-5 cleanups, they are
sequenced to land before any real snapshot implementation work, and
they are tracked as `1.4 deferred follow-ups` in
`close_loop.todo.md`.

1. **`RemoteExecutionSPI.createSnapshot` fail-loud**
   (`framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java#L214-L225`):
   replace the placeholder return with a `CompletableFuture` that
   completes exceptionally with `UnsupportedOperationException("EVM
   snapshot/revert is not supported in RR Phase 1 (LD-6); see
   close_loop.planning.md")`. Remove the `// TODO: Implement in Task
   2` comment and the placeholder log line.
2. **`RemoteExecutionSPI.revertToSnapshot` fail-loud** (lines 228-239):
   same treatment.
3. **Rust gRPC `create_evm_snapshot` fail-loud**
   (`rust-backend/crates/core/src/service/grpc/mod.rs#L1870-L1884`):
   change to return `tonic::Status::unimplemented("EVM snapshot
   unsupported in Phase 1 (LD-6)")`. Remove the placeholder UUID
   allocation.
4. **Rust gRPC `revert_to_evm_snapshot` fail-loud** (lines 1886-1899):
   same treatment.
5. **Storage `getFromSnapshot` honesty**
   (`rust-backend/crates/core/src/service/grpc/mod.rs` snapshot read
   handler and `engine.get_from_snapshot`): change from "read live DB"
   to option (b) keep the live-read behavior but emit a loud
   `tracing::warn!` on every call cross-referencing LD-6 (and now
   LD-9). The warn must be loud enough to trip a CI grep.
   **LD-9 update**: the original LD-6 #1.4 cleanup left this as a
   disjunction between (a) `tonic::Status::unimplemented("storage
   snapshot unsupported in Phase 1 (LD-6)")` and (b) live-read +
   warn. LD-9 locks option (b) because `EmbeddedStorageSPI.getFromSnapshot`
   and the existing `StorageSPIIntegrationTest` round-trip depend
   on the live-read fallback compiling; flipping to (a) would break
   the embedded path without any real caller needing it. Option (a)
   is no longer open for Phase 1. See LD-9 "Locked decision" item 2.
6. **Storage gRPC read handlers honesty**: this overlaps with the
   LD-5 follow-up "log when `snapshot_id` is observed" and is
   intentionally not duplicated here. The LD-5 follow-up already
   covers `Get` / `Has` / `BatchGet` / `Iterator` / `PrefixQuery` /
   `GetKeysNext` / `GetValuesNext` / `GetNext`; LD-6 only adds the
   snapshot-specific `GetFromSnapshot` path above.
7. **`ExecutionSPI.createSnapshot` / `revertToSnapshot` Javadoc note**:
   add a Javadoc paragraph (Javadoc form, not a `// ` line comment)
   explicitly stating "Phase 1 (LD-6): EVM snapshot and revert are
   not supported in `RR`. Remote callers receive
   `UnsupportedOperationException`. The `EmbeddedExecutionSPI`
   implementation is itself a placeholder (see
   `EmbeddedExecutionSPI.java#L193-L220`) and does not provide real
   snapshot semantics either. Real semantics deferred to a future
   phase with a concrete consumer (likely block importer)."
8. **`ShadowExecutionSPI` snapshot path**: do **not** alter shadow
   in Phase 1 (it is being de-emphasized per LD-1). The fact that
   LD-6 will break shadow's snapshot round trip is the intended
   signal that shadow should not be relied on for snapshot
   verification. Track the shadow-fallout investigation as a
   separate item; LD-6 does not require fixing shadow.
9. **Conformance / test sweep**: confirm zero Phase 1 production
   tests depend on `create_evm_snapshot` / `revert_to_evm_snapshot`
   returning `success: true`. The audit found zero such tests but a
   final sweep should accompany the fail-loud flip in case anything
   landed since the audit was written.

These cleanups are all small surface changes; the largest behavior
change is the gRPC handler flip from `success: true` to
`Unimplemented`. None of them change Phase 1's actual transaction
execution path because no production code calls these APIs.

### LD-7: Contract Support Matrix

#### Audit (current state, evidence-based)

Phase 1 needs an unambiguous answer to "which contract types are
actually `RR`-ready, which are explicitly blocked, and which are still
just experimental?". Today the answer is implicit, scattered across
Java JVM properties, Rust config flags, and code-default disagreements.
LD-7 makes it explicit.

**Contract types in the proto enum.** [`Tron.proto` lines 338-380](../protocol/src/main/protos/core/Tron.proto)
defines `Transaction.Contract.ContractType` with **41 variants** (values
in `0..59` with non-contiguous gaps — e.g. `7`, `21..29`, `34..40`, `47`,
`50`). The full list:

`AccountCreateContract(0)`, `TransferContract(1)`,
`TransferAssetContract(2)`, `VoteAssetContract(3)`,
`VoteWitnessContract(4)`, `WitnessCreateContract(5)`,
`AssetIssueContract(6)`, `WitnessUpdateContract(8)`,
`ParticipateAssetIssueContract(9)`, `AccountUpdateContract(10)`,
`FreezeBalanceContract(11)`, `UnfreezeBalanceContract(12)`,
`WithdrawBalanceContract(13)`, `UnfreezeAssetContract(14)`,
`UpdateAssetContract(15)`, `ProposalCreateContract(16)`,
`ProposalApproveContract(17)`, `ProposalDeleteContract(18)`,
`SetAccountIdContract(19)`, `CustomContract(20)`,
`CreateSmartContract(30)`, `TriggerSmartContract(31)`,
`GetContract(32)`, `UpdateSettingContract(33)`,
`ExchangeCreateContract(41)`, `ExchangeInjectContract(42)`,
`ExchangeWithdrawContract(43)`, `ExchangeTransactionContract(44)`,
`UpdateEnergyLimitContract(45)`,
`AccountPermissionUpdateContract(46)`, `ClearABIContract(48)`,
`UpdateBrokerageContract(49)`, `ShieldedTransferContract(51)`,
`MarketSellAssetContract(52)`, `MarketCancelOrderContract(53)`,
`FreezeBalanceV2Contract(54)`, `UnfreezeBalanceV2Contract(55)`,
`WithdrawExpireUnfreezeContract(56)`, `DelegateResourceContract(57)`,
`UnDelegateResourceContract(58)`, `CancelAllUnfreezeV2Contract(59)`.

**Java dispatch surface.** [`RemoteExecutionSPI.buildExecuteTransactionRequest`](../framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java#L403)
contains the contract-type switch (~lines 403-1053). Three behaviors
exist per case:

1. **Unconditional send** — Java forwards the contract to the Rust
   execution service for every transaction (no JVM flag).
2. **JVM-flag gated send** — Java only forwards if the corresponding
   `-Dremote.exec.<group>.enabled=true` JVM property is set. The
   default for every JVM gate is **false**.
3. **`default:` fall-through** — Java throws `UnsupportedOperationException`
   for any contract type not listed in the switch (~line 1047-1053).
   This is the path that catches `VoteAssetContract`, `CustomContract`,
   `GetContract`, and `ShieldedTransferContract`.

The JVM gate groups are:

| JVM property | Contract types it gates |
|---|---|
| `-Dremote.exec.trc10.enabled` | `TransferAsset`, `AssetIssue`, `ParticipateAssetIssue`, `UnfreezeAsset`, `UpdateAsset` |
| `-Dremote.exec.proposal.enabled` | `ProposalCreate`, `ProposalApprove`, `ProposalDelete` |
| `-Dremote.exec.account.enabled` | `SetAccountId`, `AccountPermissionUpdate` |
| `-Dremote.exec.contract.enabled` | `UpdateSetting`, `UpdateEnergyLimit`, `ClearABI` |
| `-Dremote.exec.brokerage.enabled` | `UpdateBrokerage` |
| `-Dremote.exec.resource.enabled` | `WithdrawExpireUnfreeze`, `DelegateResource`, `UnDelegateResource`, `CancelAllUnfreezeV2` |
| `-Dremote.exec.exchange.enabled` | `ExchangeCreate`, `ExchangeInject`, `ExchangeWithdraw`, `ExchangeTransaction` |
| `-Dremote.exec.market.enabled` | `MarketSellAsset`, `MarketCancelOrder` |

**Rust dispatch surface.** [`service/mod.rs` lines 648-996](../rust-backend/crates/core/src/service/mod.rs)
contains `execute_non_vm_contract`, a per-`TronContractType` match
that calls into a per-contract handler. The wildcard arm (`_`) at
~line 984-990 returns `Err(format!(...))` for any variant not listed.
Each handler is gated by a per-contract `execution.remote.<...>_enabled`
flag declared in [`crates/common/src/config.rs`](../rust-backend/crates/common/src/config.rs).
For VM contracts, [`execution/src/lib.rs` line 62](../rust-backend/crates/execution/src/lib.rs#L62)
runs `TriggerSmartContract` through revm and [`line 104`](../rust-backend/crates/execution/src/lib.rs#L104)
runs `CreateSmartContract` through revm.

**Config-default vs config.toml mismatch.** Many `_enabled` flags have
`default: false` in `config.rs` but are set to `true` in the
checked-in `rust-backend/config.toml`. This is the same pattern
LD-2 / LD-3 originally called out for `rust_persist_enabled`
(now closed by the §1.1 follow-up #1 flip): `config.toml` is
**not** the safe rollout default, it is an opinionated override
for the parity work. Flags where code-default = `false` but
config.toml = `true`:

`vote_witness_enabled`, `trc10_enabled`, `freeze_balance_enabled`,
`unfreeze_balance_enabled`, `freeze_balance_v2_enabled`,
`unfreeze_balance_v2_enabled`, `withdraw_balance_enabled`,
`account_create_enabled`, `participate_asset_issue_enabled`,
`unfreeze_asset_enabled`, `update_asset_enabled`,
`emit_freeze_ledger_changes`, `emit_global_resource_changes`. All
Phase-2 surfaces (proposals, account-mgmt, contract-metadata, brokerage,
resource/delegation, exchange, market) remain `false` in both layers.

**Known parity gaps documented in code or `CLAUDE.md`:**

- **TriggerSmartContract TRC-10 pre-execution transfer**: explicitly
  rejected at [`execution/src/lib.rs` lines 507-521](../rust-backend/crates/execution/src/lib.rs#L507)
  with `"TRC-10 pre-execution transfer not yet implemented for
  TriggerSmartContract"`. `CreateSmartContract` has its own working
  path via [`extract_create_contract_trc10_transfer` at
  `service/grpc/mod.rs` line 1473-1487](../rust-backend/crates/core/src/service/grpc/mod.rs#L1473).
  This is §5.1's still-open gap.
- **`TriggerSmartContract` / `CreateSmartContract` energy limit**:
  LD-4 work is deferred. On the happy path
  [`RemoteExecutionSPI.computeEnergyLimitWithFixRatio` at L309-L376](../framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java#L309)
  already converts `feeLimit` into energy units
  (`energyFromFeeLimit = feeLimit / sunPerEnergy`, then
  `min(availableEnergy, energyFromFeeLimit)` at L363-L366), so the
  value handed to Rust is already energy-unit-denominated. However
  every fallback branch (missing store / account / exception, see
  L315, L321, L329, L336, L375) returns the raw `feeLimit` in SUN,
  which Rust then re-interprets as an energy budget. LD-4 tracks the
  full cutover — collapsing the fallback semantics, fixing the
  Rust-side unit assumption, and removing the SUN-vs-energy-units
  asymmetry. Both sides know about it.
- **`AssetIssueContract`**: Phase 1 is fee-charging only; full TRC-10
  persistence is deferred to Phase 2 (per `CLAUDE.md` lessons).
- **`WithdrawBalanceContract`**: the Rust handler lives at
  [`service/contracts/withdraw.rs` line 229](../rust-backend/crates/core/src/service/contracts/withdraw.rs#L229)
  and already computes delegation rewards via
  [`delegation::withdraw_reward` at L326](../rust-backend/crates/core/src/service/contracts/withdraw.rs#L326),
  i.e. it is **not** the "allowance-only" Phase 1 contract that
  earlier drafts of this doc assumed. Inline unit tests for the
  reward/allowance arithmetic exist in the same file (see the
  `#[cfg(test)] mod tests` block starting around
  [`withdraw.rs` L623](../rust-backend/crates/core/src/service/contracts/withdraw.rs#L623)),
  but there is **no dedicated handler test file under
  `rust-backend/crates/core/src/service/tests/contracts/`** — that
  directory is the convention for per-contract dispatch-level tests
  the rest of the matrix uses.
- **`VoteWitnessContract`**: dual-store update (account + votes)
  required, gated by `vote_witness_seed_old_from_account=true`.

**Test and fixture coverage gaps.** Per-contract Rust unit tests
exist under `rust-backend/crates/core/src/service/tests/contracts/`
for most enabled handlers. Handlers with **no dedicated handler
test file** in that directory (some still have inline
`#[cfg(test)] mod tests` blocks in `service/contracts/*.rs`):
`WithdrawBalanceContract` (inline tests exist in `withdraw.rs`),
`ProposalCreateContract`, `ClearABIContract`,
`UpdateBrokerageContract`, `WithdrawExpireUnfreezeContract`,
`ExchangeInjectContract`, `ExchangeWithdrawContract`,
`ExchangeTransactionContract`, `MarketCancelOrderContract`.

Conformance fixtures **do** exist on disk:
[`conformance/fixtures/`](../conformance/fixtures) has populated
per-contract directories for most of the current handler set
(30+ directories including `transfer_contract/`,
`freeze_balance_contract/`, `vote_witness_contract/`,
`withdraw_balance_contract/`, `create_smart_contract/`, …), and the
conformance runner at
[`crates/core/src/conformance/runner.rs` ~L1416](../rust-backend/crates/core/src/conformance/runner.rs#L1416)
already points at that tree. What is still missing is (a) a
one-to-one mapping between LD-7 matrix rows and existing fixture
directories, (b) per-contract coverage accounting (which variants
of each contract have fixture rows), and (c) the **offline
`EE`-vs-`RR` replay harness** — the only *live* parity comparison
today is the in-process `ShadowExecutionSPI`, which LD-1
explicitly de-emphasizes as a Phase 1 acceptance mechanism.

#### Locked decision

**LD-7 locks the contract support matrix.** Every contract type is
classified into exactly one of four buckets. The classification rule
is concrete and falsifiable, not aspirational.

**Bucket definitions (Phase 1):**

1. **`EE only`** — Embedded-only by definition. Java's `default:` arm
   throws on this contract type; Rust has no handler at all. Phase 1
   does not attempt to enable this contract type in `RR`. Either the
   semantics are out of scope (e.g. shielded ZK-proof transfers) or
   the contract type is structurally a query, not a mutation.

2. **`RR blocked`** — Has a Rust handler implementation, but is gated
   off by default in **at least one** of (a) Java JVM property, (b)
   Rust `config.rs` code-default, **and** is not on the Phase 1
   whitelist. May be enabled by hand for experiments, but not part of
   the Phase 1 acceptance matrix. The Rust handler may be partial,
   missing tests, or known to have parity gaps.

3. **`RR candidate`** — Has a Rust handler implementation that is
   wired and tested at the Rust unit level (inline or dedicated
   tests), Java forwards it to Rust, and the only thing standing
   between it and `RR canonical-ready` status is a known parity gap
   that LD-2..LD-6 or §5.x already tracks as deferred work. Missing
   conformance fixture coverage or missing offline `EE`-vs-`RR`
   replay coverage is **not** a candidate-vs-canonical-ready gate in
   Phase 1 — those are uniform §3.4 deliverables and apply equally
   to every whitelist-target contract. Eligible for promotion to
   canonical-ready when the parity gap closes.

4. **`RR canonical-ready`** — All of: (a) Java forwards to Rust
   unconditionally with no JVM gate, (b) Rust handler exists with no
   per-contract config flag *or* the flag is `true` in both
   `config.rs` and `config.toml`, (c) a Rust unit test exists in
   `crates/core/src/service/tests/contracts/`, (d) no parity gap
   documented in `CLAUDE.md` lessons or in source comments, (e) no
   sidecar dependency that LD-2..LD-6 marks as deferred. **Conformance
   fixture and EE-vs-RR replay coverage are NOT yet required** for
   canonical-ready in Phase 1 — those are separate Phase 1 deliverables
   tracked under §3.4 / §1.5 acceptance, not per-contract gates. LD-7
   only locks the dispatch / handler / unit-test gate. Once §3.4
   replay coverage exists, a Phase 1.5 follow-up will tighten this
   definition.

**Locked classification (the matrix).**

| Contract type | Bucket | Notes |
|---|---|---|
| `TransferContract(1)` | **RR canonical-ready** | Java unconditional; no flag; `transfer.rs` test exists. |
| `WitnessCreateContract(5)` | **RR canonical-ready** | Java unconditional; `witness_create_enabled` default=true; `witness_create.rs` test exists. |
| `WitnessUpdateContract(8)` | **RR canonical-ready** | Java unconditional; `witness_update_enabled` default=true; `witness_update.rs` test exists. |
| `AccountUpdateContract(10)` | **RR canonical-ready** | Java unconditional; no per-contract flag; `account_update.rs` test exists. |
| `VoteWitnessContract(4)` | **RR candidate** | Code-default `false`, config.toml `true`; dual-store semantics depend on `vote_witness_seed_old_from_account`; tests exist. Promotes to canonical-ready when the code-default flips to `true` (LD-2 cleanup). |
| `AccountCreateContract(0)` | **RR candidate** | Code-default `false`, config.toml `true`; tests exist. Same promotion path as `VoteWitness`. |
| `FreezeBalanceContract(11)` | **RR candidate** | Code-default `false`, config.toml `true`; tests exist; `emit_freeze_ledger_changes` Phase 2 sidecar still gated off in code-default. Promotion blocked on §5.2 sidecar parity. |
| `UnfreezeBalanceContract(12)` | **RR candidate** | Same as `Freeze`. |
| `FreezeBalanceV2Contract(54)` | **RR candidate** | Same as `Freeze`. |
| `UnfreezeBalanceV2Contract(55)` | **RR candidate** | Same as `Freeze`. |
| `TriggerSmartContract(31)` | **RR candidate** | Dispatched to revm; **two** known gaps block canonical-ready: §5.1 TRC-10 pre-execution transfer, and LD-4 `energy_limit` SUN-vs-energy-units cutover. |
| `CreateSmartContract(30)` | **RR candidate** | Dispatched to revm; TRC-10 create-call transfer handled; LD-4 cutover applies. Promotes when LD-4 lands. |
| `WithdrawBalanceContract(13)` | **RR candidate** | Java unconditional; Rust handler at [`service/contracts/withdraw.rs:229`](../rust-backend/crates/core/src/service/contracts/withdraw.rs#L229) computes delegation rewards via `delegation::withdraw_reward` at L326 (not allowance-only); inline unit tests in `withdraw.rs`. Code-default `withdraw_balance_enabled=false`, config.toml `true`. Same promotion path as `VoteWitness` — blocked on the LD-2 code-default flip. Deferred cleanup item also adds a dedicated `service/tests/contracts/withdraw_balance.rs` test file for consistency with the existing matrix naming convention (`account_create.rs`, `freeze_balance.rs`, `vote_witness.rs`, etc.). |
| `TransferAssetContract(2)` | **RR blocked** | TRC-10 group, double-gated. Phase 1 acceptance does not require it; promotion is a §5.2 / Phase 2 deliverable. |
| `AssetIssueContract(6)` | **RR blocked** | Phase 1 fee-only; full persistence Phase 2. |
| `ParticipateAssetIssueContract(9)` | **RR blocked** | TRC-10 group, double-gated. |
| `UnfreezeAssetContract(14)` | **RR blocked** | TRC-10 group, double-gated. |
| `UpdateAssetContract(15)` | **RR blocked** | TRC-10 group, double-gated. |
| `ProposalCreateContract(16)` | **RR blocked** | Proposal group, double-gated; **no unit test**. |
| `ProposalApproveContract(17)` | **RR blocked** | Proposal group, double-gated; tests exist. |
| `ProposalDeleteContract(18)` | **RR blocked** | Proposal group, double-gated; tests exist. |
| `SetAccountIdContract(19)` | **RR blocked** | Account-mgmt group, double-gated; tests exist. |
| `AccountPermissionUpdateContract(46)` | **RR blocked** | Account-mgmt group, double-gated; tests exist. |
| `UpdateSettingContract(33)` | **RR blocked** | Contract-metadata group, double-gated; tests exist. |
| `UpdateEnergyLimitContract(45)` | **RR blocked** | Contract-metadata group, double-gated; tests exist. |
| `ClearABIContract(48)` | **RR blocked** | Contract-metadata group, double-gated; **no unit test**. |
| `UpdateBrokerageContract(49)` | **RR blocked** | Brokerage group, double-gated; **no unit test**. |
| `WithdrawExpireUnfreezeContract(56)` | **RR blocked** | Resource group, double-gated; **no unit test**. |
| `DelegateResourceContract(57)` | **RR blocked** | Resource group, double-gated; tests exist; complex "available FreezeV2" validation per `CLAUDE.md`. |
| `UnDelegateResourceContract(58)` | **RR blocked** | Resource group, double-gated; tests exist. |
| `CancelAllUnfreezeV2Contract(59)` | **RR blocked** | Resource group, double-gated; tests exist. |
| `ExchangeCreateContract(41)` | **RR blocked** | Exchange group, double-gated; tests exist. |
| `ExchangeInjectContract(42)` | **RR blocked** | Exchange group, double-gated; **no unit test**. |
| `ExchangeWithdrawContract(43)` | **RR blocked** | Exchange group, double-gated; **no unit test**. |
| `ExchangeTransactionContract(44)` | **RR blocked** | Exchange group, double-gated; **no unit test**. |
| `MarketSellAssetContract(52)` | **RR blocked** | Market group, double-gated; tests exist. |
| `MarketCancelOrderContract(53)` | **RR blocked** | Market group, double-gated; **no unit test**. |
| `ShieldedTransferContract(51)` | **EE only** | Java `default:` throws; Rust enum has variant but no dispatch arm; ZK-proof infrastructure absent. Out of Phase 1 scope. |
| `VoteAssetContract(3)` | **EE only** | Java `default:` throws; Rust wildcard `Err`. Never wired anywhere. Out of Phase 1 scope. |
| `CustomContract(20)` | **EE only** | Java `default:` throws; Rust wildcard `Err`. Out of Phase 1 scope. |
| `GetContract(32)` | **EE only** | Java `default:` throws; Rust wildcard `Err`. Query type, not a mutation; out of Phase 1 scope. |

**Phase 1 `RR` whitelist target (acceptance §1.5).** For Phase 1
acceptance the explicit `RR` whitelist target is the **union of the
canonical-ready set today plus the candidate set whose blocking gap
is already tracked in LD-1..LD-6 or §5.x**. Concretely:

- **Phase 1 canonical-ready** (target end-of-phase, must reach
  `EE`-vs-`RR` parity): `TransferContract`, `WitnessCreateContract`,
  `WitnessUpdateContract`, `AccountUpdateContract`,
  `VoteWitnessContract` (after LD-2 code-default flip),
  `AccountCreateContract` (after LD-2 code-default flip),
  `WithdrawBalanceContract` (after LD-2 code-default flip),
  `FreezeBalanceContract`, `UnfreezeBalanceContract`,
  `FreezeBalanceV2Contract`, `UnfreezeBalanceV2Contract` (all four
  after §5.2 sidecar parity), `TriggerSmartContract` and
  `CreateSmartContract` (after LD-4 cutover and §5.1 TRC-10
  pre-execution transfer). Total: 13 contracts.

- **Phase 1 RR blocked** (explicitly **not** in the whitelist target;
  may unblock per-contract in later phases): all 24 contracts listed
  as `RR blocked` above. They keep their JVM/Rust gates and stay
  off-by-default. Enabling any of them in production `RR` is a
  deliberate per-contract decision tracked separately.

- **Phase 1 EE only**: `ShieldedTransferContract`, `VoteAssetContract`,
  `CustomContract`, `GetContract`. These do not need any Rust handler
  work in Phase 1 and the Java `default:` throw is the locked
  behavior.

**Promotion rules** (when does an `RR candidate` become
`RR canonical-ready`?):

- The blocking gap referenced in the candidate notes must be closed.
  E.g. `VoteWitnessContract` graduates when LD-2's code-default flip
  for `vote_witness_enabled` lands. `TriggerSmartContract` graduates
  when both LD-4 and §5.1 land.
- Promotion does **not** require offline replay coverage in Phase 1.
  Replay coverage is a §3.4 deliverable that uniformly promotes the
  whole canonical-ready set later, not per-contract.
- Demotion: any newly discovered parity gap in a canonical-ready
  contract drops it back to `RR candidate` and forces a deferred
  follow-up cleanup item.

#### Why these choices

- **Buckets are tied to *current* code state, not aspiration.** "RR
  canonical-ready" can only be claimed for a contract that is already
  shipped, tested, and free of known gaps. Without that
  conservatism the matrix becomes wishful thinking and the §1.5
  acceptance "remote enablement is no longer driven by config
  convenience" cannot be checked.
- **`config.toml` is treated as opinionated override, not source of
  truth.** Per LD-2 and LD-3, the safe defaults live in `config.rs`,
  and `config.toml` is the parity-experiment profile. Promotion
  therefore requires the **code-default** to flip, not just a
  `config.toml` override. This forces the LD-2 code-default cleanup
  before any candidate becomes canonical-ready.
- **Replay/fixture coverage is separated from per-contract promotion.**
  Bundling §3.4 replay infrastructure into each contract's
  promotion gate would block the whole matrix on a single deliverable
  and create chicken-and-egg ordering with §1.5. Instead LD-7 locks
  the per-contract dispatch/handler/test gate, and §3.4 will later
  apply a uniform replay gate to the whole canonical-ready set.
- **JVM and Rust dual-gating is preserved, not rationalized away.**
  The double-gating of TRC-10 / proposal / etc. groups exists for a
  reason (defense in depth, per-deployment opt-in). LD-7 does not
  collapse it; it just makes the gating policy explicit by listing
  the JVM-property table and the Rust per-contract flag table side
  by side.
- **Wildcard arm in Rust is locked, not removed.** Removing the
  `_ => Err(...)` wildcard in `execute_non_vm_contract` would force
  a compile-time exhaustiveness check on `TronContractType`. That is
  desirable long-term but introduces a follow-up burden every time
  Tron adds a new variant. LD-7 keeps the wildcard for Phase 1 and
  defers the exhaustiveness conversion to a later phase.

#### What this lock does NOT cover

- **Per-contract conformance-fixture mapping and coverage accounting**
  are **not a per-contract LD-7 gate**. Populated fixtures already
  exist under `conformance/fixtures/` (see the audit), but neither
  their completeness nor the one-to-one mapping between LD-7 rows
  and fixture directories is required for candidate → canonical-ready
  promotion in Phase 1. That work is owned by §3.4.
- **Removal of the JVM `-Dremote.exec.<group>.enabled` gates** is
  **not** mandated. They remain as the deployment-time opt-in for
  experimental contract groups.
- **The classification of any contract type added to the Tron proto
  enum *after* this audit** is unspecified. Any new `ContractType`
  variant is implicitly `EE only` until LD-7 is updated.
- **The behavior of `TronContractType` Rust enum variants for
  `VoteAssetContract`, `CustomContract`, `GetContract`, and
  `ShieldContract`** that exist in the enum but have no dispatch
  arm — LD-7 locks them as `EE only` from the Java side, but does
  not require Rust to remove the unused enum variants. They are
  forward-compatible placeholders.

#### Mandatory cleanup (LD-7 deferred follow-ups)

These are the minimum changes that LD-7 demands so that the
classification remains honest as code evolves. Like the LD-5 / LD-6
cleanups, they are tracked as `1.5 deferred follow-ups` in
`close_loop.todo.md`.

1. **Code-default flip for the `RR canonical-ready` flags** — every
   per-contract flag whose code-default is `false` but whose
   `config.toml` value is `true` AND whose contract is on the Phase 1
   canonical-ready list must have its `config.rs` default flipped to
   `true`. Concretely: `vote_witness_enabled`,
   `account_create_enabled`, `withdraw_balance_enabled`,
   `freeze_balance_enabled`, `unfreeze_balance_enabled`,
   `freeze_balance_v2_enabled`, `unfreeze_balance_v2_enabled`.
   After the flip, `config.toml` no longer needs the override line
   for *these specific* flags — other `config.toml` overrides (for
   non-whitelist-target contracts or operational flags) remain
   intentional per LD-3 / LD-7's "opinionated profile" framing.
   Bundles cleanly with the LD-2 §1.1 code-default flip cleanup.
2. **Add dedicated handler test files under `service/tests/contracts/`**
   for handlers that have no dedicated file there today (some still
   have inline `#[cfg(test)] mod tests` blocks in
   `service/contracts/*.rs`). For the `RR blocked` set this covers
   `ProposalCreateContract`, `ClearABIContract`,
   `UpdateBrokerageContract`, `WithdrawExpireUnfreezeContract`,
   `ExchangeInjectContract`, `ExchangeWithdrawContract`,
   `ExchangeTransactionContract`, `MarketCancelOrderContract` before
   any of them is considered for promotion. The
   `RR candidate` `WithdrawBalanceContract` is covered separately
   in cleanup item #3 (it already has inline tests but no
   dispatch-level file in the directory). Tests live in
   `rust-backend/crates/core/src/service/tests/contracts/`.
3. **Add a dedicated `service/tests/contracts/withdraw_balance.rs`
   test file** for `WithdrawBalanceContract`. The handler at
   [`service/contracts/withdraw.rs:229`](../rust-backend/crates/core/src/service/contracts/withdraw.rs#L229)
   already computes both allowance and delegation reward, and
   inline tests exist in the handler file, but the rest of the
   matrix convention is one dispatch-level test file per contract
   under `service/tests/contracts/`. Adding this file removes the
   "dedicated test file missing" gap and clears the last
   consistency item for `WithdrawBalanceContract` before its LD-2
   code-default flip promotes it to `RR canonical-ready`.
4. **Resolve §5.1 TRC-10 pre-execution transfer for
   `TriggerSmartContract`**. The reject at
   `execution/src/lib.rs#L507-L521` must be replaced with a real
   transfer implementation matching `VMActuator.call()` Java
   behavior, before `TriggerSmartContract` can promote to
   `RR canonical-ready`.
5. **Resolve LD-4 `energy_limit` cutover for both VM contracts**.
   This is already tracked under §1.2 deferred follow-ups; LD-7 only
   notes the dependency.
6. **Audit doc and runtime alignment between Java JVM property
   names and Rust config flag names**. Today the Java grouping
   (`-Dremote.exec.proposal.enabled` covers all three proposal
   contracts) does not mirror the Rust per-contract `_enabled` flags
   (`proposal_create_enabled`, `proposal_approve_enabled`,
   `proposal_delete_enabled`). This means it is possible to enable
   the Java side without enabling all Rust sides, or vice-versa.
   Document the asymmetry, and either add a runtime warning when
   the layers disagree or collapse one of them. This bundles with
   LD-2 cleanup item #2 (runtime guard for unsafe mode combinations).
7. **Decide whether to remove the Rust `_ => Err(...)` wildcard
   arm** in `execute_non_vm_contract` once the matrix is stable, so
   adding a new `TronContractType` variant becomes a compile-time
   error that forces explicit classification. LD-7 defers this to a
   follow-up phase but tracks it as a known follow-up.
8. **Re-audit the matrix after every Tron proto enum change**. New
   `ContractType` variants must be classified before they can be
   merged. Add a CI grep / lint that flags any new `ContractType`
   not present in the LD-7 matrix.
9. **Conformance fixture coverage audit** — fixtures already exist
   under [`conformance/fixtures/`](../conformance/fixtures) (30+
   per-contract directories, picked up by
   [`conformance/runner.rs:1416`](../rust-backend/crates/core/src/conformance/runner.rs#L1416)).
   The open work is (a) mapping each LD-7 matrix row to its
   corresponding fixture directory, (b) auditing which contracts on
   the Phase 1 whitelist have insufficient fixture rows, and (c)
   extending the tree where coverage is thin. This is owned by
   §3.4, not by LD-7 per-contract promotion. The bullet exists to
   record that LD-7 explicitly does **not** gate promotion on
   fixture presence, and to correct an earlier draft that claimed
   "zero conformance fixtures on disk".

These cleanups are sequenced after LD-2..LD-6 land. LD-7 itself does
not change any code in this iteration; it locks the classification
policy and the matrix snapshot.

### LD-8: Execution Read-Path Contract

Scope: the Phase 1 failure semantics, proto response discrimination,
timeout handling, and error mapping for the remote execution read-path
APIs (`callContract`, `estimateEnergy`, `getCode`, `getStorageAt`,
`getNonce`, `getBalance`, `createSnapshot`, `revertToSnapshot`,
`healthCheck`). This is the decision lock for §2.1, §2.2, and §2.3
of `close_loop.todo.md`. §2.4 test work depends on LD-8 but is
mostly implementation-shaped and therefore sits in the deferred
follow-up list, not in LD-8 itself.

#### Audit (current state, evidence-based)

**Java side** — all nine read-path methods in
[`framework/.../RemoteExecutionSPI.java`](../framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java)
are labelled `// TODO: Implement in Task 2`, and every one of them
returns a silently-wrong value today:

| Method | File:line | Current body | Hazard |
|---|---|---|---|
| `callContract` | `RemoteExecutionSPI.java#L119-L138` | Returns `ExecutionProgramResult` with `setRuntimeError("Remote contract call not yet implemented") + setRevert() + UNKNOWN`. No gRPC call. | Looks like a legitimate revert; callers that only check for revert see a "normal" failure. |
| `estimateEnergy` | `RemoteExecutionSPI.java#L140-L155` | `return 0L;` with `logger.warn`. No gRPC call. | Silent fake zero — callers that gate on "non-zero estimate" see an undercount. |
| `getCode` | `RemoteExecutionSPI.java#L157-L169` | `return new byte[0];` with warn. No gRPC call. | Indistinguishable from "EOA with no code". |
| `getStorageAt` | `RemoteExecutionSPI.java#L171-L183` | `return new byte[0];` with warn. | Indistinguishable from "slot never written". |
| `getNonce` | `RemoteExecutionSPI.java#L185-L197` | `return 0L;` with warn. | Indistinguishable from "fresh EOA". |
| `getBalance` | `RemoteExecutionSPI.java#L199-L211` | `return new byte[0];` with warn. | Indistinguishable from "zero balance". |
| `createSnapshot` | `RemoteExecutionSPI.java#L213-L225` | `return "remote_snapshot_" + System.currentTimeMillis();`. | Fake snapshot ID — already called out by LD-6 §1.4. |
| `revertToSnapshot` | `RemoteExecutionSPI.java#L227-L239` | `return false;`. | Less dangerous (at least `false`), but still not `UnsupportedOperationException`. Also LD-6 §1.4. |
| `healthCheck` | `RemoteExecutionSPI.java#L241-L261` | `HealthStatus(false, "Remote execution service not yet implemented")`. No gRPC call. | The only method that does **not** silently claim success — but it also never actually asks the Rust service, so it reports unhealthy unconditionally. |

None of the nine methods use the initialized `grpcClient` field, none
set any timeout via `withDeadlineAfter`, and none perform any gRPC
`Status`→Java-exception mapping. The `EmbeddedExecutionSPI`
counterparts at
[`EmbeddedExecutionSPI.java#L132-L222`](../framework/src/main/java/org/tron/core/execution/spi/EmbeddedExecutionSPI.java#L132)
are **also** placeholders (`getCode` / `getStorageAt` / `getNonce` /
`getBalance` return zero/empty, `createSnapshot` returns
`"embedded_snapshot_" + millis`, `revertToSnapshot` returns `true`).
The embedded read-path is not a usable oracle for the remote
read-path, which has direct consequences for §2.4 paired tests (see
Key gaps below).

**Rust side** —
[`rust-backend/crates/core/src/service/grpc/mod.rs`](../rust-backend/crates/core/src/service/grpc/mod.rs)
status:

| RPC method | File:line | Status |
|---|---|---|
| `health` | `mod.rs#L98-L133` | **Real.** Aggregates `module_manager.health_all()` per-module. Not a placeholder. |
| `call_contract` | `mod.rs#L1644-L1714` | **Partially real.** Executes through `execution_module.call_contract_with_storage` with a real `EngineBackedEvmStateStore`. Still carries the LD-4 `gas_limit / energy_fee_rate` divide bug. Error path returns `success: false, error_message`. |
| `estimate_energy` | `mod.rs#L1716-L1800` | **Partially real.** Executes through `execution_module.estimate_energy_with_storage`. Error path returns hardcoded `energy_estimate: 21000` (a silent fake fallback) alongside `success: false`. LD-4 divide bug applies. |
| `get_code` | `mod.rs#L1802-L1817` | **Placeholder.** Body literally `// Placeholder implementation`, returns `GetCodeResponse { code: vec![], found: false, success: false, error_message: "Not implemented" }`. Application-level error, **not** `tonic::Status::unimplemented`. |
| `get_storage_at` | `mod.rs#L1819-L1834` | **Placeholder.** Same shape. |
| `get_nonce` | `mod.rs#L1836-L1851` | **Placeholder.** Returns `nonce: 0, found: false, success: false`. The fake `0` is rendered by the same response even though `success` is false. |
| `get_balance` | `mod.rs#L1853-L1868` | **Placeholder.** Same shape as `get_code`. |
| `create_evm_snapshot` | `mod.rs#L1870-L1884` | **Dangerous placeholder.** Returns `CreateEvmSnapshotResponse { snapshot_id: uuid::Uuid::new_v4().to_string(), success: true }`. The most dangerous fake in the tree — a plausible UUID with `success: true`. Already tracked by LD-6 §1.4. |
| `revert_to_evm_snapshot` | `mod.rs#L1886-L1899` | **Dangerous placeholder.** Returns `success: true` ignoring `request.snapshot_id`. Also LD-6 §1.4. |

No `get_code` / `get_storage_at` / `get_nonce` / `get_balance` plumbing
exists in [`rust-backend/crates/execution/src/lib.rs`](../rust-backend/crates/execution/src/lib.rs)
at all — the `EvmStateStore` trait is used internally by revm but is
not exposed as a query API that the gRPC layer can call. Implementing
the Rust side of §2.2 is therefore strictly more work than "wire a
stub to an existing function"; an explicit query façade on top of
`EngineBackedEvmStateStore` has to land first.

**Proto shapes** —
[`framework/src/main/proto/backend.proto`](../framework/src/main/proto/backend.proto):

- `GetCodeRequest` / `GetStorageAtRequest` / `GetNonceRequest` /
  `GetBalanceRequest` each carry `string snapshot_id` (L896-L939).
  LD-6 locks EVM snapshot/revert as Phase 1 unsupported, so this
  field is semantically unused today.
- Their responses (`GetCodeResponse` / `GetStorageAtResponse` /
  `GetNonceResponse` / `GetBalanceResponse`, L902-L946) all carry the
  triple `bytes|int64 value`, `bool found`, `bool success`,
  `string error_message`. This is a 3-way schema: `(found=true,
  success=true)` / `(found=false, success=true)` for not-found /
  `(success=false)` for internal error. But it collapses
  "unsupported" and "transport error" into `(success=false)`, with no
  structural distinction.
- `CallContractRequest` / `CallContractResponse` (L869-L882) and
  `EstimateEnergyRequest` / `EstimateEnergyResponse` (L884-L894)
  intentionally do **not** have a `found` field — a call to a
  non-existent address is a semantic result (empty return, zero gas),
  not a not-found error. Their only error channel is `success=false +
  error_message`.
- `CreateEvmSnapshotRequest` (L949) is empty.
  `CreateEvmSnapshotResponse` (L953) and `RevertToEvmSnapshotRequest`
  (L959) / `RevertToEvmSnapshotResponse` (L963) use only
  `success + error_message`.

**Cross-LD constraints already active:**

- **LD-4** (`energy_limit` wire semantics) already governs
  `estimate_energy`/`call_contract` correctness. Until the §1.2
  LD-4 cutover lands, Rust-side results are off by the
  `energy_fee_rate` divisor.
- **LD-5** (storage transaction semantics) already rules that
  `snapshot_id` on storage-layer read requests is silently ignored
  and this is locked behavior, not a bug. LD-8 extends the
  principle to execution-layer read requests but with a **stricter
  rule**: execution `snapshot_id`s are rejected at the gRPC layer,
  not silently ignored — see locked decision below.
- **LD-6** (snapshot semantics) rules that
  `create_evm_snapshot`/`revert_to_evm_snapshot` are Phase 1
  unsupported. LD-6's locked text at §1.4 part 2 originally allowed
  two valid implementations on the Rust side — (a) transport
  `Status::unimplemented`, or (b) application-level
  `{success: false, error_message: "EVM snapshot/revert unsupported
  in Phase 1 (LD-6)"}`, with (a) marked preferred — but LD-8 closes
  that earlier choice in favor of (a) only, to be consistent with
  the LD-8 §2 rule that all non-wired Rust RPCs return
  `Status::unimplemented` at the transport layer. LD-6 §1.4 part 2
  has already been updated in-place to lock (a) as the only valid
  behavior and to cite LD-8 §2 as the source of the tightening, so
  the two LDs are in sync. LD-8 cleanup #2 is the code flip.
- **LD-7** (contract support matrix) gates the VM contracts
  (`TriggerSmartContract` / `CreateSmartContract`) on read-path
  closure (§2.1/§2.2) for promotion to `RR canonical-ready`. LD-8
  is therefore directly on the critical path for the 13-contract
  whitelist target.

#### Locked decision

**LD-8 locks the execution read-path contract for Phase 1.** The
intent is: **never silently succeed**, **never silently return a
plausible-looking zero**, **always produce a gRPC Status or
exception a caller can distinguish from a real result**. The
policy has five parts.

**1. Java `RemoteExecutionSPI` placeholders must fail loudly.** Every
read-path method that does not yet have a real RPC-backed
implementation must return
`CompletableFuture.failedFuture(new UnsupportedOperationException(
"Remote execution read-path <method> not yet implemented (LD-8)"))`
instead of returning zero/empty/revert. All eight non-health methods
have an async signature (`CompletableFuture<T>`), so the locked
pattern is the failed-future form — matching LD-6's "completes
exceptionally" phrasing for `createSnapshot` / `revertToSnapshot`
exactly. A synchronous `throw` inside the method body would also
propagate the exception, but it crosses the sync/async boundary in a
way that breaks `.exceptionally()` / `.handle()` chains at the
caller; the failed-future form is the only locked form. This
reclassifies today's silent fakes into honest failures:

| Method | Phase 1 target behavior |
|---|---|
| `callContract` | `return CompletableFuture.failedFuture(new UnsupportedOperationException(...))`. The current `ExecutionProgramResult` revert pathway is banned because it hides the bug behind a legitimate-looking revert. |
| `estimateEnergy` | `return CompletableFuture.failedFuture(new UnsupportedOperationException(...))`. Banned: `return CompletableFuture.completedFuture(0L)`. |
| `getCode` | `return CompletableFuture.failedFuture(new UnsupportedOperationException(...))`. Banned: `return CompletableFuture.completedFuture(new byte[0])`. |
| `getStorageAt` | `return CompletableFuture.failedFuture(new UnsupportedOperationException(...))`. Banned: `return CompletableFuture.completedFuture(new byte[0])`. |
| `getNonce` | `return CompletableFuture.failedFuture(new UnsupportedOperationException(...))`. Banned: `return CompletableFuture.completedFuture(0L)`. |
| `getBalance` | `return CompletableFuture.failedFuture(new UnsupportedOperationException(...))`. Banned: `return CompletableFuture.completedFuture(new byte[0])`. |
| `createSnapshot` | `return CompletableFuture.failedFuture(new UnsupportedOperationException(...))`. Reinforces LD-6 §1.4. |
| `revertToSnapshot` | `return CompletableFuture.failedFuture(new UnsupportedOperationException(...))`. Reinforces LD-6 §1.4. |
| `healthCheck` | Keep `CompletableFuture.completedFuture(new HealthStatus(false, "<reason>"))`. **Not** a failed future — health is a probe whose contract is "report unhealthy, do not crash the caller". Locked as the one exception. |

Once a real RPC-backed implementation lands for a given method, the
`failedFuture(UnsupportedOperationException)` is replaced with a
body that invokes the gRPC stub, routes success through
`mapApplicationResponse(response, "<methodName>")` and transport
failures (caught as `RemoteExecutionTransportException` from the
refactored `ExecutionGrpcClient`, cleanup #4a) through
`mapTransportStatus(e)`. The returned `CompletableFuture` either
completes with the mapped value or completes exceptionally with the
mapped exception — it is never replaced with a `return zero`
fallback. See part 4 below for the two helpers;
`RemoteExecutionTransportException` carries the method name via
`e.getMethodName()`, so `mapTransportStatus` takes a single
argument.

**2. Rust gRPC placeholders must return `tonic::Status::unimplemented`,
not a success-shaped error response.** Every placeholder RPC in
`service/grpc/mod.rs` that currently returns an application-level
`{ success: false, error_message: "Not implemented" }` must return
`Err(tonic::Status::unimplemented("<rpc_method> not implemented in
Phase 1 (LD-8)"))` at the transport layer. The rationale:
transport-layer `UNIMPLEMENTED` is how the gRPC contract encodes
"method not wired", and the Java stub's `Status`-to-exception
mapping (part 4) depends on it. Application-layer `{success:
false}` is reserved for **semantic failures of a real
implementation** (e.g. "storage read returned an error"), not for
"the method doesn't exist yet".

Methods affected: `get_code`, `get_storage_at`, `get_nonce`,
`get_balance`, `create_evm_snapshot`, `revert_to_evm_snapshot`.
LD-6 §1.4 had allowed either `Status::unimplemented` or application
`{success: false}` for the last two (with `unimplemented` only
"preferred", not required); LD-8 closes that choice and locks the
transport-layer `Status::unimplemented` as the **only** valid
implementation for both of them too, aligned with the other four.

**3. No silent fake fallbacks on error paths.** The
`estimate_energy` error path at `mod.rs#L1716-L1800` currently
returns `energy_estimate: 21000` (a plausible Ethereum-style base
gas cost) when the underlying `execution_module` fails. LD-8 bans
this: an error response must return `energy_estimate: 0`
(explicitly documented as "no estimate, see `error_message`")
alongside `success: false`. No non-zero fallback energy estimate
is allowed. Same rule extends to any similar "plausible default"
anywhere in the read-path: `call_contract` may not return a
successful-shaped `CallContractResponse` on error, `get_*`
responses may not return a "reasonable default" value when the
underlying lookup fails.

**4. Java-side error mapping: two helpers, split by layer.** The
[current `ExecutionGrpcClient`](../framework/src/main/java/org/tron/common/client/ExecutionGrpcClient.java#L91-L261)
catches every `StatusRuntimeException` and wraps it into a generic
`RuntimeException("Remote contract call failed: ...", e)`, which
destroys the transport `Status.Code` before it ever reaches
`RemoteExecutionSPI`. LD-8 therefore cannot implement a single
"status→exception" helper against the current client surface — the
error-mapping work is split between two distinct layers:

- **Transport layer (`ExecutionGrpcClient`):** refactor the nine
  wrap-and-rethrow sites (L91–L291) to wrap every caught
  `StatusRuntimeException` into a new
  `RemoteExecutionTransportException(io.grpc.Status status, String
  methodName, Throwable cause)` that exposes
  `io.grpc.Status getStatus()` and `String getMethodName()`. This is
  the **single locked exception type** the SPI layer sees for
  transport failures; no "rethrow `StatusRuntimeException` unchanged"
  alternative is allowed, so that consumers never need to import
  `io.grpc.*`. Tracked as LD-8 cleanup #4a. Without this refactor,
  the SPI layer cannot distinguish `UNIMPLEMENTED` from `INTERNAL`,
  which collapses the whole mapping table.

- **SPI layer (`RemoteExecutionSPI`):** once the transport layer
  propagates the status via `RemoteExecutionTransportException`,
  every **non-health** read-path method (the eight methods from
  part 1's table — `callContract`, `estimateEnergy`, `getCode`,
  `getStorageAt`, `getNonce`, `getBalance`, `createSnapshot`,
  `revertToSnapshot`) routes the exception through
  `mapTransportStatus(RemoteExecutionTransportException e)` (which
  reads `e.getStatus().getCode()` and `e.getMethodName()`) and
  routes the successfully-returned response proto through a second
  helper `mapApplicationResponse(response, String methodName)`.
  **`healthCheck` is explicitly carved out** of this mapping layer —
  it has its own probe contract (part 1's "one exception" row and
  cleanup #7's `HealthResponse`→`HealthStatus` wiring) and never
  completes exceptionally, so the two helpers never see a health
  input. The two helpers have **disjoint** input types on purpose —
  they cannot
  be merged into one helper because they consume different objects
  (an exception vs a success proto).

The combined translation table is:

Both helpers return an outcome that the calling read-path method
folds into its `CompletableFuture<T>`: on the success path the value
is wrapped with `CompletableFuture.completedFuture(...)`, on any
failure path the exception is wrapped with
`CompletableFuture.failedFuture(...)`. The helpers themselves are
defined in terms of what they produce (value or exception), not how
they throw — that keeps them usable from both sync and async call
sites.

| Layer | Input | Outcome |
|---|---|---|
| Transport | `Status.UNIMPLEMENTED` | fail with `UnsupportedOperationException("Remote execution API not implemented: " + methodName)` |
| Transport | `Status.UNAVAILABLE`, `Status.DEADLINE_EXCEEDED` | fail with `RuntimeException("Remote execution transport error on " + methodName + ": " + status)` |
| Transport | `Status.INTERNAL` with no usable response | fail with `RuntimeException("Remote execution error on " + methodName + ": " + status.getDescription())` |
| Transport | `Status.NOT_FOUND` | succeed with the per-API "not found" value (empty bytes for `getCode`/`getStorageAt`/`getBalance`, `0` for `getNonce`). Only `CallContract`/`EstimateEnergy` callers may not use this branch because their response protos have no `found` field (LD-8 §7). |
| Transport | Any other `Status` | fail with `RuntimeException("Remote execution unexpected status on " + methodName + ": " + status)` |
| Application | `success=false` on `CallContract`/`EstimateEnergy`/`Get*` response | fail with `RuntimeException("Remote execution error on " + methodName + ": " + response.errorMessage)` |
| Application | `success=true, found=false` on `GetCode`/`GetStorageAt`/`GetNonce`/`GetBalance` response | succeed with the per-API "not found" value (same as Transport `NOT_FOUND`). |
| Application | `success=true, found=true` (or no `found` field) | succeed with the response value |

There is **no** fallback branch that recovers from an error by
returning a default value (e.g. no "if the Rust side is down, return
zero"). Fallback recovery is explicitly locked out in Phase 1 — the
only "recovery" is "fail and let the caller decide".

**5. `snapshot_id` rejection policy on execution read requests.**
Non-empty `snapshot_id` on `GetCodeRequest`, `GetStorageAtRequest`,
`GetNonceRequest`, or `GetBalanceRequest` must be rejected with
`tonic::Status::unimplemented("snapshot read not supported in Phase 1
(LD-6/LD-8)")`. Empty `snapshot_id` means "read current state" and
is the only permitted value. This is **stricter** than LD-5's
silent-ignore rule for storage-layer `snapshot_id` on purpose:
execution read requests are a higher-level API whose callers are
actively trying to use a feature LD-6 locked out; silently ignoring
the field would give them a wrong answer. The LD-5 silent-ignore
stays for storage-layer requests because there the `snapshot_id`
field exists only as a forward-compatibility placeholder. Document
the asymmetry in the LD-8 cleanup.

**6. Timeout normalization.** Every read-path method on the Java
side applies a default per-call deadline via
`blockingStub.withDeadlineAfter(<budget>, TimeUnit.SECONDS)` on the
generated `BackendGrpc` stub inside `ExecutionGrpcClient`. The
current `ExecutionGrpcClient` already uses a single hardcoded
`DEFAULT_DEADLINE_MS` on every method (see L78–L289); LD-8
replaces that with a per-method budget table:

| Method group | Default deadline |
|---|---|
| `getCode`, `getStorageAt`, `getNonce`, `getBalance` | 5 s |
| `callContract`, `estimateEnergy` | 30 s (matches block-budget expectation) |
| `healthCheck` | 2 s |
| `createSnapshot`, `revertToSnapshot` | N/A — fail fast before any RPC (LD-6) |

The concrete mechanism is a new `ExecutionGrpcClient` constructor
parameter or a per-method overload (e.g.
`ExecutionGrpcClient.callContract(req, Duration deadline)`) that the
SPI can use. There is no public `ExecutionGrpcClient.withDeadlineAfter`
helper today, so this is a client API addition tracked under the same
cleanup item that fixes error mapping. No method is allowed to call
the stub without a deadline.

**7. `callContract` / `estimateEnergy` do not gain a `found` field.**
LD-8 explicitly rejects the proto-shape proposal to add `bool found`
to `CallContractResponse` or `EstimateEnergyResponse`. A call to a
non-existent contract is a valid semantic result (empty return data,
zero gas consumed), not a not-found error. Keeping the shape as
`(return_data, success, error_message, energy_used)` is locked.

**8. `estimateEnergy` cross-mode comparison rule.** For §2.3
verification of `estimateEnergy` between `EE` and `RR`:
- **Before LD-4 lands**: **no cross-mode parity is expected.** The
  LD-4 bug at
  [`lib.rs#L534/L554`](../rust-backend/crates/execution/src/lib.rs#L534)
  divides `gas_limit` by `energy_fee_rate` **before** the EVM runs,
  which changes the cap **multiplicatively**. For any non-trivial
  `energy_fee_rate` (e.g. 100), a call whose real cost is near the
  cap will either halt early with an out-of-gas error or return a
  vastly-scaled estimate — not a small additive delta. LD-8
  therefore locks the pre-LD-4 interim rule as **"no exact parity
  expectation; record the observed `EE`/`RR` pair, do not alert on
  mismatch, and block any attempt to tune a tolerance window"**.
  Per-contract exception lists, tolerance windows, and bounded-delta
  assertions are all **banned** until LD-4 lands.
- **After LD-4 lands**: exact match is the target. Any delta
  triggers a regression alert. Per-contract exception list is still
  not used — deltas are either real bugs or noise that must be
  root-caused.

No per-contract exception list is sanctioned in LD-8. If a specific
contract's estimate is persistently off, the fix is to identify the
real source of the gap, not to ignore it.

#### Why these choices

- **Silent fakes are the worst failure mode.** Every current
  placeholder is either a silent-zero-return or a plausible-looking
  successful response that hides the fact that no RPC was made. A
  caller that doesn't explicitly check `success` (or doesn't get a
  `success` field at all, like `getCode`) will treat the fake result
  as real data and write it into downstream state. Fail-loud is the
  only way to keep the Phase 1 gap honest while the real
  implementations land incrementally.
- **`Status::unimplemented` vs application-level `{success: false}`
  is load-bearing.** The Java side can only correctly throw
  `UnsupportedOperationException` if the Rust side reports
  `UNIMPLEMENTED` at the transport layer. Collapsing "not wired" and
  "wired but failed" into the same `{success: false}` bucket means
  the Java mapping table cannot distinguish them, and any automated
  retry / alerting / CI coverage loses the distinction.
- **Not-found is part of the contract, so it stays.** LD-8 is
  explicit that `found=false, success=true` is a legitimate return
  for `getCode`/`getStorageAt`/`getNonce`/`getBalance` — a query on
  an EOA with no code is a normal outcome, not an error. The
  fail-loud rule applies only to the "no implementation" and
  "internal error" branches, not to not-found.
- **Fallback is banned in Phase 1.** LD-1 already de-emphasizes
  SHADOW, which was the main rationale for `RR`-to-`EE` fallback in
  earlier planning. Without SHADOW as a safety net, any fallback in
  LD-8 would silently hide `RR` regressions behind `EE` results and
  break the Phase 1 acceptance contract ("`EE`-vs-`RR` parity
  provable"). The locked decision is: if `RR` read fails, the call
  fails — no cross-mode rescue.
- **Execution `snapshot_id` rejection is stricter than storage
  `snapshot_id` silent-ignore on purpose.** Storage-layer consumers
  rarely set `snapshot_id` at all (the field is a forward-compat
  placeholder), while execution-layer consumers who set it are
  actively asking for snapshot reads. Giving them "current state"
  silently would be a worse bug than failing with
  `unimplemented`.

#### What this lock does NOT cover

- **The actual RPC wiring code for `getCode`/`getStorageAt`/
  `getNonce`/`getBalance`/`callContract`/`estimateEnergy`.** LD-8
  locks the **contract** (error semantics, timeouts, mapping, not-
  found discrimination). The implementation work — writing the
  gRPC stub calls in Java, building a query façade on top of
  `EngineBackedEvmStateStore` in Rust, handling streaming
  scenarios — is deferred under §2.1 / §2.2 cleanup.
- **EE-path implementations.** `EmbeddedExecutionSPI` is also a
  placeholder for read-path methods. LD-8 does not require EE to
  get real implementations before RR does — but §2.4 paired tests
  are explicitly restricted to "both sides fail consistently"
  until one side or the other lands real reads.
- **LD-4 `energy_limit` cutover.** LD-8 notes the dependency but
  does not duplicate the LD-4 locked decision. §2.3 verification
  items gated on LD-4 are tracked via cross-reference, not
  replicated here.
- **Retry / circuit-breaker policy.** LD-8 says "no fallback",
  which includes "no automatic retry" for Phase 1. Retry policy is
  a later-phase concern and not locked here.
- **Structured logging schema.** The audit found no shared logging
  format for read-path failures. LD-8 requires (normatively, via
  cleanup #15) that every failure path log at `warn` with the
  method name and status, but does **not** lock a specific
  structured-logging format — only the level and the required
  fields. Any later migration to a structured schema is a
  later-phase concern.

#### Mandatory cleanup (LD-8 deferred follow-ups)

Tracked as `2.x deferred follow-ups` in `close_loop.todo.md`. LD-8
itself is a doc-only lock; none of the code edits below land in this
iteration.

1. **Flip the eight non-health Java placeholders to fail-loud.**
   Replace the `CompletableFuture.completedFuture(0L)` /
   `CompletableFuture.completedFuture(new byte[0])` / revert-shaped
   `ExecutionProgramResult` bodies in `RemoteExecutionSPI.java`
   L119-L239 with `return CompletableFuture.failedFuture(new
   UnsupportedOperationException("Remote execution read-path
   <method> not yet implemented (LD-8)"))`. Signatures are
   `CompletableFuture<T>`, so the failed-future form is the only
   locked form — a synchronous `throw` would cross the sync/async
   boundary and break `.exceptionally()` chains at callers.
   `healthCheck` is the one exception — it keeps its current
   "unhealthy + message" shape per part 1. Covers all eight
   non-health methods: `callContract`, `estimateEnergy`, `getCode`,
   `getStorageAt`, `getNonce`, `getBalance`, plus the LD-6 items
   `createSnapshot` / `revertToSnapshot`.
2. **Flip the four Rust gRPC placeholders to `Status::unimplemented`.**
   `mod.rs#L1802-L1868` (`get_code`, `get_storage_at`, `get_nonce`,
   `get_balance`) must return `Err(Status::unimplemented(...))`.
   The LD-6 items `create_evm_snapshot` / `revert_to_evm_snapshot`
   at `mod.rs#L1870-L1899` are covered by the same sweep.
3. **Remove the `estimate_energy` fake 21000 fallback.** On the
   error path in `mod.rs#L1716-L1800`, set `energy_estimate: 0`
   (or return `Err(Status::internal(...))` if the error is not
   semantic) alongside `success: false`. Never 21000.
4a. **Refactor `ExecutionGrpcClient` transport-error handling.**
    The nine methods at
    [`ExecutionGrpcClient.java#L91-L291`](../framework/src/main/java/org/tron/common/client/ExecutionGrpcClient.java#L91)
    currently `catch StatusRuntimeException` and rethrow as a
    generic `RuntimeException("Remote ... failed: " + msg, e)`,
    which strips `io.grpc.Status.Code` before the SPI layer can
    branch on it. Replace the wrap-and-rethrow with a new checked-or-
    unchecked exception `RemoteExecutionTransportException(io.grpc.Status
    status, String methodName, Throwable cause)` that exposes
    `io.grpc.Status getStatus()` and `String getMethodName()`. This
    is the **single locked exception type** the SPI layer sees for
    transport failures (alternative "rethrow `StatusRuntimeException`
    unchanged" is NOT allowed — it forces every caller to import
    `io.grpc.*` and is inconsistent with the rest of the framework's
    exception hierarchy). Same refactor adds the per-method deadline
    overloads used by cleanup #5.
4b. **Add SPI-layer mapping helpers.** Two helpers in
    `framework/.../execution/spi/RemoteExecutionStatusMapper.java`:
    `mapTransportStatus(RemoteExecutionTransportException e)`
    implementing the transport rows of the locked decision part 4
    table (reads `e.getStatus().getCode()` and `e.getMethodName()`),
    and `mapApplicationResponse(response, String methodName)`
    implementing the application rows. The two helpers do not share
    input types — they cannot be merged. Use both in every read-path
    method in `RemoteExecutionSPI` once cleanup 4a and the real
    wiring land.
5. **Add per-method default deadlines** to every read-path call.
   The current `ExecutionGrpcClient` applies a single hardcoded
   `DEFAULT_DEADLINE_MS` via `blockingStub.withDeadlineAfter(...)`
   on every method (L78–L289); LD-8 replaces that with the
   budget table in locked decision part 6. There is **no public
   `ExecutionGrpcClient.withDeadlineAfter` helper** today, so this
   cleanup adds per-method overloads (e.g.
   `callContract(CallContractRequest, Duration deadline)`) or a
   client constructor parameter carrying the budget table.
   Deadline can be overridden per call but defaults apply
   otherwise. Bundles with cleanup 4a.
6. **Enforce `snapshot_id` rejection** in the four Rust read
   handlers: if `request.snapshot_id` is non-empty, return
   `Err(Status::unimplemented("snapshot read not supported in Phase 1
   (LD-6/LD-8)"))`. Document the asymmetry with LD-5 (which keeps
   silent-ignore for storage-layer `snapshot_id`).
7. **Wire `RemoteExecutionSPI.healthCheck`** through
   [`ExecutionGrpcClient.healthCheck()`](../framework/src/main/java/org/tron/common/client/ExecutionGrpcClient.java#L261)
   (the existing method name — not `health()`). Map the Rust
   [`HealthResponse`](../framework/src/main/proto/backend.proto#L76-L85)
   — which has fields `HealthResponse.Status status` (enum: `HEALTHY`
   / `UNHEALTHY` / `DEGRADED`), `string message`, and
   `map<string, string> module_status` — to Java's boolean-only
   `HealthStatus(boolean healthy, String message)` using this rule:
   - `HEALTHY` → `HealthStatus(true, response.getMessage())`
   - `DEGRADED` → `HealthStatus(false, "degraded: " +
     response.getMessage() + " - " + response.getModuleStatusMap())`
     (serialize the module map compactly into the message string so
     the single Java `message` field carries per-module nuance).
   - `UNHEALTHY` / any other → `HealthStatus(false, response.getMessage())`

   The `DEGRADED` case collapses to `healthy=false` because the
   Java surface is binary; the message carries the per-module
   nuance from `module_status`. Rust side is already real
   (aggregates `module_manager.health_all()` at
   `service/grpc/mod.rs#L98-L133`); this is a Java-only follow-up.
8. **Build the Rust query façade** on top of
   `EngineBackedEvmStateStore` for `get_code`/`get_storage_at`/
   `get_nonce`/`get_balance`. This is the core §2.2 implementation
   work — no `EvmStateStore` query API exists at the gRPC layer
   today, so a thin read-only façade has to land before the four
   RPC methods can return real data.
9. **Add Javadoc to `ExecutionSPI.java` interface** documenting
   the LD-8 Phase 1 failure semantics: every **non-health**
   read-path method (the eight methods listed in part 1) returns a
   `CompletableFuture` that may complete exceptionally with
   `UnsupportedOperationException("<message> (LD-8)")` for as long
   as the remote implementation is not wired, and callers must not
   silently absorb the exceptional completion (e.g. no
   unconditional `.exceptionally(e -> defaultValue)`).
   `healthCheck` is explicitly carved out: its Javadoc documents
   the probe contract (always `completedFuture(HealthStatus)`, with
   `healthy=false` on failure, never a failed future). Bundles
   with the LD-6 §1.4 Javadoc follow-up for `createSnapshot` /
   `revertToSnapshot`.
10. **Add focused Java tests** that assert each of the eight
    **non-health** placeholder methods returns a
    `CompletableFuture` that completes exceptionally with
    `UnsupportedOperationException` until real wiring lands (e.g.
    `assertThatThrownBy(() -> future.get()).hasCauseInstanceOf(
    UnsupportedOperationException.class)`). Add a separate
    `healthCheck` test that asserts the placeholder returns
    `completedFuture(new HealthStatus(false, "<reason>"))` —
    **not** a failed future — to lock the probe-contract carve-out
    from part 1. These are negative tests — the positive tests
    come in §2.4 after the real implementations land.
11. **Add Rust gRPC tests** that assert each placeholder RPC
    returns `Status::unimplemented`. Also a negative test, for
    the same reason.
12. **Refresh §2.4 paired `EE`-vs-`RR` test plan** to reflect that
    EE read methods are also placeholders: until at least one side
    has a real implementation, paired tests can only assert
    "both sides fail in the same way".
13. **Cross-link LD-8 with LD-4** in `close_loop.todo.md` §1.2
    deferred follow-ups — the LD-4 energy cutover is a hard
    prerequisite for the `estimateEnergy` exact-match target
    state in locked decision part 8. **Done as part of this
    doc-only iteration** — the §1.2 "LD-4 ↔ LD-8 cross-link"
    bullet now captures this relationship.
14. **Remove `// TODO: Implement in Task 2` comments** after
    follow-ups #1 / #2 land; they are stale once the fail-loud
    flip happens.
15. **Add normative `warn`-level failure logging** on every
    read-path failure path in both Java and Rust. The "Why these
    choices" subsection above requires `warn` logs with method
    name + status, but that behavior does not fall out of
    cleanups #1–#14 automatically. Java: the SPI layer's
    `mapTransportStatus` and `mapApplicationResponse` helpers
    (cleanup #4b) log at `warn` before throwing, including the
    method name and the raw `Status.Code` (or `error_message`).
    Rust: every placeholder RPC and every real RPC's error path
    logs at `warn` with the RPC method name and the returned
    `tonic::Status` or application error. No structured-logging
    format is locked — only the level and the required fields.

These cleanups are sequenced after LD-6 code flips land (LD-6
§1.4 cleanups #1 and #2 are the natural prefix for LD-8 cleanup
#1 and #2).

### LD-9: Storage Semantic Hardening Scope for Phase 1

**Scope.** The §3 workstream ("Storage semantic hardening") in
`close_loop.todo.md` covers four subsections — §3.1 `transaction_id`
end-to-end plumbing, §3.2 transaction buffer semantics in Rust
storage, §3.3 snapshot correctness, and §3.4 storage tests and
`EE/RR` comparison checks. LD-5 and LD-6 already locked the Phase 1
treatment of transactions and snapshots as "structural placeholder"
and "explicitly unsupported" respectively. LD-9's job is to (a)
make that inheritance explicit, (b) audit which §3 tasks are
actually doable in Phase 1 versus already deferred, (c) lock
**§3.4 tests as the only Phase 1 storage hardening deliverable**
— with §3.4 itself narrowed to the subset that LD-5/LD-6 actually
leave testable — and (d) make two new decisions of its own: the
storage-layer `getFromSnapshot` option that LD-6 §1.4 left open
(LD-9 picks option (b) live-read + loud warn), and the normative
rule that `DualStorageModeIntegrationTest`-only tests do not count
toward §3.4 acceptance.

#### Audit: what §3 tasks are doable in Phase 1?

**§3.1 `transaction_id` end-to-end plumbing** — six tasks. LD-5
`#1` locks Phase 1 required semantics as "none". LD-5 "What this
lock does NOT cover" explicitly points at §3.1 / §3.2 as "deferred
to a future phase (almost certainly the block importer phase)"
and says LD-5 "does not require it for Phase 1 acceptance". Of
the six tasks, four are implementation work (audit, define
ownership, plumb through Java writes, branch Rust handlers) and
inherit LD-5's deferral wholesale. The remaining two tasks
("Document default behavior for non-transaction-scoped writes"
and "Add tracing/logging that makes it obvious whether a write
was transactional or direct") are **already covered by LD-5
§1.3 cleanups**: cleanup #1 (`backend.proto` field-level
comment on `PutRequest.transaction_id` / `DeleteRequest.transaction_id`
/ `BatchWriteRequest.transaction_id` — "silently ignored, writes
always go directly to the durable store") discharges the
documentation task, and cleanup #5 (gRPC handler honesty warn
when a non-empty `transaction_id` arrives) discharges the
tracing/logging task. LD-9 marks both as `[x]` with an explicit
LD-5 §1.3 cross-reference and does not open new work for them.

**§3.2 Transaction buffer semantics in Rust storage** — nine
tasks. Same inheritance: LD-5 `#1`/`#4`/`#6` reject real per-tx
buffering, per-tx routing, atomic commit, buffered rollback,
read-your-writes, layered reads over buffers, and tx-scoped
iterators as Phase 1 concerns. Six of the nine tasks are the
implementation rows (buffer, put routing, delete routing,
batch_write routing, atomic commit, rollback discard) and inherit
LD-5's deferral. The remaining three rows are decision-flavored
and **already answered by LD-5 #4**: "decide read-your-writes
behavior" → not supported; "if read-your-writes is required,
design layered read behavior" → vacuously discharged because the
predicate is false; "decide whether transaction-scoped iterators
are in scope" → explicitly unsupported. LD-9 marks all three as
`[x]` against LD-5 #4.

**§3.3 Snapshot correctness** — six tasks (counting the four
sub-bullets on lifecycle as one item each). LD-6 `#2`/`#3`
locked "EVM snapshot in Phase 1 = explicitly unsupported in RR"
and "EVM snapshot/revert is not built on storage snapshot in
Phase 1" — storage snapshot for PIT reads is likewise deferred.
Task-by-task:

| §3.3 task | Phase 1 status |
|---|---|
| Replace current "snapshot reads current DB" behavior with real point-in-time semantics | **Deferred** (LD-6 "What this lock does NOT cover": "Real per-RocksDB-handle snapshot implementation is deferred to the block importer phase"). |
| If real snapshot is not implemented this phase, remove fake behavior and surface explicit unsupported | **Partly locked by LD-6 §1.4 cleanup** — `create_evm_snapshot` / `revert_to_evm_snapshot` flip to `Status::unimplemented`. **LD-9 makes a new choice** for the storage-layer `getFromSnapshot` disjunction that LD-6 §1.4 left open: pick option (b) live-read + loud `tracing::warn!` **on every call** (not first-call-only) cross-referencing LD-6, not option (a) `Status::unimplemented`. Justification: `EmbeddedStorageSPI.getFromSnapshot` and the existing `StorageSPIIntegrationTest` round-trip assume the live-read fallback compiles, and flipping to `unimplemented` would break the embedded path without any real caller needing it. LD-9 does not open a new cleanup item; the LD-6 §1.4 cleanup path is the single work stream, now specialized to option (b). |
| Define snapshot lifecycle (creation / read paths / deletion / cleanup on shutdown) | **Deferred**. Lifecycle only matters for a real implementation; Phase 1 has none. |
| Define interaction rules between transactions and snapshots | **Trivially answered** (LD-5 / LD-6 inheritance). No transactions + no snapshots = no interaction to define. Marked `[x]` in the todo because the decision is discharged, not because any work landed. |
| Decide whether iterator APIs against snapshot are needed now or later | **Locked as "later"** — LD-6 defers PIT snapshot entirely, so iterator-over-snapshot is trivially out of scope. |

**§3.4 Storage tests and `EE/RR` comparison checks** — the only
§3 subsection with Phase 1 deliverables. LD-9 splits §3.4 into
two halves: tests that LD-5/LD-6 permit, and tests that LD-5/LD-6
make impossible (because the feature under test does not exist).

| §3.4 Rust task | LD-9 status | Why |
|---|---|---|
| Add unit tests for CRUD | **Phase 1 actionable** | CRUD exists in the current storage engine; no LD-5/LD-6 dependency. |
| Add unit tests for batch writes | **Phase 1 actionable** | Same — batch write is a current real feature. |
| Add unit tests for transaction commit | **Blocked — reframed**: add tests that assert `commit_transaction` is a **loud no-op** (per LD-5 cleanup #4: Rust engine logs `tracing::warn!` and returns success without persisting a real tx buffer). The positive test for "commit persists the buffer" is deferred to the block importer phase along with LD-5 item #1. |
| Add unit tests for transaction rollback | **Blocked — reframed**: same. Add tests that assert `rollback_transaction` is a loud no-op. Positive rollback tests are deferred. |
| Add unit tests for snapshot correctness | **Blocked — reframed**: add tests that assert `get_from_snapshot` reads live and emits a `tracing::warn!` **on every call** (not first-call-only) tagged with the LD-6/LD-9 cross-reference. LD-9 locks option (b) (live-read + warn on every call); option (a) `Status::unimplemented` is no longer open. Positive PIT tests are deferred. |
| Add tests for absent `transaction_id` | **Phase 1 actionable** — LD-5 #5 locks "absent `transaction_id` = direct write to durable store". Test asserts that. |
| Add tests for transaction not found / snapshot not found | **Phase 1 actionable** — `engine.rs:406` / `engine.rs:431` already fail on unknown `transaction_id`, `engine.rs:465` already fails on unknown `snapshot_id`, and `StorageSPIIntegrationTest.java:307` already asserts invalid-snapshot failure. LD-9 keeps this row actionable: add symmetric unit coverage on the Rust side so the failure path is locked in before LD-5/LD-6 real semantics land. |
| Add tests for concurrent transaction IDs and cleanup paths | **Blocked** — no transaction state exists to concurrently access. Phase 1 skips this entirely; re-opens with the block importer phase. |

| §3.4 Java task | LD-9 status | Why |
|---|---|---|
| Extend or add integration coverage around `RemoteStorageSPI` | **Phase 1 actionable** — covers CRUD/batch against a real Rust storage service. |
| Add tests proving Java actually carries `transaction_id` into remote writes | **Blocked — reframed**: LD-5 locks "absent `transaction_id` = direct write". Test should assert Java **does not** carry `transaction_id` in Phase 1 (the field stays zero/empty on the wire), and Rust silently ignores any legacy value. Positive carry-through is deferred. |
| Add `EE` run vs `RR` run semantic checks where possible | **Phase 1 actionable** — for CRUD/batch only. LD-9 notes that the §3.4 `EE`-vs-`RR` surface is narrower than §2.4's because storage has no execution semantics to diverge on; parity is mostly "both return the same bytes for the same key". |
| Avoid using `DualStorageModeIntegrationTest` as if mode-switch wiring alone proves semantic parity | **Phase 1 actionable** — this is a doc/review discipline item, not a code change. LD-9 promotes it to a normative rule: any new test that only proves "mode switch compiles" must be labelled as such and must not be counted toward §3.4 acceptance. |

| §3.4 acceptance | LD-9 status |
|---|---|
| Storage transaction APIs are no longer structural placeholders | **Deferred** (LD-5). Cannot be ticked in Phase 1. |
| Snapshot is real, explicitly fail-loud, or loud-degrade — never silently fake (LD-6 + LD-9 two-halves) | **Answered by LD-6 + LD-9** — the Phase 1 contract has two halves: (1) EVM snapshot/revert is **explicitly unavailable** (`RemoteExecutionSPI` returns `CompletableFuture.failedFuture(UnsupportedOperationException)`; Rust gRPC returns `Status::unimplemented`); (2) storage `getFromSnapshot` is **loud degrade to live-read + warn** per LD-9's lock of LD-6 §1.4 option (b). LD-9 treats both halves together as "explicitly not a real snapshot, and every call is loud about it" — the acceptance gate ticks when the LD-6 §1.4 cleanups land (EVM fail-loud flip + storage every-call warn). Not when §3.3 real PIT implementation lands. |
| Storage crate test suite has meaningful coverage and is no longer `0 tests` | **Phase 1 actionable** — the **single storage-crate acceptance gate LD-9 keeps open**. CRUD + batch + placeholder-honesty tests are enough to clear this one. |

#### Locked decision

**LD-9 locks the Phase 1 scope of §3 as follows.**

1. **§3.1 and §3.2 inherit LD-5's deferral wholesale for
   implementation work** and are moved into a §3.1/§3.2 deferred
   follow-ups block in `close_loop.todo.md` with an explicit
   LD-5/LD-9 cross-reference. Most of the fifteen tasks (six in
   §3.1, nine in §3.2) remain `[ ]` in the progress tracker
   because nothing is implemented. Exceptions that **do** get
   `[x]` in the todo because the decision or the work is already
   captured elsewhere:
   - **§3.1 task 5** ("Document default behavior for non-
     transaction-scoped writes") is covered by LD-5 §1.3 cleanup
     #1 (`backend.proto` field-level comment on
     `PutRequest.transaction_id` etc.) plus LD-5 #5. Marked `[x]`
     in the todo with an LD-5 §1.3 cross-reference.
   - **§3.1 task 6** ("Add tracing/logging that makes it obvious
     whether a write was transactional or direct") is covered by
     LD-5 §1.3 cleanup #5 (gRPC handler honesty warn). Marked
     `[x]` in the todo with an LD-5 §1.3 cross-reference.
   - **§3.2 tasks 7, 8, 9** (the three decision-flavored rows:
     "Decide read-your-writes", "If read-your-writes is required,
     design layered read behavior", "Decide whether transaction-
     scoped iterators are in scope") are answered by LD-5 #4:
     read-your-writes and tx-scoped iterators are explicitly
     unsupported in Phase 1, so the "design layered read behavior"
     sub-task is vacuously discharged. All three get `[x]` in
     the todo as "decided by LD-5 #4".

   No Phase 1 implementation work is opened against any of the
   fifteen §3.1/§3.2 tasks; the `[x]` exceptions above are
   decision or documentation ticks, not implementation ticks.

2. **§3.3 inherits LD-6's deferral wholesale for the
   implementation tasks**, inherits LD-6 §1.4 cleanups for the
   EVM-side "fail-loud" tasks, and **makes a new choice on the
   open storage-layer disjunction**. Specifically:
   - The "real PIT snapshot" task and the lifecycle sub-tasks are
     deferred to the block importer phase (stay `[ ]` in the todo).
     The "interaction with transactions" row and the "snapshot
     iterators" row are **trivially answered** (no transactions +
     no snapshots = no interaction to define; iterator-over-
     snapshot is trivially out of scope) and are marked `[x]` in
     the todo as decision ticks, not implementation ticks.
   - The EVM-side "remove fake behavior and surface explicit
     unsupported" work (`create_evm_snapshot` /
     `revert_to_evm_snapshot` flip to `Status::unimplemented`)
     is already tracked by LD-6 §1.4 cleanup. LD-9 does not open
     a new cleanup item for the EVM half.
   - The storage-side `getFromSnapshot` disjunction that LD-6
     §1.4 left open — option (a) `Status::unimplemented` vs
     option (b) live-read + loud `tracing::warn!` — is **newly
     decided by LD-9 in favor of option (b)**. Justification:
     Java `EmbeddedStorageSPI.getFromSnapshot` and the existing
     `StorageSPIIntegrationTest` round-trip assume a live-read
     fallback is callable; flipping to `unimplemented` breaks
     the embedded path without any real caller needing it. LD-9
     still does not open a new cleanup item for the flip itself
     — the existing LD-6 §1.4 cleanup path is the single work
     stream, now specialized to option (b).

3. **§3.4 is the only Phase 1 storage hardening deliverable**,
   and its scope is narrowed to the rows marked "Phase 1
   actionable" or "Blocked — reframed" in the audit above. The
   "Blocked" rows (concurrent transaction IDs, positive PIT
   tests, real tx commit/rollback semantics, positive
   `transaction_id` carry-through) are deferred to the block
   importer phase along with §3.1/§3.2. The "Blocked — reframed"
   rows are kept in Phase 1 but rewritten to assert placeholder
   honesty (commit is a loud no-op, rollback is a loud no-op,
   `get_from_snapshot` is live-read + warn per LD-9's choice of
   LD-6 option (b)) instead of asserting real semantics. The
   "transaction not found / snapshot not found" row is **not**
   reframed — LD-9 confirms it as Phase 1 actionable because
   `engine.rs` already fails on unknown IDs and the Java
   integration test already asserts invalid-snapshot failure.

4. **The "`DualStorageModeIntegrationTest` is not a semantic
   parity proof" caveat is normative.** LD-9 promotes it from a
   reviewer discipline item to a locked rule: any new `§3.4`
   test that demonstrates only "mode switch compiles" does **not**
   count toward the storage-crate coverage acceptance gate. The
   gate requires tests that actually exercise read/write/batch
   semantics against a live Rust storage engine, not just factory
   wiring.

5. **Only one §3 acceptance gate is open for Phase 1 work.** The
   only Phase 1 acceptance criterion LD-9 treats as open-for-work
   is "Storage crate test suite has meaningful coverage and is no
   longer `0 tests`". The other two acceptance criteria ("no
   longer structural placeholders" / "snapshot is real, explicitly
   fail-loud, or loud-degrade — never silently fake") stay `[ ]`
   in the todo but are annotated as deferred-by-LD-5 /
   decided-by-LD-6 (with LD-9 refinement) so reviewers can tell at
   a glance that Phase 1 is not allowed to tick them.
   Using `[x]` for "deferred" or "decided elsewhere" was
   considered and rejected because it reads as "implemented".

#### Why these choices

- **Mostly inheritance, with two minimum new decisions.** LD-5
  and LD-6 already did the "defer vs implement" work for
  transactions and snapshots. LD-9's primary contribution is
  making that inheritance explicit in the §3 tracking surface so
  nobody re-opens a deferred task or blocks on a deferred
  acceptance gate. LD-9 adds exactly two new decisions of its
  own, deliberately kept small: (1) picking LD-6 §1.4 option (b)
  (live-read + loud warn) for storage `getFromSnapshot` because
  `EmbeddedStorageSPI.getFromSnapshot` round-trip tests require
  the fallback to compile, and (2) promoting the
  "`DualStorageModeIntegrationTest` is not a semantic parity
  proof" caveat to a normative rule so mode-switch-only tests
  cannot be counted against the §3.4 acceptance gate. Copying
  LD-5/LD-6's reasoning wholesale into a new LD would be
  duplication; these two new decisions are the irreducible
  residue.
- **The §3.4 narrowing is necessary because the "ideal" §3.4 was
  written before LD-5/LD-6 landed.** The original §3.4 test list
  assumed real transactions and real snapshots would exist. Under
  LD-5/LD-6 they don't, so every test that asserts real semantics
  either reframes as a placeholder-honesty test or defers
  outright. LD-9 does the reframe in one pass so the implementer
  doesn't have to re-derive it per task.
- **Storage parity is genuinely narrower than execution parity.**
  §2.4 (LD-8) required an elaborate `EE`-vs-`RR` comparison surface
  because execution has rich semantic state (AEXT, receipts,
  state_changes, energy accounting). Storage just returns bytes
  for keys. The §3.4 `EE`-vs-`RR` surface is correspondingly
  smaller, and LD-9 acknowledges that instead of implying §3.4
  should match §2.4's depth.
- **Keeping `DualStorageModeIntegrationTest` labelled prevents
  false acceptance progress.** The CLAUDE.md lesson about dual
  storage mode factory integration notes that earlier work landed
  mode-switch factory plumbing without actual semantic testing.
  LD-9 locks the distinction so the same mistake can't be made
  again during Phase 1 test work.
- **One open acceptance gate simplifies the dashboard.** Having
  three acceptance gates (with two already answered) makes the
  §3 status unclear on the readiness dashboard. LD-9 marks the
  two answered gates and leaves one open so Phase 1 storage
  status is a single bit instead of three tangled ones.

#### What this lock does NOT cover

- **Storage read-path performance work.** Any LD-9 follow-up does
  **not** include RocksDB tuning, bloom filter changes, or
  column-family reorganization. Those are deferred to a dedicated
  performance pass after Phase 1 correctness is locked.
- **Storage error-handling hardening** (e.g. `RocksdbStorage`
  Java-side NPE guards). Handled separately under the §6
  verification workstream, not as a §3 item.
- **`transaction_id` and `snapshot_id` proto field removal.**
  LD-5 and LD-6 explicitly kept the fields on the wire for
  forward compatibility with the block importer phase. LD-9
  reaffirms: the fields stay.
- **Test framework selection** (Rust `#[test]` vs `cargo nextest`,
  JUnit 4 vs JUnit 5, testcontainers vs in-process). Out of
  scope; use whatever the existing crate / module uses.

#### Mandatory cleanup (LD-9 deferred follow-ups)

Tracked as `3.x deferred follow-ups` in `close_loop.todo.md`. LD-9
itself is a doc-only lock; none of the code / test edits below
land in this iteration.

1. **Reorganize §3.1 and §3.2** in `close_loop.todo.md` into a
   single "§3.1/§3.2 deferred under LD-5/LD-9 — block importer
   phase" subsection. Most tasks stay `[ ]` with an explicit
   LD-5/LD-9 cross-reference because no implementation landed. The
   five exceptions that **do** get `[x]` (doc/decision ticks, not
   implementation ticks): **§3.1 task 5** (documentation — covered
   by LD-5 §1.3 cleanup #1), **§3.1 task 6** (tracing/logging —
   covered by LD-5 §1.3 cleanup #5), and **§3.2 tasks 7/8/9** (the
   three decision-flavored rows answered by LD-5 #4: read-your-
   writes = not supported, layered-read design = vacuous, tx-scoped
   iterators = explicitly unsupported). All five carry an explicit
   cross-reference; none of them represents implementation work.
2. **Reorganize §3.3** in `close_loop.todo.md` similarly. The
   "remove fake behavior and surface explicit unsupported" task
   explicitly delegates to LD-6 §1.4 cleanup path (no new cleanup
   item). The real-implementation tasks defer to block importer
   phase with LD-6/LD-9 cross-reference.
3. **Rewrite §3.4 task list** along the Phase-1-actionable /
   Blocked-reframed / Blocked split from the audit above. Phase-1-
   actionable tasks stay as-is. Blocked-reframed tasks get their
   target behavior rewritten to assert placeholder honesty
   (commit/rollback are loud no-ops; `get_from_snapshot` is
   option (b) live-read + warn). Purely blocked tasks move to a
   §3.4 deferred follow-ups block.
4. **Mark §3.4 acceptance gates individually.** All three gates
   stay `[ ]` in the todo. The "structural placeholders" gate
   carries a `— Deferred by LD-5 (LD-9 inheritance). Stays [ ]:
   Phase 1 explicitly cannot tick this.` annotation. The
   snapshot gate is rewritten in two halves to match LD-9's new
   lock: EVM snapshot/revert is hard-unsupported (fail loudly via
   the LD-6 error path), and storage `getFromSnapshot` is a loud
   degrade to live-read with `tracing::warn!` on every call (not
   first-call-only). Ticks when the LD-6 §1.4 cleanups land. The
   "storage crate test suite" gate stays `[ ]` as the only Phase 1
   open-for-work gate. `[x]` is deliberately not used for
   deferred/decided-elsewhere rows because `[x]` reads as
   "implemented".
5. **Add a CI / grep lint for `DualStorageModeIntegrationTest`
   misuse.** Any new test file in `framework/src/test/.../storage/`
   that references `DualStorageModeIntegrationTest` without
   also exercising a real `RemoteStorageSPI` CRUD path gets
   flagged at review time. LD-9 does not lock a mechanism (grep,
   Checkstyle, or JUnit `@Tag`); only the normative rule.
6. **`RocksdbStorage` Java-side null-parameter guard tests.** LD-9
   notes these exist as `CLAUDE.md` lessons-learned items (gRPC
   parameter validation, embedded storage defensive checks) but
   tracks them as §6 (verification) work, not §3 (storage
   hardening). No action in LD-9 — this cleanup is a pointer to
   ensure they don't fall between §3 and §6.

These cleanups are doc-only reorganization plus one CI/lint item;
no Rust or Java code changes are required by LD-9.

### LD-10: Config and Feature-Flag Convergence Scope for Phase 1

**Scope.** `close_loop.todo.md` §5.3 (Config and feature-flag
convergence). LD-10 is a doc-only pointer-lock: it does not add
new runtime behavior and does not flip any flag. It exists only
to make §5.3's inheritance from LD-2, LD-3, and LD-7 explicit,
to lock the classification rule for the "orphan" flags that no
prior LD touches, and to enumerate the deviations between the
checked-in `rust-backend/config.toml` and LD-3 Profile A so
§5.3 acceptance has an unambiguous gate condition.

**Why this is mostly inheritance.** Three prior LDs already own
most of the §5.3 surface:

- **LD-2** locks `execution.remote.rust_persist_enabled`: the
  only permitted values, the Profile A override, and the
  cleanup sequence. §5.3 inherits LD-2 entirely for this flag.
- **LD-3** defines Profile A (safe `RR` parity profile) and
  Profile B (experimental / conformance isolation) as the
  **only two recognized** configuration profiles. §5.3's
  "produce one recommended conservative config" and "produce
  one experimental config" deliverables are literally Profile
  A and Profile B; LD-10 does not re-derive them.
- **LD-7** classifies every per-contract `*_enabled` flag via
  the contract support matrix (four buckets: `EE` only, `RR`
  blocked, `RR` candidate, `RR` canonical-ready). §5.3's
  "mark each flag as (`EE` baseline only / `RR` experimental /
  `RR` canonical-ready / legacy)" requirement is satisfied by
  LD-7 for per-contract flags; LD-10 only handles the
  **non-per-contract orphan flags** LD-7 left out.

LD-10's **new** contribution is therefore narrow: (a) lock the
§5.3 classification rule (target state = LD-3 Profile A; any
deviation is a tracked §5.3 follow-up); (b) enumerate the
orphan flags that LD-2/LD-3/LD-7 do not cover and classify each;
(c) lock the §5.3 acceptance gate condition ("`config.toml`
matches Profile A modulo the one remaining tracked deviation
(`market_strict_index_parity`) — the LD-2 `rust_persist_enabled`
gap has been closed by the §1.1 follow-up #1 flip"); (d)
cross-link `config.toml` deviations back to their owning LD so
future readers can trace every override.

#### Orphan-flag audit

These `rust-backend/config.toml` flags are **not** covered by
LD-2, LD-3 Profile A override list, or LD-7 per-contract
classification, and therefore need a §5.3-owned classification.
Each row carries the config.toml value, the `config.rs`
code-default, and LD-10's classification.

| Flag | `config.toml` | Code default | LD-10 classification |
|---|---|---|---|
| `execution.remote.emit_storage_changes` | `false` | `false` | **`RR` experimental** — feature-flagged CSV-parity escape hatch. Kept at code default; Profile A does not override. |
| `execution.remote.vote_witness_seed_old_from_account` | `true` | `true` | **`RR` canonical-ready** — already at canonical value; matches embedded semantics. Not in Profile A because the code default is already correct. |
| `execution.remote.market_strict_index_parity` | `true` | `false` | **Deviation from code default; RR experimental until §5.2 sidecar parity lands.** This override is not in LD-3 Profile A. LD-10 cleanup #1 tracks the decision: either fold it into Profile A (if it turns out to be required for any whitelist-target parity) or flip the code default to `true` (if it is actually safe). No Phase 1 behavior change required today. |
| `execution.remote.genesis_block_timestamp` | `1529891469000` | `1529891469000` (TRON mainnet) | **Deployment parameter, not a feature flag.** Classified as "environment data" rather than any of the four LD-7 buckets. Document-only note in LD-10; no cleanup. |
| `execution.remote.delegation_reward_enabled` | `true` | `false` | **Deprecated per inline comment** (`config.toml#L144-L146`): "delegation reward is now always computed when `CHANGE_DELEGATION` dynamic property is enabled ... This field is kept for backward config compatibility but has no effect." LD-10 cleanup #2 tracks the removal: delete the flag from `config.rs`, drop the override line, drop the `RemoteExecutionConfig` field. |
| `execution.remote.strict_dynamic_properties` | `true` | `false` | **Already Profile A.** Listed here only to note that LD-3 Profile A covers it. No LD-10 action. |
| `execution.evm_eth_coinbase_compat` | `false` | `false` | **`RR` canonical-ready** — TRON parity default; the `true` value exists only as a rollback escape hatch. Not in Profile A because the code default is already correct. |
| `execution.fees.mode` | `"blackhole"` | `"blackhole"` | **`RR` canonical-ready at the default.** Profile A is implicit. |
| `execution.fees.support_black_hole_optimization` | `false` | `false` | **`RR` canonical-ready at the default.** Profile A is implicit. |
| `execution.fees.blackhole_address_base58` | `"TLsV…"` | `"TLsV…"` | **Deployment parameter** (TRON mainnet blackhole address). Same classification as `genesis_block_timestamp`. |
| `execution.fees.experimental_vm_blackhole_credit` | `false` | `false` | **`RR` experimental; stays off.** The inline comment already documents that it should remain off by default. Profile A does not override. |
| `[genesis]` section | `enabled = true` + blackhole seed account | `enabled = false` | **Profile B / test parity seed.** This section is a Rust-side genesis-state seed for test runs where the Rust storage starts without a Java-supplied genesis. LD-10 classifies the entire `[genesis]` section as "Profile B territory, intentionally kept on in `config.toml` because the checked-in profile is used for EE-vs-RR parity runs that need a pre-seeded blackhole balance." See `CLAUDE.md` genesis-account lesson for the rationale. |

#### Locked decision

**LD-10 locks the Phase 1 scope of §5.3 as follows.**

1. **§5.3 inherits its main deliverables from LD-2, LD-3, and
   LD-7.** The "two profiles" deliverable is **LD-3 Profile A
   (recommended) + Profile B (experimental)**. The per-contract
   classification is **LD-7**. The `rust_persist_enabled` audit
   is **LD-2**. LD-10 does not re-derive any of these.
2. **Phase 1 `RR` canonical-ready config surface = LD-3 Profile A
   overrides + LD-7 whitelist-target per-contract flags + the
   `execution.fees` block at its code default + the orphan flags
   classified as "`RR` canonical-ready" above.** Anything set
   differently in `config.toml` is either a tracked LD-2 gap, a
   Profile B deviation, or a §5.3 cleanup follow-up. No fourth
   category is allowed.
3. **Orphan-flag classification is locked by the audit table
   above.** Every flag in the `[execution.remote]`,
   `[execution.fees]`, and `[genesis]` sections of
   `rust-backend/config.toml` that is not already owned by
   LD-2/LD-3/LD-7 has a classification row in the table. Future
   additions to these sections must extend the table in LD-10
   before merging; LD-10 cleanup #4 adds the CI/review rule.
4. **§5.3 acceptance gate = "`config.toml` matches LD-3 Profile A
   modulo tracked gaps".** At the LD-10 freeze point, the two
   tracked gaps were (a) `rust_persist_enabled = true` (LD-2 /
   §1.1 deferred follow-up #1) and (b) `market_strict_index_parity
   = true` (LD-10 cleanup #1). Gap (a) has since been closed —
   `rust-backend/config.toml` has been flipped to
   `rust_persist_enabled = false`, matching the code default and
   the LD-2 lock. The one remaining tracked deviation is
   `market_strict_index_parity = true`. The gate ticks only when
   this remaining deviation is also resolved **and** no new
   untracked deviation has appeared. The acceptance wording "the
   repo no longer looks 'stable by config file, experimental by
   code comment'" is locked to this formal gate condition.
5. **No new runtime flag or config section may be added to
   `rust-backend/config.toml` without classifying it in LD-10's
   audit table first.** The audit table is the single source of
   truth for "is this flag known to LD-10, and which bucket
   does it belong in?". Review-time enforcement is tracked as
   LD-10 cleanup #4 (CI grep / lint).

#### Why these choices

- **Pointer-lock, not a re-derivation.** LD-2, LD-3, and LD-7
  already own most of the §5.3 surface. Re-deriving them in
  LD-10 would duplicate content and invite drift. Inheritance
  is the only honest framing.
- **Orphan flags need an owner.** Without LD-10, flags like
  `market_strict_index_parity` or the `[genesis]` seed section
  have no explicit classification — they would drift into
  "whatever the current `config.toml` says" by default. The
  audit table forces a decision for each.
- **Deviation tracking is the acceptance gate.** §5.3's stated
  acceptance ("no longer looks stable by config file,
  experimental by code comment") is vague without a concrete
  "what counts as stable?" answer. LD-10 makes that
  answer equal to "matches Profile A modulo tracked gaps".
- **No new CI infrastructure required to lock the decision.**
  LD-10 cleanup #4 (CI grep / lint for new config flags) is
  optional enforcement, not a prerequisite for locking the
  decision. The normative rule stands even without a CI gate.

#### What this lock does NOT cover

- **Any change to LD-2's `rust_persist_enabled` status.** LD-10
  inherits LD-2 verbatim; it does not re-open Profile A's
  `rust_persist_enabled = false` or re-classify dev/experiment
  usage of `true`.
- **The LD-7 per-contract enable flag classification.** LD-10
  does not renegotiate which contracts are `RR` canonical-ready,
  `RR` candidate, `RR` blocked, or `EE` only. LD-7 is the single
  source of truth for that.
- **The checked-in `config.toml` `rust_persist_enabled` gap
  (historical).** That flip was owned by §1.1 deferred follow-up
  #1 under LD-2 and has since landed — `config.toml` now sets
  `rust_persist_enabled = false`, matching the code default.
  LD-10 originally cross-linked this as one of the two §5.3
  tracked gaps; the cross-link remains for historical traceability
  but the gap is closed, leaving only `market_strict_index_parity`
  as the outstanding deviation.
- **Server / storage / module / execution limit flags** (e.g.
  `[server]`, `[storage]`, `[execution]` `max_call_depth` /
  `max_code_size` / `energy_limit`). These are runtime-shaping
  parameters, not Phase 1 parity flags; they are outside §5.3's
  scope by LD-10 fiat. LD-10's audit table is scoped to
  `[execution.remote]`, `[execution.fees]`, and `[genesis]`.

#### Mandatory LD-10 cleanups

LD-10 itself is a doc-only lock; none of the code / test /
config edits below land in this iteration. They are tracked as
§5.3 deferred follow-ups.

1. **Classify the `market_strict_index_parity = true` deviation.**
   The `config.toml` override (`true`) disagrees with the
   `config.rs` default (`false`). Either (a) fold the override
   into LD-3 Profile A (if it is required for any Phase 1
   whitelist-target contract's parity), or (b) flip the code
   default to `true` (if it is actually safe by default) and
   drop the override line, or (c) document the deviation as
   intentional Profile B behavior and leave it alone. LD-10
   does not pick between the three — the decision is deferred
   to the §5.2 sidecar parity work that actually exercises
   market contracts. Until that lands, the deviation is
   **tracked but not resolved**.
2. **Remove `delegation_reward_enabled` from `RemoteExecutionConfig`.**
   The inline `config.toml` comment documents that the flag is
   deprecated and has no effect. The cleanup: delete the field
   from `config.rs`, drop the struct field reader, drop the
   override line from `config.toml`. Bundles with any future
   config.rs refactor — no urgency, but LD-10 tracks it so the
   orphan does not linger forever.
3. **Add a short "LD-10 classification" comment header above
   each orphan flag in `config.toml`.** Cross-reference the LD-10
   audit-table row so a future reader can trace every override
   back to its classification without searching the planning
   doc. Applies to the flags flagged as "`RR` experimental",
   "deployment parameter", or "deprecated" in the audit table;
   the "`RR` canonical-ready" rows do not need a comment
   because they match the code default.
4. **Add a CI grep / review rule** that flags any new flag
   appearing in `config.toml` `[execution.remote]`,
   `[execution.fees]`, or `[genesis]` sections if it is not
   classified in the LD-10 audit table (or reclassified via a
   new LD). Mechanism (grep, Checkstyle, or review checklist)
   is not locked; only the normative rule.
5. **Re-tick the §5.3 acceptance gate** once LD-2 `rust_persist_enabled`
   flip lands (§1.1 deferred follow-up #1) **and** LD-10 cleanup
   #1 resolves the `market_strict_index_parity` deviation. The
   gate is a joint condition on both — not an OR. **Status:** the
   `rust_persist_enabled` half has landed (`config.toml` now
   matches the code default and the LD-2 lock); the
   `market_strict_index_parity` half is still pending. The joint
   gate does not tick yet.

These cleanups are doc-only reorganization plus one CI/lint item
and one deprecated-field removal; no Rust or Java runtime
behavior changes are required by LD-10.

### LD-11: State-Ownership and Bridge Debt Scope Lock for Phase 1

#### Scope statement

This LD locks **the audit, classification, and removal sequence of
every Java→Rust mutation-data push path** that exists in the `RR`
execution path as of the Phase 1 freeze point, plus the Phase B
conformance-mirror path and the stateless-executor block-context
handshake. It does **not** classify the LD-1 canonical post-exec
apply path (`applyStateChanges*` / `applyFreezeLedgerChanges` /
`applyGlobalResourceChange` / `applyTrc10Changes` / `applyVoteChanges`
/ `applyWithdrawChanges`) as bridge debt — LD-1 explicitly locks
those as the canonical `RR` writer in Phase 1, so they are out of
§4's "temporary bridge" scope by definition. LD-11 enumerates and
classifies the paths that §4 actually targets: **split-ownership
debt** hiding behind ad-hoc push, sync, or re-read mechanisms.

LD-11 does **not** implement any removal. It does not migrate any
logic from Java to Rust. It only turns "temporary bridge debt" from
an implicit pile of workarounds into an explicit, enumerated,
classified list with a documented removal order — so that §4 of
`close_loop.todo.md` can be ticked on audit-complete grounds rather
than on implementation-complete grounds.

#### Definition of "bridge"

A "bridge" for LD-11's purposes is **any code path that moves
mutation data from Java to Rust pre-execution, or re-reads Rust
state back into Java post-execution, for a reason other than "Rust
is already the single authoritative writer"**. Specifically:

1. Java→Rust pre-execution pushes that exist because Java still
   co-owns state Rust needs to see (e.g. pre-consumed bandwidth /
   multi-sign fees / memo fees, block-reward distribution).
2. Rust→Java post-execution re-reads that exist to keep Java's
   in-memory stores coherent when Rust has already persisted (the
   Phase B `postExecMirror` conformance path).
3. Block-context handshakes that exist because Rust has no
   persistent block/tx-scoped session — these are a separate
   category from (1) and (2) because they carry envelope data, not
   mutation data, but they still represent split ownership of the
   block boundary.

By this definition, the LD-1 canonical post-exec typed-changelog
apply path (`apply*` methods in `RuntimeSpiImpl` fed by gRPC
results) is **not** a bridge: it is the locked Phase 1 production
write path for `RR`, not split-ownership debt. Pure query reads
(`get` / `has` / iterators / prefix queries) are not bridges.
Reporting / observability hooks (`StateChangeRecorderBridge`,
`DomainChangeRecorderBridge`) are not bridges.

#### Bridge inventory (authoritative)

The following **5 items** — 3 pre-exec push bridges + 1 block-context
handshake + 1 Phase B conformance-mirror — are enumerated as the
complete Phase 1 inventory in §4's sense. The
LD-1 canonical post-exec apply path is separately listed for
completeness at the end but is **not** counted as bridge debt.

Any item not in this inventory is either (a) nonexistent as of the
freeze point, (b) reporting / observability, (c) pure query read, or
(d) a new bridge introduced after the freeze — which is itself an
LD-11 violation and requires a new LD entry.

**Category 1 — Java→Rust pre-execution push (split-ownership debt):**

| # | Bridge | File:line | What it pushes | Why it exists |
|---|---|---|---|---|
| 1 | `collectPreExecutionAext` | `RemoteExecutionSPI.java:1667` | AEXT (bandwidth / energy usage windows, latest consume timestamps) for owner + recipient as `AccountAextSnapshot` proto messages embedded in `ExecuteTransactionRequest.pre_execution_aext` | Rust cannot compute bandwidth/energy consumption without seeing current per-account usage windows. Gated by `-Dremote.exec.preexec.aext.enabled` (default true). |
| 2 | `ResourceSyncContext.flushPreExec` → `ResourceSyncService.flushResourceDeltas` | `Manager.java:1570` → `ResourceSyncService.java:149` | Dirty Java-store bytes written directly to remote storage via `StorageSPI.batchWrite`. Per-tx scope covers only: `account` (balances after fee deduction), `properties` (e.g. `LATEST_BLOCK_HEADER_TIMESTAMP`, `BURN_TRX_AMOUNT`), `asset-issue` V1, `asset-issue` V2. **`delegation` is deliberately not in the per-tx payload** — see Known bug #3. | Java still performs pre-execution resource consumption (`consumeBandwidth`, `consumeMultiSignFee`, `consumeMemoFee`). Rust must see those mutations before executing. **This is the single load-bearing bridge** — everything else in Category 1 depends on Java co-owning pre-exec resource accounting. |
| 3 | `Manager.syncPostBlockRewardDeltas` | `Manager.java:2091` | Per-block reward flush: witness account allowances, `TRANSACTION_FEE_POOL`, and `delegation` reward keys (the only place `delegation` is pushed to Rust at all — see Known bug #3). | Block-level reward accounting still runs in Java; Rust must see it before the next block. Per-block frequency, not per-tx. |

**Category 2 — Block-context handshake (stateless-executor compensation):**

| # | Handshake | File:line | What it carries | Why it exists |
|---|---|---|---|---|
| 4 | `buildExecuteTransactionRequest` | `RemoteExecutionSPI.java:381` | Block envelope (`blockNumber`, `blockTimestamp`, `blockHash`, `coinbase`), energy limit read from `AccountStore` + `DynamicPropertiesStore`, `txBytesSize`, `contractParameter` raw bytes | Rust is currently a **stateless executor**: every call carries the full block context and tx envelope because Rust has no block/tx-scoped session. This is a handshake, not mutation-data movement — envelope data, not state deltas — but it is listed here because it is part of the split-ownership story and will shrink as Rust gains session state. |

**Category 3 — Phase B conformance-mirror (re-read pattern):**

| # | Bridge | File:line | What it applies | Activation |
|---|---|---|---|---|
| 11 | `postExecMirror` | `RuntimeSpiImpl.java:1615` | Re-reads `result.getTouchedKeys()` from remote `StorageSPI`, batch-fetches authoritative values, and writes them into every Java local store via `store.putRawBytes()` / `store.delete()`. Covers all stores in `getStoreByDbName`. | **Target state (LD-1 / LD-2):** only reachable when `write_mode == PERSISTED`, which LD-1 / LD-2 lock to "conformance / isolation tests only — never in production `RR`". **Current state caveat (until §1.1 follow-up #5 lands):** the NonVm execution path in `rust-backend/crates/core/src/service/grpc/mod.rs` still forces buffered writes + `WriteMode::PERSISTED` for every successful NonVm transaction regardless of `rust_persist_enabled`, so `postExecMirror` still runs in production `RR` for NonVm contracts today. The VM half is closed post-LD-2-flip + §1.1 follow-up #3 startup guard; the NonVm half remains open and is tracked by §1.1 deferred follow-up #5. |

> **LD-1 canonical apply path, listed for completeness but NOT counted
> as bridge debt:**
> `applyStateChangesToLocalDatabase` (`RuntimeSpiImpl.java:177`),
> `applyFreezeLedgerChanges` (`:211`),
> `applyGlobalResourceChange` (`:432`),
> `applyTrc10Changes` (`:470`),
> `applyVoteChanges` (`:519`),
> `applyWithdrawChanges` (`:609`). These are the locked Phase 1
> production write path for `RR` per LD-1. They are out of §4 scope
> by construction. LD-11's classification matrix does **not** assign
> them a "removal wave" because they are not scheduled for removal —
> they are the target architecture. Separate correctness bugs in
> this path (e.g. `updateAccountStorage` silent no-op, see Known
> bugs below) are still tracked as LD-11 cleanups because LD-11's
> audit surfaced them.

#### Classification matrix

| Bridge | Required in Phase 1? | Removable after write-ownership freeze? | Must survive into block importer phase? |
|---|---|---|---|
| 1. `collectPreExecutionAext` | Yes | Yes — removable when bandwidth/energy accounting moves into Rust (Wave 2) | No |
| 2. `ResourceSyncContext` + `ResourceSyncService.flushPreExec` | **Yes — load-bearing root** | Yes — removable when pre-exec resource accounting (`consumeBandwidth` / `consumeMultiSignFee` / `consumeMemoFee`) moves into Rust (Wave 2) | No |
| 3. `syncPostBlockRewardDeltas` | Yes | Yes — removable when block-reward accounting moves into Rust (Wave 2) | No |
| 4. `buildExecuteTransactionRequest` | Yes (envelope handshake) | Partially — the content shrinks after LD-4 §1.2 `energy_limit` cutover removes the energy-limit read, and again after Rust owns tx bytes / contract params (Wave 3) | Yes (the RPC shim survives in minimal form until Rust owns block/tx session state; that is a Phase 2+ concern) |
| 11. `postExecMirror` | **Target state: No for production `RR` (LD-1/LD-2 forbid `WriteMode::PERSISTED` in `RR`); yes for conformance / isolation tests. Current state: still reachable in production `RR` from the NonVm path until §1.1 follow-up #5 closes the NonVm bypass — the VM half is already closed post-LD-2-flip + §1.1 follow-up #3 startup guard.** | Target: outside production `RR`. Current: production `RR` reachability removed only after §1.1 follow-up #5 lands (close NonVm bypass). | Tooling, not production (target state). May survive as conformance tooling; may be reduced or removed depending on conformance test needs post-importer. Not on the removal critical path. |

#### `ResourceSyncService` classification

`ResourceSyncService` is classified as **fundamentally incompatible
with "Rust owns state transitions" in its current form, and
load-bearing for the duration of Phase 1**. It is not a
transitional patch and not a medium-term integration layer — it is
the concrete symptom of Java still co-owning pre-execution resource
accounting. Removal is gated on migrating `consumeBandwidth` /
`consumeMultiSignFee` / `consumeMemoFee` (and block-reward
accounting) into Rust, which is Wave 2 / Phase 2 block importer
scope. Until that migration, the service is not debt that can be
paid down early — it is holding the current correctness story
together.

#### Removal sequence

The removal of bridge debt happens in two waves plus one handshake
shrinkage, each gated on a prior wave landing. The LD-1 canonical
apply path is **not** in this sequence — it is not removed.

1. **Wave 1 — Close the NonVm bypass so production `RR` never
   takes the Phase B mirror path.** Gate: `rust-backend/crates/core/src/service/grpc/mod.rs`
   stops forcing `use_buffered_writes` + `WriteMode::PERSISTED` for
   every `TxKind::NonVm` execution regardless of
   `rust_persist_enabled`, so NonVm and VM both respect the target
   state from LD-1/LD-2. This is tracked independently under §1.1
   deferred follow-up "Reconcile the NonVm execution path with
   LD-1/LD-2". Wave 1 is **not** "remove bridges 5–10"; it is "stop
   routing NonVm production traffic through bridge 11". After Wave
   1, bridge 11 becomes conformance-only in reality, matching its
   LD-1 classification.

2. **Wave 2 — Pre-exec resource-accounting migration (bridges 1–3).**
   Gate: `consumeBandwidth`, `consumeMultiSignFee`, `consumeMemoFee`,
   and block-reward accounting are all owned by Rust. This is a
   Phase 2 block importer activity. When this gate is met,
   `ResourceSyncService` can be deleted entirely;
   `collectPreExecutionAext` becomes unnecessary;
   `syncPostBlockRewardDeltas` becomes dead code.

3. **Wave 3 — Block-context handshake shrinkage (bridge 4).**
   Gate: Rust owns block/tx-scoped session state. At this point
   `buildExecuteTransactionRequest` still exists as a thin RPC
   shim but no longer reads energy limits from Java stores (that
   half lands earlier via LD-4 §1.2), tx bytes, or contract params.
   The shim may never be fully removed; it simply shrinks until it
   is a pure envelope handshake.

No bridge is removed outside this sequence; no new bridge is added
without a new LD entry classifying it against the matrix above.

#### Known bugs caught during the audit

The audit surfaced three pre-existing correctness issues that are
**not** blockers for LD-11 itself — LD-11 can be locked without
fixing them — but must be tracked as cleanups so they do not get
lost. They are ordered by severity (highest first).

1. **`updateAccountStorage` silent drop is a direct VM-parity
   correctness hole, not a latent edge case**
   (`RuntimeSpiImpl.java:1104`). The method is a `TODO` stub that
   logs at DEBUG and returns, silently discarding every contract
   storage slot write delivered via `StateChange` entries with a
   non-empty key. This is **actively exercised today**: the Rust VM
   path (`rust-backend/crates/execution/src/tron_evm.rs`, the
   `execute_transaction` handler in
   `rust-backend/crates/core/src/service/grpc/mod.rs`) emits
   storage-slot `StateChange` entries on every stateful VM
   execution, and in any run where `write_mode != PERSISTED`, Java
   drops all of them. This is a direct violation of the CLAUDE.md
   Java-parity rule "never silently succeed where Java would fail",
   and it is the most severe correctness gap surfaced by the audit.
   Cleanup #1 below must land before the §4 second acceptance
   ("visible and sequenced, not hidden") can tick.

2. **Dead / misleading post-apply dirty-marking across the
   canonical apply path.** The following canonical apply-path
   helpers in `RuntimeSpiImpl.java` call
   `ResourceSyncContext.record*Dirty(...)` on the same keys they
   just wrote into the Java store (not a claim that _every_
   canonical apply-path helper does so — e.g. `updateAccountState`
   at `RuntimeSpiImpl.java:970` writes Rust-computed account state
   but does not call `record*Dirty`):
   - `RuntimeSpiImpl.java:301` (`recordAccountDirty` after account write)
   - `RuntimeSpiImpl.java:446–455` (`recordDynamicKeyDirty` after
     `applyGlobalResourceChange` writes the four
     `TOTAL_NET_WEIGHT` / `TOTAL_NET_LIMIT` / `TOTAL_ENERGY_WEIGHT` /
     `TOTAL_ENERGY_CURRENT_LIMIT` totals)
   - `RuntimeSpiImpl.java:588, 659, 781, 912, 913` (`recordAccountDirty`
     after canonical account writes in `applyFreezeLedgerChanges` /
     `applyTrc10Changes` / `applyVoteChanges` / `applyWithdrawChanges`
     and the dual-side sender+recipient marks)
   - `RuntimeSpiImpl.java:710` (`recordDynamicKeyDirty("TOKEN_ID_NUM")`
     after TRC-10 asset-issue write)

   The audit initially mistook the `applyGlobalResourceChange` block
   for a two-hop "next-tx echo loop", but a closer read of
   `ResourceSyncContext.java` shows that (a) `flushPreExec` runs
   **before** any canonical `apply*` method is reached (per
   `Manager.java:1570`, which flushes the pre-exec dirty set before
   the actuator runs), (b) `syncData.flushed` blocks a second flush
   in the same transaction, and (c) `ResourceSyncContext.finish()`
   clears the thread-local at tx end. The same reasoning applies to
   **every** post-apply `record*Dirty(...)` call listed above: the
   mark never reaches Rust. None of these are correctness bugs; they
   are **dead, misleading code** that implies an echo loop that does
   not exist and muddies the ownership story. Cleanup #2 below
   removes all of them together (not just the global-resource block)
   so the cleanup matches the scope of the dead pattern.

3. **`ResourceSyncContext` per-tx `DelegationStore` gap.**
   Delegation keys are only synced in the per-block
   `syncPostBlockRewardDeltas`, not in the per-tx `flushPreExec`
   path. If a transaction in the middle of a block mutates a
   delegation entry via a Rust-routed contract, Rust will see stale
   delegation data until the next block flush. The audit did not
   prove this affects any whitelist-target contract today, but it
   is a known gap worth tracking as LD-11 cleanup #3. Note that
   LD-11's bridge 2 inventory entry explicitly excludes `delegation`
   from the per-tx payload for this reason — do not add it back
   without first fixing `ResourceSyncContext`.

#### Locked decision

1. **The 5-item inventory above — 3 pre-exec push bridges +
   1 block-context handshake + 1 Phase B conformance-mirror —
   is authoritative for Phase 1.** Any Java↔Rust
   mutation-data movement not in this list, and not in the LD-1
   canonical apply path, either does not exist or is a new LD-11
   violation requiring a new LD entry.
2. **The classification matrix is the single source of truth** for
   whether a given bridge is Phase 1 debt, post-freeze removable,
   or must-survive. §4's "required / removable / must-survive"
   classification is inherited from this matrix.
3. **`ResourceSyncService` is classified as load-bearing-for-Phase-1
   and fundamentally incompatible with final ownership goals.** §4's
   three-way question is answered as "not transitional patch, not
   medium-term integration layer, incompatible — and load-bearing
   until Wave 2 lands".
4. **The two-wave + handshake-shrinkage removal sequence is the
   Phase 1 / Phase 2 boundary plan.** Wave 1 is
   bypass-closure (LD-1/LD-2 alignment), Wave 2 is pre-exec
   resource accounting migration, Wave 3 is handshake shrinkage.
   No bridge is removed out of order.
5. **The three known bugs are tracked as LD-11 cleanups** and are
   the joint prerequisite for the §4 second acceptance ("visible
   and sequenced, not hidden"). LD-11 is locked the moment the
   audit / classify / document / sequence items tick; the
   cleanups close the "visible" half of acceptance.
6. **No new bridge mechanism is added** without first classifying
   it against this LD's matrix. This is the §4 "confirm no new
   bridge mechanism should be added without first checking
   ownership implications" commitment. Enforcement mechanism is
   tracked as cleanup #4.

#### Why these choices

1. **Bridges 5–10 (LD-1 canonical apply path) are deliberately
   excluded from §4 debt.** LD-1 explicitly locks `RuntimeSpiImpl`'s
   `apply*` methods as the canonical `RR` writer and classifies
   `RuntimeSpiImpl` itself as "canonical, not transitional, not
   legacy". Treating those methods as "temporary bridges" would
   contradict LD-1. LD-11 therefore scopes "bridge debt" to the
   pre-exec push direction (where Java→Rust movement really is
   split-ownership debt) and the Phase B conformance mirror (which
   LD-1/LD-2 lock out of production `RR` **as target state** — the
   current-state caveat in the bridge-11 row/matrix above still
   applies, see §1.1 deferred follow-up #5).

2. **Bridge 11 (`postExecMirror`) is classified as
   conformance-tooling, not production must-survive.** Under
   LD-1/LD-2 **target state**, the `WriteMode::PERSISTED`
   short-circuit must only be reachable from conformance / isolation
   lanes. Any framing that made bridge 11 "the eventual replacement
   for the typed-changelog apply path" would directly contradict
   LD-1. LD-11 instead classifies it as tooling — available to
   conformance tests, forbidden from production `RR` **in target
   state**. **Current state caveat (until §1.1 follow-up #5 lands):**
   the NonVm execution path in
   `rust-backend/crates/core/src/service/grpc/mod.rs` still forces
   buffered writes + `WriteMode::PERSISTED` for every successful
   NonVm transaction regardless of `rust_persist_enabled`, so
   bridge 11 still runs in production `RR` for NonVm contracts
   today. This classification is the **target** that §1.1
   follow-up #5 closes; it is not a description of present-day
   production behavior. See the bridge-11 row/matrix above for the
   same target-vs-current split stated at the row level.

3. **Audit-and-classify is the only Phase 1 deliverable that is
   actually in scope.** Actually removing bridges 1–3 requires
   migrating resource accounting (pre-exec consume) into Rust,
   which is explicitly Phase 2 block importer work. Phase 1 cannot
   pay down bridge debt; it can only enumerate it.

4. **The three known bugs are listed but not blocking LD-11
   acceptance** because LD-11 is a scope lock, not an
   implementation gate. However, cleanup #1 (`updateAccountStorage`)
   is a direct correctness hole, not a latent edge, so it is
   ordered first and must land before the §4 second acceptance
   ticks.

#### What this lock does NOT cover

1. **Actual bridge removal.** No bridge is deleted by LD-11. Wave 1,
   Wave 2, and Wave 3 above are sequence documentation, not a work
   order.

2. **The LD-1 canonical apply path** (`applyStateChangesToLocalDatabase`,
   `applyFreezeLedgerChanges`, `applyGlobalResourceChange`,
   `applyTrc10Changes`, `applyVoteChanges`, `applyWithdrawChanges`).
   LD-11 surfaces correctness bugs in this path (known bug #1)
   but does not re-classify the path itself. Those methods are
   canonical `RR` writers per LD-1.

3. **The Rust-side receiver of each bridge.** LD-11 only classifies
   the Java side; it does not redesign the Rust gRPC handlers.
   Those are LD-5, LD-6, LD-8 and the per-contract LDs.

4. **Pre-execution resource accounting migration itself** (the
   root cause of bridges 1–3). That is a Phase 2 block importer
   scope item, not LD-11.

5. **`StateChangeRecorderBridge` / `DomainChangeRecorderBridge`
   observability paths.** These are journaling / reporting hooks,
   not mutation bridges by LD-11's definition. Mentioned in the
   audit for completeness but not in the inventory.

#### Mandatory LD-11 cleanups

These follow-up items are captured as §4 deferred follow-ups. They
do not block LD-11 acceptance — LD-11 locks on audit / classify /
document / sequence — but cleanups #1–#4 are the joint prerequisite
for §4's second acceptance ("visible and sequenced, not hidden").
Each cleanup lands independently.

1. **Fix `updateAccountStorage` direct VM-parity correctness hole.**
   Either implement contract storage slot writes in the non-Phase-B
   path, or add a strict failure
   (`UnsupportedOperationException` / strict error log + throw) so
   slot mutations cannot be silently discarded. Per CLAUDE.md
   Java-parity rule. **Highest severity of the three cleanups** —
   this is the only one that directly causes state divergence
   today.

2. **Remove dead / misleading post-apply dirty-marking across the
   canonical apply path.** Delete every
   `ResourceSyncContext.record*Dirty(...)` call that sits inside a
   canonical `apply*` method in `RuntimeSpiImpl.java`, not just the
   `applyGlobalResourceChange` block. Concretely: lines 301, 446,
   449, 452, 455, 588, 659, 710, 781, 912, and 913. Per Known bug
   #2 above, `flushPreExec` runs before any `apply*` method is
   reached, `syncData.flushed` blocks a second flush in the same
   transaction, and `ResourceSyncContext.finish()` clears the
   thread-local at tx end — so every one of these marks is dead.
   Removal eliminates the appearance of a non-existent echo loop
   and makes the ownership story cleaner to reason about. Leave a
   short comment in each affected `apply*` method explaining why
   no post-apply dirty-marking is needed (Rust is the source of
   truth for the apply input, so Java has nothing new to push).
   This is a clarity cleanup, not a correctness fix.

3. **Close the `ResourceSyncContext` per-tx `DelegationStore` gap.**
   Add `DelegationStore` to the per-tx dirty-key thread-local (not
   only the per-block `syncPostBlockRewardDeltas` path), so
   mid-block delegation mutations via Rust-routed contracts see
   fresh data. Once this lands, LD-11 bridge 2's inventory entry
   should be updated to include `delegation` in the per-tx payload.

4. **Add a CI grep / review rule** that flags any new bridge
   mechanism introduced without a new LD entry. Grep scope is
   **narrow on purpose** to avoid collisions with unrelated
   network sync code:
   - Any new `apply*` method in
     `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
     that writes to a Java store.
   - Any new class under
     `framework/src/main/java/org/tron/core/storage/sync/**` (this
     namespace is LD-11 territory; `org/tron/core/net/**` and
     `org/tron/core/consensus/**` sync code is explicitly out of
     scope and must not be caught by the grep).
   - Any new method in
     `framework/src/main/java/org/tron/core/db/Manager.java` that
     writes directly to `StorageSPI.*` for mutation data.
   Enforcement mechanism (grep, Checkstyle, or review checklist)
   is not locked; only the normative rule.

5. **Joint re-tick** of §4 acceptance "Temporary bridge debt is
   visible and sequenced, not hidden" once cleanups #1–#4 have
   all landed. Joint condition on all four — not an OR.
   Sequencing is already in place via the LD-11 removal
   sequence; these cleanups close the "visible" (bug #1) +
   "clean" (bug #2) + "complete" (bug #3) + "not hidden" (CI
   grep #4) halves of the acceptance.

These cleanups are one severe correctness fix (#1), one clarity
cleanup (#2), one real coverage gap fix (#3), one CI/lint item
(#4), and one joint acceptance re-tick (#5). No Rust runtime
behavior changes are required by LD-11 itself.
