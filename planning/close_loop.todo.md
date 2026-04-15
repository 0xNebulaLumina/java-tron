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

All items in this section are frozen in the sibling note `close_loop.scope.md`.
That file is the durable source of truth; the checkboxes here only track that
each decision has been written down.

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
- [x] Define Phase 1 exit criteria (definitions live in `close_loop.scope.md`;
      sub-items below remain unchecked until they are actually achieved):
  - [x] Java execution read/query APIs are no longer placeholders in the `RR` path
        (iter 5 + iter 6: `estimateEnergy`, `getCode`, `getStorageAt`,
        `getNonce`, `getBalance`, and `healthCheck` route through
        `ExecutionGrpcClient` (iter 5). Snapshot/revert return
        explicit unsupported errors (iter 2). `callContract` now
        carries the full `TronTransaction` via
        `CallContractRequest.transaction` and reads a structured
        `CallContractResponse.Status` enum (iter 6), closing the
        shape-mismatch and response-discriminator fragilities
        tracked as iter-5 follow-ups.)
  - [x] Rust execution read/query APIs are either implemented or explicitly unsupported
        (iter 4: `get_code` / `get_storage_at` / `get_nonce` /
        `get_balance` now read through `EngineBackedEvmStateStore`;
        `create_evm_snapshot` / `revert_to_evm_snapshot` return
        explicit `success=false` errors per `close_loop.snapshot.md`;
        `call_contract` / `estimate_energy` were already real;
        `health` already reflects real module readiness. No fake
        placeholder remains on the Rust side.)
  - [x] Storage transaction semantics are real enough for execution needs
        (iter 3: `put_in_tx` / `delete_in_tx` / `batch_write_in_tx` now
        route through real per-tx buffers with atomic commit + discard
        on rollback, and the gRPC handlers branch on `transaction_id`.
        Read-your-writes is explicitly out of scope per 1.3.)
  - [x] Storage snapshot semantics are real, or snapshot is explicitly unavailable and not silently fake
        (iter 2 + iter 3: storage engine, Java `RemoteStorageSPI`,
        Java `EmbeddedStorageSPI`, and Java `RemoteExecutionSPI`
        snapshot APIs all return explicit unsupported errors. The
        contract is locked by unit tests in `engine.rs` and by the
        integration test `testSnapshotOperationsAreUnsupported`.)
  - [ ] `energy_limit` wire semantics are locked
        (still open — decision frozen in iter 1 but producer/consumer
        migration not yet applied)
  - [x] Write ownership is unambiguous in `EE` and `RR`
        (iter 1: decision frozen in `close_loop.write_ownership.md`;
        `config.toml` and `config.rs` comments aligned; write-path
        matrix published.)
  - [ ] A first contract whitelist reaches stable `EE-vs-RR` parity
  - [x] Storage crate has real tests
        (iter 3: `cargo test -p tron-backend-storage` runs 22 tests,
        all green — closes the "0 tests" baseline signal.)
  - [ ] Replay + CI can continuously report `EE-vs-RR` parity state

---

## 1. Semantic freeze and architectural decisions

Goal: stop the project from moving forward on top of ambiguous semantics.

### 1.1 Canonical write ownership

Primary touchpoints:

- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- `rust-backend/config.toml`
- `rust-backend/crates/common/src/config.rs`

Decisions are frozen in sibling note `close_loop.write_ownership.md`.

- [x] Write down the authoritative write-path matrix for:
  - [x] `EE`
  - [x] `RR`
- [x] Explicitly de-emphasize current `SHADOW` as a legacy / optional path, not a Phase 1 acceptance mode
- [x] Define whether `RuntimeSpiImpl` Java-side apply is canonical, transitional, or legacy-only
- [x] Define whether `rust_persist_enabled=true` is allowed in:
  - [x] development only
  - [x] targeted experiments only
  - [x] `RR` candidate mode
  - [x] never, until later phase
- [x] Align code defaults, checked-in config, and comments
- [x] Add a future implementation item to fail fast when an unsafe mode combination is detected
      (the future implementation item itself is tracked: see
      `close_loop.write_ownership.md` § "Follow-up
      implementation items" — Rust-side and Java-side
      startup detection / warning follow-ups for unsafe
      `execution.mode` × `rust_persist_enabled` combinations
      are queued there as logged warnings, not literal
      startup aborts. This checkbox only tracks that the
      follow-up is captured, not that the runtime check
      itself has shipped.)
- [x] Document one recommended safe rollout profile and one experimental profile

Acceptance:

- [x] Any engineer can answer "who writes the final state in this mode?" without ambiguity
- [x] `config.toml`, `config.rs`, and planning docs no longer contradict each other

### 1.2 Lock `energy_limit` wire semantics

Primary touchpoints:

- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- `framework/src/main/proto/backend.proto`
- `rust-backend/crates/execution/src/lib.rs`
- fixture/conformance generators and readers

Decision is frozen in sibling note `close_loop.energy_limit.md`:
`energy_limit` on the wire is expressed in **energy units**, not fee-limit SUN.

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
      (tracked as open follow-ups in `close_loop.energy_limit.md`)

Acceptance:

- [ ] No remaining ambiguity on whether Java sends fee-limit SUN or already-computed energy units
      (decision doc records the future contract, but live code still mixes both:
      VM path sends energy units via `computeEnergyLimitWithFixRatio`
      (`RemoteExecutionSPI.java:636`, called from `:878` for
      `CreateSmartContract` and `:902` for `TriggerSmartContract`)
      with a raw-feeLimit exception fallback, non-VM path keeps the
      raw fee-limit SUN default established at
      `RemoteExecutionSPI.java:735` (`long energyLimit =
      transaction.getRawData().getFeeLimit();`, with the close_loop
      Phase 1 lock comment immediately above at `:720-734`) and
      serialized into the wire request at `RemoteExecutionSPI.java:1411`
      / `:1440`, and fixture generators still send fee-limit SUN. The
      Rust receiver still divides by `energy_fee_rate`
      (`rust-backend/crates/execution/src/lib.rs:138`,
      with the close_loop Phase 1 transition comment block at
      `:113-135` and a `TODO(close_loop 1.2)` marker at `:134`).
      Acceptance remains open until the producer/consumer migration
      follow-ups in `close_loop.energy_limit.md` §"Follow-ups
      tracked from this decision" land. Migration is sequenced as
      Phase 2.E in `close_loop.handoff.md` §"Phase 2.E — energy_limit
      wire migration".)
- [ ] Java, Rust, and conformance tooling target the same unit contract
      (not yet — migration follow-ups in `close_loop.energy_limit.md`
      §"Follow-ups tracked from this decision" still open. All five
      follow-ups (remove Rust divide-by-energy_fee_rate, update Java
      fixture generators, set energy_limit=0 for non-VM in Java, extend
      `computeEnergyLimitWithFixRatio` for `CreateSmartContract`
      creator/caller split, add Rust SUN-scale guard) are sequenced as
      Phase 2.E per `close_loop.handoff.md`. Phase 1 acceptance bar
      explicitly does NOT include the migration itself — only the
      decision freeze, which is locked.)

### 1.3 Lock storage transaction semantics

Primary touchpoints:

- `framework/src/main/proto/backend.proto`
- `framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java`
- `framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java`
- `rust-backend/crates/storage/src/engine.rs`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

Decisions are frozen in sibling note `close_loop.storage_transactions.md`.

- [x] Decide the required semantics for `beginTransaction/commit/rollback`
- [x] Decide whether transaction scope is:
  - [x] per DB
  - [ ] cross DB
  - [x] only "execution-local enough", not generic DB transaction
- [x] Decide whether transaction-scoped reads need read-your-writes visibility for:
  - [x] `get`
  - [x] `has`
  - [x] `batchGet`
  - [x] iterators / prefix / range reads
- [x] Decide what execution actually needs versus what can be deferred
- [x] Explicitly reject turning `StorageSPI` into a generic database product if that is not needed now
- [x] Write down behavior when `transaction_id` is absent on a write call

Acceptance:

- [x] There is a clear "minimum transaction semantics required by execution/block importer" statement
- [x] No one needs to infer semantics from partial code paths

### 1.4 Lock snapshot semantics

Primary touchpoints:

- `framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java`
- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- `rust-backend/crates/storage/src/engine.rs`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

Decisions are frozen in sibling note `close_loop.snapshot.md`.

- [x] Decide whether storage snapshot must be a true RocksDB point-in-time snapshot
      (decision: NO for Phase 1 — snapshot is explicitly unsupported, must not silently read live DB)
- [x] Decide whether EVM snapshot/revert must be built on top of:
  - [ ] storage snapshot
  - [x] execution-local journaling (REVM intra-tx journaling only — SPI snapshot/revert is unsupported in Phase 1)
  - [ ] both
- [x] Decide whether temporary "unsupported" is safer than fake success
      (yes — fake success is explicitly banned)
- [x] Write explicit unsupported behavior for any API not implemented in this phase

Acceptance:

- [x] Fake snapshot success is no longer an accepted state
      (decision locked; storage-engine and Java SPI snapshot APIs now
      return explicit unsupported errors — see iter 2 changes to
      `engine.rs` and `RemoteExecutionSPI.java`)
- [x] Snapshot-dependent APIs either have real guarantees or fail explicitly
      (storage engine + Java `RemoteStorageSPI` /
      `EmbeddedStorageSPI` snapshot APIs were closed in
      iter 2 with explicit unsupported errors. Iter 4 closed
      the Rust execution gRPC `create_evm_snapshot` and
      `revert_to_evm_snapshot` handlers — both now return
      `success = false` with an explicit Phase 1 unsupported
      message instead of the previous fake-success
      placeholder, and the contract is locked by
      `create_evm_snapshot_returns_explicit_unsupported` /
      `revert_to_evm_snapshot_returns_explicit_unsupported`
      in `iter4_read_path_tests` (`grpc/mod.rs:2697`,
      `grpc/mod.rs:2740`). Iter 5 closed
      `RemoteExecutionSPI.healthCheck` — it now calls
      `grpcClient.healthCheck()` and renders the real
      `HealthResponse.Status` (HEALTHY / DEGRADED / UNHEALTHY)
      plus per-module detail (`RemoteExecutionSPI.java:546`),
      with transport failure mapped to a non-healthy
      `HealthStatus` instead of a synthetic OK. Iter 13
      closed the last two fake-success holes flagged by
      codex review: `EmbeddedExecutionSPI.createSnapshot`
      and `revertToSnapshot` previously returned
      `embedded_snapshot_<millis>` and a literal `true`,
      and `ShadowExecutionSPI.createSnapshot` /
      `revertToSnapshot` fanned out to both engines and
      recursively retried the embedded path on failure;
      both implementations now fail their futures with
      `UnsupportedOperationException` citing
      `close_loop.snapshot.md`. With this, every
      snapshot-touching API on the Java SPI surface AND the
      Rust execution gRPC surface fails explicitly — no
      Phase 1 path can silently believe it has a usable
      snapshot handle.)

### 1.5 Build a contract support matrix

Primary touchpoints:

- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- `rust-backend/crates/core/src/service/mod.rs`
- `rust-backend/config.toml`

The matrix and Phase 1 whitelist target are frozen in sibling note
`close_loop.contract_matrix.md`. Attribute audits (`tbd` entries) and
dashboarding are tracked there as follow-ups.

- [x] Enumerate all contract types currently seen by `RemoteExecutionSPI`
- [x] Mark each as:
  - [x] `EE` only
  - [x] `RR` blocked
  - [x] `RR` candidate
  - [x] `RR` canonical-ready
- [x] For each contract type, record:
  - [x] depends on read-path closure
  - [x] depends on TRC-10 semantics
  - [x] depends on freeze/resource sidecars
  - [x] depends on dynamic-property strictness
  - [x] has fixture coverage (initial pass — many rows still `tbd`)
  - [x] has Rust unit coverage (initial pass — many rows still `tbd`)
  - [x] has `EE-vs-RR` replay coverage (all rows `tbd` until replay pipeline lands)
- [x] Split the matrix into:
  - [x] core high-value contracts for Phase 1 acceptance
  - [x] secondary contracts that can remain `RR` blocked longer

Acceptance:

- [x] Remote enablement is no longer driven only by config convenience
      (matrix overrides raw config-flag enablement for acceptance claims)
- [x] There is an explicit `RR` whitelist target for the end of this phase
      (TransferContract, CreateSmartContract, UpdateSettingContract — see
      `close_loop.contract_matrix.md`)

---

## 2. Execution read-path closure

Goal: close the biggest gap between "write-path already exists" and "node-level execution capability is still incomplete".

### 2.1 Java execution bridge tasks

Primary touchpoints:

- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`

- [x] Replace placeholder `callContract(...)` with a real RPC-backed path
      (iter 5 + iter 6: the round-trip is now fully real with both
      the shape and the status-discriminator follow-ups closed.
      **iter 5** replaced the placeholder with a real
      `grpcClient.callContract` call and fixed the Rust handler to
      propagate `TronExecutionResult.success`/`error` instead of
      hardcoding `success: true`. **iter 6** added the additive
      proto fields that close the remaining two fragilities:
      (1) `CallContractRequest.transaction` (field 5) now carries
      the full `TronTransaction` produced by
      `buildExecuteTransactionRequest`, so `value` / `energy_limit` /
      `tx_kind` / `contract_type` / `asset_id` /
      `contract_parameter` all round-trip to the Rust converter
      (which prefers `transaction` when set and falls back to the
      legacy flat fields for older clients). (2)
      `CallContractResponse.status` (field 5) is a structured enum
      `{UNSPECIFIED, SUCCESS, REVERT, HALT, HANDLER_ERROR}` set by
      the Rust handler based on `TronExecutionResult.success`/`error`;
      the Java side reads the enum directly instead of matching on
      error-message string prefixes. The legacy `success`/
      `error_message` fields are still populated for backward
      compatibility with pre-iter-6 clients, and the Java side
      retains the iter-5 string-match path as an explicit
      "status == UNSPECIFIED" legacy fallback.)
- [x] Replace placeholder `estimateEnergy(...)` with a real RPC-backed path
      (iter 5: builds an `EstimateEnergyRequest` by the same
      route, calls `grpcClient.estimateEnergy`, and fails-hard on
      `!response.success` or transport failure.)
- [x] Replace placeholder `getCode(...)`
      (iter 5: real gRPC call; `success=false` → fail-hard with
      remote error_message.)
- [x] Replace placeholder `getStorageAt(...)`
      (iter 5: real gRPC call; same fail-hard contract.)
- [x] Replace placeholder `getNonce(...)`
      (iter 5: real gRPC call; returns nonce as long.)
- [x] Replace placeholder `getBalance(...)`
      (iter 5: real gRPC call; returns 32-byte BE balance blob from
      the Rust backend unchanged so callers can decode to BigInteger.)
- [x] Replace placeholder `createSnapshot(...)`
      (iter 2: now throws `UnsupportedOperationException` via
      `completeExceptionally`, per `close_loop.snapshot.md`)
- [x] Replace placeholder `revertToSnapshot(...)`
      (iter 2: now throws `UnsupportedOperationException` via
      `completeExceptionally`, per `close_loop.snapshot.md`)
- [x] Replace placeholder `healthCheck(...)`
      (iter 5: real `grpcClient.healthCheck()` call; maps
      `HEALTHY`/`DEGRADED` → `HealthStatus(true, …)` with the
      degraded-module map rendered into the detail string,
      `UNHEALTHY`/transport-failure → `HealthStatus(false, …)`.)
- [ ] Normalize timeout handling across all remote execution APIs
      (partial — all RPC methods now go through `ExecutionGrpcClient`
      which applies a 30s deadline (`DEFAULT_DEADLINE_MS`) via
      `.withDeadlineAfter(...)` on every call
      (`ExecutionGrpcClient.java:152` / `:171` / `:190` / `:209` and
      the parallel `callContract` / `estimateEnergy` / `healthCheck`
      blocks). The bridge itself does not yet impose a per-method
      override or surface timeouts distinctly from other transport
      errors. Iter 14 §2.3 audit confirmed timeout failures collapse
      into the same `RuntimeException` shape as engine-reported
      errors — distinguishable today only via cause-chain inspection
      (`StatusRuntimeException` only present for transport, with
      `Status.Code.DEADLINE_EXCEEDED` specifically for timeout).
      Closing this item requires either a typed
      `RemoteExecutionException` hierarchy (see the
      `Normalize error mapping across all remote execution APIs`
      item directly below) or a per-method timeout config knob;
      neither is in Phase 1.)
- [ ] Normalize error mapping across all remote execution APIs
      (partial — read APIs now fail-hard on `!response.success`;
      `callContract` maps to `REVERT`; `estimateEnergy` and the
      four read APIs throw via `completeExceptionally`. A shared
      helper that produces a typed `RemoteExecutionException` is
      still open work. Iter 14 §2.3 audit explicitly framed this as
      the Phase 2 parent for: (a) propagating `found=false` as a
      first-class outcome instead of collapsing into empty/zero
      bytes — see §2.3 "not found" sub-item; (b) distinguishing
      transport vs engine errors via exception type instead of
      cause-chain inspection — see §2.3 "transport error"
      sub-item. Both Phase 2 follow-ups should land together with
      this item so `RemoteExecutionSPI` callers can distinguish
      success-with-found / success-without-found / engine-error /
      transport-error / unsupported via `catch` instead of
      reflection on `RuntimeException.getCause()` / message
      prefix.)
- [x] Decide when Java should:
  - [x] fail hard
        (iter 5: read APIs + estimateEnergy fail-hard via
        `completeExceptionally` on any backend error; transport
        failures also fail-hard. This is the default for Phase 1.)
  - [x] return explicit remote unsupported
        (iter 2: snapshot/revert APIs return explicit unsupported
        via `UnsupportedOperationException` per
        `close_loop.snapshot.md`.)
  - [x] fall back only where this is still intentionally allowed
        (no fallbacks in the Java bridge. `callContract` returns a
        reverted `ExecutionProgramResult` carrying the backend error
        string, but that is not a fallback — it is a typed error
        channel the existing VM result pipeline already handles.)
- [x] Remove planning assumptions that these APIs will be validated mainly through `SHADOW`
      (covered by `close_loop.scope.md` "why not SHADOW as the main validator")

### 2.2 Rust execution gRPC tasks

Primary touchpoints:

- `rust-backend/crates/core/src/service/grpc/mod.rs`
- `rust-backend/crates/execution/src/lib.rs`
- storage adapter code used by execution query paths

- [x] Implement `get_code`
      (iter 4: real RPC-backed path wired through
      `EngineBackedEvmStateStore::get_code`. Rejects non-empty
      `snapshot_id` per `close_loop.snapshot.md`. Accepts 20-byte
      and 21-byte TRON addresses via `normalize_tron_address`.)
- [x] Implement `get_storage_at`
      (iter 4: wired through `EngineBackedEvmStateStore::get_storage`;
      key left-padded to 32 bytes, result returned as 32-byte BE blob.)
- [x] Implement `get_nonce`
      (iter 4: reads nonce from `AccountInfo` via `get_account`.)
- [x] Implement `get_balance`
      (iter 4: reads balance from `AccountInfo` and serializes it as
      32-byte BE to preserve the full U256 range.)
- [x] Decide whether `create_evm_snapshot` is in scope this phase:
  - [ ] if yes, define storage/journal backing model
  - [x] if no, return explicit unsupported
        (iter 4: handler now returns `success=false` with a clear
        error message pointing at `close_loop.snapshot.md`. The old
        fake-success path that generated a synthetic UUID is removed.)
- [x] Decide whether `revert_to_evm_snapshot` is in scope this phase:
  - [ ] if yes, define rollback semantics
  - [x] if no, return explicit unsupported
        (iter 4: same treatment as `create_evm_snapshot`.)
- [x] Make `health` reflect actual module readiness, not a placeholder
      (pre-existing: the backend `health` RPC already walks the
      module manager and reports real per-module status; verified
      as non-placeholder during the iter 4 audit. The only Phase 1
      gap remaining under this header is the Java side
      `RemoteExecutionSPI.healthCheck`, tracked separately in 2.1.)
- [x] Add logging/diagnostics that can explain which remote query path failed
      (iter 4: each read handler logs `debug!(...)` on entry and
      `error!(...)` on engine failures with the concrete error
      string; `snapshot_unsupported_error(method)` produces a stable
      wording so operator log-grep finds every surface with one
      query.)

### 2.3 Request/response semantic alignment

- [x] Verify Java request builders carry all fields required by Rust query APIs
      (iter 14 audit: `RemoteExecutionSPI.java:372-376/399-404/424-428/448-452`
      build `GetCodeRequest` / `GetStorageAtRequest` / `GetNonceRequest` /
      `GetBalanceRequest`. `GetCodeRequest` / `GetNonceRequest` /
      `GetBalanceRequest` carry `address` + `snapshot_id`;
      `GetStorageAtRequest` additionally carries `key` (set at
      `RemoteExecutionSPI.java:402`, consumed by the Rust handler at
      `rust-backend/crates/core/src/service/grpc/mod.rs:2056` /
      `:2067`). For each of the four read APIs, every field the
      corresponding Rust handler currently consumes is set by the
      Java builder — verified against `get_code` at lines 1965-2020
      and the parallel `get_storage_at` / `get_nonce` / `get_balance`
      handlers. `EstimateEnergyRequest`
      (`RemoteExecutionSPI.java:322`) and `CallContractRequest`
      (`RemoteExecutionSPI.java:197`) carry the full `TronTransaction`
      via the iter-6 additive `transaction` field
      (`CallContractRequest.transaction`, field 5) so `value` /
      `energy_limit` / `tx_kind` / `contract_type` / `asset_id` /
      `contract_parameter` round-trip without truncation. No required
      field is missing on either surface for Phase 1.)
- [x] Verify snapshot identifiers, if kept, have stable cross-side meaning
      (iter 14 audit: snapshot identifiers are **not** kept in Phase 1.
      `close_loop.snapshot.md` froze the decision that EVM snapshot/revert
      is unsupported across all SPI implementations; consequently every
      surface that takes a `snapshot_id` either rejects non-empty values
      explicitly or never emits them in the first place. Concretely:
      (1) Java side: `RemoteExecutionSPI.createSnapshot` /
      `revertToSnapshot` / `EmbeddedExecutionSPI.createSnapshot` /
      `revertToSnapshot` / `ShadowExecutionSPI.createSnapshot` /
      `revertToSnapshot` all return `completeExceptionally(new
      UnsupportedOperationException(...))` (iter 2 + iter 13). No Java
      caller ever holds a usable snapshot handle to pass back.
      (2) Rust read handlers: every `get_*` handler at
      `rust-backend/crates/core/src/service/grpc/mod.rs` rejects
      non-empty `snapshot_id` with
      `error_message=snapshot_unsupported_error(method)` before doing
      any work (`get_code` lines 1972-1979, plus the parallel
      `get_storage_at` / `get_nonce` / `get_balance` handlers; iter-4
      `*_rejects_non_empty_snapshot_id` tests at lines 2497-2570 lock
      the behavior). (3) Rust execution handlers:
      `create_evm_snapshot` and `revert_to_evm_snapshot` likewise return
      `success=false` with the same error wording, with iter-11 negative
      tests at lines 2707+ (`create_evm_snapshot_returns_explicit_unsupported`
      / `revert_to_evm_snapshot_returns_explicit_unsupported`).
      `create_evm_snapshot_returns_explicit_unsupported` specifically
      asserts the returned `snapshot_id` is empty so no fake synthetic
      id leaks; the `revert` test asserts only `success == false` /
      error wording, since `RevertToEvmSnapshotResponse` has no
      `snapshot_id` field to assert against.
      Cross-side meaning of `snapshot_id` is therefore stable: empty
      string only, anything else is an error. Re-introducing real
      snapshot handles is Phase 2+ and tracked under the §1.4 sub-items
      "if yes, define storage/journal backing model" / "if yes, define
      rollback semantics", which remain `[ ]` on purpose.)
- [x] Verify query responses distinguish:
  - [x] not found
        (iter 14 audit: the **wire protocol** distinguishes "not found"
        from "engine error" by exposing `found` as a separate field
        from `success`. Rust read handlers return
        `success=true, found=false, data=zeroed` on
        `Ok(None)` / unknown-account paths.
        `get_code` lines 2004-2009 (`bytecode=None`),
        `get_nonce`/`get_balance`/`get_storage_at` use the same shape.
        Locked by iter-4 tests
        `get_nonce_unknown_address_returns_not_found_success`
        (line 2610), `get_balance_unknown_address_returns_zeroed_balance`
        (line 2632), `get_code_unknown_address_returns_not_found_success`
        (line 2653), and
        `get_storage_at_unknown_slot_returns_zero_with_found_false`
        (line 2671).
        **Known Java-side gap (intentional Phase 1 limitation):**
        `RemoteExecutionSPI` currently checks only `resp.getSuccess()`
        and returns the (empty/zeroed) payload on `found=false`
        without exposing the flag (`RemoteExecutionSPI.java:378-382`
        for `getCode` and the parallel branches at `:405-410`,
        `:430-434`, `:454-462`). This mirrors the pre-Phase-1 Java
        SPI signature (`getCode → byte[]`, `getNonce → long`,
        `getBalance → byte[]`) which has no `Optional`/`found`
        channel. Phase 1 acceptance only requires the **wire
        protocol** to carry the distinction so the comparator and
        future Java consumers can tell apart "missing account" and
        "engine error"; widening the Java SPI return type to
        propagate `found` is a Phase 2 follow-up tracked under
        §2.1 "Normalize error mapping".)
  - [x] unsupported
        (iter 14 audit: `success=false,
        error_message=snapshot_unsupported_error(method)` is the
        canonical "unsupported" shape on the Rust side. Used by all
        four read handlers when `snapshot_id` is non-empty
        (`get_code` lines 1972-1979 etc.) and by both
        `create_evm_snapshot` / `revert_to_evm_snapshot` execution
        handlers. Wording is shared via the `snapshot_unsupported_error`
        helper so an operator log-grep finds every surface with one
        query. Java side surfaces it via the explicit
        `UnsupportedOperationException` path on the snapshot APIs.)
  - [x] internal error
        (iter 14 audit: `success=false,
        error_message=format!("get_code engine error: {}", e)`
        is the canonical "internal/engine error" shape on the four
        read handlers. See `get_code` lines 2010-2018 for the
        engine-`Err(e)` branch (and the `error!(...)` log line for
        diagnostics, closing the §2.2 logging follow-up). The other
        three read handlers (`get_storage_at`, `get_nonce`,
        `get_balance`) use the same `<method> engine error: <e>`
        prefix. The execution-side snapshot handlers
        (`create_evm_snapshot` / `revert_to_evm_snapshot`) currently
        have only the unsupported branch (no engine read is performed),
        so they have no engine-error prefix to assert against — that
        is the correct shape for Phase 1 because the explicit
        unsupported decision short-circuits before any engine call.
        Java side maps an engine error to a `RuntimeException` via
        `if (!resp.getSuccess()) throw new RuntimeException("Remote
        getCode failed: " + resp.getErrorMessage())` at
        `RemoteExecutionSPI.java:378-381` and the parallel branches.)
  - [x] transport error
        (iter 14 audit: transport errors are deliberately **not**
        encoded in the response enum on the Rust side — they cannot
        be, since a transport failure means no response was produced.
        On the Java side the transport boundary lives in
        `ExecutionGrpcClient`: `getCode`/`getStorageAt`/`getNonce`/
        `getBalance` (and the parallel `callContract` / `estimateEnergy`
        / `healthCheck`) catch `StatusRuntimeException` and rethrow as
        `new RuntimeException("Remote get code failed: " + e.getMessage(), e)`
        with the original `StatusRuntimeException` preserved as the
        `cause` (`ExecutionGrpcClient.java:154-157` and the parallel
        catch blocks at `:173-176`, `:192-195`, `:211-214`).
        `RemoteExecutionSPI.java:378-386` etc. then bubbles that
        `RuntimeException` out via `CompletableFuture.supplyAsync`.
        **Known Java-side gap (intentional Phase 1 limitation):** the
        Java SPI surface currently collapses both transport errors
        and engine errors (`success=false` responses) to the same
        `RuntimeException` type — they are **not** distinguishable by
        catching a more specific exception class. They are
        distinguishable today only by inspecting the cause chain
        (`StatusRuntimeException` only present for transport) or the
        message prefix (engine errors carry the response
        `error_message`, transport errors carry the `Remote get …
        failed: …` wrapper text). Adding a typed
        `RemoteExecutionException` hierarchy that distinguishes
        transport / engine / unsupported is tracked as Phase 2 work
        under §2.1 "Normalize error mapping". Phase 1 acceptance only
        requires that the system can in principle distinguish the
        four cases (it can — via cause inspection), not that the SPI
        return type is widened.)
- [x] Verify `estimateEnergy` comparison rules in `EE-vs-RR` validation:
  - [x] exact match
        (iter 14 audit: Phase 1 `estimateEnergy` and the broader
        execution comparator both run on **exact match**. The harness
        is `scripts/compare_exec_csv.py` (the iter-12 4-category
        classifier). Energy parity is checked column-by-column at
        byte-level; any difference in the energy family columns is
        classified — never tolerated. The energy family is
        `ENERGY_FAMILY_COLUMNS = ENERGY_COLUMNS | {"bandwidth_used"}`
        (iter 12 folded `bandwidth_used` into the energy family so
        Phase 1 has exactly four classification buckets:
        `state-change / sidecar difference`, `result-code only`,
        `energy only`, `return-data only` — matching the constants
        in `scripts/compare_exec_csv.py` exactly). The state-digest
        field
        (`state_digest_sha256`) is also a strict equality compare.
        See `scripts/compare_exec_csv.py` and the 37-test
        `scripts/test_compare_exec_csv.py` regression suite that
        locks the four categories, the empty-CSV / blank-line /
        one-side-empty failure modes, and the default exit-0
        behavior used by the artifact-collection wrapper.)
  - [x] tolerated delta
        (iter 14 audit: explicitly **not** in Phase 1 scope. The
        comparator has no tolerance window for energy or bandwidth
        — every diff is classified into the energy-only family and
        surfaced. If a tolerated delta is later required for a
        specific opcode/precompile, it must (a) be added as a
        per-contract exception list — see next sub-item — and
        (b) be explicitly opted-into by the comparator under a flag
        rather than as a default. Tracked here as future work
        (Phase 2+); not a Phase 1 acceptance gate.)
  - [x] per-contract exception list if needed
        (iter 14 audit: explicitly **not** in Phase 1 scope. There
        is no per-contract exception list in `compare_exec_csv.py`;
        the only contract-type filtering today is at the
        whitelist/blocklist level via `close_loop.contract_matrix.md`
        (which gates which contract types are eligible for `RR`
        execution at all, not which fields are tolerated). Reasoning:
        Phase 1 acceptance is "for the whitelisted contract types,
        every field must match byte-for-byte". An exception list
        would only be reachable once the whitelist itself is closed
        and a specific contract type produces a deterministic,
        documented divergence — at which point the divergence becomes
        a Phase 2 spec item, not an exception. Tracked here as future
        work; not a Phase 1 acceptance gate.)

### 2.4 Execution read-path tests

Java-focused:

- [ ] Add focused Java tests for each remote execution read/query API
- [ ] Add paired `EE` baseline vs `RR` target tests where the harness can run both paths separately
- [ ] Add targeted `callContract` coverage for: success (non-payable const
      call), explicit REVERT, REVM halt (out-of-energy / invalid opcode),
      payable (non-zero callValue), and handler-side transport failure.
      This is what would have caught the iter 5 "hardcoded success=true"
      and "dropped value/energy_limit" bugs before landing.
      (Partial — iter 6 added 4 Rust-side converter / wire-value tests in
      `crates/core/src/service/grpc/conversion.rs`
      (`call_contract_request_prefers_transaction_field_when_present`,
      `call_contract_request_falls_back_to_legacy_fields_when_transaction_absent`,
      `call_contract_request_legacy_fallback_rejects_malformed_address`,
      `call_contract_response_status_wire_values_are_stable`). The
      Java-side `CallContractResponse.Status` → `contractResult` mapping
      is still not directly unit-tested. Closing this requires either
      (a) extracting a pure helper `interpretCallContractResponse(resp)
      → ExecutionProgramResult` and adding JUnit tests against it, or
      (b) using a Mockito mock of `ExecutionGrpcClient`. Blocked locally
      by the pre-existing gradle/Lombok environment issue that prevents
      `:framework:compileJava` from running, so any Java test added now
      can only be verified via codex review and CI — still worth doing
      once the env is unblocked.)

Rust-focused:

- [x] Add gRPC service tests for each query API
      (iter 4: `iter4_read_path_tests` in `crates/core/src/service/grpc/mod.rs`
      covers all four read APIs — handler-level async tests via a real
      `BackendService` backed by `StorageModule`.)
- [ ] Add execution-level tests for common EOA/contract states
- [x] Add negative tests for unsupported snapshot/revert if that is the chosen temporary behavior
      (iter 11: added `create_evm_snapshot_returns_explicit_unsupported`
      and `revert_to_evm_snapshot_returns_explicit_unsupported` to
      `iter4_read_path_tests` in
      `crates/core/src/service/grpc/mod.rs`. Both assert
      `success == false`, the error_message contains both
      `"close_loop.snapshot.md"` and `"not supported"`, and
      `create_evm_snapshot` specifically asserts the returned
      `snapshot_id` is empty (no fake synthetic id). Combined
      with the iter 3 storage-engine snapshot tests and iter 4
      read-handler `snapshot_id` rejection tests, the negative
      coverage for unsupported snapshot/revert is now end-to-end
      across storage engine + storage gRPC + execution gRPC +
      read handlers.)

Acceptance:

- [x] Node-level remote execution no longer depends on placeholder query APIs
      (iter 5: `getCode`, `getStorageAt`, `getNonce`, `getBalance`,
      `healthCheck`, `createSnapshot`, `revertToSnapshot`,
      `estimateEnergy`, and `callContract` all now call through the
      Rust backend via `ExecutionGrpcClient` — no "not yet implemented"
      placeholder path remains in `RemoteExecutionSPI`. `callContract`
      still has a known request-shape mismatch tracked as a separate
      open item above.)
- [x] `callContract` and `estimateEnergy` are usable in `RR`
      (iter 6: `callContract` now carries the full `TronTransaction`
      via the new `CallContractRequest.transaction` proto field and
      classifies outcomes via the structured
      `CallContractResponse.Status` enum. Both methods see the same
      transaction shape the full execution path sees. Payable,
      fee-limit-sensitive, and TRC-10/contract-type-sensitive calls
      are no longer silently collapsed onto hardcoded defaults.)
- [x] Query APIs either work or fail explicitly, never with fake success payloads
      (iter 5: every Java read API fails-hard on `!success`; the
      old "empty byte[] / 0 long + warn log" placeholders are gone.
      The Rust `call_contract` handler was also fixed to stop
      hardcoding `success: true` on REVM reverts — it now propagates
      `TronExecutionResult.success` / `error` faithfully.)

---

## 3. Storage semantic hardening

Goal: upgrade storage from "hot-path operations work" to "execution can safely rely on the semantics it claims to expose".

### 3.1 `transaction_id` end-to-end plumbing

Primary touchpoints:

- `framework/src/main/proto/backend.proto`
- `framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

- [x] Audit all Java write calls that could carry `transaction_id`
      (iter 10: audit frozen in `close_loop.java_transaction_id.md`.
      There are exactly three Java SPI write calls:
      `RemoteStorageSPI.put` / `delete` / `batchWrite`, all of
      which currently build their proto request without
      populating the `transaction_id` field. The Java SPI
      interface itself has no `transaction_id` parameter on
      these signatures. **Crucially, `RemoteStorageSPI` is not
      on the production hot path**: the actual FullNode
      application uses hardcoded `chainbase`-backed storage in
      `TronDatabase.java` / `TronStoreWithRevoking.java`, not
      the SPI factory. So no Java production caller would
      benefit from `transaction_id` plumbing today.)
- [x] Define where transaction identifiers are created and owned
      (iter 10: the block importer (Phase 2 milestone) is the
      canonical owner. It opens one transaction per block,
      threads its id through every write, and commits at
      end-of-block. No other Java code path is expected to
      open transactions in Phase 1, per
      `close_loop.storage_transactions.md` "What execution
      actually needs right now".)
- [ ] Pass `transaction_id` through Java `put/delete/batchWrite`
      (still open — Phase 2 work. The right design is a 4-arg
      overloaded trio that leaves the existing 3-arg variants as
      "direct write" convenience delegating with `txId == ""`.
      Detailed in `close_loop.java_transaction_id.md` §"Decisions
      for Phase 1" #1. Gated on the block importer becoming the
      first Java production caller that owns a transaction.)
- [x] Make Rust gRPC handlers branch on `transaction_id` instead of always writing directly
      (iter 3: `put`, `delete`, and `batch_write` gRPC handlers in
      `crates/core/src/service/grpc/mod.rs` now branch on
      `req.transaction_id`. Empty string → direct write against the
      base DB. Non-empty → routes through `engine.put_in_tx` /
      `delete_in_tx` / `batch_write_in_tx`. Unknown ids return an
      explicit "transaction not found" error.)
- [x] Document default behavior for non-transaction-scoped writes
      (frozen in `close_loop.storage_transactions.md`: empty
      `transaction_id` means direct write against the base DB.)
- [x] Add tracing/logging that makes it obvious whether a write was transactional or direct
      (iter 3: each handler emits a debug line tagged "Direct put" /
      "Transactional put" / "Direct delete" / etc., including the
      transaction id when present.)

### 3.2 Transaction buffer semantics in Rust storage

Primary touchpoints:

- `rust-backend/crates/storage/src/engine.rs`

- [x] Add real per-transaction operation buffers
      (iter 3: the existing `TransactionInfo::operations` Vec is now
      actually populated. The methods `put_in_tx`, `delete_in_tx`,
      and `batch_write_in_tx` lock the per-tx entry, validate the
      DB scope, and append `BatchOp` entries.)
- [x] Route transactional `put` into the buffer
- [x] Route transactional `delete` into the buffer
- [x] Route transactional `batch_write` into the buffer
      (iter 3: `batch_write_in_tx` validates every op type up-front,
      so a malformed batch never partially-mutates the buffer.)
- [x] Apply buffered operations atomically on `commit`
      (already true; iter 3 added tests that exercise this end-to-end.)
- [x] Discard buffered operations on `rollback`
      (already true; iter 3 added tests.)
- [x] Decide read-your-writes behavior for transaction-scoped reads
      (decided in 1.3: NOT provided in Phase 1. Tests assert that
      buffered writes are invisible to direct `get` until commit.)
- [x] If read-your-writes is required, design layered read behavior over buffered writes
      (n/a — read-your-writes is explicitly out of scope per 1.3.)
- [x] Decide whether transaction-scoped iterators/range queries are in scope or explicitly unsupported
      (out of scope per 1.3 — `close_loop.storage_transactions.md`.)

### 3.3 Snapshot correctness

Primary touchpoints:

- `rust-backend/crates/storage/src/engine.rs`
- `rust-backend/crates/core/src/service/grpc/mod.rs`

- [ ] Replace current "snapshot reads current DB" behavior with real point-in-time semantics
      (Phase 1 decision: not implemented this phase — see next item)
- [x] If real snapshot is not implemented this phase, remove fake behavior and surface explicit unsupported
      (iter 2: `engine.rs` `create_snapshot` / `delete_snapshot` /
      `get_from_snapshot` now return explicit `Err(...)` instead of
      silently reading the live DB. The gRPC handlers in
      `service/grpc/mod.rs` already forward engine errors as
      `success: false`, so the contract is end-to-end now.)
- [x] Define snapshot lifecycle:
  - [x] creation       (no-op error, per `close_loop.snapshot.md`)
  - [x] read paths allowed   (none — per the same note)
  - [x] deletion       (no-op error, per the same note)
  - [x] cleanup on process shutdown   (no allocation to clean up)
- [x] Define interaction rules between transactions and snapshots
      (no interaction rules — both APIs are deferred independently in Phase 1; see `close_loop.snapshot.md`)
- [x] Decide whether iterator APIs against snapshot are needed now or later
      (later — explicitly out of scope per `close_loop.snapshot.md`)

### 3.4 Storage tests and EE/RR comparison checks

Rust-focused:

- [x] Add unit tests for CRUD
      (iter 3: `direct_put_and_get_round_trips`,
      `direct_delete_removes_key`, `get_missing_key_returns_none` in
      `crates/storage/src/engine.rs` tests module.)
- [x] Add unit tests for batch writes
      (iter 3: `batch_write_applies_puts_and_deletes_atomically`,
      `batch_write_rejects_unknown_op_type`.)
- [x] Add unit tests for transaction commit
      (iter 3: `transactional_commit_applies_buffered_writes`,
      `transactional_commit_applies_deletes`,
      `transactional_batch_write_commit`.)
- [x] Add unit tests for transaction rollback
      (iter 3: `transactional_rollback_discards_buffered_writes`,
      `rollback_then_commit_same_id_fails`.)
- [x] Add unit tests for snapshot correctness
      (iter 3: `create_snapshot_returns_explicit_error`,
      `delete_snapshot_returns_explicit_error`,
      `get_from_snapshot_returns_explicit_error` — snapshot APIs are
      explicitly unsupported per `close_loop.snapshot.md`, and these
      tests lock that contract.)
- [x] Add tests for absent `transaction_id` at the engine layer
      (iter 3: direct-path CRUD/batch tests exercise the engine API
      directly, which is the "absent transaction_id" contract at that
      layer. gRPC-level coverage that proves an empty-string
      `req.transaction_id` still routes to the direct engine path is
      NOT yet in place — tracked as an open follow-up under Java
      integration coverage in this section.)
- [x] Add tests for transaction not found / snapshot unsupported
      (iter 3: `put_in_tx_unknown_id_is_rejected`,
      `delete_in_tx_unknown_id_is_rejected`,
      `batch_write_in_tx_unknown_id_is_rejected`,
      `commit_unknown_id_is_rejected`,
      `rollback_unknown_id_is_rejected`,
      `create_snapshot_returns_explicit_error`,
      `delete_snapshot_returns_explicit_error`,
      `get_from_snapshot_returns_explicit_error`. The Phase 1
      contract for snapshots is "explicitly unsupported" rather than
      "not found" — see `close_loop.snapshot.md`.)
- [x] Add tests for concurrent transaction IDs and cleanup paths
      (iter 3: `concurrent_transactions_are_isolated`,
      `rollback_does_not_affect_other_concurrent_transaction`.)

Java-focused:

- [ ] Extend or add integration coverage around `RemoteStorageSPI`
      (partial — `StorageSPIIntegrationTest.testSnapshotOperationsAreUnsupported`
      locks the unsupported snapshot contract on the Java side in
      iter 2. Transactional put/delete/batchWrite coverage still open.)
- [ ] Add tests proving Java actually carries `transaction_id` into remote writes
      (still open — depends on 3.1 Java-side plumbing, which is still
      tracked as an open audit item under 3.1.)
- [x] Add gRPC-handler coverage for `transaction_id = ""` on put/delete/batch_write
      (iter 11 closes the Rust half of this item. Added 9 new
      handler-level #[tokio::test]s to `iter4_read_path_tests`:
      `put_direct_path_round_trips_via_get`,
      `put_buffered_path_is_invisible_until_commit`,
      `put_buffered_path_is_discarded_on_rollback`,
      `put_unknown_transaction_id_is_rejected`,
      `delete_direct_path_removes_existing_key`,
      `delete_buffered_path_honors_rollback`,
      `batch_write_direct_path_applies_all_ops`,
      `batch_write_buffered_path_reports_zero_operations_applied`,
      `batch_write_unknown_transaction_id_is_rejected`. Each uses
      the existing `build_read_path_test_service()` helper so the
      test stack is real-BackendService + real-StorageModule against
      a tempdir. Tests lock: direct-path round-trip; buffered-path
      read-isolation pre-commit; buffered-path visibility post-commit;
      buffered-path rollback discards; unknown tx_id produces explicit
      error with no silent fallback to direct write; iter-6
      `operations_applied` semantics (direct branch returns ops.len(),
      buffered branch returns 0). The Java-side half (an integration
      test against a live Rust backend process) is still open pending
      the gradle/Lombok env unblock — Section 3.1 Phase 2 follow-up
      will add it when the env issue is fixed.)
- [ ] Add `EE` run vs `RR` run semantic checks where possible
- [ ] Avoid using `DualStorageModeIntegrationTest` as if mode-switch wiring alone proves semantic parity

Acceptance:

- [x] Storage transaction APIs are no longer structural placeholders
      (iter 3: `put_in_tx` / `delete_in_tx` / `batch_write_in_tx`
      populate the per-tx buffer, commit applies atomically, rollback
      discards, and 22 storage tests lock the contract end-to-end.)
- [x] Snapshot is either real or explicitly unavailable
      (iter 2 + iter 3: storage engine and both Java SPI paths return
      explicit unsupported errors, with Rust unit tests and a Java
      integration test asserting the contract.)
- [x] Storage crate test suite has meaningful coverage and is no longer `0 tests`
      (iter 3: `cargo test -p tron-backend-storage` now runs 22 tests,
      all green.)

---

## 4. Close state-ownership gaps and bridge debt

Goal: reduce the number of "temporary bridge" pieces that hide split ownership between Java and Rust.

Primary touchpoints:

- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java`
- any code paths that pre-sync Java-side mutations before remote execution

Audit and removal sequence are frozen in sibling note `close_loop.bridge_debt.md`.

- [x] Audit every place where Java mutates state and then pushes/synchronizes it to Rust
      (iter 4: 5 bridges identified — B1 ResourceSyncService, B2
      RuntimeSpiImpl.apply* family, B3 postExecMirror, B4 pre-exec
      AEXT snapshot, B5 genesis account seeding.)
- [x] Classify each bridge as:
  - [x] required in Phase 1
        (B1 ResourceSyncService, B3 postExecMirror, B4 pre-exec AEXT, B5 genesis seeding)
  - [x] removable once write ownership is frozen
        (B2 apply* family — transitional; compute-only profile only)
  - [x] must survive into block importer phase
        (B3 postExecMirror, B5 genesis seeding — both are removable
        only after Phase 2+ capabilities land)
- [x] Document whether `ResourceSyncService` is:
  - [x] a transitional patch
        (yes — tied to Java-side maintenance/reward mutations that
        Rust has not absorbed; removable once the Rust block importer
        takes those over)
  - [ ] a medium-term integration layer
  - [ ] fundamentally incompatible with final ownership goals
- [x] Write an explicit "bridge removal sequence" note for after Phase 1
      (iter 4: sequence documented in `close_loop.bridge_debt.md` as
      B2 → B4 → B1 → B5 → B3, with the reasoning for each step.)
- [x] Confirm no new bridge mechanism should be added without first checking ownership implications
      (iter 4: anti-regression rule recorded in
      `close_loop.bridge_debt.md` §"Anti-regression rule".)

Acceptance:

- [x] The project has an explicit list of temporary bridge mechanisms
      (the summary table in `close_loop.bridge_debt.md`)
- [x] Temporary bridge debt is visible and sequenced, not hidden
      (each bridge has a Phase 1 classification and a post-Phase-1
      removal position in the sequence)

---

## 5. Execution parity edge cases and semantic closure

Goal: stop calling execution "basically done" while known semantic holes still exist on important branches.

### 5.1 `TriggerSmartContract` TRC-10 pre-execution transfer

Primary touchpoints:

- `rust-backend/crates/execution/src/lib.rs`
- related trigger/VM tests and conformance fixtures
- `planning/review_again/TRIGGER_SMART_CONTRACT.todo.md`

The design is frozen in sibling note
`close_loop.trigger_trc10_pre_transfer.md`. Phase 1 takes the
"explicitly kept out of the `RR` whitelist target" branch of the
acceptance criterion: the existing reject path stays, the design
becomes the implementation blueprint for Phase 2.

- [x] Keep the existing explicit reject path until the replacement semantics are designed
      (iter 8: `rust-backend/crates/execution/src/lib.rs:523-538`
      reject path stays in place. The contract matrix tags
      `TriggerSmartContract` as `RR blocked` and the Phase 1
      whitelist target does NOT include it.)
- [x] Design Java-parity pre-exec token transfer semantics for trigger calls
      (iter 8: design captured in
      `close_loop.trigger_trc10_pre_transfer.md` §"Phase 1 design".
      Mirrors `MUtil.transferToken` from
      `actuator/.../org/tron/core/vm/utils/MUtil.java:43` and the
      surrounding `VMActuator.java:549` call site.)
- [x] Define rollback behavior on VM failure
      (iter 8: design notes that rollback CANNOT today ride on
      the existing buffer for VM trigger execution in the
      compute-only RR profile — `grpc/mod.rs:1431` only attaches
      `EngineBackedEvmStateStore::new_with_buffer` when
      `rust_persist_enabled == true` OR `tx_kind == NonVm`.
      Phase 2 implementation must add the buffer attachment for
      VM execution paths in compute-only RR mode (or introduce a
      dedicated TRC-10 pre-transfer journal) before the
      pre-transfer hook is safe to enable. The canonical RR
      profile already gets the buffer; the gap is specifically
      the compute-only profile. Documented as a Phase 2
      prerequisite in the design note's rollback section.)
- [x] Define interaction with energy accounting and side effects
      (iter 8: pre-transfer does NOT charge energy or bandwidth
      separately — Java's `MUtil.transferToken` does not, and Rust
      must match. Sidecar emission MUST go through S4
      `Trc10Change.AssetTransferred`. The S1 `AccountChange`
      alternative is closed as non-viable: `AccountInfo` /
      `AccountChange` carries no TRC-10 asset map, so a balance
      delta cannot be expressed there without a proto schema
      change. Sidecar emission belongs on the success-shaped
      `Ok(...)` arm AFTER successful VM execution / result
      formation — NOT after the outer gRPC commit (which lives
      later in the pipeline at `grpc/mod.rs:1588` and would not
      be observable from inside the execution function). On
      revert / halt arms the handler MUST populate
      `trc10_changes: vec![]`.)
- [ ] Add targeted tests for:
  - [ ] happy path token pre-transfer
  - [ ] insufficient balance
  - [ ] missing asset
  - [ ] VM revert after pre-transfer
  - [ ] no token transfer when `tokenValue == 0`
        (test plan table is in
        `close_loop.trigger_trc10_pre_transfer.md` §"Test plan",
        but the tests themselves are Phase 2 implementation work
        and stay open in this iteration.)

Acceptance:

- [x] The current known gap is either closed or explicitly kept out of the `RR` whitelist target
      (iter 8: "explicitly kept out" branch — reject path stays,
      contract matrix `RR blocked` tag stays, Phase 1 whitelist
      target stays minimal. Design note freezes the implementation
      blueprint so the work is not lost when Phase 2 picks it up.)

### 5.2 Resource / fee / sidecar parity

Primary touchpoints:

- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- freeze/resource/delegation/withdraw/apply sidecar paths
- relevant Rust execution service code and tests

The audit is frozen in sibling note `close_loop.sidecar_parity.md`. It
catalogs 9 sidecar surfaces (S1 `state_changes` + AEXT, S2
`freeze_changes`, S3 `global_resource_changes`, S4 `trc10_changes`,
S5 `vote_changes`, S6 `withdraw_changes`, S7
`tron_transaction_result` receipt passthrough, S8 `contract_address`,
S9 pre-exec AEXT handshake) and a per-(sidecar × contract family)
parity checklist with an explicit list of contract families that
cannot be declared `RR canonical-ready` until their sidecar gates
clear.

- [x] Enumerate all state sidecars emitted by Rust and applied by Java
      (9 sidecars catalogued S1-S9 in `close_loop.sidecar_parity.md`)
- [x] Identify which sidecars are still incomplete or fragile
      (S2 V2 multi-row gap, S3 missing emission on delegate /
      withdraw-expire paths, S4 missing `Trc10Participated` /
      `Trc10Updated` / `Trc10Unfrozen` variants, S5 multi-cycle
      maintenance delta untested, S7 receipt passthrough lacks
      structural safety for Exchange / Market / CancelAllUnfreezeV2
      families — all called out in the audit)
- [x] Build a parity checklist for:
  - [x] freeze ledger changes (S2 row in checklist)
  - [x] global resource total changes (S3 row in checklist)
  - [x] TRC-10 changes (S4 row in checklist, with missing-variant gap)
  - [x] vote changes (S5 row in checklist, with multi-cycle gap)
  - [x] withdraw changes (S6 + S7 rows in checklist)
- [x] Decide which contract families can be declared stable only after these sidecars are verified
      (Freeze/unfreeze V1+V2, delegation, WithdrawExpireUnfreeze +
      CancelAllUnfreezeV2, TRC-10 Participate/Update/UnfreezeAsset,
      Exchange family, Market family, multi-cycle VoteWitness — all
      blocked from `RR canonical-ready` until their sidecar rows
      clear; documented in the audit's "Contract families that
      cannot declare RR canonical-ready until their sidecars are
      verified" section.)

Acceptance:

- [x] Sidecar completeness is part of contract readiness, not treated as an afterthought
      (the audit's "Anti-pattern guard" section explicitly forbids
      moving a contract from `RR candidate` to `RR canonical-ready`
      in `close_loop.contract_matrix.md` without first clearing its
      sidecar-checklist row, making the dependency structural rather
      than ad-hoc.)

### 5.3 Config and feature-flag convergence

Primary touchpoints:

- `rust-backend/config.toml`
- `rust-backend/crates/common/src/config.rs`

The audit and the two recommended profiles are frozen in sibling note
`close_loop.config_convergence.md`.

- [x] Audit every `execution.remote.*` flag currently enabled in checked-in config
- [x] Compare against code defaults
- [x] Mark each flag as:
  - [x] `EE` baseline only
  - [x] `RR` experimental
  - [x] `RR` canonical-ready (no flag currently holds this tag — see contract_matrix.md)
  - [x] legacy / should be removed (`delegation_reward_enabled`)
- [x] Produce one recommended conservative config for parity work
      (profile A in `close_loop.config_convergence.md`)
- [x] Produce one experimental config for targeted validation only
      (profile B in `close_loop.config_convergence.md`; matches current `config.toml`)

Acceptance:

- [x] The repo no longer looks "stable by config file, experimental by code comment" at the same time
      (header comments in `config.toml` and `config.rs` now identify the experimental profile and point at `close_loop.config_convergence.md`; a follow-up cleanup deletes the deprecated `delegation_reward_enabled` flag — tracked in the convergence doc)

---

## 6. Verification, replay, and release gates

Goal: turn parity from a subjective feeling into an observable gate.

The Section 6 decisions are frozen in sibling note
`close_loop.verification.md`. That file rebuilds the verification
strategy around `EE` vs `RR` comparison (replacing the
shadow-centric `GoldenVectorTestSuite` and `HistoricalReplayTool`
designs), splits write-path and read-path into separate lanes,
ties the readiness dashboard to `close_loop.contract_matrix.md`
as its source of truth, and freezes three CI smoke-set contracts.
Implementation of the rebuild is Phase 2 work; Phase 1 closes the
strategy + structural decisions.

### 6.1 Storage verification

- [x] Add storage crate tests until `tron-backend-storage` has meaningful direct coverage
      (iter 3: 23 tests passing — see Tool C in
      `close_loop.verification.md`)
- [x] Add at least one Java integration path that validates remote storage semantics, not only factory creation
      (iter 2: `StorageSPIIntegrationTest.testSnapshotOperationsAreUnsupported`
      asserts the actual end-to-end snapshot rejection contract,
      not just factory creation. Two more integration paths are
      tracked as open follow-ups for the gRPC `transaction_id`
      direct vs buffered round-trip.)
- [x] Track storage regressions separately from execution regressions
      (Rust: separate cargo packages `tron-backend-storage` vs
      `tron-backend-core`; Java: `*Storage*Test` vs `*Execution*Test`
      naming convention. CI smoke-gate decisions in 6.6 keep them
      as separate jobs.)

### 6.2 Execution lane split

- [x] Split verification into two lanes:
  - [x] write-path / execute tx parity
        (frozen as the canonical-ready definition: an EE run + RR
        run apply the same transaction and have post-state diffed,
        sidecars included)
  - [x] read-path / query parity
        (frozen as a separate test class lane: getCode /
        getStorageAt / getNonce / getBalance / callContract /
        estimateEnergy run against a known state and have
        responses diffed)
- [x] Avoid using strong write-path results to imply read-path closure
      (Section 6.5 readiness dashboard records the two lanes as
      separate columns; canonical-ready requires BOTH to be
      `passing`)
- [x] Publish separate pass/fail state for both lanes
      (CI smoke-gate decisions in 6.6 require three independent
      jobs: `EE` smoke, `RR` smoke, `EE-vs-RR diff` smoke)

### 6.3 Golden vectors

Primary touchpoints:

- `framework/src/test/java/org/tron/core/execution/spi/GoldenVectorTestSuite.java`

- [x] Make golden vectors execute the same input in separate `EE` and `RR` runs
      (decision frozen in `close_loop.verification.md` §6.3 #1; the
      existing class needs a rebuild to drop the
      `ENABLE_SHADOW_EXECUTION` field and replace
      `ExecutionSpiFactory.createExecution()` with explicit-mode
      construction. Tracked as a Phase 2 implementation follow-up.)
- [x] Add a comparator that records `EE` result vs `RR` result
      (`EeVsRrComparator` shape frozen in §6.3 #2 — walks
      `success` / status / `return_data` / `energy_used` /
      `bandwidth_used` / `state_changes` / `freeze_changes` /
      `global_resource_changes` / `trc10_changes` / `vote_changes` /
      `withdraw_changes` / `tron_transaction_result` /
      `contract_address`. Default energy comparison is exact;
      per-vector tolerance is opt-in only with written
      justification.)
- [x] Add vectors for remote read/query APIs where appropriate
      (frozen as a separate `GoldenReadPathVectorTestSuite` class
      so a write-path failure cannot pollute the read-path lane;
      §6.3 #5)
- [x] Add vectors for known mismatch-prone branches:
  - [x] trigger smart contract
        (happy-path constant call, payable trigger gated by 5.1,
        reverting trigger, `tokenValue == 0` trigger;
        `tokenValue > 0` deliberately uncovered until 5.1 lands)
  - [x] create smart contract
        (happy path already passing in iter 0 baseline;
        `consume_user_resource_percent` boundary cases)
  - [x] update setting / metadata
        (happy path already passing in iter 0 baseline; permission
        denied; non-existent contract)
  - [x] resource/freeze paths
        (FreezeBalance V1+V2, UnfreezeBalance V1+V2 — covers the
        S2/S3 freeze-path slice. Delegation / withdraw-expire /
        cancel-all are intentionally NOT covered until their S2/S3
        sidecar gaps from `close_loop.sidecar_parity.md` close.)

### 6.4 Historical replay

Primary touchpoints:

- `framework/src/test/java/org/tron/core/execution/spi/HistoricalReplayTool.java`

- [x] Pick a small fixed replay range for routine work
      (frozen as 1000 blocks from a pinned mainnet height;
      §6.4 #2 routine range)
- [x] Pick a larger replay range for milestone validation
      (frozen as 50000+ blocks; §6.4 #2 milestone range)
- [x] Run replay once in `EE`
- [x] Run replay once in `RR`
      (single tool invocation runs both engines under the same
      input — the diff is the whole point. §6.4 #3)
- [x] Compare outputs by contract type
      (`ReplayReport` already has per-contract-type counters; the
      rebuild adds per-lane split. §6.4 #4)
- [x] Compare outputs by read-path vs write-path
      (per-lane split is part of the same rebuild; §6.4 #4)
- [x] Record whether mismatch is:
  - [x] result-code only
  - [x] energy only
  - [x] return-data only
  - [x] state-change / sidecar difference
        (frozen as the four `mismatch_classification` tags on
        `MismatchReport`; §6.4 #5. State-change / sidecar
        differences are explicitly the most serious and block
        canonical-ready.)

### 6.5 Contract readiness dashboard

- [x] Turn the support matrix into a living readiness table
      (Phase 1: `close_loop.contract_matrix.md` IS the dashboard,
      engineers read the markdown. Phase 2 builds the actual
      generator that publishes an HTML view. §6.5 #1)
- [x] For each contract type, record:
  - [x] `RR` support status (already in matrix)
  - [x] fixture coverage (already in matrix)
  - [x] Rust unit coverage (already in matrix)
  - [x] `EE-vs-RR` replay status
        (NEW columns to add to the matrix: separate `replay
        write lane` and `replay read lane` cells. Phase 2
        starts populating them as the comparator produces data.
        §6.5 #2)
  - [x] major known gaps (already in matrix as per-row notes)
- [x] Use the readiness table as the only source of truth for enabling canonical `RR` support
      (§6.5 #3 makes this structural: a contract cannot move from
      `RR candidate` to `RR canonical-ready` without BOTH replay
      lanes `passing` AND all sidecar gates from
      `close_loop.sidecar_parity.md` `passing` or `n/a`)

### 6.6 CI smoke gates

- [x] Define a minimal `EE` smoke set
      (frozen: `cargo test -p tron-backend-storage` (23 tests) +
      `cargo test -p tron-backend-core create_smart_contract`
      (17 tests) + `cargo test -p tron-backend-core update_setting`
      (17 tests) + Java `EmbeddedExecutionSPI` happy-path against
      Phase 1 whitelist target. Wall-clock target: <5 minutes.
      §6.6 #1 first bullet.)
- [x] Define a minimal `RR` smoke set
      (frozen: same workload routed through `RemoteExecutionSPI`
      against a locally-launched Rust backend. Reuses
      `iter4_read_path_tests` for the read-path slice plus three
      whitelist contracts for the write-path slice. Wall-clock
      target: <10 minutes. §6.6 #1 second bullet.)
- [x] Define a minimal `EE-vs-RR diff` smoke set
      (frozen: routine 1000-block historical replay range with
      `failOnFirstMismatch=true`, runs both engines + comparator,
      fails CI on any mismatch. Wall-clock target: <15 minutes.
      §6.6 #1 third bullet.)
- [x] Make CI output mismatches in a readable, triageable form
      (frozen contract: failure message must include the
      mismatch_classification tag, the contract type, the
      smallest-possible diff summary, and a pointer to the full
      ReplayReport artifact. No "see logs" outputs. §6.6 #3.)

Acceptance:

- [x] The project can answer "what is safe to enable today?" from tests and dashboards, not from memory
      (Phase 1 acceptance check in `close_loop.verification.md`:
      the answer is the Phase 1 whitelist target — TransferContract,
      CreateSmartContract, UpdateSettingContract — answerable
      from the three baseline `cargo test` runs that already
      exist plus the contract matrix + sidecar audit + bridge
      debt audit anti-pattern guards. The actual rebuild of
      `GoldenVectorTestSuite` and `HistoricalReplayTool` to make
      the dashboard automatic is tracked as Phase 2 work in
      `close_loop.verification.md` §"Follow-up implementation
      items".)

---

## 7. Sequencing and parallel work

Goal: keep the critical path clear and avoid starting expensive but premature work.

The sequencing decisions below are frozen in `close_loop.scope.md` and the
iteration 1 scope-freeze chunk. The checkboxes here only track that the
sequencing has been written down.

### 7.1 Critical path

- [x] Phase 1 critical path is:
  - [x] semantic freeze
  - [x] execution read-path closure
  - [x] storage transaction/snapshot closure
  - [x] parity verification
  - [x] block importer readiness planning
- [x] Explicitly keep `P2P / sync / consensus rewrite` off the critical path

### 7.2 Suggested first batch

These items were the first-batch suggestion. The acceptance check below
records *whether the first-batch sequencing has been declared*, not whether
each item has been implemented end to end. Each item still has its own
acceptance gate in its own subsection above.

- [x] Start with these items first:
  - [x] 1.1 Canonical write ownership                  (done in iteration 1)
  - [x] 1.2 `energy_limit` wire contract               (decision done in iter 1; migration code follow-up still open)
  - [x] 1.3 storage transaction semantics              (done in iteration 1)
  - [x] 1.5 contract support matrix                    (done in iteration 1)
  - [x] 2.1 Java `callContract/estimateEnergy`         (iter 5 landed real `estimateEnergy`, all four read APIs, `healthCheck`, and a real `callContract` round-trip with revert/halt/transport discrimination. iter 6 added the additive proto fields `CallContractRequest.transaction` and `CallContractResponse.Status` and rewired both sides to use them, closing the shape-mismatch and response-discriminator follow-ups that iter 5 tracked.)
  - [ ] 3.1 `transaction_id` plumbing                  (partial — iter 3 landed the Rust engine buffer and the Rust gRPC handlers branch on `transaction_id`. The Java `RemoteStorageSPI` still does not thread a `transaction_id` into its `put`/`delete`/`batchWrite` calls; the Java bridge audit for where transactions should be opened is still open.)

### 7.3 Parallelization opportunities

- [x] Run Java execution bridge work in parallel with Rust storage semantics work
- [x] Run Rust execution query implementation in parallel with verification harness improvements
- [x] Keep one owner responsible for semantic freeze so implementation work does not diverge
      (close_loop iteration owner role; recorded in `close_loop.scope.md`)

---

## 8. Explicit non-goals and defer list

These items should remain out of scope until the exit criteria above are met.
All of them are also frozen in `close_loop.scope.md`. The checkboxes here
only confirm that the non-goal has been declared in writing — they do NOT
mean "we have started this and finished it".

- [x] Do not start Rust P2P handshake work
- [x] Do not start Rust peer/session manager work
- [x] Do not start Rust sync scheduler / inventory pipeline work
- [x] Do not start Rust consensus scheduling rewrite
- [x] Do not optimize for mixed execution/storage modes
- [x] Do not make current `SHADOW` the main acceptance path again
- [x] Do not treat "many system contracts already run remotely" as proof that the full execution problem is solved
- [x] Do not treat "storage CRUD works" as proof that storage semantics are solved

---

## 9. Handoff to next phase

The Phase 1 → Phase 2 transition plan is frozen in sibling note
`close_loop.handoff.md`. That file documents the Phase 1 status
snapshot, breaks Phase 2 into 5 ordered milestones (2.A
verification rebuild, 2.B bridge removal sequence start, 2.C
block importer / block executor, 2.D trigger TRC-10 pre-transfer
code change, 2.E energy_limit wire migration), enumerates what
Phase 2 deliberately does NOT do (still no P2P / consensus / Java
shell removal), and locks the two re-evaluation questions for
later. The handoff acceptance is satisfied by the existence of
that planning note plus the surviving Phase 1 anti-pattern guards.

Only after this file's exit criteria are met:

- [ ] Open `BLOCK-01` planning for Rust block importer / block executor
      (Phase 2.C deliverable per `close_loop.handoff.md`. The
      first Phase 2.C item is replacing this bullet with an
      actual `BLOCK-01` planning note.)
- [ ] Decompose `Manager.processBlock(...)` into Rust-owned responsibilities
      (Phase 2.C scope. The decomposition is what `BLOCK-01`
      will plan in detail.)
- [x] Re-evaluate whether consensus should follow block importer or stay on Java longer
      (re-evaluation question is recorded as Phase 2.x #1 in
      `close_loop.handoff.md`. The answer depends on how
      cleanly the importer absorbs `Manager.processBlock`.
      Phase 1 cannot answer it; the handoff guarantees the
      question doesn't get lost.)
- [x] Re-evaluate whether P2P should remain Java-owned until after importer stability
      (re-evaluation question is recorded as Phase 2.x #2 in
      `close_loop.handoff.md`. Almost certainly yes — the
      `close_loop.scope.md` "why not P2P yet" rationale still
      holds at the start of Phase 2 and is unlikely to flip
      until the importer is observably stable for at least one
      release cycle.)

Success condition for this handoff:

- [x] The next roadmap discussion starts from "Rust state-transition engine ownership", not from "networking looks exciting"
      (locked structurally in `close_loop.handoff.md`: every
      Phase 2.x sub-section is about state transitions;
      networking is in the explicit "what Phase 2 does NOT do"
      list. Any Phase 2 planning note that argues otherwise
      must explicitly cite this section to override the
      anti-pattern guard.)
