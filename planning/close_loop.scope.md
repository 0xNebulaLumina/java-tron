# Close Loop — Phase 1 Scope Freeze

This is the durable companion to `close_loop.planning.md` and `close_loop.todo.md`.
Its purpose is to lock the Phase 1 boundaries so that later planning work does
not silently drift back into scope that this phase explicitly rejected.

If you find yourself arguing about "should we do X now?" and X is listed here
as out of scope, the answer is no — reopen this file instead.

## Phase 1 scope (frozen)

Phase 1 covers only three workstreams:

1. **Execution semantics** — close Java/Rust read/query placeholders, lock
   `energy_limit` wire contract, lock snapshot behavior, and reach EE-vs-RR
   parity on a first contract whitelist.
2. **Storage semantics** — real transaction buffering on the Rust side, real
   or explicitly unsupported snapshot behavior, end-to-end `transaction_id`
   plumbing, and direct Rust-side test coverage for the `tron-backend-storage`
   crate.
3. **Parity verification** — EE-vs-RR golden vectors, EE-vs-RR replay, CI
   smoke gates, and a contract readiness dashboard.

Nothing else in Phase 1 requires a plan. Anything that looks like it belongs
in Phase 1 but is not on that list should be routed through the exit-criteria
review below before being treated as in scope.

## Strategic modes (frozen)

Phase 1 recognizes only two target modes:

- `EE` — embedded execution + embedded storage (Java canonical baseline).
- `RR` — remote execution + remote storage (Rust target).

Mixed combinations (`remote execution + embedded storage`, etc.) are **not**
target modes for this phase. They may still compile, but they are not the
mode we are trying to validate against the Java baseline, and we do not
optimize or guard them.

The current in-process `SHADOW` mode is **not** a Phase 1 acceptance mode.
See the "Why not SHADOW as the main validator" section below.

`EmbeddedExecutionSPI` and `EmbeddedStorageSPI` are the canonical reference
implementations. `RemoteExecutionSPI` and `RemoteStorageSPI` are the Phase 1
target implementations.

## Explicit non-goals (frozen)

None of the following are Phase 1 work, and none of them may be pulled
forward by any Phase 1 task:

- Rust P2P networking rewrite.
- Rust peer / session / handshake manager work.
- Rust sync scheduler, inventory pipeline, or fetch-queue work.
- Rust consensus scheduling rewrite.
- Any attempt to remove the Java node shell in this phase.
- Any optimization effort aimed at mixed `execution + storage` combinations.
- Any regression that puts `SHADOW` back on the critical path for validation.
- Any "the write path is substantial, therefore execution is done" claim
  used as a shortcut to skip read-path closure.
- Any "storage CRUD works, therefore storage is done" claim used as a
  shortcut to skip transaction/snapshot semantics.

## Next milestone after Phase 1 (frozen)

The intended next milestone is:

- **Rust block importer / block executor readiness** (aka `BLOCK-01`).

**Not**:

- Rust P2P rewrite.
- Rust consensus scheduler.
- Full Java node removal.

## Why not P2P yet

P2P is intentionally deferred because it is the noisiest edge of the node
*and* the most tightly coupled to state transitions. Doing P2P before the
Rust state-transition core is trusted would combine the noisiest edge of
the system with its most stateful core — the highest-risk ordering we can
pick.

Concretely:

- Node startup wires many subsystems together; the P2P boot path has
  dependencies across channel managers, peer state, and backup managers.
- Message dispatch in `framework/src/main/java/org/tron/core/net/service/`
  fans out to handshake, inventory, block, sync, PBFT, relay, and fetch
  flows — replacing one means touching all of them.
- Block sync is tightly coupled to block processing, which is tightly
  coupled to execution, maintenance, rewards, and consensus apply paths.

So: do not start Rust P2P work until the state-transition engine is
trusted in Phase 1 *and* the block importer has been delivered in Phase 2.
By that point, P2P becomes a bounded "feed blocks into a trusted core"
problem, instead of "rebuild the edge and the core at the same time".

If the roadmap starts drifting toward "networking looks exciting, let's
start there", come back and re-read this section.

## Why not SHADOW as the main validator

The current in-process `SHADOW` execution mode is legacy tooling. It is
intentionally **not** the acceptance mechanism for Phase 1.

Reasons:

- `SHADOW` runs both engines inside the same JVM and relies on shared
  global state (singleton factories, singleton stores, JVM-level caches,
  thread-local context). That makes its comparison results suspect: the
  two "engines" do not actually live in clean isolation.
- Any bug in one side that mutates shared state affects the other side's
  run, so a passing `SHADOW` result does not prove that an independent
  `RR` run would reach the same state.
- The comparison we actually want is "run `EE` to completion, run `RR`
  to completion in a separate process, diff outputs and state". `SHADOW`
  cannot give us that.
- The planning work inside this repo has repeatedly reached conclusions
  that later turned out to depend on `SHADOW`-specific scaffolding. Those
  conclusions did not survive the move to real `RR`.

For Phase 1 and forward:

- `SHADOW` may still be used as a developer convenience for quick smoke
  runs, but it must never be cited as evidence of parity.
- Golden vectors, replay, and CI smoke gates all target `EE` and `RR`
  separately, then diff the two.
- If a task ends up saying "we proved this by running SHADOW", treat that
  as an unfinished task and escalate — it is not evidence for Phase 1
  acceptance.

## Phase 1 exit criteria (frozen)

Phase 1 is only complete when **all** of the following are true. None of
these may be waived by "we'll fix it in Phase 2":

- Java remote execution read / query APIs are no longer placeholders in
  the `RR` path. `callContract`, `estimateEnergy`, `getCode`, `getStorageAt`,
  `getNonce`, `getBalance` either work or fail explicitly.
- Rust remote execution read / query APIs are either implemented or
  explicitly unsupported — never silently returning fake success payloads.
- `energy_limit` wire semantics are locked: there is no remaining ambiguity
  about whether Java sends fee-limit SUN or already-converted energy units.
- Storage transaction semantics are real enough for execution needs:
  `beginTransaction` / `commit` / `rollback` actually buffer and apply
  operations atomically, and the behavior when `transaction_id` is absent
  is documented.
- Storage snapshot semantics are real, or snapshot is explicitly
  unavailable. Fake "snapshot reads from the current DB" behavior is
  not acceptable.
- Canonical write ownership in `EE` and `RR` is unambiguous, matches
  the checked-in `config.toml` and `config.rs` defaults, and matches
  the Java `RuntimeSpiImpl` apply path.
- A first contract whitelist has reached stable `EE-vs-RR` parity (both
  run in isolation, outputs diff cleanly). The contract support matrix
  identifies which contracts are in that whitelist.
- `tron-backend-storage` crate has real tests — no more `0 tests`.
- EE-vs-RR replay and CI can continuously report parity state without
  depending on `SHADOW`.

## Review cadence

Revisit this file at the start of every Phase 1 planning chunk. If the
answer to "am I about to work on something that is listed as out of
scope?" is yes, stop and escalate. If the answer to "do the current
code defaults match this document?" is no, fix the defaults or fix this
document — but do not let them diverge silently.
