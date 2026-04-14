# Close Loop — 1.4 Snapshot Semantics

This file closes Section 1.4 of `close_loop.todo.md`. It locks what
"snapshot" means on both the storage SPI surface and the execution SPI
surface in Phase 1, so that **fake success** on snapshot APIs is no
longer an accepted state.

Companion notes:

- `close_loop.storage_transactions.md` — 1.3 storage transaction contract.
- `close_loop.write_ownership.md`       — 1.1 canonical write ownership.

## Audit findings

### Storage SPI snapshot surface

File: `framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java`

```
CompletableFuture<String> createSnapshot(String dbName);
CompletableFuture<Void>   deleteSnapshot(String snapshotId);
CompletableFuture<byte[]> getFromSnapshot(String snapshotId, byte[] key);
```

No iterator API, no range / prefix API for snapshots.

### Rust storage engine snapshot implementation

File: `rust-backend/crates/storage/src/engine.rs`

Current behavior:

- `create_snapshot(database)` allocates a UUID and stores
  `SnapshotInfo { db_name }` in `self.snapshots`. No RocksDB snapshot
  handle is taken.
- `get_from_snapshot(snapshot_id, key)` looks up the entry and calls
  `self.get(db_name, key)` — **i.e. it reads from the current state of
  the underlying DB, ignoring any writes that have happened since the
  snapshot was created.**
- `delete_snapshot(snapshot_id)` removes the entry.

Comment in the source code:
```rust
// For simplicity, we just read from the current database
// In a real implementation, you'd need to maintain actual snapshots
```

This is a textbook "fake success" API: the caller thinks they got a
point-in-time view, but they actually got the live DB view.

### Execution SPI snapshot surface

File: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`

```
createSnapshot()              → placeholder, returns "remote_snapshot_<millis>"
revertToSnapshot(snapshotId)  → placeholder, returns false
```

Both are hard-coded placeholders in the Java remote bridge, returning
success-looking values without doing any work. The Rust side for these
APIs is likewise unimplemented.

The in-Rust EVM execution crate does its own journaling for the
intra-transaction case, so EVM-style `create_snapshot` / `revert` is
currently handled via REVM's built-in state journaling inside a single
VM execution — it does not depend on the SPI-level snapshot APIs. The
SPI-level APIs are only needed for caller-visible snapshots across
multiple transactions, which no production code path currently uses.

## Decision

### Storage snapshots

**Storage snapshot is explicitly unsupported in Phase 1.** Fake
success is worse than an explicit error.

Concretely:

- `create_snapshot(database)` MUST return `Err("storage snapshot is not
  supported in Phase 1 — see planning/close_loop.snapshot.md")` or the
  equivalent gRPC `UNIMPLEMENTED` / `FailedPrecondition` error code.
- `get_from_snapshot(snapshot_id, key)` MUST return the same error.
  It MUST NOT silently read from the live DB.
- `delete_snapshot(snapshot_id)` MUST also return the same error.
  Allowing it to succeed trivially would hide bugs in callers that
  assumed snapshots were real.
- The Java `StorageSPI` javadoc (or its Phase 1 note) must record this.
- The `RemoteStorageSPI` gRPC client MUST surface the error up through
  `CompletableFuture.completeExceptionally` rather than returning a
  placeholder snapshot id.
- No Phase 1 code path may rely on SPI-level snapshots. Tests that
  exercise snapshots must test the "unsupported error is returned"
  contract, not the historical "fake success" contract.

Rationale:

- Fake snapshot success has been biting us. Callers write diagnostic
  code that assumes snapshot isolation and then silently observe
  live-DB reads, producing bug reports that are impossible to debug.
- Real RocksDB point-in-time semantics require either
  `rocksdb::SnapshotWithThreadMode` handles tracked per-snapshot-id,
  or full `OptimisticTransactionDB` support, or periodic checkpoints.
  None of those are trivial to plumb across a gRPC boundary — the
  snapshot lifetime has to live on the Rust side and be bound to the
  remote caller, and Java would need a reference-tracking scheme to
  make sure snapshots are not leaked on Java JVM crash.
- Phase 1 has no consumer that actually needs SPI-level snapshots.
  The block importer in Phase 2 can be redesigned around
  single-transaction atomicity (Section 1.3), and EVM journaling is
  handled inside REVM.
- So: mark it unsupported, get the fake out of the way, revisit in a
  later phase only when a real consumer appears.

### EVM snapshots (`createSnapshot` / `revertToSnapshot` on execution SPI)

Same call: **EVM-level SPI snapshots are unsupported in Phase 1.**

- `RemoteExecutionSPI.createSnapshot()` MUST stop returning
  `"remote_snapshot_<millis>"` and start failing with an explicit
  unsupported error via `CompletableFuture.completeExceptionally(
  new UnsupportedOperationException(...))`.
- `RemoteExecutionSPI.revertToSnapshot(...)` MUST also fail explicitly
  via `CompletableFuture.completeExceptionally(new
  UnsupportedOperationException(...))`. An earlier draft of this
  document said it should "return `false` with a logged error" — that
  was the half-measure, and it has been replaced with the same
  exception-completion contract as `createSnapshot()` so that the
  two APIs behave identically and there is no silent boolean fallback.
- The gRPC server on the Rust side MUST return a clear error code for
  both APIs (Rust execution gRPC change is still tracked as a follow-up
  below; the Rust storage snapshot APIs already return errors).
- Phase 1 acceptance treats "snapshot API no longer fake" as a
  must-have. Section 2.2 tracks the actual code change for the Rust
  execution gRPC handlers.

Rationale:

- REVM provides intra-transaction journaling, which is what actual VM
  execution needs. That's already covered inside `TronEvm`.
- The cross-transaction snapshot/revert use case (save a point, run
  some transactions, undo back to the saved point) is not used by any
  current Phase 1 consumer.
- If a consumer appears later, the right answer is to build it on top
  of a proper storage-level transaction abstraction plus execution
  journaling — not to resurrect this half-working API. Relying on the
  current shape would lock us into the wrong design.

### Interaction with the storage transaction API (Section 1.3)

Because both "storage snapshot" and "transactional iterators" are
explicitly deferred in Phase 1, there is no required interaction rule
between snapshots and transactions — they are independent no-ops.

If and when we re-introduce snapshots in a later phase, the rule will
be: a snapshot captured **before** `begin_transaction` must not see
the transaction's buffered writes, and must not be invalidated by
the commit of that transaction. This is a future design item, not a
Phase 1 commitment.

### Lifecycle of snapshot objects (when implemented in a later phase)

Not in scope for Phase 1. When snapshots are eventually implemented,
the lifecycle plan will cover:

- Creation (which operation returns the id).
- Read paths allowed against the snapshot id.
- Deletion (explicit free).
- Automatic cleanup on process shutdown.
- Behavior if a snapshot-backed read outlives the snapshot id.

None of those need to be answered now, because we are not implementing
snapshots now.

### Iterator APIs against a snapshot

Not in scope for Phase 1 and not in scope for the deferred snapshot
implementation either, until a concrete consumer asks for it. We will
not speculatively design snapshot-backed iterators.

## Definition of "locked"

Section 1.4 is locked once:

- This file exists and is referenced from `close_loop.todo.md`.
- The Rust engine docs reference this file.
- Section 2.2 tracks the code change that replaces fake success with
  explicit unsupported errors on the execution SPI side.
- Section 3.3 tracks the code change that replaces fake success with
  explicit unsupported errors on the storage SPI side.

Until those code changes land, the "snapshot is real or explicitly
unavailable" exit criterion in Section 0 remains unchecked.

## Follow-up implementation items

Tracked against Section 2.2 (execution) and Section 3.3 (storage):

- Replace `create_snapshot` / `get_from_snapshot` / `delete_snapshot`
  in `engine.rs` with explicit unsupported errors. Do NOT fall back
  to live-DB reads.
- Replace `createSnapshot` / `revertToSnapshot` placeholders in
  `RemoteExecutionSPI.java` with explicit unsupported errors.
- On the Rust execution service, implement the same for
  `create_evm_snapshot` / `revert_to_evm_snapshot` gRPC handlers —
  explicit unsupported, no placeholders.
- Update unit/integration tests to assert the unsupported contract,
  not historical fake success.
- Remove the "snapshot" mention from any "it works" marketing copy or
  planning language; the API is explicitly a no-op in Phase 1.
