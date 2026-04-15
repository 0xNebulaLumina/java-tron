# Close Loop — Section 6 Verification, Replay, Release Gates

This file closes Section 6 of `close_loop.todo.md`. It catalogs the
verification surfaces the project has today, decides how each one
should work in the post-`SHADOW` `EE`-vs-`RR` model, and freezes
the smoke-gate / readiness-dashboard contracts.

The acceptance bar for Section 6 is:

> The project can answer "what is safe to enable today?" from tests
> and dashboards, not from memory.

Today the answer comes from "I think this passes" or "look at what
the contract matrix says and trust it". The decisions below give the
project a structural way to answer that question instead.

Companion notes:

- `close_loop.scope.md` — why `SHADOW` is not the validator.
- `close_loop.contract_matrix.md` — the readiness data Section 6.5
  turns into a dashboard.
- `close_loop.sidecar_parity.md` — the per-(sidecar × contract)
  parity gates Section 6.4 must observe.
- `close_loop.bridge_debt.md` — the bridges Section 6.4 has to
  watch for silent drift.

## Existing verification surfaces (audit)

Three tools exist today on the Java side; one Rust crate exists
on the Rust side. None of them currently match the post-`SHADOW`
`EE`-vs-`RR` strategy from `close_loop.scope.md`.

### Tool A. `GoldenVectorTestSuite`

File: `framework/src/test/java/.../GoldenVectorTestSuite.java`
(517 lines).

Shape today:

- Single execution mode per JVM run, controlled by
  `-Dexecution.mode` (defaults to `EMBEDDED`) plus a
  `-Dtest.shadow.enabled=true` flag for the legacy `ShadowExecutionSPI`
  path.
- Uses `ExecutionSpiFactory.createExecution()` to pick ONE engine
  per run. There is no in-process EE-vs-RR diff; the suite is a
  per-engine smoke test, not a comparator.
- Most golden vectors today are validation-shape tests, not
  end-to-end execution tests — the top-level `testAllGoldenVectors`
  is just "framework structure validation" and was clearly built
  around shadow-mode ambitions that never materialized.
- Tear-down still references `ShadowExecutionSPI.cleanup()` and
  prints shadow mismatch stats, which is dead code under the
  Phase 1 scope freeze.

Status: **needs rework** to match the EE-vs-RR strategy. The
existing structure of named vectors per category is fine; the
in-process shadow comparator and the `EXECUTION_MODE` system
property need to be replaced.

### Tool B. `HistoricalReplayTool`

File: `framework/src/test/java/.../HistoricalReplayTool.java`
(402 lines).

Shape today:

- Constructor reads `replay.block_count` / `replay.max_concurrent` /
  `replay.detailed_logging` / `replay.fail_on_mismatch`.
- Holds `private final ExecutionSPI shadowExecutionSPI;` —
  **explicitly built for SHADOW mode**. Created via
  `ExecutionSpiFactory.createExecution()`, which uses whatever
  the configured execution mode is, but the field name and the
  Javadoc are entirely shadow-flavored.
- Concurrent block processing through an `ExecutorService` with
  metric counters (`totalBlocks`, `totalTransactions`,
  `mismatches`, etc.).
- Returns a `ReplayReport` with mismatch reports (`MismatchReport`).

Status: **needs rework**. The shape of "iterate historical blocks,
record per-tx outcomes, summarize mismatches" is the correct
architecture for replay. The shadow-coupling, the fact that the
tool runs ONE engine and doesn't actually compare against another
run, and the `failOnFirstMismatch` semantics are all wrong for
EE-vs-RR.

### Tool C. `tron-backend-storage` Rust unit tests

File: `rust-backend/crates/storage/src/engine.rs` `#[cfg(test)] mod tests`.

Shape today (added in iter 3):

- 23 unit tests covering CRUD, batch writes, transactional commit
  and rollback, snapshot rejection, absent / unknown / cross-DB
  transaction id, concurrent transaction isolation.
- Uses `tempfile::TempDir` per test for hermetic state.
- All tests passing (`cargo test -p tron-backend-storage` reports
  23 / 23).

Status: **already covers Section 6.1 storage verification at the
Rust unit-test layer**. The remaining 6.1 work is on the Java
integration side (see decisions below).

### Tool E. CSV record + canonicalizer infrastructure

Files:

- `framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecord.java`
  — schema + per-transaction record builder.
- `framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java`
- `framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvLogger.java`
  — singleton writer (sample rate, rotation, queue size).
- `framework/src/main/java/org/tron/core/execution/reporting/StateChangeCanonicalizer.java`
  — SHA-256 over a deterministic ordering of `(address, key,
  oldValue, newValue)` tuples.
- `framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java`
  — sibling canonicalizer for domain-level digests (separate
  granularity from `StateChangeCanonicalizer` but same idea).
- `framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournal.java`
  + `StateChangeJournalRegistry.java` + `StateChangeRecorderBridge.java`
  — per-execution journal that the CSV builder consumes.
- Test side (`framework/src/test/java/.../execution/reporting/`):
  `ExecutionCsvRecordTest`, `ExecutionCsvRecordBuilderTest`,
  `StateChangeJournalTest`, `StateChangeJournalRegistryTest`,
  `StorageHookIntegrationTest`, `AccountHookIntegrationTest`,
  `EmbeddedStateChangeIntegrationTest`.

Shape today:

- Per-transaction CSV record carrying the full structured state
  payload: account changes, EVM storage changes, TRC-10 balance
  + issuance changes, vote changes, freeze changes, global
  resource changes, AEXT changes, log entries — covering the
  same surface the `EeVsRrComplete` comparator from Section 6.3
  needs to walk.
- Recorded into a CSV file (sample rate / rotation / queue
  size all configurable) by `ExecutionCsvLogger.getInstance().logRecord(record)`
  called from `Manager.processBlock` in
  `framework/.../core/db/Manager.java:2846` (gated on
  `ExecutionCsvLogger.isEnabled()`).
- Output is intended for **offline** comparison: run a node in
  EE mode, dump its CSV; run another node in RR mode, dump its
  CSV; diff the two files row-by-row outside the JVM.
- A pre-existing programmatic comparator script
  `scripts/compare_exec_csv.py` (104 lines) consumes two CSV
  files (embedded vs remote), strips noise columns
  (`run_id` / `exec_mode` / `storage_mode` / `ts_ms`), and
  reports row-level diffs. A pre-existing collection script
  `collect_remote_results.sh` (294 lines) drives an end-to-end
  embedded-vs-remote run and feeds the resulting CSVs into the
  comparator. There is also a pre-existing planning note
  `planning/csv_comparator.planning.md` that describes the
  Option-1 two-run + offline-comparator approach this whole
  area was built around.
- `StateChangeCanonicalizer.computeStateDigest(...)` produces a
  deterministic SHA-256 digest of state changes, which is the
  same primitive the `state-digest-jni` Rust crate exposes for
  cross-language verification.

Status: **substantially overlooked by an earlier draft of this
audit, but already exists**. This is the closest thing the
project has today to an EE-vs-RR comparator — it just compares
offline (CSV diff) rather than in-process. The Section 6.3
golden-vector rebuild and the Section 6.4 historical replay
rebuild should both look at this infrastructure first before
introducing a new `EeVsRrComparator` from scratch. In particular:

- The CSV schema in `ExecutionCsvRecord` already covers every
  field the new comparator from Section 6.3 #2 needs to walk
  (StorageChange, AccountChange, FreezeLedgerChange,
  GlobalResourceTotalsChange, Trc10Change, VoteChange,
  WithdrawChange, AEXT). Phase 2 should reuse the CSV builder
  rather than re-implementing the field walk.
- `StateChangeCanonicalizer` already knows how to canonicalize
  state changes. Phase 2 should reuse it for the in-process
  diff path instead of writing a parallel canonicalizer.

Open gaps:

- The existing `scripts/compare_exec_csv.py` is a row-by-row
  diff that strips a fixed noise-column set, but it does NOT
  classify mismatches into the 4 categories from Section 6.4
  #5 (`result-code only` / `energy only` / `return-data only` /
  `state-change / sidecar difference`). Phase 2 needs a
  classification layer on top — either inside the Python tool
  or as a Java sibling that consumes the same CSV schema.
- The CSV is per-transaction, not per-block; aggregating to
  per-block-per-contract for the readiness dashboard is
  another layer of work.
- `ExecutionCsvLogger` is a singleton that gets initialized
  from system properties at first use. Plugging it into a
  hermetic test harness requires reset hooks that don't exist
  today — same caveat as any singleton-of-state.
- `collect_remote_results.sh` drives the two-run flow but is a
  shell script, which is fine for nightly long-runs but is not
  CI-suitable in its current shape. Phase 2 work to wire the
  flow into a Gradle / cargo job needs to either reuse the
  shell as-is or port the orchestration to a hermetic test
  fixture.

### Tool F. State digest JNI

Files:

- `state-digest-jni/Cargo.toml` + Rust source.
- `framework/src/test/java/.../execution/spi/StateDigestJniTest.java`.
- The Cargo manifest is explicit: "JNI wrapper for StateDigest
  utility used in shadow execution verification".

Shape today:

- Native (Rust) library compiled as a `cdylib` exposing
  SHA-3 + serde-based state-digest helpers to Java via JNI.
- Used by the test to verify the Rust state-digest produces
  the same hash as Java's `StateChangeCanonicalizer` for the
  same canonical input.

Status: **already-existing cross-language digest verification**.
Its Cargo description still says "shadow execution verification"
which dates it to before the Phase 1 scope freeze, but the
underlying primitive (SHA-3 over a canonical byte stream) is
mode-agnostic and is exactly what an `EE-vs-RR` digest comparison
needs. Reuse rather than rebuild.

Open gaps:

- The JNI bridge has the usual native-library headaches
  (separate build, separate platform packaging, can't be
  exercised in a pure-Java unit test). For Phase 2 CI smoke
  gates this is a tradeoff: hermetic comparators win, native
  comparators are only worth it where Java cannot reproduce
  the Rust calculation without the JNI.

### Tool D. Rust execution gRPC handler tests

File: `rust-backend/crates/core/src/service/grpc/mod.rs`
`#[cfg(test)] mod iter4_read_path_tests`.

Shape today (added in iter 4):

- 23 helper + handler-level tests covering read-path semantics
  (`get_code` / `get_storage_at` / `get_nonce` / `get_balance`),
  snapshot rejection, address normalization, storage key
  padding, U256 BE round-trip.
- Plus 4 iter-6 tests in `crates/core/src/service/grpc/conversion.rs`
  for `CallContractRequest.transaction` preference and
  `CallContractResponse.Status` wire codes.

Status: **already covers a meaningful slice of Section 6.2
read-path lane** at the Rust level. Java-side coverage for the
same APIs is open work tracked under Section 2.4.

## Section 6.1 — Storage verification

Decisions:

1. **Rust storage crate has real tests**, satisfied by the iter 3
   test module (23 tests, all green). This is the "no longer
   `0 tests`" exit criterion from Section 0.

2. **At least one Java integration path that validates remote
   storage semantics, not only factory creation** — the iter 2
   `StorageSPIIntegrationTest.testSnapshotOperationsAreUnsupported`
   counts as the first such path: it asserts the actual
   request/response semantics (snapshot calls fail-hard end to
   end, including across the gRPC layer to the Rust engine and
   back), not just "did `StorageSpiFactory.createStorage()`
   return non-null". Two more Java integration paths are needed:

   - A test that proves Java `RemoteStorageSPI.put(transaction_id,
     key, value)` actually buffers in the Rust engine and applies
     atomically on `commit_transaction`. (Tracked under Section
     3.4 Java-focused as an open follow-up.)
   - A test that proves the gRPC `transaction_id = ""` direct
     path round-trips correctly. (Tracked under Section 3.4
     Java-focused as an open follow-up.)

3. **Track storage regressions separately from execution
   regressions**: store + execution Rust crate tests live in
   separate cargo packages (`tron-backend-storage` and
   `tron-backend-core`). CI gates can therefore run them as
   separate jobs and surface a separate red/green status per
   crate. The CI smoke gates (Section 6.6 below) explicitly do
   so. Java-side is harder because gradle runs everything in
   one job, but the test class names already follow a
   `*Storage*Test` / `*Execution*Test` convention that grep can
   split — this is a documentation-and-naming convention, not a
   structural change.

## Section 6.2 — Execution lane split

Decisions:

1. **Two lanes are required**:
   - **Write-path lane**: an EE run and an RR run apply the same
     transaction (or block) and have their post-state diffed.
     This is what canonical-ready means in
     `close_loop.contract_matrix.md` — every contract on the
     whitelist target needs to clear this.
   - **Read-path lane**: a series of `getCode` / `getStorageAt` /
     `getNonce` / `getBalance` / `callContract` / `estimateEnergy`
     calls run against a known state and have their responses
     diffed. This lane never mutates state; it only reads.

2. **Strong write-path results MUST NOT imply read-path closure.**
   A contract whose write-path tests pass is not automatically
   `RR canonical-ready`; the read-path lane's status is a
   separate column on the readiness dashboard (Section 6.5) and
   it can be red even when the write-path lane is green. The
   contract matrix already separates these via the "Read-path
   closure" attribute column; the lane split formalizes that
   in the dashboard.

3. **Publish separate pass/fail state for both lanes.** The
   CI smoke gate (Section 6.6) and the readiness dashboard
   (Section 6.5) both report `(write_pass, read_pass)` per
   contract / per smoke set, never a single collapsed boolean.

4. **Sidecar parity is part of the write-path lane**, not a
   third lane. A write-path comparison that ignores sidecars
   would let us silently regress freeze ledger / TRC-10 / vote
   updates. The diff comparator in Section 6.4 below treats
   sidecars as part of the post-state.

## Section 6.3 — Golden vectors

Decisions:

1. **The existing `GoldenVectorTestSuite` is rebuilt around
   the EE-vs-RR comparator pattern.** No longer a single-engine
   run with optional shadow. Each `@Test` runs the same vector
   twice — once via an explicitly-constructed `EmbeddedExecutionSPI`,
   once via an explicitly-constructed `RemoteExecutionSPI` —
   and asserts the two outputs are equal under a structured
   diff. The `ExecutionSpiFactory.createExecution()` indirection
   is dropped from the test path because Phase 1 wants the test
   to control which engine runs, not the JVM-level system
   property.

2. **The diff comparator is shared, not per-test, and is built
   on top of the existing CSV + canonicalizer infrastructure
   (Tool E above), not from scratch.** A new helper
   `EeVsRrComparator.compareExecutionResult(eeResult, rrResult)`
   walks the relevant fields and returns a structured
   `ComparatorReport { kind, field, eeValue, rrValue }` for any
   mismatch. The field walk SHOULD reuse
   `ExecutionCsvRecordBuilder` (which already knows how to
   extract every relevant field from an execution result) and
   `StateChangeCanonicalizer` (which already knows how to
   produce a deterministic digest). Phase 2 implementation
   should add the diff layer on top, not re-implement the field
   extraction. Fields covered:

   - `success` / status / result code
   - `return_data`
   - `energy_used` (with the per-vector exact / tolerated
     decision, see #4 below)
   - `bandwidth_used`
   - `state_changes` (sorted by `(address, key)` or
     `(address, balance)` depending on the variant)
   - `freeze_changes` / `global_resource_changes` /
     `trc10_changes` / `vote_changes` / `withdraw_changes`
     (sidecars, per the parity audit in
     `close_loop.sidecar_parity.md`)
   - `tron_transaction_result` receipt bytes
   - `contract_address`

3. **Vectors for known mismatch-prone branches**: each of the
   four families called out in the close_loop todo gets at
   least one targeted vector (or one for each sub-shape):

   - **Trigger smart contract** — happy-path constant call;
     payable trigger (gated until 5.1 lands); reverting
     trigger; `tokenValue == 0` trigger. The `tokenValue > 0`
     case STAYS deliberately uncovered until Section 5.1 is
     implemented in Phase 2 — running it under the existing
     reject path would just produce identical errors, which
     proves nothing.
   - **Create smart contract** — happy path (already passing
     in the iter 0 baseline); CREATE2 (out of scope until
     proven needed); `consume_user_resource_percent` boundary
     cases.
   - **Update setting / metadata** — happy path (already
     passing in the iter 0 baseline); permission denied;
     non-existent contract.
   - **Resource / freeze paths** — FreezeBalance V1, V2;
     UnfreezeBalance V1, V2. Covers the freeze-path slice of
     S2 / S3 in the sidecar audit. Delegation / withdraw-expire
     / cancel-all are intentionally NOT covered until their
     S2 / S3 sidecar gaps close.

4. **Energy comparison rule**: by default, golden vectors
   require **exact** `energy_used` equality. Vectors that
   knowingly tolerate a delta (e.g. when Java and Rust have
   slightly different per-opcode accounting) must declare a
   per-vector tolerance and a written justification — the
   default is exact, deltas are opt-in. There is no global
   tolerance.

   This decision aligns with the
   `close_loop.energy_limit.md` lock — until the producer-side
   `energy_limit` migration lands, fixtures can be re-generated
   under the corrected wire contract, but the comparator should
   still demand bit-for-bit equality between EE and RR runs of
   the same fixture.

5. **Vectors for remote read/query APIs** are appropriate **only
   for the read-path lane** (Section 6.2). They live in a
   separate test class (e.g.
   `GoldenReadPathVectorTestSuite`) with its own
   `EeVsRrComparator` invocation, so a write-path failure
   cannot pollute the read-path lane and vice versa.

6. **Migration path** for the existing class:

   - Strip the `ENABLE_SHADOW_EXECUTION` field and the
     `tearDown` that prints shadow stats.
   - Replace the `private ExecutionSPI executionSPI;` field
     with `private ExecutionSPI embeddedExecutionSPI;` plus
     `private ExecutionSPI remoteExecutionSPI;`, both
     constructed via the explicit-mode factory call
     `ExecutionSpiFactory.createExecution(ExecutionMode.EMBEDDED)`
     etc.
   - Replace each `@Test` body that today runs against
     `executionSPI` with a call to a shared helper that runs
     both engines and feeds the results to `EeVsRrComparator`.
   - The validation-only tests (`testAllGoldenVectors`,
     `testBasicTransferVectors`, etc. that just check
     vector-structure) can stay as cheap smoke tests in their
     current shape; they do not need EE-vs-RR comparison.

## Section 6.4 — Historical replay

Decisions:

1. **`HistoricalReplayTool` is rebuilt around two engines.**
   The `private final ExecutionSPI shadowExecutionSPI;` field is
   replaced with `private final ExecutionSPI embeddedExecutionSPI;`
   and `private final ExecutionSPI remoteExecutionSPI;`, each
   constructed via the explicit-mode factory. The replay loop
   runs each historical transaction through both engines (in
   the same order, against the same starting state for each
   engine) and feeds the two outputs into the same
   `EeVsRrComparator` from Section 6.3. The shadow-flavored
   Javadoc is rewritten.

2. **Two replay ranges** are defined:

   - **Routine range** — a small fixed window (default 1000
     blocks starting from a known stable height) that runs in
     a few minutes on a developer laptop and is intended as
     CI smoke (Section 6.6). The exact starting height is
     pinned per release; if a contract's behavior changes, the
     pin moves forward and the test is re-baselined
     deliberately.
   - **Milestone range** — a larger window (default 50 000
     blocks or more) that runs on demand for milestone
     validation, not in CI. Run results are persisted as
     `ReplayReport` artifacts and compared across milestones
     to detect regression drift.

3. **Run replay once in `EE`, once in `RR`, then compare** is
   the strict ordering. A single tool invocation runs both
   engines under the same input. Running them as separate JVM
   invocations would be acceptable for one-off debugging but
   is NOT the canonical mode — the diff is the whole point.
   The compare step should reuse the CSV + canonicalizer
   infrastructure from Tool E above, plus the `state-digest-jni`
   helpers from Tool F where the cross-language byte-level
   digest is the right granularity. Do not introduce a third
   parallel canonicalizer.

4. **Compare outputs by contract type AND by lane.** The
   `ReplayReport` already has per-contract-type counters; the
   rebuild adds a per-lane (`write` / `read`) split so the
   readiness dashboard can flip a contract's
   `EE-vs-RR replay status` cell from `tbd` to
   `passing` / `regressing` per lane independently.

5. **Mismatch classification** — every recorded mismatch must
   be tagged with one of:

   - `result-code only` — Java reports SUCCESS, Rust reports
     REVERT, or vice versa. No state delta. (Often a sign of
     a missing validation gate on one side.)
   - `energy only` — same status, same state, but
     `energy_used` differs.
   - `return-data only` — same status, same state, same
     energy, but the bytes differ (often a string-encoding
     issue or an off-by-one in receipt assembly).
   - `state-change / sidecar difference` — the post-state
     diverges. This is the most serious kind; any
     state-change mismatch blocks the affected contract from
     `RR canonical-ready` until resolved.

   The classification is recorded on the `MismatchReport`
   struct and surfaced in `ReplayReport`.

6. **`failOnFirstMismatch` semantics flip.** The current
   default is `false` (collect all mismatches). The new
   default for the routine smoke range is `true` — a CI run
   that hits even one mismatch is red. The milestone range
   keeps `false` so a single regression doesn't mask the
   broader picture.

## Section 6.5 — Contract readiness dashboard

Decisions:

1. **The dashboard is a generated view of
   `close_loop.contract_matrix.md`**, not a separate hand-edited
   table. Phase 1 keeps the matrix as the authoritative source
   of truth (it's already structured per contract with
   per-attribute columns). Phase 2 builds the actual generator
   that turns the markdown table into a CI-published HTML view.
   For Phase 1, "the matrix IS the dashboard" — engineers read
   the markdown.

2. **Per-contract columns the dashboard MUST surface**:

   - `RR support status` — one of `EE only`, `RR blocked`,
     `RR candidate`, `RR canonical-ready`. (Already in the
     matrix.)
   - `fixture coverage` — one of `yes` / `no` / `tbd`. (Already
     in the matrix as a column attribute.)
   - `Rust unit coverage` — same. (Already in the matrix.)
   - `EE-vs-RR replay status (write lane)` — one of
     `passing` / `regressing` / `tbd` / `n/a`. (NEW column.
     The matrix needs to grow this column when the comparator
     in Section 6.4 starts producing data.)
   - `EE-vs-RR replay status (read lane)` — same shape. (NEW
     column.)
   - `major known gaps` — free text. The contract matrix
     already has per-row notes for this.
   - `sidecar gates` — links to the relevant rows in
     `close_loop.sidecar_parity.md` for any contract whose
     sidecar parity is open.

3. **The matrix is the only source of truth for enabling
   canonical RR support.** No contract may be moved from
   `RR candidate` to `RR canonical-ready` without:

   - Both replay-status cells being `passing` (not `tbd`).
   - All sidecar gate cells from
     `close_loop.sidecar_parity.md` being `passing` or `n/a`
     (not `tbd` or `missing` or `partial`).
   - Fixture and Rust unit coverage being `yes`.

   This is the structural enforcement of the anti-pattern
   guards in `close_loop.sidecar_parity.md` and
   `close_loop.contract_matrix.md`.

4. **Phase 1 deliverable** is the matrix in its current shape,
   PLUS the two new columns above (`replay write lane` /
   `replay read lane`) as a tracked open item against the
   matrix file. The actual HTML generator is Phase 2.

## Section 6.6 — CI smoke gates

Decisions:

1. **Three minimal smoke sets**, each runnable as an independent
   CI job:

   - **`EE` smoke set** — runs the existing baseline tests:
     `cargo test -p tron-backend-storage` (23 tests),
     `cargo test -p tron-backend-core create_smart_contract`
     (17 tests), `cargo test -p tron-backend-core update_setting`
     (17 tests), plus a Java-side `EmbeddedExecutionSPI`
     happy-path against the Phase 1 whitelist target
     (TransferContract / CreateSmartContract /
     UpdateSettingContract). Total target: under 5 minutes
     wall clock. Goal: catch any regression to the EE
     baseline.

   - **`RR` smoke set** — same workload but routed through
     `RemoteExecutionSPI` against a locally-launched
     `tron-backend` Rust process. Reuses the existing
     `iter4_read_path_tests` for the read-path slice and adds
     the three Phase 1 whitelist contracts for the write-path
     slice. Total target: under 10 minutes wall clock. Goal:
     catch any regression to the RR baseline.

   - **`EE-vs-RR diff` smoke set** — runs the routine
     historical replay range from Section 6.4 (1000 blocks)
     with `failOnFirstMismatch=true`. Runs both engines, runs
     the comparator, fails the CI job on any mismatch. Total
     target: under 15 minutes wall clock. Goal: catch any
     drift between EE and RR before it lands in main.

2. **The three smoke sets must run as separate CI jobs**, not
   as one combined job. A red `EE` smoke is "we broke the
   baseline"; a red `RR` smoke is "we broke remote execution";
   a red diff smoke is "EE and RR diverged". These three
   failure modes have very different triage paths, and
   collapsing them into one signal makes triage harder.

3. **CI mismatch output must be readable and triageable.** The
   diff smoke job's output, on failure, must include:

   - The `mismatch_classification` from Section 6.4 (1 of the
     4 categories).
   - The contract type that produced the mismatch.
   - The smallest-possible summary of what differs (e.g.
     "freeze_changes[0].amount: EE=1000000, RR=999999"), not
     a full state dump.
   - A pointer back to the `ReplayReport` artifact for full
     detail.

   No "the test failed, see logs" outputs. The CI failure
   message is the triage starting point.

4. **Smoke gates do NOT replace the Section 6.4 milestone
   replay.** Smoke runs the routine range only. Milestone is
   on-demand and human-driven.

## Phase 1 acceptance check

The acceptance bar for Section 6:

> The project can answer "what is safe to enable today?" from
> tests and dashboards, not from memory.

is satisfied by the **structural decisions above** (which freeze
how the verification machinery should be shaped post-`SHADOW`),
even though most of the implementation is Phase 2 work. The
`close_loop.contract_matrix.md` matrix in its current shape is
the Phase 1 dashboard; the existing iter 3 storage tests and
iter 4 read-path tests cover the Rust side of the lane split;
the rest is implementation follow-up.

Specifically:

- The Phase 1 whitelist target (TransferContract /
  CreateSmartContract / UpdateSettingContract) is small enough
  that "what is safe to enable today" is answerable from the
  three baseline `cargo test` runs that already exist. No
  contract outside the whitelist target is on the safe-to-enable
  list.
- The contract matrix, sidecar audit, and bridge debt audit all
  have explicit anti-pattern guards that prevent silent
  flips. So even without the dashboard tooling, the
  question is answerable by reading those three planning notes.
- The smoke-gate decisions above lock the format the eventual
  dashboard will use, so Phase 2 implementation has a target
  to hit instead of starting from scratch.

## Follow-up implementation items

These are NOT closed in Phase 1; they are listed to make the
debt visible.

- [ ] Rebuild `GoldenVectorTestSuite` around explicit
      `EmbeddedExecutionSPI` + `RemoteExecutionSPI` instances
      with a shared `EeVsRrComparator`. Strip the
      `ENABLE_SHADOW_EXECUTION` field and the shadow tear-down.
- [ ] Add an `EeVsRrComparator` helper class to
      `framework/src/test/java/.../execution/spi/` that walks
      the fields listed in Section 6.3 #2 and produces
      structured `ComparatorReport` mismatches. Built on top of
      `ExecutionCsvRecordBuilder` + `StateChangeCanonicalizer`
      from `framework/.../execution/reporting/` (Tool E) — do
      NOT re-implement the field walk.
- [x] Extend `scripts/compare_exec_csv.py` (or add a Java
      sibling that consumes the same CSV schema) so it
      classifies mismatches into the 4-category set from
      Section 6.4 #5 (`result-code only` / `energy only` /
      `return-data only` / `state-change / sidecar difference`)
      rather than just returning row-level diffs. The existing
      `collect_remote_results.sh` orchestration can stay as the
      driver for the two-run flow, but its output then needs to
      flow into the classifier instead of straight to a human
      diff viewer.
      (iter 12: `scripts/compare_exec_csv.py` rewritten to
      classify every mismatch using the `_classify_row` helper
      against the frozen 4-category set from Section 6.4 #5
      (`state-change / sidecar difference`, `result-code only`,
      `energy only`, `return-data only`). Column-family groups:
      `STATE_DIGEST_COLUMNS`, `RESULT_CODE_COLUMNS`,
      `ENERGY_FAMILY_COLUMNS` (= `ENERGY_COLUMNS` ∪
      `{bandwidth_used}` — the spec only freezes `energy only`
      so bandwidth is folded in rather than published as a new
      tag), `RETURN_DATA_COLUMNS`. Unknown columns default to
      the most-serious `state-change / sidecar difference`
      bucket so a newly-added column cannot silently mask a
      regression. `CATEGORY_SEVERITY` orders rows that trip
      multiple families to the most-serious one; only the
      "state-change is most serious" precedence is locked by
      tests, the rest is internal display policy. Three output
      modes: default (first-mismatch + stderr classification
      tag, exit 0 — intentionally matches the legacy semantic
      because `collect_remote_results.sh` runs with `set -e`
      and would abort at step 11 on a non-zero exit),
      `--classify-all` (per-category summary + per-tx details,
      exit 1 on mismatch), `--json` (machine-readable report
      with `header_mismatch` envelope for downstream dashboard
      generators, exit 1 on mismatch). `*_json` columns are
      excluded from classification so JSON-whitespace drift
      cannot trip a false state-change mismatch. Added
      `scripts/test_compare_exec_csv.py` with 37 unittest
      cases covering: identity, each family, the
      state-change-most-serious precedence, bandwidth-used
      folding into the energy family, unknown-column fallback
      to state-change, ignored-column handling, end-to-end
      `walk_mismatches` pipeline, and CLI surface
      (`_parse_args` + `main`) — including (a) the
      regression-locking test that default mode exits 0 on
      mismatch so the wrapper-vs-CI exit-code split cannot
      regress and (b) empty/headerless CSV regression tests
      (zero-byte, blank-line `"\n"` which `csv.reader` parses
      as `[[]]`, and one-side-empty) that lock the script
      into rejecting empty inputs with exit 2 instead of
      silently passing as "no mismatches" (a broken
      artifact-collection step would otherwise produce a
      false negative). All 37 tests pass.)
- [ ] Aggregate per-transaction CSV records up to
      per-block-per-contract counters so the readiness dashboard
      (Section 6.5) can populate the new `replay write lane` /
      `replay read lane` columns automatically. Without this
      aggregation the dashboard cells stay manually-edited.
- [ ] Provide a hermetic reset hook for `ExecutionCsvLogger`
      (currently a singleton initialized from system properties
      at first use). Phase 2 in-process tests need to be able
      to spin up the logger against a tempdir and tear it down
      cleanly; the singleton blocks that.
- [ ] Rebuild `HistoricalReplayTool` around two engines (drop
      the `shadowExecutionSPI` field naming; introduce
      `embeddedExecutionSPI` + `remoteExecutionSPI`). Update
      the Javadoc.
- [ ] Implement the per-mismatch classification on
      `MismatchReport` (`result-code only` / `energy only` /
      `return-data only` / `state-change / sidecar difference`).
- [ ] Pin a routine replay range (1000 blocks from a stable
      mainnet height) and a milestone replay range (50 000+
      blocks), and check the pinned heights into a config
      file alongside the test code.
- [ ] Add the two new columns
      (`replay write lane` / `replay read lane`) to
      `close_loop.contract_matrix.md` and start populating
      them as the comparator produces data.
- [ ] Wire the three smoke sets (`EE`, `RR`, `EE-vs-RR diff`)
      into CI as three separate jobs with the wall-clock
      budgets above. Phase 1 ends with the decisions frozen;
      Phase 2 lands the YAML.
- [ ] Build the actual HTML / markdown generator that turns
      `close_loop.contract_matrix.md` into a CI-published
      readiness dashboard. Until then, "the matrix IS the
      dashboard" — engineers read the markdown.
- [ ] Add Java integration coverage for the gRPC
      `transaction_id = ""` direct path and the
      `transaction_id = <opaque>` buffered path. Tracked
      separately under Section 3.4 Java-focused as well.
- [ ] Once the comparator + replay rebuild lands, run a
      milestone replay against the Phase 1 whitelist target
      and flip the three contracts from `RR candidate` to
      `RR canonical-ready` in `close_loop.contract_matrix.md`
      if the diff is clean.

## Anti-pattern guards

**Do not introduce any new "shadow" path.** The whole point of
the post-`SHADOW` Section 6 design is that comparison happens
between two clean processes / two cleanly-instantiated SPI
objects, not via in-process global state. Any new tool that
reaches for `ShadowExecutionSPI` or that tries to fold both
engines into one execution context is reverting Phase 1 work.

**Do not collapse the two lanes into a single boolean.** The
write-path and read-path lanes have different failure modes,
different blast radii, and different remediation paths. Reporting
them as a single "RR works" / "RR broken" boolean defeats the
Section 6.2 decision and lets read-path regressions hide behind
write-path successes.

**Do not move a contract from `RR candidate` to
`RR canonical-ready` based on smoke-gate green alone.** The
contract matrix gate (Section 6.5 #3) requires BOTH replay
lanes green AND all sidecar parity rows for that contract green.
A green smoke run is necessary but not sufficient — smoke runs
on the routine range, not on the contract-specific edge cases
the sidecar audit identifies.

**Do not flip the readiness dashboard cells by hand without a
corresponding test result.** If the dashboard says `tbd`, the
fix is to run the test and observe a result, not to type
`passing` into the markdown. The dashboard's value is that it
mirrors observed reality; manual edits break that contract.
