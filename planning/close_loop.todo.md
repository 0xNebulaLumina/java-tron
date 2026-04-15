# TODO: execution/storage close loop before block importer

Objective: close the execution + storage semantics gap so the Rust backend can be treated as a trustworthy state-transition core, instead of a partial remote accelerator.

This phase is intentionally about correctness, ownership, and verification.

This phase is intentionally not about:

- Rust P2P replacement
- Rust full sync pipeline
- Rust consensus replacement
- Replacing the Java node shell in one jump

Operating model for this planning:

- `EE` = embedded execution + embedded storage
- `RR` = remote execution + remote storage

Out of scope as target modes:

- current in-process `SHADOW`
- mixed combinations such as remote execution + embedded storage

Project decision for this planning:

- The current `SHADOW` path is not the primary validation strategy we want.
- The comparison we want must be trustworthy and isolated.
- Shared JVM singleton / global-state behavior makes the current shadow approach unsuitable as the main acceptance mechanism.
- Going forward, the primary comparison model is: run `EE`, run `RR`, compare results outside the in-process shadow path.

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

- [x] Freeze Phase 1 scope as: execution semantics + storage semantics + parity verification only
- [x] Freeze the only two strategic modes for this planning:
  - [x] `EE`
  - [x] `RR`
- [x] Freeze explicit non-goals for this phase:
  - [x] No Rust P2P networking rewrite
  - [x] No Rust sync scheduler / peer manager rewrite
  - [x] No Rust consensus rewrite
  - [x] No attempt to remove the Java node shell in this phase
  - [x] No optimization for mixed execution/storage combinations
  - [x] No reliance on current `SHADOW` as the main proof mechanism
- [x] Freeze the intended next milestone as `Rust block importer / block executor readiness`
- [x] Publish a short "why not P2P yet" note inside this file or a sibling planning note so the roadmap does not drift
- [x] Publish a short "why not SHADOW as the main validator" note so the roadmap does not drift back
- [x] Define Phase 1 exit criteria:
  - [ ] Java execution read/query APIs are no longer placeholders in the `RR` path
  - [ ] Rust execution read/query APIs are either implemented or explicitly unsupported
  - [ ] Storage transaction semantics are real enough for execution needs
  - [ ] Storage snapshot semantics are real, or snapshot is explicitly not-a-real-snapshot and loud about it (LD-6 + LD-9: EVM snapshot/revert is hard-unsupported via `Status::unimplemented`; storage `getFromSnapshot` is loud degrade to live-read with `tracing::warn!` on every call). Never silently fake.
  - [x] `energy_limit` wire semantics are locked *(LD-4 — spec lock only; the implementation cutover is tracked in §1.2 deferred follow-ups)*
  - [ ] Write ownership is unambiguous in `EE` and `RR`
  - [ ] A first contract whitelist reaches stable `EE-vs-RR` parity
  - [ ] Storage crate has real tests
  - [ ] Replay + CI can continuously report `EE-vs-RR` parity state

> Section 0 frozen by `close_loop.planning.md` ("Recommended Roadmap",
> "Mode Decision", "Why The Next Step Is Not P2P", "Phase 1 Exit Criteria",
> and `Locked Decisions § LD-1`). Most exit-criteria sub-checkboxes above
> are **implementation/achievement gates** (not definition gates) and
> remain `[ ]` until the corresponding work in §§ 2–6 lands. The one
> exception is the spec-lock gate "`energy_limit` wire semantics are
> locked", which is achieved by writing down LD-4 itself — the
> implementation cutover is separately tracked in §1.2 deferred
> follow-ups and does not need to land for this gate to be considered
> met.

---

## 1. Semantic freeze and architectural decisions

Goal: stop the project from moving forward on top of ambiguous semantics.

### 1.1 Canonical write ownership

Primary touchpoints:

- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- `rust-backend/config.toml`
- `rust-backend/crates/common/src/config.rs`

- [x] Write down the authoritative write-path matrix for:
  - [x] `EE`
  - [x] `RR`
- [x] Explicitly de-emphasize current `SHADOW` as a legacy / optional path, not a Phase 1 acceptance mode
- [x] Define whether `RuntimeSpiImpl` Java-side apply is canonical, transitional, or legacy-only
- [x] Define whether `rust_persist_enabled=true` is allowed in:
  - [x] development only — **Yes** (LD-2)
  - [x] targeted experiments only — **Yes** (LD-2)
  - [x] `RR` candidate mode — **No** (LD-2)
  - [x] never, until later phase — **No** (LD-2 still permits it for dev/experiments today)
- [x] Align code defaults, checked-in config, and comments — **Done:** `rust-backend/config.toml` now sets `rust_persist_enabled = false`, matching the code default in `rust-backend/crates/common/src/config.rs:607/681` and the LD-2 lock. Supporting comment block in `config.toml` updated to cross-reference LD-2 explicitly. LD-2 action-items block, LD-7 "mirror pattern" analogy, and LD-10 tracked-gap wording in `close_loop.planning.md` all updated to reflect the closed state. The remaining Phase 1 `config.toml` deviation from Profile A is `market_strict_index_parity` (LD-10 cleanup #1).
- [x] Add a future implementation item to fail fast when an unsafe mode combination is detected
- [x] Document one recommended safe rollout profile and one experimental profile

Acceptance:

- [ ] Any engineer can answer "who writes the final state in this mode?" without ambiguity
- [x] `config.toml`, `config.rs`, and planning docs no longer contradict each other — **Ticked** after the §1.1 deferred follow-up #1 flip + no-contradiction doc sweep (follow-ups #1 and #2 below). `rust_persist_enabled` in `config.toml`, the code default in `config.rs`, and the LD-2 / LD-7 / LD-10 prose in `close_loop.planning.md` now all agree on `false`. The only remaining `config.toml` deviation from Profile A is `market_strict_index_parity = true`, which is intentionally tracked under LD-10 cleanup #1 and is therefore not a contradiction — it is a documented deviation.

> Decisions captured in `close_loop.planning.md § Locked Decisions LD-1
> (Canonical Write Ownership)`, `LD-2 (rust_persist_enabled allowed usage)`,
> and `LD-3 (Recommended Configuration Profiles)`. The four
> `rust_persist_enabled` sub-checkboxes are intentionally all checked
> because LD-2 explicitly answers each one (yes / yes / no / no).
>
> The first acceptance item is intentionally still `[ ]`: LD-1 / LD-2 are
> the **target state**, but `RR` today still has a NonVm bypass where the
> Rust execution service forces `WriteMode::PERSISTED` regardless of
> `rust_persist_enabled`. So the answer to "who writes the final state in
> RR?" is still conditional on contract type. It cannot be ticked until the
> NonVm bypass deferred follow-up below lands.

#### 1.1 deferred follow-ups

These are the open follow-ups (code, config, and doc/sweep work) that fall
out of LD-1/LD-2/LD-3 and stay open until the supporting work (and impact
analysis on conformance / tests) is done:

- [x] Flip checked-in `rust-backend/config.toml` so `execution.remote.rust_persist_enabled = false` matches Profile A and the code default — **Done.** `rust-backend/config.toml:188` now reads `rust_persist_enabled = false` (line shifted from :174 → :188 by the new LD-1/LD-2 cross-reference comment block immediately above the flag). Verification that no Java-less test path silently depends on the `true` value: (a) the conformance runner at `rust-backend/crates/core/src/conformance/runner.rs:233` explicitly forces `rust_persist_enabled: true` internally (not driven by `config.toml`), so it is unaffected; (b) the NonVm bypass in `rust-backend/crates/core/src/service/grpc/mod.rs:1317` forces buffered writes regardless of the flag, so NonVm tests are unaffected; (c) the VM path in `rust-backend/crates/execution/src/lib.rs:126` and `service/contracts/withdraw.rs:405` now takes the `false` branch by default, which matches the code default (`config.rs:607/681`) and the LD-2 lock — the `true` branch is reserved for dev/experiment profiles that override the flag explicitly. Config-file comment expanded to cross-reference LD-2.
- [x] Run the §0 "no-contradiction" doc/config sweep across `rust-backend/config.toml`, `rust-backend/crates/common/src/config.rs`, `close_loop.planning.md`, and inline comments after the remaining locks (LD-4..) land, then re-tick the §1.1 acceptance "`config.toml`, `config.rs`, and planning docs no longer contradict each other" together with the §0 acceptance gate — **Done** for the §1.1-half of the joint acceptance: after the flip, the planning doc sweep updated LD-2 action-items (§ LD-2), the LD-7 "mirror pattern" analogy (§ LD-7 config-default vs config.toml mismatch), and LD-10's "two tracked gaps today" / "what this lock does not cover" / cleanup #5 status notes, all to reflect that the `rust_persist_enabled` gap has been closed. The §1.1 acceptance row "`config.toml`, `config.rs`, and planning docs no longer contradict each other" is now ticked. The §0 acceptance-row half of this joint gate (the exit-criteria items at todo.md §0 lines 77–85) remains `[ ]` because those are implementation/achievement gates, not doc-contradiction gates — they are intentionally out of scope for this follow-up.
- [x] Add a runtime guard in the Rust execution service that fails fast when `rust_persist_enabled = true` is observed alongside an active Java apply lane (i.e. the `RR` production mode), so unsafe combinations cannot be reached by accident. — **Done.** `rust-backend/src/main.rs` now runs an LD-2 startup guard immediately after `Config::load()`: if the loaded `ExecutionConfig` has `execution.remote.rust_persist_enabled = true`, the binary emits a loud `error!` citing LD-1 / LD-2 and returns an `Err` from `main`, so the process exits non-zero before any gRPC handler runs. The "active Java apply lane" signal is proxied by "`main.rs` is the in-tree production binary entrypoint" — the conformance runner at `rust-backend/crates/core/src/conformance/runner.rs` builds its own `ExecutionConfig` in code and never calls `Config::load()`, so the conformance / isolation path is not affected. Escape hatch: there is none by design — a developer who legitimately needs `rust_persist_enabled = true` is expected to use the conformance runner, not the production binary. The guard is deliberately narrow: it does NOT cover the separate NonVm buffered-write bypass at `crates/core/src/service/grpc/mod.rs`, which still forces `WriteMode::PERSISTED` regardless of the flag; closing that half is tracked under §1.1 deferred follow-up #5 and requires a separate design decision. Planning-doc cross-reference updated at `close_loop.planning.md` § LD-2 (runtime guard action items).
- [x] Audit Java-side `WriteMode::PERSISTED` usage and document reachability of the `skipApply` short-circuit in `RuntimeSpiImpl.execute(...)` from both VM and NonVm paths in `RR`, so the current-vs-target gap is visible in code. (Actually *ensuring* the short-circuit is unreachable from production `RR` is tracked separately: the VM half lands with §1.1 follow-up #1 flip + #3 startup guard, and the NonVm half remains open under §1.1 follow-up #5.) — **Done** as an audit + inline documentation task. Audit scope: every in-tree consumer of `ExecutionSPI.WriteMode.PERSISTED` was enumerated (`framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`, `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`, `framework/src/main/java/org/tron/core/execution/spi/ShadowExecutionSPI.java`). Findings: (a) `RuntimeSpiImpl.execute(...)` is the only place the mode has behavioral effect (the `skipApply` short-circuit); (b) `RemoteExecutionSPI.convertExecuteTransactionResponse` uses it only for a logging branch; (c) `ShadowExecutionSPI` is out of scope per LD-6. VM-path reachability: after the §1.1 follow-up #1 flip + the new §1.1 follow-up #3 startup guard, Rust never emits `WriteMode::PERSISTED` for VM transactions in RR production, so the `skipApply` branch is unreachable from VM in RR. OK. NonVm-path reachability: the Rust gRPC handler's unconditional NonVm buffering still emits `WriteMode::PERSISTED` for every successful NonVm transaction, so the `skipApply` branch is still reachable from NonVm in RR production. This is the outstanding LD-1/LD-2 half tracked by §1.1 deferred follow-up #5 ("Reconcile the NonVm execution path with LD-1/LD-2"). The audit result is recorded **in code** via an `// LD-2 audit:` Javadoc block added at `RuntimeSpiImpl.java` around line 96 (where `skipApply` is computed) so future readers hit the callout at the same place they read the short-circuit. Planning-doc cross-reference updated at `close_loop.planning.md` § LD-2 (audit action item).
- [ ] **Reconcile the NonVm execution path with LD-1/LD-2.** Today `rust-backend/crates/core/src/service/grpc/mod.rs` forces buffered Rust-side commit + `WriteMode::PERSISTED` for every `TxKind::NonVm` execution regardless of `rust_persist_enabled`, which makes Rust the canonical writer for NonVm contracts in `RR`. Either drop the unconditional NonVm buffering (and let Java apply own NonVm state too), or formally extend LD-1/LD-2 to declare NonVm a Rust-canonical lane and document the safety story for re-applying its sidecars idempotently. Once this lands, re-tick the §1.1 acceptance "Any engineer can answer 'who writes the final state in this mode?' without ambiguity".

### 1.2 Lock `energy_limit` wire semantics

Primary touchpoints:

- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- `framework/src/main/proto/backend.proto`
- `rust-backend/crates/execution/src/lib.rs`
- fixture/conformance generators and readers

- [x] Audit current Java sender behavior for VM txs:
  - [x] `CreateSmartContract`
  - [x] `TriggerSmartContract`
  - [x] any other path that sets `ExecutionContext.energy_limit`
- [x] Audit current Rust receiver behavior and conversion logic
- [x] Audit conformance fixture assumptions for `energy_limit`
- [x] Choose one canonical wire contract:
  - [ ] send SUN, convert in Rust
  - [x] send energy units, do not reconvert in Rust
  - [ ] introduce an explicit unit field/flag if neither is safe enough
- [x] Record migration impact for:
  - [x] Java bridge
  - [x] Rust execution
  - [x] fixtures
  - [x] `EE-vs-RR` comparison tooling
  - [x] replay tooling
- [x] Update comments in `backend.proto`
- [x] Add a follow-up implementation item to prevent mixed old/new interpretations during transition

Acceptance:

- [x] No remaining ambiguity on whether Java sends fee-limit SUN or already-computed energy units
- [ ] Java, Rust, and conformance tooling target the same unit contract

> Decision captured in `close_loop.planning.md § LD-4 (energy_limit Wire
> Semantics)` with full audit, locked contract, migration impact table,
> and transition-safety rationale. `backend.proto` comments on
> `Transaction.energy_limit` and `ExecutionContext.energy_limit` updated
> to reference LD-4. `rust-backend/crates/execution/src/lib.rs` comment
> on the pre-lock division updated to point at LD-4 as well.
>
> The "send SUN, convert in Rust" and "explicit unit field" sub-options
> are intentionally left `[ ]` because the locked decision rejects them.
>
> The second acceptance ("Java, Rust, and conformance tooling target the
> same unit contract") is intentionally still `[ ]`: the contract is
> locked but the implementation has not yet been migrated. It cannot be
> ticked until the deferred follow-ups land.

#### 1.2 deferred follow-ups

LD-4 explicitly notes that the cutover **cannot be staged field-by-field**.
The following items must land as a single coherent change set:

- [ ] **Java**: Harden `RemoteExecutionSPI.computeEnergyLimitWithFixRatio` so the `null StoreFactory` / `null ChainBaseManager` / `null AccountStore` / `null DynamicPropertiesStore` / `null AccountCapsule` / `catch Exception` branches all fail loudly (return `0` or throw a strict `RuntimeException`) instead of silently returning the raw `feeLimit` SUN value.
- [ ] **Java (non-VM zeroing)**: In `RemoteExecutionSPI.buildExecuteTransactionRequest` (around `RemoteExecutionSPI.java#L381`), set `Transaction.energy_limit = 0` for every non-VM `ContractType`. Today the field is seeded once at line ~393 from `transaction.getRawData().getFeeLimit()` (raw SUN) and only the VM `TriggerSmartContract` / `CreateSmartContract` arms re-assign it via `computeEnergyLimitWithFixRatio`, so non-VM contracts currently send raw SUN on the wire even though Rust ignores it. LD-4 locks non-VM `Transaction.energy_limit` as `0` (or a documented sentinel) — this must land in the same atomic cutover as the Rust divide removal.
- [ ] **Java**: Stop reusing the per-transaction `energyLimit` as `ExecutionContext.energy_limit`; populate the block envelope from the actual block-level cap instead.
- [ ] **Rust**: Remove the `adjusted_tx.gas_limit / energy_fee_rate` division in `execute_transaction_with_storage`, `call_contract_with_storage`, and `estimate_energy_with_storage` in `rust-backend/crates/execution/src/lib.rs`. Trust the wire as energy units.
- [ ] **Rust**: Audit `convert_protobuf_transaction` (and any other gRPC ingress) to confirm no other path silently converts SUN → energy.
- [ ] **Conformance fixtures**: Update fixture generators (Java and Rust) to encode `energy_limit` as energy units. Also sweep the non-generator SUN-assuming literals/comments called out in LD-4's migration impact table — at least `rust-backend/crates/core/src/service/tests/contracts/create_smart_contract.rs`, `rust-backend/crates/core/src/conformance/runner.rs` (lines ~1478 and ~1622), and the stale comment in `rust-backend/crates/execution/src/tron_evm.rs`. Add a one-time changelog note so old fixtures cannot be replayed against the new Rust path without explicit regeneration.
- [ ] **Transition guard**: Add a runtime sanity check in both Java pre-send and Rust on-receive that rejects any wire `energy_limit` whose magnitude is "obviously SUN". Threshold must be derived from runtime constants, **not** a hardcoded `10_000_000` (Java parity tests routinely use energy limits above `10^7` units — see `RuntimeImplTest.java#L157-L162` and `#L247-L253`). Recommended formulation: reject if `wire_value > MAX_FEE_LIMIT` (i.e. `> 10_000_000_000`, [ProposalUtil.java#L395-L405](../actuator/src/main/java/org/tron/core/utils/ProposalUtil.java#L395-L405)), or, when the receiver has access to the chain parameter, the tighter bound `wire_value > MAX_FEE_LIMIT / max(1, current_sun_per_energy)`. Keep the guard active until the parity dashboard reports a clean `EE-vs-RR` window with no fallbacks.
- [ ] **Cleanup**: Remove the LD-4 reference comment from `backend.proto`, `lib.rs`, and the planning doc once the cutover is complete and the parity dashboard is green for at least one Phase 1 milestone window.
- [ ] **LD-4 ↔ LD-8 cross-link (LD-8 cleanup #13)**: The LD-4 `energy_limit` cutover is a **hard prerequisite** for LD-8 §8's `estimateEnergy` exact-match target state. Until every item above lands atomically, LD-8 §8 locks the pre-LD-4 interim rule as **"no cross-mode parity expectation"** — no tolerance windows, no bounded-delta assertions, and no per-contract exception lists are allowed (the `gas_limit / energy_fee_rate` divide at [`lib.rs#L534/L554`](../rust-backend/crates/execution/src/lib.rs#L534) changes the cap multiplicatively, so mismatches near the cap can be arbitrarily large, not a small additive delta). `estimateEnergy`-parity regression alerts stay off until this cutover lands.

### 1.3 Lock storage transaction semantics

Primary touchpoints:

- `framework/src/main/proto/backend.proto`
- `framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java`
- `framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java`
- `rust-backend/crates/storage/src/engine.rs`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

- [x] Decide the required semantics for `beginTransaction/commit/rollback`
- [x] Decide whether transaction scope is:
  - [x] per DB
  - [ ] cross DB
  - [x] only "execution-local enough", not generic DB transaction
- [x] Decide whether transaction-scoped reads need read-your-writes visibility for:
  - [ ] `get`
  - [ ] `has`
  - [ ] `batchGet`
  - [ ] iterators / prefix / range reads
- [x] Decide what execution actually needs versus what can be deferred
- [x] Explicitly reject turning `StorageSPI` into a generic database product if that is not needed now
- [x] Write down behavior when `transaction_id` is absent on a write call

Acceptance:

- [x] There is a clear "minimum transaction semantics required by execution/block importer" statement
- [x] No one needs to infer semantics from partial code paths

> Decisions captured in `close_loop.planning.md § Locked Decisions LD-5
> (Storage Transaction Semantics)`. The locked answers, per the audit
> and locked-decision sections of LD-5:
>
> - **Phase 1 required semantics: none.** No production caller needs
>   real `beginTransaction` / `commit` / `rollback` semantics today;
>   the Java revoking layer (`TronStoreWithRevoking`) remains the
>   canonical Phase 1 transaction boundary in `RR`.
> - **Scope: per DB only.** Cross-DB atomicity is **rejected** at the
>   `StorageSPI` layer; multi-store atomicity belongs to the layer
>   above (the block importer's commit step). Selected sub-options:
>   `per DB` and `only "execution-local enough"`. The `cross DB`
>   sub-option is intentionally left `[ ]` because LD-5 rejects it.
> - **Read-your-writes: rejected for Phase 1.** All four sub-options
>   (`get` / `has` / `batchGet` / iterators) are intentionally left
>   `[ ]` because the wire format already forbids them
>   (`GetRequest` / `HasRequest` / `BatchGetRequest` / `IteratorRequest`
>   carry `snapshot_id` but not `transaction_id`) and no Phase 1
>   caller needs them. They are locked as "not supported", not as
>   "to be added".
> - **`transaction_id` absent on a write**: locked as **direct write
>   to the durable store**, matching today's behavior.
> - **Generic database product**: explicitly rejected.
>
> The minimum semantics statement (first acceptance item) lives in
> LD-5 §"Locked decision" item 1 ("Phase 1 required semantics: none").
> The "no inference from partial code paths" acceptance is satisfied
> by the LD-5 audit, which file-grounds every relevant claim.

#### 1.3 deferred follow-ups

LD-5 demands that the codebase stop silently lying about transaction
support, even before real semantics land. These are doc/log-only
cleanups; none of them changes runtime behavior. They are sequenced to
land ahead of any real implementation work in §3.1 / §3.2.

- [ ] **`backend.proto` comments**: two distinct annotations are required because the field-level and message-level honesty stories differ.
  - On the **field** `PutRequest.transaction_id`, `DeleteRequest.transaction_id`, and `BatchWriteRequest.transaction_id`, add: "LD-5: this `transaction_id` field is **silently ignored** by the current Rust gRPC write handlers; writes always go directly to the durable store. Per-DB scope only. Real semantics deferred to the block importer phase."
  - On the **messages** `BeginTransactionRequest`, `CommitTransactionRequest`, and `RollbackTransactionRequest`, add: "LD-5: structural placeholder. The gRPC handler **does** process these RPCs and forwards them to `engine.begin_transaction` / `engine.commit_transaction` / `engine.rollback_transaction`, but the engine never populates the per-transaction buffer (no `engine.put_in_transaction` exists), so `commit_transaction` observably writes an empty `WriteBatch` and `rollback_transaction` discards an already-empty buffer. Per-DB scope only. Real semantics deferred to the block importer phase."
  - On the **field** `snapshot_id` of `GetRequest`, `HasRequest`, `BatchGetRequest`, `IteratorRequest`, `PrefixQueryRequest`, `GetKeysNextRequest`, `GetValuesNextRequest`, and `GetNextRequest`, add: "LD-5/LD-6: silently ignored by the current handler; reads always hit the live durable store. Read-your-writes is intentionally not supported in Phase 1."
- [ ] **Java SPI Javadoc**: add a `// LD-5:` block to `StorageSPI.beginTransaction` / `commitTransaction` / `rollbackTransaction` explicitly stating "Phase 1: structural placeholder. No production caller relies on real semantics. Will be implemented in the block importer phase. Per-DB scope only. Read-your-writes is intentionally not supported."
- [ ] **`EmbeddedStorageSPI` honesty**: replace the silent no-op implementations of `beginTransaction` / `commitTransaction` / `rollbackTransaction` with either (a) a logged warning on first call, or (b) explicit `UnsupportedOperationException` if any caller is ever discovered. Today no caller exists, so logged-warning is the minimal acceptable form.
- [ ] **Rust engine honesty** (`rust-backend/crates/storage/src/engine.rs`): keep `begin_transaction` / `commit_transaction` / `rollback_transaction` for now (some test paths exercise the round trip) but log loudly via `tracing::warn!` that the transaction was a no-op, and add a doc comment cross-referencing LD-5 to the `TransactionInfo.operations` field explaining why the buffer is dead state today.
- [ ] **gRPC handler honesty** (`rust-backend/crates/core/src/service/grpc/mod.rs`): in the `put` / `delete` / `batch_write` handlers, log a warning when a request arrives with a non-empty `transaction_id`, since today that field is silently ignored and a future caller could mistakenly believe it is honored. Similarly log in `get` / `has` / `batch_get` / `iterator` / `prefix_query` / `get_keys_next` / `get_values_next` / `get_next` when a non-empty `snapshot_id` is observed.

### 1.4 Lock snapshot semantics

Primary touchpoints:

- `framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java`
- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- `rust-backend/crates/storage/src/engine.rs`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

- [x] Decide whether storage snapshot must be a true RocksDB point-in-time snapshot — **No** in Phase 1 (LD-6 #1: deferred to block importer phase, no production caller exists).
- [x] Decide whether EVM snapshot/revert must be built on top of (sub-checkboxes mean "decision answered", not "option selected" — see LD-6 #3):
  - [x] storage snapshot — answered: **rejected** for Phase 1 (LD-6 #3)
  - [x] execution-local journaling — answered: **chosen**, already in use and unchanged (revm `journaled_state` in `tron_evm.rs`, LD-6 #3)
  - [x] both — answered: **rejected** for Phase 1 (LD-6 #3)
- [x] Decide whether temporary "unsupported" is safer than fake success — **Yes** (LD-6 #5)
- [x] Write explicit unsupported behavior for any API not implemented in this phase — **Specified by LD-6 §Locked decision items #1, #2, and #5** (with LD-9 refinement). Two halves: (1) **EVM snapshot/revert is explicitly unsupported** — `RemoteExecutionSPI.createSnapshot` / `revertToSnapshot` return `CompletableFuture.failedFuture(UnsupportedOperationException)`, and the Rust gRPC `create_evm_snapshot` / `revert_to_evm_snapshot` handlers return `tonic::Status::unimplemented`. (2) **Storage `getFromSnapshot` is loud degrade, not hard unsupported** — LD-9 locks LD-6 §1.4 option (b): keep the live-read fallback so `EmbeddedStorageSPI.getFromSnapshot` and existing integration tests compile, but emit a loud `tracing::warn!` **on every call** (not first-call-only) cross-referencing LD-6/LD-9. Option (a) `Status::unimplemented` is no longer open for Phase 1. The doc-level specification is locked here; the code flips are tracked in the deferred follow-ups below.

Acceptance:

- [ ] Fake snapshot success is no longer an accepted state
- [ ] Snapshot-dependent APIs are either real, explicitly fail-loud, or loud-degrade — never silently fake. Two halves (LD-6 + LD-9 refinement): (a) EVM snapshot/revert hard-unsupported via `UnsupportedOperationException` / `tonic::Status::unimplemented`; (b) storage `getFromSnapshot` loud degrade to live-read with `tracing::warn!` on every call (not first-call-only) cross-referencing LD-6/LD-9.

> Decisions captured in `close_loop.planning.md § Locked Decisions LD-6
> (Snapshot Semantics)` with LD-9's refinement of the storage half. LD-6
> covers both halves (storage snapshot and EVM snapshot) under one
> decision because they share the same root cause (no production caller,
> no real implementation, fake success at every layer). The Phase 1
> remediation is **not uniform "fail explicitly"**: LD-9 splits it into
> (a) hard-unsupported for EVM snapshot/revert and (b) loud-degrade to
> live-read with `tracing::warn!` on every call for storage
> `getFromSnapshot`. In both halves the common rule is: be loud about
> it, never silently lie.
>
> Both acceptance items are intentionally still `[ ]`: LD-6 (with LD-9
> refinement) **decides** the policy, but the actual flips in
> `RemoteExecutionSPI.createSnapshot`/`revertToSnapshot` (fail-loud), the
> Rust gRPC `create_evm_snapshot`/`revert_to_evm_snapshot` handlers
> (`Status::unimplemented`), and the storage `getFromSnapshot` honesty
> change (warn-on-every-call) are implementation work tracked in the
> deferred follow-ups below. Until those land, fake `success: true` is
> still observable on the wire and the acceptance gates cannot be ticked.

#### 1.4 deferred follow-ups

These are the open follow-ups (code, doc, and test sweep work) that fall
out of LD-6 and stay open until the supporting work (and impact analysis
on shadow / conformance / tests) is done:

- [ ] **`RemoteExecutionSPI.createSnapshot` fail-loud** (`framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java#L214-L225`): replace the placeholder `"remote_snapshot_" + System.currentTimeMillis()` return with a `CompletableFuture` that completes exceptionally with `UnsupportedOperationException("EVM snapshot/revert is not supported in RR Phase 1 (LD-6); see close_loop.planning.md")`. Remove the `// TODO: Implement in Task 2` comment and the placeholder log line.
- [ ] **`RemoteExecutionSPI.revertToSnapshot` fail-loud** (`framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java#L228-L239`): same treatment — completes exceptionally with `UnsupportedOperationException` carrying the LD-6 reference.
- [ ] **Rust gRPC `create_evm_snapshot` fail-loud** (`rust-backend/crates/core/src/service/grpc/mod.rs#L1870-L1884`): change to return `tonic::Status::unimplemented("EVM snapshot unsupported in Phase 1 (LD-6)")`. Remove the placeholder UUID allocation that fakes `success: true`.
- [ ] **Rust gRPC `revert_to_evm_snapshot` fail-loud** (`rust-backend/crates/core/src/service/grpc/mod.rs#L1886-L1899`): same treatment. Today this handler ignores `request.snapshot_id` entirely and unconditionally returns `success: true`.
- [ ] **Storage `getFromSnapshot` honesty** (`rust-backend/crates/core/src/service/grpc/mod.rs` snapshot read handler and `rust-backend/crates/storage/src/engine.rs` `get_from_snapshot` at L460-L472): keep the live-read behavior but emit a loud `tracing::warn!` on every call cross-referencing LD-6 (and LD-9). **LD-9 update**: this is now locked as option (b); the original LD-6 §1.4 cleanup left this as a disjunction between (a) `tonic::Status::unimplemented` and (b) live-read + warn, and LD-9 picks (b) because `EmbeddedStorageSPI.getFromSnapshot` and the existing `StorageSPIIntegrationTest` round-trip depend on the live-read fallback compiling. Option (a) is no longer open. The warning must be loud enough to trip a CI grep.
- **Note (LD-6 cleanup item 6 — non-`GetFromSnapshot` `snapshot_id` honesty)**: intentionally **not duplicated here**. The §1.3 "gRPC handler honesty" follow-up already covers logging when a non-empty `snapshot_id` is observed on `Get` / `Has` / `BatchGet` / `Iterator` / `PrefixQuery` / `GetKeysNext` / `GetValuesNext` / `GetNext`. §1.4 only adds the snapshot-specific `getFromSnapshot` / `GetFromSnapshot` honesty item above; LD-6 explicitly delegates the rest to the LD-5 follow-up to avoid duplication.
- [ ] **`ExecutionSPI.createSnapshot` / `revertToSnapshot` Javadoc note**: add a Javadoc paragraph (Javadoc form, not a `// ` line comment) to `framework/src/main/java/org/tron/core/execution/spi/ExecutionSPI.java#L86-L99` explicitly stating "Phase 1 (LD-6): EVM snapshot and revert are not supported in `RR`. Remote callers receive `UnsupportedOperationException`. The `EmbeddedExecutionSPI` implementation is itself a placeholder (see `EmbeddedExecutionSPI.java#L193-L220`) and does not provide real snapshot semantics either. Real semantics deferred to a future phase with a concrete consumer (likely block importer)."
- [ ] **Do not alter `ShadowExecutionSPI` snapshot path in Phase 1.** Per LD-6 #4, shadow is being de-emphasized (LD-1) and is the only Java caller of `createSnapshot` / `revertToSnapshot`. The fact that the fail-loud flip will break shadow's snapshot round trip is the **intended signal** that shadow should not be relied on for snapshot verification. Track the shadow-fallout investigation as a separate item — LD-6 does not require fixing shadow.
- [ ] **Conformance / test sweep**: confirm zero Phase 1 production tests depend on `create_evm_snapshot` / `revert_to_evm_snapshot` returning `success: true` before flipping the handlers. The audit found zero such tests but a final sweep should accompany the fail-loud flip in case anything landed since LD-6 was written. Once the flip lands, re-tick the §1.4 acceptance items "Fake snapshot success is no longer an accepted state" and "Snapshot-dependent APIs are either real, explicitly fail-loud, or loud-degrade — never silently fake" (LD-6 + LD-9 two-halves form) together.
- [ ] **LD-6 reference comment cleanup**: once all the cleanups above land and the parity dashboard is green for one Phase 1 milestone window, remove the LD-6 cross-reference comments from `RemoteExecutionSPI.java`, `ExecutionSPI.java`, `mod.rs`, and `engine.rs`.

### 1.5 Build a contract support matrix

Primary touchpoints:

- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- `rust-backend/crates/core/src/service/mod.rs`
- `rust-backend/config.toml`

- [x] Enumerate all contract types currently seen by `RemoteExecutionSPI` — **41 variants** captured in LD-7 audit (full proto enum from `Tron.proto` lines 338-380, with classification of which are seen by Java's `buildExecuteTransactionRequest` switch and which fall through `default:`).
- [x] Mark each as (sub-checkboxes mean "decision answered", not "option selected" — see LD-7 matrix):
  - [x] `EE` only — answered: 4 contracts (`ShieldedTransferContract`, `VoteAssetContract`, `CustomContract`, `GetContract`)
  - [x] `RR` blocked — answered: 24 contracts (TRC-10 group, proposal group, account-mgmt group, contract-metadata group, brokerage, resource/delegation, exchange, market)
  - [x] `RR` candidate — answered: 9 contracts (`VoteWitnessContract`, `AccountCreateContract`, `WithdrawBalanceContract`, all 4 freeze variants, `TriggerSmartContract`, `CreateSmartContract`, with explicit blocking gaps to LD-2/LD-4/§5.1/§5.2)
  - [x] `RR` canonical-ready — answered: 4 contracts (`TransferContract`, `WitnessCreateContract`, `WitnessUpdateContract`, `AccountUpdateContract`)
- [x] For each contract type, record (LD-7 matrix entries explicitly capture each dimension via the per-contract Notes column and the cross-cutting findings; LD-7 defines the classification rule rather than enumerating every dimension as a separate per-contract checkbox):
  - [x] depends on read-path closure — Tracked as a category-level dependency (VM contracts depend on §2.1/§2.2 read-path closure; non-VM contracts do not).
  - [x] depends on TRC-10 semantics — Captured: TRC-10 group (5 contracts) gated by `trc10_enabled`; `TriggerSmartContract` separately blocked by §5.1 pre-execution transfer gap.
  - [x] depends on freeze/resource sidecars — Captured: 4 freeze contracts gated on §5.2 sidecar parity (`emit_freeze_ledger_changes`, `emit_global_resource_changes`).
  - [x] depends on dynamic-property strictness — Captured globally via LD-2 / `strict_dynamic_properties` Profile A override; not a per-contract gate in Phase 1.
  - [x] has fixture coverage — Captured globally: conformance fixtures **do** exist under `conformance/fixtures/` (30+ per-contract directories picked up by `conformance/runner.rs:1416`); per-row mapping and coverage accounting are intentionally **not** a per-contract LD-7 promotion gate, they are a §3.4 deliverable.
  - [x] has Rust unit coverage — Captured per-contract; LD-7 explicitly lists handlers with **no dedicated handler test file under `service/tests/contracts/`** (some still have inline `#[cfg(test)] mod tests` blocks in `service/contracts/*.rs`, e.g. `WithdrawBalanceContract` via `withdraw.rs` inline tests): `WithdrawBalance`, `ProposalCreate`, `ClearABI`, `UpdateBrokerage`, `WithdrawExpireUnfreeze`, `ExchangeInject`, `ExchangeWithdraw`, `ExchangeTransaction`, `MarketCancelOrder`.
  - [x] has `EE-vs-RR` replay coverage — Captured globally: **no offline EE-vs-RR replay harness exists** (only in-process `ShadowExecutionSPI`, which LD-1 de-emphasizes). LD-7 explicitly excludes replay coverage from per-contract promotion gates and assigns it uniformly to §3.4.
- [x] Split the matrix into:
  - [x] core high-value contracts for Phase 1 acceptance — LD-7 "Phase 1 RR whitelist target" defines this set: 4 already-canonical-ready + 7 freeze/account/vote/withdraw candidates after LD-2 flip + 2 VM contracts after LD-4/§5.1 = 13 whitelist-target contracts.
  - [x] secondary contracts that can remain `RR` blocked longer — LD-7 lists all 24 `RR blocked` contracts as explicitly **not** in the Phase 1 whitelist, with per-group justification (TRC-10/proposal/exchange/market/etc are deferred to later phases).

Acceptance:

- [ ] Remote enablement is no longer driven only by config convenience
- [x] There is an explicit `RR` whitelist target for the end of this phase — **LD-7 §"Phase 1 RR whitelist target"** lists the explicit set of 13 contracts targeted for canonical-ready status by end-of-phase, with promotion rules that gate each candidate on a specific LD-2..LD-6 / §5.x deliverable.

> Decisions captured in `close_loop.planning.md § Locked Decisions LD-7
> (Contract Support Matrix)`. LD-7 contains the full audit (41 proto
> variants, Java dispatch surface, Rust dispatch surface, JVM
> property table, code-default vs config.toml mismatch table, known
> parity gaps), the locked classification rule with 4 buckets, the
> per-contract matrix, the Phase 1 RR whitelist target, and 9
> mandatory cleanup items.
>
> The first acceptance item ("Remote enablement is no longer driven
> only by config convenience") is intentionally still `[ ]` because
> it is a state assertion, not a decision. The gate is **scoped to
> Phase 1 whitelist-target contracts**: today `vote_witness_enabled`,
> `account_create_enabled`, `withdraw_balance_enabled`, and the four
> freeze flags are all `code-default false` but `config.toml true`,
> which means the whitelist-target contracts are enabled in `RR`
> only because of a `config.toml` override. LD-7 cleanup item #1
> flips those specific code-defaults to `true`; once that lands, the
> whitelist-target contracts no longer depend on a `config.toml`
> override for `RR` enablement. This does **not** require
> `config.toml` to stop being an opinionated override in general
> (LD-3/LD-7 keep it as a parity-experiment profile for
> non-whitelist flags) — only that whitelist-target contracts have
> safe code-defaults of their own.

#### 1.5 deferred follow-ups

These are the open follow-ups (code, test, and CI work) that fall out
of LD-7 and stay open until the supporting work (and impact analysis
on §3.4 replay coverage and §5.x sidecar parity) is done:

- [ ] **Code-default flip for Phase 1 canonical-ready flags** — flip `vote_witness_enabled`, `account_create_enabled`, `withdraw_balance_enabled`, `freeze_balance_enabled`, `unfreeze_balance_enabled`, `freeze_balance_v2_enabled`, `unfreeze_balance_v2_enabled` from `default: false` to `default: true` in `rust-backend/crates/common/src/config.rs`, then drop the *specific* matching override lines for those flags from `rust-backend/config.toml` (other `config.toml` overrides remain intentional per LD-3/LD-7). Bundles with the LD-2 §1.1 code-default flip cleanup. Required before any of these contracts can promote from `RR candidate` to `RR canonical-ready`.
- [ ] **Add dedicated handler test files under `service/tests/contracts/`** for `RR blocked` handlers that currently have no dedicated file in that directory (some still have inline `#[cfg(test)] mod tests` coverage in `service/contracts/*.rs`): `ProposalCreateContract`, `ClearABIContract`, `UpdateBrokerageContract`, `WithdrawExpireUnfreezeContract`, `ExchangeInjectContract`, `ExchangeWithdrawContract`, `ExchangeTransactionContract`, `MarketCancelOrderContract`. (The `RR candidate` `WithdrawBalanceContract` is covered separately in follow-up #3 below — it already has inline tests in `withdraw.rs` but no dispatch-level file in the directory.) Tests live in `rust-backend/crates/core/src/service/tests/contracts/`. Each handler has a corresponding `mod.rs` dispatch entry; tests should at minimum cover happy path + one validation failure.
- [ ] **Add a dedicated `service/tests/contracts/withdraw_balance.rs` test file** — the handler at `rust-backend/crates/core/src/service/contracts/withdraw.rs:229` already computes both allowance and delegation reward (via `delegation::withdraw_reward` at `withdraw.rs:326`, not allowance-only), and inline unit tests exist in the same file (~`withdraw.rs:623`). The cleanup is purely a consistency item so every Phase 1 whitelist-target contract has a dispatch-level test file under `service/tests/contracts/`. Part of follow-up #2 above; tracked separately so it can be sequenced against the LD-2 code-default flip that promotes `WithdrawBalanceContract` from `RR candidate` to `RR canonical-ready`.
- [ ] **Resolve §5.1 TRC-10 pre-execution transfer for `TriggerSmartContract`** — replace the explicit reject at `rust-backend/crates/execution/src/lib.rs#L507-L521` with a real implementation matching `VMActuator.call()`'s Java semantics for `MUtil.transferToken(caller → contract)`. This is a §5.1 deliverable; LD-7 only tracks the dependency. Required before `TriggerSmartContract` can promote to `RR canonical-ready`.
- [ ] **Resolve LD-4 `energy_limit` cutover for both VM contracts** — already tracked under §1.2 deferred follow-ups. LD-7 only notes the dependency: both `TriggerSmartContract` and `CreateSmartContract` are blocked from canonical-ready promotion until the SUN-vs-energy-units cutover lands.
- [ ] **Audit Java JVM property names vs Rust config flag names for asymmetry** — Java groups multiple contracts under one JVM property (e.g. `-Dremote.exec.proposal.enabled` covers all three proposal contracts) while Rust has per-contract `_enabled` flags. This means Java can be enabled without all Rust sides, or vice-versa. Either (a) document the asymmetry with a warning in `RemoteExecutionSPI` startup, (b) add a runtime guard that errors when the layers disagree, or (c) collapse one side. Bundles with LD-2 cleanup #2 (runtime guard for unsafe mode combinations).
- [ ] **Decide whether to remove the Rust `_ => Err(...)` wildcard arm in `execute_non_vm_contract`** at `rust-backend/crates/core/src/service/mod.rs` ~L984-L990 — removing it would force a compile-time exhaustiveness check on `TronContractType` so any new variant becomes a build error rather than a runtime miss. LD-7 defers this to a later phase but tracks it as a known follow-up.
- [ ] **Add CI grep / lint that flags any new `ContractType` proto variant not present in the LD-7 matrix** — re-classify every new variant before merge. Today the matrix is a snapshot from the audit; without a CI gate, future proto changes will silently desync from LD-7.
- [ ] **Per-contract conformance fixture coverage audit** — explicitly **not** an LD-7 per-contract gate. Fixtures already exist under `conformance/fixtures/` (30+ per-contract directories picked up by `rust-backend/crates/core/src/conformance/runner.rs:1416`). The deliverable is owned by §3.4 (storage tests + EE/RR comparison checks): (a) map each LD-7 matrix row to its corresponding fixture directory, (b) identify which Phase 1 whitelist contracts have insufficient fixture rows, (c) extend coverage where thin. LD-7 records the fixtures-exist finding only to correct an earlier draft that claimed "zero conformance fixtures on disk".
- [ ] **Re-tick the §1.5 acceptance "Remote enablement is no longer driven only by config convenience"** once cleanup item #1 (code-default flips for the seven whitelist-target contract flags) lands. Gate condition is scoped to whitelist-target contracts only — `config.toml` can remain an opinionated parity-experiment profile for non-whitelist flags per LD-3/LD-7. Joint acceptance gate with the LD-2 §1.1 acceptance item that depends on the same flip.

---

## 2. Execution read-path closure

Goal: close the biggest gap between "write-path already exists" and "node-level execution capability is still incomplete".

### 2.1 Java execution bridge tasks

Primary touchpoints:

- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`

- [ ] Replace placeholder `callContract(...)` with a real RPC-backed path (implementation; LD-8 locks the contract, see §2.1 deferred follow-ups)
- [ ] Replace placeholder `estimateEnergy(...)` with a real RPC-backed path (implementation; LD-8 locks the contract, see §2.1 deferred follow-ups)
- [ ] Replace placeholder `getCode(...)` (implementation; LD-8)
- [ ] Replace placeholder `getStorageAt(...)` (implementation; LD-8)
- [ ] Replace placeholder `getNonce(...)` (implementation; LD-8)
- [ ] Replace placeholder `getBalance(...)` (implementation; LD-8)
- [ ] Replace placeholder `createSnapshot(...)` (implementation; LD-6/LD-8 — locked as Phase 1 unsupported, the "replacement" is `return CompletableFuture.failedFuture(new UnsupportedOperationException(...))`)
- [ ] Replace placeholder `revertToSnapshot(...)` (implementation; LD-6/LD-8 — same as above)
- [ ] Replace placeholder `healthCheck(...)` (implementation; wire to `ExecutionGrpcClient.healthCheck()` per LD-8 cleanup #7)
- [x] Normalize timeout handling across all remote execution APIs — **DECISION locked in LD-8 §6**: `getCode`/`getStorageAt`/`getNonce`/`getBalance` = 5s, `callContract`/`estimateEnergy` = 30s, `healthCheck` = 2s, snapshot/revert fail fast (no RPC). The current `ExecutionGrpcClient` uses a single hardcoded `DEFAULT_DEADLINE_MS` on every method; LD-8 requires per-method overloads or a constructor-time budget table — there is **no public `ExecutionGrpcClient.withDeadlineAfter` helper** today, so the mechanism is a client API addition tracked as cleanup #5 (bundled with cleanup #4a). Per-call override allowed. No method may call the stub without a deadline.
- [x] Normalize error mapping across all remote execution APIs — **DECISION locked in LD-8 §4**: error mapping is **split between two layers** because `ExecutionGrpcClient` today catches `StatusRuntimeException` and wraps into a generic `RuntimeException`, destroying the transport `Status.Code` before the SPI layer sees it. The split is: (a) **transport layer** — `ExecutionGrpcClient` refactored (cleanup #4a) to wrap every caught `StatusRuntimeException` into a new `RemoteExecutionTransportException(io.grpc.Status status, String methodName, Throwable cause)` that exposes `getStatus()` / `getMethodName()`; (b) **SPI layer** — two helpers (cleanup #4b) `mapTransportStatus(RemoteExecutionTransportException e)` and `mapApplicationResponse(response, String methodName)` implementing the LD-8 §4 translation table (`UNIMPLEMENTED`→`UnsupportedOperationException`, `UNAVAILABLE`/`DEADLINE_EXCEEDED`→transport `RuntimeException`, `INTERNAL`/application `success=false`→execution `RuntimeException`, `NOT_FOUND`/application `found=false`→per-API empty value, `OK`+`success=true`+`found=true`→value). No fallback branch that recovers with a default.
- [x] Decide when Java should: — **DECISION locked in LD-8 §1**
  - [x] fail hard — locked: `return CompletableFuture.failedFuture(new UnsupportedOperationException("Remote execution read-path <method> not yet implemented (LD-8)"))` for all eight non-health placeholders until real RPC wiring lands. All eight methods return `CompletableFuture<T>`, so the failed-future form (not a synchronous `throw`) is the only locked form — it matches LD-6's async phrasing and does not cross the sync/async boundary at callers. `callContract` must NOT mask the gap with a legitimate-looking `ExecutionProgramResult` revert.
  - [x] return explicit remote unsupported — locked: the Java failed-future is the explicit signal; Rust side returns `Status::unimplemented` at the transport layer (LD-8 §2), which the SPI-layer `mapTransportStatus` helper (LD-8 §4) translates back to `UnsupportedOperationException` surfaced via `CompletableFuture.failedFuture` — only after cleanup #4a stops `ExecutionGrpcClient` from swallowing the status code.
  - [x] fall back only where this is still intentionally allowed — locked: **no fallback in Phase 1**. LD-1 de-emphasizes SHADOW, so the cross-mode rescue rationale is gone. If `RR` read fails the call fails — the only "recovery" is "fail and let the caller decide". `healthCheck` is the one exception (`CompletableFuture.completedFuture(new HealthStatus(false, reason))` instead of a failed future, because health is a probe).
- [x] Remove planning assumptions that these APIs will be validated mainly through `SHADOW` — **DECISION locked**: LD-1 already de-emphasizes SHADOW; LD-8 makes the consequence explicit in the "Why these choices" subsection ("Without SHADOW as a safety net, any fallback in LD-8 would silently hide `RR` regressions behind `EE` results"). Validation is now `EE`-vs-`RR` paired, with the caveat that §2.4 paired tests can only assert "both sides fail consistently" until at least one side has a real read implementation (LD-8 cleanup #12).

#### 2.1 deferred follow-ups

These are the implementation-shaped items captured in LD-8 but NOT done in this
doc-only iteration. They remain `[ ]` until code lands.

- [ ] **LD-8 cleanup #1** — Flip the **eight non-health** Java placeholders in `RemoteExecutionSPI.java#L119-L239` (`callContract`, `estimateEnergy`, `getCode`, `getStorageAt`, `getNonce`, `getBalance`, `createSnapshot`, `revertToSnapshot`) to `return CompletableFuture.failedFuture(new UnsupportedOperationException("Remote execution read-path <method> not yet implemented (LD-8)"))`. All eight signatures are `CompletableFuture<T>`, so the failed-future form is the only locked form (matches LD-6's async phrasing; a synchronous `throw` would break `.exceptionally()` chains at callers). `healthCheck` is the one exception — it keeps its `CompletableFuture.completedFuture(new HealthStatus(false, ...))` shape.
- [ ] **LD-8 cleanup #4a** — Refactor `ExecutionGrpcClient` transport-error handling. The nine methods at `ExecutionGrpcClient.java#L91-L291` currently wrap `StatusRuntimeException` into a generic `RuntimeException`, stripping `io.grpc.Status.Code` before the SPI layer sees it. Replace the wrap-and-rethrow with a new `RemoteExecutionTransportException(io.grpc.Status status, String methodName, Throwable cause)` that exposes `io.grpc.Status getStatus()` and `String getMethodName()`. This is the **single locked exception type** the SPI layer sees for transport failures; no "rethrow `StatusRuntimeException` unchanged" alternative is allowed, so that SPI consumers never need to import `io.grpc.*`. Hard prerequisite for cleanups #4b and #5.
- [ ] **LD-8 cleanup #4b** — Add two SPI-layer mapping helpers in `framework/.../execution/spi/RemoteExecutionStatusMapper.java`: `mapTransportStatus(RemoteExecutionTransportException e)` — reads `e.getStatus().getCode()` and `e.getMethodName()`, implements the transport rows of the LD-8 §4 table — and `mapApplicationResponse(response, String methodName)` — implements the application rows. Disjoint input types; they cannot be merged into one helper. Used in every **non-health** read-path method (the eight methods: `callContract`, `estimateEnergy`, `getCode`, `getStorageAt`, `getNonce`, `getBalance`, `createSnapshot`, `revertToSnapshot`) once cleanup #4a and real wiring land. `healthCheck` is explicitly carved out — it has its own probe contract (cleanup #7) and never routes through these helpers.
- [ ] **LD-8 cleanup #5** — Add per-method default deadlines to every read-path call (LD-8 §6 budget table: 5 s / 30 s / 2 s). The current `ExecutionGrpcClient` applies a single hardcoded `DEFAULT_DEADLINE_MS`; there is **no public `ExecutionGrpcClient.withDeadlineAfter` helper**, so this cleanup adds per-method overloads (e.g. `callContract(CallContractRequest, Duration deadline)`) or a client-constructor parameter carrying the budget table. Per-call override allowed. Bundles with cleanup #4a.
- [ ] **LD-8 cleanup #7** — Wire `RemoteExecutionSPI.healthCheck` through `ExecutionGrpcClient.healthCheck()` (the existing method name — **not** `health()`). Map the Rust `HealthResponse` (proto fields: `Status status` enum with `HEALTHY`/`UNHEALTHY`/`DEGRADED`, `string message`, `map<string,string> module_status` — see `backend.proto#L76-L85`) to Java's boolean-only `HealthStatus(boolean, String)`: `HEALTHY`→`(true, response.getMessage())`, `DEGRADED`→`(false, "degraded: " + response.getMessage() + " - " + response.getModuleStatusMap())`, `UNHEALTHY`/other→`(false, response.getMessage())`. The `DEGRADED` case collapses to `healthy=false` with the per-module nuance serialized into the message. Rust side is already real; Java-only.
- [ ] **LD-8 cleanup #9** — Add Javadoc to `ExecutionSPI.java` documenting the LD-8 Phase 1 failure semantics: every **non-health** read-path method (the eight listed in LD-8 §1) returns a `CompletableFuture` that may complete exceptionally with `UnsupportedOperationException("<message> (LD-8)")` until wired, and callers must not silently absorb the exceptional completion (no unconditional `.exceptionally(e -> defaultValue)`). `healthCheck` is explicitly carved out — its Javadoc documents the probe contract (always `completedFuture(HealthStatus)`, `healthy=false` on failure, never a failed future). Bundles with LD-6 §1.4 Javadoc.
- [ ] **LD-8 cleanup #14** — Remove `// TODO: Implement in Task 2` comments from `RemoteExecutionSPI.java` after cleanup #1 lands; they are stale once the fail-loud flip happens.
- [ ] **LD-8 cleanup #15 (Java half)** — Add normative `warn`-level failure logging in the SPI-layer helpers (#4b): `mapTransportStatus` and `mapApplicationResponse` log at `warn` with the method name plus the raw `Status.Code` (or application `error_message`) before throwing. No structured-logging format is locked — only the level and the required fields.

### 2.2 Rust execution gRPC tasks

Primary touchpoints:

- `rust-backend/crates/core/src/service/grpc/mod.rs`
- `rust-backend/crates/execution/src/lib.rs`
- storage adapter code used by execution query paths

- [ ] Implement `get_code` (implementation; LD-8 locks the contract — see §2.2 deferred follow-ups)
- [ ] Implement `get_storage_at` (implementation; LD-8)
- [ ] Implement `get_nonce` (implementation; LD-8)
- [ ] Implement `get_balance` (implementation; LD-8)
- [x] Decide whether `create_evm_snapshot` is in scope this phase: — **DECISION locked in LD-6 §1.4 and reinforced by LD-8 §2**
  - [ ] if yes, define storage/journal backing model — N/A (not in scope)
  - [x] if no, return explicit unsupported — **locked**: Rust returns `Err(tonic::Status::unimplemented("create_evm_snapshot not implemented in Phase 1 (LD-6/LD-8)"))` at the transport layer, **not** application-level `{success: true, snapshot_id: "<uuid>"}` (the current placeholder is a dangerous fake that must be removed — see cleanup #2). Java side returns `CompletableFuture.failedFuture(new UnsupportedOperationException(...))`.
- [x] Decide whether `revert_to_evm_snapshot` is in scope this phase: — **DECISION locked in LD-6 §1.4 and reinforced by LD-8 §2**
  - [ ] if yes, define rollback semantics — N/A (not in scope)
  - [x] if no, return explicit unsupported — **locked**: same rule as `create_evm_snapshot`. Rust returns `Status::unimplemented`; Java returns `CompletableFuture.failedFuture(new UnsupportedOperationException(...))`. Current `{success: true}` placeholder (ignoring `snapshot_id`) must be removed.
- [x] Make `health` reflect actual module readiness, not a placeholder — **Rust side already real** per LD-8 audit: `service/grpc/mod.rs#L98-L133` aggregates `module_manager.health_all()` per-module. No placeholder on the Rust side. Java-side `healthCheck` in `RemoteExecutionSPI` is still a placeholder but is tracked as LD-8 cleanup #7 (§2.1 deferred follow-ups).
- [x] Add logging/diagnostics that can explain which remote query path failed — **DECISION locked in LD-8 "What this lock does NOT cover"**: every read-path failure path logs at `warn` with method name + status (not a specific structured-logging format, which is a later-phase concern). The behavior is normative and tracked as **LD-8 cleanup #15** (Java half in §2.1 deferred follow-ups, Rust half in §2.2 deferred follow-ups below) — this `[x]` only marks the decision that logging is required, not the implementation.

#### 2.2 deferred follow-ups

Implementation-shaped items captured in LD-8 but NOT done in this doc-only iteration.

- [ ] **LD-8 cleanup #2** — Flip the four Rust gRPC placeholders at `service/grpc/mod.rs#L1802-L1868` (`get_code`, `get_storage_at`, `get_nonce`, `get_balance`) from `{success: false, error_message: "Not implemented"}` to `Err(tonic::Status::unimplemented("<rpc_method> not implemented in Phase 1 (LD-8)"))`. Same sweep covers `create_evm_snapshot` / `revert_to_evm_snapshot` at `mod.rs#L1870-L1899` (LD-6 §1.4 cleanup).
- [ ] **LD-8 cleanup #3** — Remove the `estimate_energy` fake `21000` fallback at `mod.rs#L1716-L1800` error path. Set `energy_estimate: 0` alongside `success: false` (or return `Err(Status::internal(...))` when the error is not semantic). No plausible-default values anywhere in the read-path.
- [ ] **LD-8 cleanup #6** — Enforce `snapshot_id` rejection in the four Rust read handlers: non-empty `request.snapshot_id` → `Err(Status::unimplemented("snapshot read not supported in Phase 1 (LD-6/LD-8)"))`. Document the asymmetry with LD-5 (storage-layer `snapshot_id` is silently ignored; execution-layer is rejected — LD-8 §5 explains why).
- [ ] **LD-8 cleanup #8** — Build the Rust query façade on top of `EngineBackedEvmStateStore` for `get_code` / `get_storage_at` / `get_nonce` / `get_balance`. This is the core §2.2 implementation work — no `EvmStateStore` query API exists at the gRPC layer today, so a thin read-only façade has to land before the four RPC methods can return real data.
- [ ] **LD-8 cleanup #11** — Add Rust gRPC tests asserting each placeholder RPC returns `Status::unimplemented`. Negative tests only — the positive tests come after cleanup #8.
- [ ] **LD-8 cleanup #15 (Rust half)** — Add normative `warn`-level failure logging on every Rust read-path RPC (`get_code` / `get_storage_at` / `get_nonce` / `get_balance` / `call_contract` / `estimate_energy` / `create_evm_snapshot` / `revert_to_evm_snapshot`). Log at `warn` with the RPC method name plus the returned `tonic::Status` or application error before returning. No structured-logging format is locked — only the level and the required fields.

### 2.3 Request/response semantic alignment

- [ ] Verify Java request builders carry all fields required by Rust query APIs (implementation verification; gated on LD-8 cleanup #1/#8 — no real requests exist yet)
- [x] Verify snapshot identifiers, if kept, have stable cross-side meaning — **DECISION locked in LD-6 §1.4 and LD-8 §5**: the only stable cross-side meaning for execution-layer `snapshot_id` in Phase 1 is "empty string = current state; anything else = `Status::unimplemented`". Storage-layer `snapshot_id` retains its LD-5 silent-ignore behavior; the asymmetry is intentional (execution-layer callers are actively requesting a feature LD-6 locks out, so failing loud is correct).
- [x] Verify query responses distinguish: — **DECISION locked in LD-8 §4** (the Java error-mapping table is the single source of truth for how each branch is surfaced)
  - [x] not found — locked: `found=false, success=true` on `GetCodeResponse` / `GetStorageAtResponse` / `GetNonceResponse` / `GetBalanceResponse`; collapses to per-API empty value in Java. `CallContractResponse` / `EstimateEnergyResponse` do NOT have a `found` field per LD-8 §7 (a call to a non-existent contract is a semantic result, not a not-found error).
  - [x] unsupported — locked: `tonic::Status::UNIMPLEMENTED` at the transport layer (LD-8 §2), wrapped into `RemoteExecutionTransportException` by `ExecutionGrpcClient` (cleanup #4a), then translated to `UnsupportedOperationException` by the SPI-layer `mapTransportStatus` helper (LD-8 §4 / cleanup #4b). NOT collapsed into application-level `{success: false}`.
  - [x] internal error — locked: application-level `{success: false, error_message}` on the success-shaped response (handled by `mapApplicationResponse`) OR transport `Status::INTERNAL` (handled by `mapTransportStatus`). Both map to execution `RuntimeException`. Reserved for semantic failures of a real implementation, never for "the method doesn't exist yet".
  - [x] transport error — locked: `Status::UNAVAILABLE` / `DEADLINE_EXCEEDED` → `RemoteExecutionTransportException` → `mapTransportStatus` → transport `RuntimeException`. Triggered by the LD-8 §6 per-method deadlines; no fallback to a default value (LD-8 §1 fallback ban).
- [x] Verify `estimateEnergy` comparison rules in `EE-vs-RR` validation: — **DECISION locked in LD-8 §8**
  - [x] exact match — locked as the **post-LD-4** target state. Any delta triggers a regression alert.
  - [x] tolerated delta — **locked as NOT used, even pre-LD-4**. The LD-4 bug (`gas_limit / energy_fee_rate` at `lib.rs#L534/L554`) changes the cap **multiplicatively**, so near-cap mismatches can be arbitrarily large — not a small additive delta. LD-8 §8 therefore locks the pre-LD-4 interim rule as "no cross-mode parity expectation; record the observed pair, do not alert, do not tune a tolerance window". Bounded-delta assertions are banned until LD-4 lands.
  - [x] per-contract exception list if needed — **locked as NOT used**: LD-8 §8 explicitly rejects this. Deltas are either real bugs or noise that must be root-caused; persistent per-contract exceptions are banned.

#### 2.3 deferred follow-ups

- [x] **LD-8 cleanup #13** — Cross-link LD-8 with LD-4 in `close_loop.todo.md` §1.2 deferred follow-ups. **Done in this doc iteration**: the §1.2 "LD-4 ↔ LD-8 cross-link" bullet above captures the hard-prerequisite relationship and the pre-LD-4 "no cross-mode parity expectation" interim rule.

### 2.4 Execution read-path tests

Java-focused:

- [ ] Add focused Java tests for each remote execution read/query API
- [ ] Add paired `EE` baseline vs `RR` target tests where the harness can run both paths separately

Rust-focused:

- [ ] Add gRPC service tests for each query API
- [ ] Add execution-level tests for common EOA/contract states
- [ ] Add negative tests for unsupported snapshot/revert if that is the chosen temporary behavior

Acceptance:

- [ ] Node-level remote execution no longer depends on placeholder query APIs
- [ ] `callContract` and `estimateEnergy` are usable in `RR`
- [ ] Query APIs either work or fail explicitly, never with fake success payloads

#### 2.4 deferred follow-ups

- [ ] **LD-8 cleanup #10** — Add focused Java tests asserting each of the **eight non-health** placeholder methods in `RemoteExecutionSPI` returns a `CompletableFuture` that completes exceptionally with `UnsupportedOperationException` until real wiring lands (e.g. `assertThatThrownBy(() -> future.get()).hasCauseInstanceOf(UnsupportedOperationException.class)`). Add a separate `healthCheck` test asserting the placeholder returns `completedFuture(new HealthStatus(false, "<reason>"))` — **not** a failed future — to lock the LD-8 §1 probe-contract carve-out. Negative tests only; positive tests come after cleanup #1 / #8 land.
- [ ] **LD-8 cleanup #12** — Refresh the §2.4 paired `EE`-vs-`RR` test plan to reflect that `EmbeddedExecutionSPI` read methods are **also** placeholders (audit confirmed zero/empty returns at `EmbeddedExecutionSPI.java#L132-L222`). Until at least one side has a real read implementation, paired tests can only assert "both sides fail in the same way" — not a usable parity oracle.

---

## 3. Storage semantic hardening

Goal: upgrade storage from "hot-path operations work" to "execution can safely rely on the semantics it claims to expose".

> **LD-9 scope lock.** §3.1 and §3.2 implementation work is deferred wholesale to the block importer phase (inheritance from LD-5); two §3.1 rows and three §3.2 rows are marked `[x]` because LD-5 §1.3 cleanups and LD-5 #4 already discharge them (see §3.1 / §3.2 status lines). §3.3 implementation is deferred (inheritance from LD-6); the EVM-side "surface explicit unsupported" work is already tracked under LD-6 §1.4 cleanups, not re-opened here, and LD-9 newly picks LD-6 §1.4 option (b) (live-read + loud warn) for the storage-layer `getFromSnapshot` disjunction LD-6 left open. §3.4 tests are the only Phase 1 storage hardening deliverable, narrowed to the subset LD-5/LD-6 leave testable — see the §3.4 breakdown and §3 deferred follow-ups below. See `close_loop.planning.md § Locked Decisions LD-9` for the full audit.

### 3.1 `transaction_id` end-to-end plumbing

Primary touchpoints:

- `framework/src/main/proto/backend.proto`
- `framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

**Status: four of six tasks deferred to block importer phase under LD-5 / LD-9; two tasks are already covered by LD-5 §1.3 cleanups and are marked `[x]` here.** Deferred tasks stay `[ ]` and are tracked in the §3.1/§3.2 deferred follow-ups block below with an LD-5/LD-9 cross-reference.

- [ ] Audit all Java write calls that could carry `transaction_id` *(deferred; LD-5/LD-9)*
- [ ] Define where transaction identifiers are created and owned *(deferred; LD-5/LD-9)*
- [ ] Pass `transaction_id` through Java `put/delete/batchWrite` *(deferred; LD-5/LD-9)*
- [ ] Make Rust gRPC handlers branch on `transaction_id` instead of always writing directly *(deferred; LD-5/LD-9)*
- [x] Document default behavior for non-transaction-scoped writes — **Already covered by LD-5 §1.3 cleanup #1** (`backend.proto` field comment on `PutRequest.transaction_id` / `DeleteRequest.transaction_id` / `BatchWriteRequest.transaction_id`: "silently ignored by the current Rust gRPC write handlers; writes always go directly to the durable store"). LD-5 #5 locks the default as "direct write to durable store". No new LD-9 action.
- [x] Add tracing/logging that makes it obvious whether a write was transactional or direct — **Already covered by LD-5 §1.3 cleanup #5** (gRPC handler honesty: `mod.rs` logs a warning when `PutRequest`/`DeleteRequest`/`BatchWriteRequest` arrives with a non-empty `transaction_id`). No new LD-9 action.

### 3.2 Transaction buffer semantics in Rust storage

Primary touchpoints:

- `rust-backend/crates/storage/src/engine.rs`

**Status: six of nine tasks deferred to block importer phase under LD-5 / LD-9; three decision-flavored tasks are already answered by LD-5 #4 and are marked `[x]` here.** Deferred implementation rows stay `[ ]` and are tracked in the §3.1/§3.2 deferred follow-ups block below. The three `[x]` rows are "Decide read-your-writes" (not supported), "If read-your-writes is required, design layered read" (vacuous because predicate is false), and "Decide tx-scoped iterators" (explicitly unsupported) — all answered by LD-5 #4.

- [ ] Add real per-transaction operation buffers *(deferred; LD-5/LD-9)*
- [ ] Route transactional `put` into the buffer *(deferred; LD-5/LD-9)*
- [ ] Route transactional `delete` into the buffer *(deferred; LD-5/LD-9)*
- [ ] Route transactional `batch_write` into the buffer *(deferred; LD-5/LD-9)*
- [ ] Apply buffered operations atomically on `commit` *(deferred; LD-5/LD-9)*
- [ ] Discard buffered operations on `rollback` *(deferred; LD-5/LD-9)*
- [x] Decide read-your-writes behavior for transaction-scoped reads — **Answered by LD-5 #4**: not supported in Phase 1; the wire already forbids it (no `transaction_id` on `GetRequest` / `HasRequest` / `BatchGetRequest`).
- [x] If read-your-writes is required, design layered read behavior over buffered writes — **Not required** (LD-5 #4). No design work needed in Phase 1.
- [x] Decide whether transaction-scoped iterators/range queries are in scope or explicitly unsupported — **Answered by LD-5 #4**: explicitly unsupported. Iterator / prefix / range against a `transaction_id` is rejected at the wire level.

### 3.3 Snapshot correctness

Primary touchpoints:

- `rust-backend/crates/storage/src/engine.rs`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

**Status: implementation tasks deferred under LD-6 / LD-9; the fail-loud task is already tracked under LD-6 §1.4 cleanups and is not re-opened here.**

- [ ] Replace current "snapshot reads current DB" behavior with real point-in-time semantics *(deferred; LD-6/LD-9 — block importer phase)*
- [x] If real snapshot is not implemented this phase, remove fake behavior and surface explicit unsupported — **Tracked under LD-6 §1.4 cleanups**: `create_evm_snapshot` / `revert_to_evm_snapshot` flip to `tonic::Status::unimplemented`. **LD-9 makes a new decision here**: LD-6 §1.4 left `getFromSnapshot` with two options (a) `Status::unimplemented` vs (b) live-read + loud `tracing::warn!` **on every call** (not first-call-only); LD-9 picks option (b) as the Phase 1 default because `EmbeddedStorageSPI.getFromSnapshot` and the existing `StorageSPIIntegrationTest` round-trip assume the live-read fallback compiles. See planning.md LD-9 "Locked decision" item 2 for the full rationale. No new cleanup item here beyond what LD-6 §1.4 already tracks.
- [ ] Define snapshot lifecycle: — **Deferred as a whole** (LD-6/LD-9); lifecycle only matters for a real implementation, which Phase 1 does not have. Stays `[ ]` (nothing designed).
  - [ ] creation *(deferred; LD-6/LD-9)*
  - [ ] read paths allowed *(deferred; LD-6/LD-9)*
  - [ ] deletion *(deferred; LD-6/LD-9)*
  - [ ] cleanup on process shutdown *(deferred; LD-6/LD-9)*
- [x] Define interaction rules between transactions and snapshots — **Trivially answered**: no transactions + no snapshots = no interaction to define. Both sides are deferred (LD-5 / LD-6 / LD-9).
- [x] Decide whether iterator APIs against snapshot are needed now or later — **Locked as "later"**: LD-6 defers PIT snapshot entirely, so iterator-over-snapshot is trivially out of Phase 1 scope.

### 3.4 Storage tests and EE/RR comparison checks

> **LD-9 scope.** §3.4 is the only Phase 1 storage hardening deliverable. Tasks below are split into Phase-1-actionable rows (do now), Blocked-reframed rows (rewrite to assert placeholder honesty), and Blocked rows (move to §3.4 deferred follow-ups). See `planning.md § LD-9` for the full audit.

Rust-focused:

- [ ] Add unit tests for CRUD *(Phase 1 actionable; LD-9)*
- [ ] Add unit tests for batch writes *(Phase 1 actionable; LD-9)*
- [ ] Add unit tests asserting `commit_transaction` is a **loud no-op** *(LD-9 reframe of "unit tests for transaction commit"; LD-5 §1.3 cleanup #4 "Rust engine honesty" selects option (b) — keep the no-op code path and log loudly via `tracing::warn!`, without persisting a real tx buffer. Positive "commit persists the buffer" tests are deferred to block importer phase.)*
- [ ] Add unit tests asserting `rollback_transaction` is a **loud no-op** *(LD-9 reframe of "unit tests for transaction rollback"; same rationale — anchored in LD-5 §1.3 cleanup #4 option (b).)*
- [ ] Add unit tests asserting `get_from_snapshot` is live-read + `tracing::warn!` **on every call** (not first-call-only) cross-referencing LD-6 *(LD-9 reframe of "unit tests for snapshot correctness"; **LD-9 makes this decision**: it picks LD-6 §1.4 option (b) (live-read + loud warn on every call) over option (a) (`Status::unimplemented`) as the Phase 1 default. Positive PIT tests are deferred.)*
- [ ] Add tests for absent `transaction_id` *(Phase 1 actionable; LD-5 #5 locks "absent `transaction_id` = direct write to durable store" — test asserts that.)*
- [ ] Add tests for `transaction_id` not found / `snapshot_id` not found *(Phase 1 actionable; LD-9. `engine.rs:406` / `engine.rs:431` already fail on unknown `transaction_id`, and `engine.rs:465` already fails on unknown `snapshot_id` — plus `StorageSPIIntegrationTest.java:307` already asserts invalid-snapshot failure. Phase 1 keeps this row actionable: add symmetric unit coverage on the Rust side so the failure path is locked in before LD-5/LD-6 real semantics land.)*

Java-focused:

- [ ] Extend or add integration coverage around `RemoteStorageSPI` for CRUD + batch only *(Phase 1 actionable; LD-9)*
- [ ] Add tests proving Java **does not** carry `transaction_id` in Phase 1 writes, and Rust silently ignores any legacy value *(LD-9 reframe of "Java actually carries `transaction_id`"; LD-5 #5 locks absent as the canonical Phase 1 behavior. Positive carry-through deferred.)*
- [ ] Add `EE` run vs `RR` run semantic checks for CRUD/batch only *(Phase 1 actionable; LD-9 notes the storage `EE`-vs-`RR` surface is narrower than §2.4's because storage has no execution semantics to diverge on — parity is mostly "both return the same bytes for the same key".)*
- [x] Avoid using `DualStorageModeIntegrationTest` as if mode-switch wiring alone proves semantic parity — **Locked as a normative rule by LD-9**: any new test that only proves "mode switch compiles" must be labelled as such and does **not** count toward §3.4 acceptance. Enforcement mechanism (grep / Checkstyle / JUnit `@Tag`) is tracked as §3 deferred follow-up #5.

Acceptance:

- [ ] Storage transaction APIs are no longer structural placeholders — **Deferred by LD-5 (LD-9 inheritance)**. Stays `[ ]`: Phase 1 explicitly cannot tick this. Re-opens with block importer phase.
- [ ] Snapshot is real, explicitly fail-loud, or loud-degrade — never silently fake (LD-6 + LD-9 two-halves) — **Decided by LD-6 + LD-9**: the Phase 1 contract has two halves: (1) EVM snapshot/revert is **explicitly unavailable** (`UnsupportedOperationException` at the Java SPI + `Status::unimplemented` at Rust gRPC); (2) storage `getFromSnapshot` is **loud degrade to live-read + `tracing::warn!` on every call** per LD-9's lock of LD-6 §1.4 option (b). LD-9 treats both halves as "explicitly not a real snapshot, and every call is loud about it". Ticks when LD-6 §1.4 cleanups land (EVM fail-loud flip + storage every-call warn), not when §3.3 real PIT implementation lands. Stays `[ ]` until those cleanups land.
- [ ] Storage crate test suite has meaningful coverage and is no longer `0 tests` — **The only Phase 1 open-for-work acceptance gate for §3** (LD-9). CRUD + batch + placeholder-honesty tests are enough to clear this.

#### 3.1/3.2 deferred follow-ups (block importer phase)

Implementation for all fifteen tasks from §3.1 and §3.2 is deferred to the block importer phase under LD-5 (inherited by LD-9). Most rows remain `[ ]` in the progress tracker; the exceptions are:

- **§3.1 tasks 5 and 6** are `[x]` above because LD-5 §1.3 cleanups #1 and #5 already cover the proto field comments and the gRPC handler warn log. No new work here.
- **§3.2 tasks 7, 8, 9** (the three decision-flavored rows) are `[x]` above because LD-5 #4 already answered them as "read-your-writes and tx-scoped iterators are explicitly unsupported in Phase 1".

The implementation work that no Phase 1 chunk is allowed to open:

- [ ] **LD-5/LD-9 block importer phase**: Implement §3.1 `transaction_id` end-to-end plumbing (the four `[ ]` subtasks: audit Java writes, define tx-id ownership, pass through Java `put/delete/batchWrite`, branch Rust gRPC handlers). Do not start Phase 1 work on this; it depends on the block importer phase first defining a real commit boundary.
- [ ] **LD-5/LD-9 block importer phase**: Implement §3.2 transaction buffer semantics in Rust storage (the six implementation subtasks: buffer, put routing, delete routing, batch_write routing, atomic commit, rollback discard). Phase 1 does not open this work.

#### 3.3 deferred follow-ups (block importer phase)

- [ ] **LD-6/LD-9 block importer phase**: Implement real PIT snapshot semantics in `rust-backend/crates/storage/src/engine.rs` + corresponding gRPC handler, plus lifecycle (creation / read paths / deletion / shutdown cleanup). Phase 1 does not open this work.
- **Note**: The "surface explicit unsupported" task for Phase 1 is already tracked under §1.4 deferred follow-ups (`create_evm_snapshot` / `revert_to_evm_snapshot` fail-loud flip + `getFromSnapshot` option (b) loud-warn). LD-9 does not duplicate these items here.

#### 3.4 deferred follow-ups

- [ ] **Add tests for concurrent transaction IDs and cleanup paths** — **Deferred by LD-9**; no transaction state exists to concurrently access in Phase 1. Re-opens with block importer phase.
- [ ] **Positive "commit persists the buffer" tests** — deferred; requires LD-5 real implementation.
- [ ] **Positive "rollback discards the buffer" tests** — deferred; requires LD-5 real implementation.
- [ ] **Positive PIT snapshot tests** — deferred; requires LD-6 real implementation.
- [ ] **Positive "Java carries `transaction_id` through writes" tests** — deferred; requires LD-5 real implementation.
- [ ] **CI / grep lint for `DualStorageModeIntegrationTest` misuse** *(LD-9 cleanup #5)*: flag any new test file in `framework/src/test/.../storage/` that references `DualStorageModeIntegrationTest` without also exercising a real `RemoteStorageSPI` CRUD path. Mechanism is not locked (grep / Checkstyle / JUnit `@Tag` are all acceptable); only the normative rule.
- **Pointer (LD-9 cleanup #6)**: `RocksdbStorage` Java-side null-parameter guard tests exist as `CLAUDE.md` lessons-learned items (gRPC parameter validation, embedded storage defensive checks). LD-9 tracks them as §6 (verification) work, not §3 (storage hardening). No action in LD-9 — this note exists to ensure they don't fall between §3 and §6.

---

## 4. Close state-ownership gaps and bridge debt

Goal: reduce the number of "temporary bridge" pieces that hide split ownership between Java and Rust.

Primary touchpoints:

- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java`
- any code paths that pre-sync Java-side mutations before remote execution

- [x] Audit every place where Java mutates state and then pushes/synchronizes it to Rust — **LD-11 audit inventory** enumerates 5 bridges in §4 scope (3 pre-exec push + 1 block-context handshake + 1 Phase B conformance-mirror) with file:line references. The LD-1 canonical post-exec apply path (`applyStateChanges*` / `applyFreezeLedgerChanges` / `applyGlobalResourceChange` / `applyTrc10Changes` / `applyVoteChanges` / `applyWithdrawChanges`) is listed "for completeness" in LD-11 but is **explicitly excluded from §4's bridge-debt scope** because LD-1 locks it as the canonical `RR` writer in Phase 1.
- [x] Classify each bridge as:
  - [x] required in Phase 1 — **LD-11 classification matrix**: Category 1 pre-exec push bridges 1 (`collectPreExecutionAext`), 2 (`ResourceSyncContext` + `ResourceSyncService.flushPreExec`), 3 (`syncPostBlockRewardDeltas`) all fall here. Bridge 2 is flagged as the **load-bearing root** — removing it is the gate for Wave 2.
  - [x] removable once write ownership is frozen — **LD-11 classification matrix**: bridges 1–3 become removable once pre-exec resource accounting (`consumeBandwidth` / `consumeMultiSignFee` / `consumeMemoFee` + post-block reward accounting) migrates into Rust (Wave 2). Bridge 11 (Phase B conformance-mirror) becomes production-unused once the NonVm bypass closes in Wave 1, leaving bridge 11 as conformance-only tooling.
  - [x] must survive into block importer phase — **LD-11 classification matrix**: bridge 4 (`buildExecuteTransactionRequest` block-context handshake) must survive in minimal form because Rust has no persistent block/tx-scoped session; Wave 3 only shrinks its payload, does not eliminate it. Bridge 11 does **not** need to survive into block importer as production path — it is conformance-only tooling per LD-1/LD-2.
- [x] Document whether `ResourceSyncService` is:
  - [x] a transitional patch — **No, per LD-11**: it is not a transitional patch.
  - [x] a medium-term integration layer — **No, per LD-11**: it is not a medium-term integration layer either.
  - [x] fundamentally incompatible with final ownership goals — **Yes, per LD-11**: load-bearing symptom of split ownership for Phase 1. Classified as "incompatible with 'Rust owns state transitions' in its current form, and load-bearing for the duration of Phase 1". Removal is gated on migrating `consumeBandwidth` / `consumeMultiSignFee` / `consumeMemoFee` + block-reward accounting into Rust — a Phase 2 block importer activity (Wave 2).
- [x] Write an explicit "bridge removal sequence" note for after Phase 1 — **LD-11 three-wave removal sequence**: Wave 1 (close the NonVm bypass in `rust-backend/crates/core/src/service/grpc/mod.rs` so production `RR` never takes `WriteMode::PERSISTED` + `postExecMirror`, aligning NonVm with LD-1/LD-2; bridge 11 becomes conformance-only in reality), Wave 2 (migrate pre-exec resource accounting into Rust, at which point bridges 1–3 + `ResourceSyncService` can be deleted), Wave 3 (shrink bridge 4's handshake payload to the minimum block-context envelope once Rust owns block-session state — bridge 4 survives in minimal form, it does not disappear). Each wave is gated on the prior wave landing.
- [x] Confirm no new bridge mechanism should be added without first checking ownership implications — **Locked by LD-11**: "No new bridge mechanism is added without first classifying it against this LD's matrix." Enforcement mechanism is deferred to cleanup #4 (CI grep / review rule) with explicitly narrowed scope to the `framework/src/main/java/org/tron/core/storage/sync/**` namespace (not `org/tron/core/net/**`, not `org/tron/core/consensus/**`).

Acceptance:

- [x] The project has an explicit list of temporary bridge mechanisms — **LD-11 audit inventory** provides the explicit list (5 numbered items in §4 scope, each with file:line + what-it-does + why-it-exists + classification, plus LD-1 canonical apply methods listed separately for completeness). Ticked on LD-11 landing.
- [ ] Temporary bridge debt is visible and sequenced, not hidden — **Sequenced by LD-11** (three-wave removal sequence), but **visibility is not yet complete** — LD-11 cleanups #1–#3 (one severe correctness hole plus one clarity cleanup plus one real coverage gap fix caught during the audit: `updateAccountStorage` drops Rust-emitted storage-slot `StateChange`s in any non-Phase-B path, dead/misleading post-apply dirty-marking across the canonical apply path, per-tx `DelegationStore` gap) must land before "visible" can tick, and cleanup #4 (CI grep) must land before "not hidden" can tick. Joint gate on all four plus the #5 re-tick.

> Decisions captured in `close_loop.planning.md § Locked Decisions LD-11
> (State-Ownership and Bridge Debt Scope Lock for Phase 1)` with the
> authoritative bridge inventory (3 pre-exec push + 1 block-context
> handshake + 1 Phase B conformance-mirror = 5 items in §4 scope, with
> the LD-1 canonical apply path listed separately and excluded from
> §4 debt count), classification matrix, three-wave removal sequence,
> and `ResourceSyncService` classification. The first acceptance item
> ticks on audit-complete grounds; the second stays `[ ]` until the
> cleanup items below land.

#### 4 deferred follow-ups

These are the LD-11 cleanups. They are **not** acceptance blockers
for LD-11 itself — LD-11 is locked the moment the audit / classify /
document / sequence items above tick against the inventory — but
they are the joint prerequisite for §4's second acceptance row
("visible and sequenced, not hidden"):

- [ ] **LD-11 cleanup #1 (severe correctness fix — highest severity)** — Fix `updateAccountStorage` at `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:1104`. It is a TODO no-op today. Meanwhile the Rust VM path (`rust-backend/crates/execution/src/tron_evm.rs`, the `execute_transaction` handler in `rust-backend/crates/core/src/service/grpc/mod.rs`) emits storage-slot `StateChange` entries on every stateful VM execution, and in any run where `write_mode != PERSISTED`, Java drops all of them. This is a direct VM-parity correctness hole, not a latent edge case. Either implement contract-storage-slot writes in the non-Phase-B apply path, or add a strict failure (`UnsupportedOperationException` or strict error log + throw) so slot mutations cannot be silently discarded. Per CLAUDE.md Java-parity rule ("never silently succeed where Java would fail").
- [ ] **LD-11 cleanup #2 (clarity cleanup, not correctness fix)** — Remove the dead/misleading post-apply dirty-marking across the canonical apply path. Delete every `ResourceSyncContext.record*Dirty(...)` call that sits inside a canonical `apply*` method in `RuntimeSpiImpl.java`, not just the `applyGlobalResourceChange` block. Affected lines: 301 (`recordAccountDirty`), 446/449/452/455 (`recordDynamicKeyDirty` for the four global-resource totals), 588/659/781/912/913 (`recordAccountDirty` in `applyFreezeLedgerChanges` / `applyTrc10Changes` / `applyVoteChanges` / `applyWithdrawChanges` plus dual-side marks), and 710 (`recordDynamicKeyDirty("TOKEN_ID_NUM")` after TRC-10 asset-issue write). Rationale (per Known bug #2): (a) `flushPreExec` runs before any `apply*` method is reached, (b) `syncData.flushed` blocks a second flush in the same transaction, and (c) `ResourceSyncContext.finish()` clears the thread-local at tx end — so none of these marks reach Rust. This is not a correctness bug; it is dead, misleading code that muddies the ownership story. Leave a short comment in each affected `apply*` method explaining why no post-apply dirty-marking is needed.
- [ ] **LD-11 cleanup #3 (real coverage gap fix)** — Close the `ResourceSyncContext` per-tx `DelegationStore` gap. Today delegation keys are only synced in the per-block `syncPostBlockRewardDeltas`, not the per-tx `flushPreExec` path (`ResourceSyncContext.java:210` uses a 5-arg overload that deliberately omits delegation). Add `DelegationStore` to the per-tx dirty-key thread-local so mid-block delegation mutations via Rust-routed contracts see fresh data.
- [ ] **LD-11 cleanup #4 (CI/lint)** — Add a CI grep / review rule that flags any new bridge mechanism introduced without a new LD entry. Narrowed candidate greps: any new `apply*` method in `RuntimeSpiImpl.java` that writes to a Java store; any new class under `framework/src/main/java/org/tron/core/storage/sync/**` (this namespace is LD-11 territory — `org/tron/core/net/**` and `org/tron/core/consensus/**` sync code is explicitly out of scope and must not be caught by the grep); any new method in `Manager.java` that writes directly to `StorageSPI.*` for mutation data. Enforcement mechanism (grep, Checkstyle, or review checklist) is not locked; only the normative rule.
- [ ] **LD-11 cleanup #5 (joint re-tick)** — Re-tick the §4 acceptance "Temporary bridge debt is visible and sequenced, not hidden" once cleanups #1–#4 have all landed. Joint condition on all four — not an OR. Sequencing is already in place via the three-wave LD-11 removal sequence; these cleanups close the "visible" + "not hidden" halves.

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

- [ ] The current known gap is either closed or explicitly kept out of the `RR` canonical whitelist

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

- [x] Audit every `execution.remote.*` flag currently enabled in checked-in config — **LD-10 audit table** enumerates every flag in `[execution.remote]`, `[execution.fees]`, and `[genesis]` that is not already owned by LD-2/LD-3/LD-7. Server / storage / module / execution-limit sections are out of §5.3 scope by LD-10 fiat.
- [x] Compare against code defaults — **LD-10 audit table** carries the `config.toml` value and the `config.rs` code-default side-by-side for every orphan flag. Deviations (`rust_persist_enabled`, `market_strict_index_parity`) are explicitly called out.
- [x] Mark each flag as:
  - [x] `EE` baseline only — **Answered by LD-10**: no orphan flag falls into this bucket; `EE`-only behavior is determined by `ExecutionMode`, not by `execution.remote.*` flags.
  - [x] `RR` experimental — **Answered by LD-10**: `emit_storage_changes`, `market_strict_index_parity` (tracked deviation), `experimental_vm_blackhole_credit` (already named `experimental_*` for honesty).
  - [x] `RR` canonical-ready — **Answered by LD-10** + **LD-3 Profile A** + **LD-7 whitelist flags**: Profile A overrides (`system_enabled`, `accountinfo_aext_mode=hybrid`, `emit_freeze_ledger_changes`, `emit_global_resource_changes`, `strict_dynamic_properties`) plus orphan flags already at canonical code default (`vote_witness_seed_old_from_account`, `evm_eth_coinbase_compat`, `execution.fees.*`) plus the seven LD-7 whitelist-target per-contract flags.
  - [x] legacy / should be removed — **Answered by LD-10**: `delegation_reward_enabled` (deprecated per inline comment; LD-10 cleanup #2 tracks removal).
- [x] Produce one recommended conservative config for parity work — **Answered by LD-3 Profile A**. LD-10 does not re-derive Profile A; it cross-links to LD-3.
- [x] Produce one experimental config for targeted validation only — **Answered by LD-3 Profile B**. LD-10 does not re-derive Profile B; the `[genesis]` seed section in `config.toml` is explicitly classified by LD-10 as "Profile B territory".

Acceptance:

- [ ] The repo no longer looks "stable by config file, experimental by code comment" at the same time — **Decided by LD-10** as a joint condition on (a) LD-2 `rust_persist_enabled` flip landing (§1.1 deferred follow-up #1) **and** (b) LD-10 cleanup #1 resolving the `market_strict_index_parity` deviation. **Status:** half (a) has landed — `rust-backend/config.toml` now sets `rust_persist_enabled = false`, matching the code default and the LD-2 lock, and the planning-doc sweep has been run. Half (b) is still pending (LD-10 cleanup #1 defers to the §5.2 sidecar parity work). The gate does not tick yet because it is a joint AND, not an OR.

> Decisions captured in `close_loop.planning.md § Locked Decisions LD-10
> (Config and Feature-Flag Convergence Scope for Phase 1)` with full
> orphan-flag audit table, inheritance-from-LD-2/LD-3/LD-7 framing, and
> five mandatory cleanup items.
>
> The acceptance item is intentionally still `[ ]`: LD-10 **decides**
> what the gate condition is (matches Profile A modulo tracked gaps).
> At the LD-10 freeze point the gate had two open deviations
> (`rust_persist_enabled` and `market_strict_index_parity`); the
> `rust_persist_enabled` half has since landed via §1.1 deferred
> follow-up #1, leaving only the `market_strict_index_parity` half
> (LD-10 cleanup #1) outstanding. The row ticks when cleanup #1 lands.

#### 5.3 deferred follow-ups

LD-10 itself is doc-only; the implementation-shaped items below stay
`[ ]` until the supporting work lands.

- [ ] **LD-10 cleanup #1** — Classify the `market_strict_index_parity = true` deviation: either fold into Profile A, flip the code default, or document as intentional Profile B behavior. Deferred to §5.2 sidecar parity work that exercises market contracts.
- [ ] **LD-10 cleanup #2** — Remove the deprecated `delegation_reward_enabled` flag: delete the field from `rust-backend/crates/common/src/config.rs`, drop the override line from `rust-backend/config.toml`, and drop any remaining readers in the Rust backend. Bundles with any future `config.rs` refactor.
- [ ] **LD-10 cleanup #3** — Add "LD-10 classification" comment headers above each orphan flag in `rust-backend/config.toml` (except the `RR` canonical-ready rows that match code default). Cross-reference the LD-10 audit-table row so readers can trace every override back to its classification.
- [ ] **LD-10 cleanup #4** — Add a CI grep / review rule that flags any new flag in `config.toml` `[execution.remote]`, `[execution.fees]`, or `[genesis]` sections if it is not classified in the LD-10 audit table. Enforcement mechanism (grep, Checkstyle, or review checklist) is not locked.
- [ ] **LD-10 cleanup #5 (joint re-tick)** — Re-tick the §5.3 acceptance "the repo no longer looks 'stable by config file, experimental by code comment'" once LD-2 `rust_persist_enabled` flip (§1.1 deferred follow-up #1) **and** LD-10 cleanup #1 both land. Joint condition, not OR.

---

## 6. Verification, replay, and release gates

Goal: turn parity from a subjective feeling into an observable gate.

### 6.1 Storage verification

- [ ] Add storage crate tests until `tron-backend-storage` has meaningful direct coverage
- [ ] Add at least one Java integration path that validates remote storage semantics, not only factory creation
- [ ] Track storage regressions separately from execution regressions

### 6.2 Execution lane split

- [ ] Split verification into two lanes:
  - [ ] write-path / execute tx parity
  - [ ] read-path / query parity
- [ ] Avoid using strong write-path results to imply read-path closure
- [ ] Publish separate pass/fail state for both lanes

### 6.3 Golden vectors

Primary touchpoints:

- `framework/src/test/java/org/tron/core/execution/spi/GoldenVectorTestSuite.java`

- [ ] Make golden vectors execute the same input in separate `EE` and `RR` runs
- [ ] Add a comparator that records `EE` result vs `RR` result
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
- [ ] Run replay once in `EE`
- [ ] Run replay once in `RR`
- [ ] Compare outputs by contract type
- [ ] Compare outputs by read-path vs write-path
- [ ] Record whether mismatch is:
  - [ ] result-code only
  - [ ] energy only
  - [ ] return-data only
  - [ ] state-change / sidecar difference

### 6.5 Contract readiness dashboard

- [ ] Turn the support matrix into a living readiness table
- [ ] For each contract type, record:
  - [ ] `RR` support status
  - [ ] fixture coverage
  - [ ] Rust unit coverage
  - [ ] `EE-vs-RR` replay status
  - [ ] major known gaps
- [ ] Use the readiness table as the only source of truth for enabling canonical `RR` support

### 6.6 CI smoke gates

- [ ] Define a minimal `EE` smoke set
- [ ] Define a minimal `RR` smoke set
- [ ] Define a minimal `EE-vs-RR diff` smoke set
- [ ] Make CI output mismatches in a readable, triageable form

Acceptance:

- [ ] The project can answer "what is safe to enable today?" from tests and dashboards, not from memory

---

## 7. Sequencing and parallel work

Goal: keep the critical path clear and avoid starting expensive but premature work.

### 7.1 Critical path

- [x] Phase 1 critical path is the five Workstreams A–E from
  `close_loop.planning.md § Phase 1 Workstreams`:
  - [x] **A. Semantic freeze** — Workstream A is "Decide and document"
    scope only (`close_loop.planning.md § Phase 1 Workstreams § A`).
    Doc-lock is **complete** via LD-1 (canonical write ownership in `EE`
    and `RR`), LD-4 (`energy_limit` wire contract), LD-5 (storage
    transaction semantics), LD-6 (snapshot semantics), and LD-7 (contract
    support readiness matrix). LD-2/LD-3 extend LD-1, LD-8 locks the
    Workstream B contract, LD-9 locks the Workstream C scope, LD-10
    locks the §5.3 pointer-lock. The post-lock implementation cleanups
    (flag flips, energy-unit cutover, placeholder-honesty landings,
    NonVm reconcile) are tracked inside §1.1..§1.5 deferred
    follow-ups — they are **not** part of Workstream A's "decide and
    document" gate and do not block this sub-bullet.
  - [ ] **B. Execution read-path closure** — Workstream B per
    `close_loop.planning.md § Phase 1 Workstreams § B`. Doc-locked by
    LD-8; implementation open. §2 cleanups #1..#15 are the Phase 1
    deliverables.
  - [ ] **C. Storage semantic hardening** — Workstream C per
    `close_loop.planning.md § Phase 1 Workstreams § C`. Per LD-9 the
    Phase 1 portion is narrowed to §3.3/§3.4 placeholder-honesty plus
    CRUD + batch test coverage; the §3.1 `transaction_id` plumbing and
    §3.2 buffer semantics are deferred to the block importer phase
    under LD-5/LD-6. The narrowed Phase 1 portion is open.
  - [ ] **D. Execution edge-case parity** — Workstream D per
    `close_loop.planning.md § Phase 1 Workstreams § D`. Covers §4
    (state-ownership / bridge debt), §5.1 (`TriggerSmartContract` TRC-10
    pre-transfer), §5.2 (resource / fee / sidecar parity), and §5.3
    (config flag drift — LD-10 closed the scoping question, cleanups
    #1..#5 remain).
  - [ ] **E. `EE`-vs-`RR` verification pipeline** — Workstream E per
    `close_loop.planning.md § Phase 1 Workstreams § E`. §6 golden
    vectors + replay + CI smoke gates. Not yet started.
- [x] Explicitly keep `P2P / sync / consensus rewrite` off the critical path

> §7.1 now mirrors `close_loop.planning.md § Phase 1 Workstreams` A–E
> exactly. The prior list had three structural bugs that this ralph
> micro-iteration corrects: (1) Workstream C was over-narrowed to just
> "§3.4 placeholder-honesty tests" when the full Workstream-C surface
> is §3.3/§3.4 + CRUD test coverage (narrowing rationale moved into
> the sub-bullet text so it is still visible); (2) Workstream D was
> missing entirely; (3) `block importer readiness planning` was
> wrongly listed as a Phase 1 critical-path item when it is in fact
> the Phase 2 milestone per `close_loop.planning.md § Phase 2: Rust
> Block Importer / Block Executor Readiness` — removed.
>
> Two sub-bullets are now ticked on inheritance grounds: (a) "keep
> P2P/sync/consensus off critical path" inherits from the §0
> non-goals freeze plus `close_loop.planning.md § Why The Next Step
> Is Not P2P`; (b) "A. Semantic freeze" inherits from LD-1, LD-4,
> LD-5, LD-6, and LD-7, which between them decide-and-document every
> item Workstream A asks for. All post-lock implementation cleanups
> that people sometimes mentally roll into "semantic freeze" are
> deliberately **not** in Workstream A's definition and are tracked
> under §1.1..§1.5 deferred follow-ups instead — so ticking "A" here
> does **not** claim those cleanups are done.

### 7.2 Suggested first batch

- [ ] Start with these items first:
  - [ ] 1.1 Canonical write ownership
  - [ ] 1.2 `energy_limit` wire contract
  - [ ] 1.3 storage transaction semantics *(LD-5 doc/log cleanups only; real semantics deferred to block importer phase)*
  - [ ] 1.5 contract support matrix
  - [ ] 2.1 Java `callContract/estimateEnergy`
  - [ ] 3.4 storage placeholder-honesty tests *(LD-9; §3.1 `transaction_id` plumbing is deferred to block importer phase)*

### 7.3 Parallelization opportunities

- [ ] Run Java execution bridge work in parallel with Rust storage semantics work
- [ ] Run Rust execution query implementation in parallel with verification harness improvements
- [ ] Keep one owner responsible for semantic freeze so implementation work does not diverge

---

## 8. Explicit non-goals and defer list

These items should remain out of scope until the exit criteria above are met.

> **Reading legend for this section:** every `[x]` below is an
> **out-of-scope commitment confirmed by upstream lock or
> Current-State judgement**, not "implementation done". Ticking an
> item here means "Phase 1 has committed to *not* doing this" — the
> upstream lock or judgement source for each row is listed in the
> inheritance note at the end of the section.

- [x] Do not start Rust P2P handshake work
- [x] Do not start Rust peer/session manager work
- [x] Do not start Rust sync scheduler / inventory pipeline work
- [x] Do not start Rust consensus scheduling rewrite
- [x] Do not optimize for mixed execution/storage modes
- [x] Do not make current `SHADOW` the main acceptance path again
- [x] Do not treat "many system contracts already run remotely" as proof that the full execution problem is solved
- [x] Do not treat "storage CRUD works" as proof that storage semantics are solved

> §8 is a restatement of the §0 non-goals freeze with one extra row per
> category. Every item above is inherited from one of:
>
> (a) §0 "Freeze explicit non-goals for this phase" (P2P / peer-session /
>     sync / consensus / mixed modes / SHADOW-as-main-validator) and the
>     mirroring lock in `close_loop.planning.md § Why The Next Step Is
>     Not P2P`;
>
> (b) `LD-1` (current `SHADOW` path de-emphasized as a legacy / optional
>     validation mode, not a Phase 1 acceptance mode);
>
> (c) `close_loop.planning.md § Current State` (the `Storage` and
>     `Smart Contract / Execution` subsections) plus the §0 todo-file
>     "Current judgement" paragraph — both explicitly warn against
>     treating "substantial remote execution coverage" as proof that the
>     execution problem is closed, or "storage CRUD works" as proof that
>     storage semantics are closed. The last two §8 rows are therefore
>     direct restatements of those Current-State judgement blocks, not
>     net-new claims.
>
> No new decision is captured here; §8 exists only as a sequencing
> reminder that these items must stay out of scope for the duration of
> Phase 1. Ticking them does **not** mean the work is done — it means the
> out-of-scope commitment is confirmed and inherited from the upstream
> lock or Current-State judgement listed above.

---

## 9. Handoff to next phase

Only after this file's exit criteria are met:

- [ ] Open `BLOCK-01` planning for Rust block importer / block executor
- [ ] Decompose `Manager.processBlock(...)` into Rust-owned responsibilities
- [ ] Re-evaluate whether consensus should follow block importer or stay on Java longer
- [ ] Re-evaluate whether P2P should remain Java-owned until after importer stability

Success condition for this handoff:

- [ ] The next roadmap discussion starts from "Rust state-transition engine ownership", not from "networking looks exciting"
