# Close Loop — Section 4 Bridge Debt Audit

This file closes Section 4 of `close_loop.todo.md`. It enumerates every
place where Java-side state and Rust-side state are "bridged" — meaning
one side pushes data to the other, or mirrors it, because the two
stores are not yet a single source of truth. Each bridge is classified
and a removal-or-retention plan is written down so the debt does not
stay invisible.

Companion notes:

- `close_loop.write_ownership.md` — canonical writer per mode.
- `close_loop.storage_transactions.md` — transaction contract.
- `close_loop.snapshot.md` — snapshot contract.
- `close_loop.scope.md` — Phase 1 scope freeze.

## Bridge inventory

### B1. `ResourceSyncService` — pre/post-sync of Java state to remote storage

Primary touchpoints:

- `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java`
  (~690 lines; `flushResourceDeltas` is the entry point)
- `framework/src/main/java/org/tron/core/storage/sync/ResourceSyncContext.java`
- `framework/src/main/java/org/tron/core/db/Manager.java:2091` (`syncPostBlockRewardDeltas`)

What it does: when the node is in `RR` mode and the sync feature flag is
on (`remote.resource.sync.enabled=true`, default `true` for remote
storage), Java scans its local chainbase for dirty accounts, dirty
dynamic properties, dirty asset-issue rows, and dirty delegation rows,
then pushes them into the Rust storage backend via `StorageSPI.put` /
`batchWrite`. It has a circuit breaker that trips after
`FAILURE_THRESHOLD` consecutive gRPC errors.

Why it exists: the Java node still computes several state mutations
on the Java side — maintenance cycles, reward distributions, some
bandwidth accounting — and the Rust backend needs to see those
mutations before the next `executeTransaction` gRPC call or the
remote execution path reads stale data.

Classification: **required in Phase 1**.

- Without `ResourceSyncService`, `RR` runs cannot reach parity with
  `EE` because Rust would read pre-update Java state.
- The sync is a symptom of split ownership: Java still owns some
  state-mutation paths (maintenance, rewards) that Rust has not
  absorbed. Until the block importer (Phase 2) takes over those
  mutations, the push is mandatory.

Post-Phase-1 plan: the bridge becomes **removable once the block
importer takes over maintenance + reward mutations**. It must **not**
survive into the final Rust-owned state-transition engine — if it
does, we still have dual-writer semantics, and the whole point of
the close_loop phase is to end up with exactly one canonical writer.

Open risks (tracked here, not fixed in Phase 1):

- Circuit breaker hides sync failures as "probe retries", which can
  mask a real divergence. A Phase 2 task should upgrade this into a
  hard failure mode when we are running under the canonical RR
  profile.
- The list of dirty key sources (account / dynamic / asset-issue /
  delegation) is hand-maintained. Anything Java starts mutating that
  is not in this list silently drifts from Rust. Every new Java-side
  state mutation must add itself to the dirty-key collection, or the
  bridge will hide the bug.

### B2. `RuntimeSpiImpl` `apply*` family — Java applies Rust state changes

Primary touchpoints:

- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:177`
  `applyStateChangesToLocalDatabase`
- `...:211` `applyFreezeLedgerChanges` (+ `applyGlobalResourceChange` at `:432`)
- `...:470` `applyTrc10Changes`
- `...:519` `applyVoteChanges`
- `...:609` `applyWithdrawChanges`

What it does: when `executionResult.getWriteMode() == COMPUTE_ONLY`
(i.e. Rust computed the state change but did not persist it), Java
reads every `StateChange`, `FreezeLedgerChange`, `TRC10Change`,
`VoteChange`, and `WithdrawChange` off the response and writes the
matching rows into Java's local stores (`AccountStore`,
`DynamicPropertiesStore`, `AssetIssueStore`, etc.).

Why it exists: it is the only way to make Java's local stores visible
to subsequent reads on the Java side when Rust is not the authoritative
writer. It is also the path that still matches the "Rust is a remote
accelerator, Java is canonical" mental model from earlier phases.

Classification: **transitional**.

- In the **`RR` canonical profile** (`rust_persist_enabled=true`), the
  whole family is **off** — Java sees `PERSISTED`, skips every apply
  call, and relies on `postExecMirror` (B3) to keep its local stores
  coherent.
- In the **`RR` compute-only profile** (`rust_persist_enabled=false`),
  the family is **still the writer**, because Rust did not persist.
  That is the development / diagnostic mode and is not the Phase 1
  acceptance path (see `close_loop.write_ownership.md`).

Post-Phase-1 plan: remove the `apply*` family **once the canonical
`RR` profile is the only supported mode**. As long as we still want
the compute-only profile for debugging individual contracts, the
family has to stay — we just stop treating it as the canonical
writer in `RR`.

Do not add new `apply*` variants. Any new state channel from Rust to
Java should travel through the mirror (B3), not through a fresh apply.

### B3. `postExecMirror` — Java refreshes from Rust touched keys

Primary touchpoint:

- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:1615`

What it does: when `writeMode = PERSISTED`, Java reads the
`touched_keys` list from the execution response and refreshes each
corresponding row in its local revoking head by reading the latest
value from the Rust backend. This keeps Java-side reads (JSON-RPC,
consensus apply glue, CSV/reporting) consistent with Rust's
authoritative state.

Why it exists: the canonical `RR` writer is Rust, but Java still has
subsystems that read from chainbase. Without `postExecMirror`, those
subsystems would see stale values after every Rust-owned commit.

Classification: **required in Phase 1, must survive into block
importer phase (possibly renamed)**.

- The mirror replaces the old "Java is canonical, Rust is an
  accelerator" pattern with "Rust is canonical, Java is a read-side
  cache". Removing it is not the goal of Phase 1.
- It is expected to continue serving Phase 2 (block importer) until
  either (a) Java stops reading from chainbase entirely, or (b) the
  read path is routed through the Rust backend directly via the
  `StorageSPI` gRPC surface.

Open risk: the mirror refreshes row by row. Any touched-key the Rust
side forgets to emit silently leaves Java's mirror stale. Section 6
verification (CI smoke gates) is where we catch this, but it is a
latent bug class.

### B4. Pre-execution AEXT snapshot — Java pushes resource usage fields

Primary touchpoint:

- `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
  (`collectPreExecutionAext` at line ~1700 + `preExecAextList` in the
  `ExecuteTransactionRequest` at line ~1154)

What it does: before executing **every** remote transaction (not only
resource-mutating contracts), Java collects the current AEXT (Account
EXTension) resource-usage fields — bandwidth window, energy window,
usage counters, timestamps — for the owner address and, for
`TransferContract` / `TransferAssetContract` /
`ParticipateAssetIssueContract`, also for the recipient address.
Those snapshots are attached to
`ExecuteTransactionRequest.preExecutionAext`. Rust uses these values
in `"hybrid"` mode to echo them back into the `AccountInfo` fields
that otherwise would not round-trip, giving CSV parity on unchanged
fields.

Why it exists: Java's `BandwidthProcessor` mutates these fields around
`RuntimeSpiImpl.execute` (sometimes before, sometimes after), and
Rust does not yet have a fully faithful bandwidth model for every
contract type. The hybrid handshake avoids a full reimplementation
of `BandwidthProcessor` on the Rust side.

Scope note: an earlier draft of this file said B4 was "only needed by
the freeze/unfreeze/delegate/withdraw families". That was wrong —
`collectPreExecutionAext` fires on every remote contract execution
by default (gated by `-Dremote.exec.preexec.aext.enabled=true`, which
defaults to `true`), and the recipient-side snapshot explicitly
supports transfer-style contracts. The freeze/unfreeze families are
the biggest *consumers* of the round-tripped values, but the
collection surface is system-wide, so the dependency is broader than
one contract family.

Classification: **required in Phase 1, removable only once Rust has
a native `BandwidthProcessor` covering every `RR`-eligible contract
type**, not just the freeze family. This is an explicit "we did not
port BandwidthProcessor" debt, and the replacement is a Rust-native
bandwidth implementation. Not a Phase 1 deliverable.

Gating flag: `-Dremote.exec.preexec.aext.enabled=true` on the Java
JVM. Turning the flag off disables the collection, but then the
Rust side sees missing AEXT for EOAs and CSV parity breaks for any
contract whose AEXT fields matter. Do not turn it off in a parity
run — see the guidance in `close_loop.contract_matrix.md` for which
contracts are affected.

### B5. Genesis account seeding from `config.toml`

Primary touchpoint:

- `rust-backend/config.toml` `[genesis]` section + `[[genesis.accounts]]` rows
- Rust startup code in `rust-backend/src/main.rs` (see iter 0.x lessons)

What it does: at startup, the Rust backend writes a fixed set of
Account protobuf rows into its own RocksDB. The current checked-in
config seeds the blackhole address with a specific negative-int64
balance that matches the Java embedded state at a specific test
block.

Why it exists: the Rust storage RocksDB starts empty on every run,
but the Java embedded test already has blackhole fee accumulation
from historical blocks. Without seeding, EE-vs-RR comparisons at
any block > 0 would diverge immediately on the blackhole row.

Classification: **required in Phase 1, must survive into block
importer phase**.

- Any EE-vs-RR run that starts from a block > 0 needs some form of
  state-seeding on the Rust side. The block importer eventually
  absorbs this via "load initial snapshot", but that is a Phase 2
  capability.
- The current hand-maintained list is fragile — it only seeds the
  blackhole address. If a future test starts from a block where any
  other system account has an unusual balance, seeding has to be
  updated by hand. Track as a Phase 2 item.

Anti-pattern guard: do not add new genesis rows to paper over a
Phase 1 EE-vs-RR mismatch. If EE and RR diverge on a non-blackhole
row, the bug is almost certainly in execution, not in seeding.

### Summary table

| Bridge | Location | Phase 1 | Post-Phase-1 |
| ------ | -------- | ------- | ------------- |
| B1 `ResourceSyncService` | `framework/.../storage/sync/` | required | removable once block importer takes over maintenance + rewards |
| B2 `RuntimeSpiImpl.apply*` family | `RuntimeSpiImpl.java` | transitional (off in canonical RR) | removable once compute-only profile is retired |
| B3 `postExecMirror` | `RuntimeSpiImpl.java:1615` | required | must survive into importer phase until Java stops reading chainbase |
| B4 Pre-exec AEXT snapshot | `RemoteExecutionSPI.java` | required | removable once Rust has a native BandwidthProcessor |
| B5 Genesis account seeding | `rust-backend/config.toml` + `main.rs` | required | must survive until importer supports "load initial snapshot" |

## Bridge removal sequence (post-Phase-1)

This is the order in which we expect each bridge to disappear during
Phase 2+ work. It is not a schedule — it is a dependency graph.

1. **B2 (`apply*` family)** can be deleted immediately after we retire
   the compute-only profile (`rust_persist_enabled=false` as a
   sanctioned mode). Nothing downstream blocks on it.
2. **B4 (pre-exec AEXT)** can be deleted once Rust has a native
   `BandwidthProcessor` equivalent for every contract type tagged
   `RR candidate` in `close_loop.contract_matrix.md`. This is the
   gate that turns a `RR candidate` contract into `RR canonical-ready`.
3. **B1 (`ResourceSyncService`)** can be deleted once the Rust block
   importer owns maintenance and reward mutations. Until then, even
   a perfectly executed transaction stream plus a perfect mirror is
   insufficient — between-block Java mutations still need to reach
   Rust.
4. **B5 (genesis seeding)** can be deleted once the block importer
   can load an initial snapshot into Rust storage from a known
   starting point. This likely lands alongside B1 as part of a
   Phase 2 "import from height" capability.
5. **B3 (`postExecMirror`)** is the last to go. It can only be
   deleted after Java no longer reads from chainbase at all — that
   is, after the JSON-RPC / solidity node / consensus glue is routed
   through `StorageSPI` against the Rust backend. This is outside the
   Phase 1 scope and very likely outside Phase 2's scope too.

**Never delete these in any other order.** In particular:

- Removing B3 before B1 means Java state drifts silently during
  maintenance.
- Removing B1 before the importer means `RR` has no way to ingest
  Java-side maintenance mutations at all.
- Removing B4 before B2 is premature. `collectPreExecutionAext` fires
  on every remote transaction regardless of `rust_persist_enabled`,
  so it is just as active under the compute-only profile as under
  the canonical `RR` profile. As long as B2 (the compute-only apply
  family) is still sanctioned, the compute-only profile is still a
  valid way to run individual contracts — and each of those runs
  still needs the AEXT snapshot to keep CSV parity. Wait to retire
  B4 until the Rust native bandwidth model covers every
  `RR candidate` contract type AND the compute-only profile is no
  longer used to debug contracts against Java-applied state. That
  means B2 must be retired first.

## Anti-regression rule (durable)

**Do not add a new bridge mechanism without first re-reading this
file.** Any new path where Java mutates state and then has to push it
to Rust, OR any new path where Rust mutates state and Java needs to
mirror it, is a new bridge. Each new bridge must:

1. Appear in the inventory above with a classification.
2. Identify what removes it (which Phase 2+ milestone makes it
   redundant).
3. Not quietly reshuffle write ownership — write ownership is frozen
   in `close_loop.write_ownership.md` and changing it requires
   updating that file first.

If a change needs a new bridge but cannot identify the milestone
that removes it, the change has probably guessed wrong about who
owns which state. Stop and escalate.

## Acceptance

Section 4 acceptance is satisfied when:

- The bridge inventory above exists and is referenced from Section 4
  of `close_loop.todo.md`.
- Every bridge has a Phase 1 classification.
- A removal sequence is documented (above).
- The anti-regression rule is documented and engineers know to
  re-read this file before adding a new bridge.

Actual bridge *removal* is explicitly outside Phase 1 and is not an
acceptance criterion. The only thing Phase 1 is required to do is
make the debt visible and sequenced.
