# Close Loop — Section 9 Handoff to Phase 2

This file closes Section 9 of `close_loop.todo.md`. The Phase 1
exit criteria (Section 0) are not all green — see the Phase 1
status snapshot below — so the handoff itself is a forward-looking
plan, not a "Phase 1 is done, here's the next thing" declaration.
The point of this file is to make sure that when Phase 1 IS
green, the next phase starts from "Rust state-transition engine
ownership" rather than "networking looks exciting", per the
Section 9 success condition.

Companion notes:

- `close_loop.scope.md` — why-not-P2P-yet rationale that this
  handoff continues to enforce.
- `close_loop.contract_matrix.md` — readiness matrix the
  handoff inherits.
- `close_loop.sidecar_parity.md` — sidecar gates Phase 2 must
  observe.
- `close_loop.bridge_debt.md` — bridge removal sequence Phase 2
  begins to execute.
- `close_loop.verification.md` — Phase 2 verification rebuild
  follow-ups.
- `close_loop.trigger_trc10_pre_transfer.md` — Phase 2 design
  blueprint for the trigger gap.
- `close_loop.java_transaction_id.md` — Phase 2 plumbing
  blueprint for the Java SPI signature reshape.

## Phase 1 status snapshot

What is green at the time of this handoff freeze:

- Section 0 + Section 1 — all decisions frozen across the
  scope, write ownership, energy_limit wire contract, storage
  transaction semantics, snapshot semantics, and contract
  support matrix.
- Section 2.1 — every Java bridge placeholder replaced
  (`callContract` / `estimateEnergy` / `getCode` / `getStorageAt`
  / `getNonce` / `getBalance` / `healthCheck` / `createSnapshot`
  / `revertToSnapshot`). The iter-6 proto reshape gives
  `callContract` a structured `Status` discriminator and the
  full `TronTransaction` shape.
- Section 2.2 — every Rust execution gRPC handler is real or
  explicitly unsupported. No fake-success placeholder remains.
- Section 2.3 — covered by the per-handler decisions in 2.1
  / 2.2 and by the iter 4–6 tests.
- Section 2.4 — Rust-side coverage in place (23 storage tests
  + 23 read-path handler tests + 4 iter-6 converter/wire-value
  tests + 17 baseline `create_smart_contract` + 17 baseline
  `update_setting`); Java-side targeted tests still open.
- Section 3.1 — Rust gRPC `transaction_id` branching done in
  iter 3; Java audit done in iter 10
  (`close_loop.java_transaction_id.md`); production plumbing
  is not needed in Phase 1 because no Java production caller
  uses `RemoteStorageSPI` today.
- Section 3.2 — Rust per-tx buffer wired through, atomic commit
  / discard, no read-your-writes (per 1.3 decision).
- Section 3.3 — snapshot APIs return explicit unsupported errors
  end to end (storage engine + both Java SPI surfaces + the
  Rust execution gRPC `create_evm_snapshot` /
  `revert_to_evm_snapshot` handlers).
- Section 3.4 — Rust-side tests in place; Java-side
  transactional + direct integration tests still open.
- Section 4 — bridge debt audit + removal sequence in
  `close_loop.bridge_debt.md` (B2 → B4 → B1 → B5 → B3).
- Section 5.1 — design freeze for `TriggerSmartContract` TRC-10
  pre-execution transfer in
  `close_loop.trigger_trc10_pre_transfer.md`. Existing reject
  path stays; design is the Phase 2 blueprint.
- Section 5.2 — sidecar parity audit in
  `close_loop.sidecar_parity.md` cataloging 11 sidecar surfaces
  (S1–S11) with per-(sidecar × contract family) parity status
  and the canonical-ready gate machinery.
- Section 5.3 — config flag convergence audit in
  `close_loop.config_convergence.md` with conservative +
  experimental profiles.
- Section 6 — verification strategy frozen in
  `close_loop.verification.md` covering Tools A–F + EE-vs-RR
  comparator design + lane split + 4-category mismatch
  classification + three CI smoke gates. Implementation
  rebuild is Phase 2 work.
- Section 7 + Section 8 — sequencing and non-goals frozen in
  `close_loop.scope.md` and reflected in the todo.

What is still open:

- Section 0 exit criteria sub-items that depend on actual
  parity runs (a first contract whitelist reaching stable
  EE-vs-RR parity; replay + CI continuously reporting
  parity state). The decisions exist; the runs that produce
  the data don't yet.
- The 1.2 `energy_limit` migration code change (decision
  frozen, code change deferred).
- Various 5.x sub-bullets that depend on Phase 2 implementation
  (TriggerSmartContract TRC-10 pre-transfer code; sidecar
  delegation/withdraw-expire emission; multi-cycle vote
  parity; Exchange / Market / CancelAllUnfreezeV2 receipt
  parity).
- The Java side of Section 2.4 / 3.4 (gradle build is
  blocked locally by a JDK/Lombok env issue — the test code
  can be written and reviewed, but locally-verified runs
  are pending).
- Section 6 implementation follow-ups (Tools A and B rebuild,
  EeVsRrComparator, classification layer on top of
  `compare_exec_csv.py`, dashboard generator, CI smoke jobs).
- All Phase 2 follow-ups itemized in
  `close_loop.bridge_debt.md`,
  `close_loop.sidecar_parity.md`,
  `close_loop.contract_matrix.md`,
  `close_loop.verification.md`,
  `close_loop.trigger_trc10_pre_transfer.md`,
  `close_loop.java_transaction_id.md`,
  and `close_loop.config_convergence.md`.

The Phase 1 acceptance posture is therefore: **decisions
complete, code change selectively complete, parity runs
not yet observed**. The whitelist target — TransferContract,
CreateSmartContract, UpdateSettingContract — is the smallest
set that the Phase 2 verification rebuild needs to drive to
canonical-ready before Phase 2 can declare its own success.

## What Phase 2 does, in order

These items come straight out of Section 7's "Phase 2: Rust
block importer / block executor readiness" framing in
`close_loop.scope.md`. The order matters and the sequence
should not be reshuffled without re-reading this file.

### Phase 2.A — Verification rebuild

The block importer cannot be safely built until the project
can answer "does this transaction execute identically in EE
and RR?" with a CI-suitable, non-interactive answer. Phase
2.A delivers that.

Concrete items, all from `close_loop.verification.md`
follow-ups:

1. Rebuild `GoldenVectorTestSuite` around explicit
   `EmbeddedExecutionSPI` + `RemoteExecutionSPI` plus a shared
   `EeVsRrComparator` built on top of the existing
   `ExecutionCsvRecordBuilder` + `StateChangeCanonicalizer`
   + `state-digest-jni` (Tool E + F from the audit).
2. Rebuild `HistoricalReplayTool` around two engines, two
   pinned ranges, and the 4-category mismatch classification.
3. Extend `scripts/compare_exec_csv.py` (or add a Java sibling)
   so it produces classification tags rather than row-level
   diffs.
4. Wire the three CI smoke-set jobs (EE / RR / EE-vs-RR diff).
5. Drive the Phase 1 whitelist target to canonical-ready and
   flip its rows in `close_loop.contract_matrix.md`.

### Phase 2.B — Bridge removal sequence start

Once verification can prove that EE and RR produce identical
outputs on the whitelist target, the project can start removing
the transitional bridges from `close_loop.bridge_debt.md` in
the documented order:

1. **B2** (`RuntimeSpiImpl.apply*` family) — first to go,
   because it is purely transitional and only the compute-only
   profile depends on it. Removing B2 forces every RR run to
   be the canonical `rust_persist_enabled = true` profile.
2. **B4** (pre-exec AEXT handshake) — second, after a Rust
   native bandwidth processor exists for every contract type
   on the (eventually-grown) whitelist target.
3. **B1** (`ResourceSyncService`) — third, after the block
   importer takes over Java-side maintenance + reward
   mutations.
4. **B5** (genesis seeding) — fourth, after the importer
   supports "load initial snapshot" from a known starting
   point.
5. **B3** (`postExecMirror`) — last, only after Java stops
   reading from `chainbase` at all (which is outside Phase 2
   scope).

### Phase 2.C — Block importer / block executor

This is the actual Section 9 milestone. With the verification
rebuild and the early bridge removals in place, Phase 2.C
decomposes `Manager.processBlock(...)` into Rust-owned
responsibilities:

- Tx selection / batching / per-block ordering.
- Per-tx execution with the existing `RemoteExecutionSPI`
  plumbing (already substantial).
- Receipt assembly with the iter-6 `tron_transaction_result`
  receipt passthrough.
- Sidecar application via the existing `RuntimeSpiImpl.apply*`
  family until B2 is removed; via the canonical RR mirror
  path after B2 is removed.
- Maintenance / reward mutation handoff (the prerequisite for
  removing B1).

The first Phase 2.C deliverable is a `BLOCK-01` planning note
that captures the importer's responsibilities, its
contract surfaces against the existing Java code, and the
checklist of "what `Manager.processBlock` does today that the
importer must absorb" cross-referenced against the Java source.
That note replaces the placeholder TODO bullet for "Open
`BLOCK-01` planning for Rust block importer / block executor"
in this section.

### Phase 2.D — Trigger TRC-10 pre-transfer code change

Once the buffer attachment for VM trigger execution in
compute-only RR mode is in place (Phase 2.C will likely add
this as a side effect of importer work), the
`close_loop.trigger_trc10_pre_transfer.md` design is finally
implementable. This unblocks `TriggerSmartContract` from
`RR blocked` and grows the canonical-ready whitelist.

### Phase 2.E — energy_limit wire migration

The `close_loop.energy_limit.md` decision (energy units, not
fee-limit SUN) becomes a coordinated single-commit migration:
fixture generators emit energy units, Rust execution stops
dividing by `energy_fee_rate`, Java bridge sets `energyLimit
= 0` for non-VM contracts. Phase 2.E because by then the
verification rebuild from 2.A can catch any regression a
mid-migration partial state would introduce.

### What Phase 2 deliberately does NOT do

These remain in the "deferred" column from
`close_loop.scope.md` and continue NOT to be Phase 2
deliverables:

- Rust P2P handshake / peer manager / sync scheduler /
  inventory pipeline.
- Rust consensus scheduling rewrite.
- Removal of the Java node shell.
- Mixed execution + storage mode optimization.
- Resurrection of `SHADOW` as a primary acceptance path.

If any of these tries to creep into Phase 2 planning, point
the speaker at this section + at
`close_loop.scope.md` and ask them to rewrite their proposal.

## Re-evaluations that Phase 2 must answer

Section 9 calls for two re-evaluations once the importer
lands. Phase 1 cannot answer them yet, but the questions
should not be lost:

1. **Should consensus follow the block importer, or stay on
   Java longer?** The answer depends on how cleanly the
   importer absorbs `Manager.processBlock`. If the importer
   ends up still calling out to Java for maintenance and
   rewards, keeping consensus on Java longer is the safer
   move. If the importer is genuinely standalone, consensus
   migration becomes the next milestone after Phase 2.C.

2. **Should P2P remain Java-owned until after importer
   stability?** Almost certainly yes. The
   `close_loop.scope.md` "why not P2P yet" rationale still
   holds at the start of Phase 2 and is unlikely to flip
   until the importer is observably stable for at least one
   release cycle.

The handoff success condition is that the next roadmap
discussion starts from question (1) above ("Rust
state-transition engine ownership"), not from "networking
looks exciting". Phase 2 planning notes that appear after
this file should explicitly cite this section when arguing
for or against either re-evaluation.

## Phase 1 acceptance for Section 9

Section 9 acceptance is satisfied by the existence of:

- This file documenting the Phase 1 → Phase 2 transition
  plan in detail.
- The `BLOCK-01` planning note placeholder bullet in the
  todo, which Phase 2.C is responsible for replacing with
  an actual planning note.
- The two re-evaluation questions called out so they don't
  get lost.
- The "what Phase 2 does NOT do" anti-pattern guard that
  carries the Phase 1 scope freeze forward.

The Phase 1 todo's Section 9 success condition ("the next
roadmap discussion starts from Rust state-transition engine
ownership, not from networking looks exciting") is locked
in this file's structure: every Phase 2.x sub-section above
is about state transitions; networking is in the explicit
"not in Phase 2" list.
