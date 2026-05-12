# Close Loop — 1.3 Storage Transaction Semantics

This file closes Section 1.3 of `close_loop.todo.md`. It locks what
`beginTransaction` / `commit` / `rollback` on the storage boundary are
**required** to do in Phase 1 and, just as importantly, what they are
**not** required to do. The point is to stop anyone from inferring a
generic-DB-product promise from the existing SPI surface.

## Audit findings

### Java SPI surface

File: `framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java`

```
CompletableFuture<String> beginTransaction(String dbName);
CompletableFuture<Void>   commitTransaction(String transactionId);
CompletableFuture<Void>   rollbackTransaction(String transactionId);
```

Semantics are effectively undefined: no specification of read-your-writes
visibility, no specification of cross-DB scope, no specification of
what happens if you issue writes with a `transactionId` that was never
opened. `RemoteStorageSPI.java` simply forwards to gRPC.

In java-tron proper, **no production code path calls these methods.**
The only call sites are `framework/src/test/java/org/tron/core/storage/spi/StorageSPIIntegrationTest.java`.
That means the Phase 1 contract does not have to be backward-compatible
with any live consumer inside the Java node — we can spec it freely.

### Rust engine surface

File: `rust-backend/crates/storage/src/engine.rs`

```rust
pub fn begin_transaction(&self, database: &str) -> Result<String>
pub fn commit_transaction(&self, transaction_id: &str) -> Result<()>
pub fn rollback_transaction(&self, transaction_id: &str) -> Result<()>
```

Current behavior:

- `begin_transaction` allocates a UUID, inserts an empty
  `TransactionInfo { db_name, operations: Vec::new() }`, returns the UUID.
- `commit_transaction` removes the entry and iterates its `operations`
  list, writing them via a `WriteBatch`. The buffer is atomic once the
  iteration runs.
- `rollback_transaction` removes the entry. No undo work, because no
  writes were persisted inside the transaction scope.

**Critical gap**: `put` / `delete` / `batch_write` on `StorageEngine`
write through to the DB immediately. Nothing routes them into the
`TransactionInfo::operations` buffer. The commit path is iterating a
vector that is *always empty*.

So: the commit/rollback API exists, but there is no real transaction
semantics behind it. From the execution crate's point of view, the
only path that actually provides atomicity is
`EngineBackedEvmStateStore::new_with_buffer`, which lives in
`crates/execution` rather than `crates/storage`.

### Production execution path

File: `rust-backend/crates/core/src/service/grpc/mod.rs` (`execute_transaction`)

Execution uses its own local buffer (`EngineBackedEvmStateStore` with an
optional `WriteBuffer`), not the storage crate's transaction API. The
storage crate's `begin_transaction` is **not** exercised by the
execution path today. Non-VM handlers buffer unconditionally for
atomicity; VM handlers buffer only when `rust_persist_enabled=true`.

This is what "storage hot-path operations work, transaction support is
structurally incomplete" means in practice.

## Decision

### Required semantics for `beginTransaction` / `commit` / `rollback`

Phase 1 locks the semantics to a **narrow, execution-local** contract:

- `beginTransaction(db)` allocates an opaque `transaction_id` and opens
  a per-transaction write buffer. The buffer captures `put` /
  `delete` / `batch_write` operations issued **against the same
  database** until commit or rollback.
- `commitTransaction(tid)` atomically applies every buffered operation
  to the underlying RocksDB via a single `WriteBatch`. No operation is
  visible outside the transaction until commit has returned success.
- `rollbackTransaction(tid)` discards every buffered operation without
  applying any of them, then frees the transaction buffer.
- If commit fails (RocksDB error, process crash, etc.), no partial
  writes are persisted.

The execution crate continues to use its `EngineBackedEvmStateStore`
buffer for VM execution. The storage crate's transaction API exists to
serve the **Java block importer** in Phase 2 and to serve parity-test
tooling in Phase 1. Those are the only two consumers we're spec'ing for.

### Scope

- **Per-DB** is the only required scope. `begin_transaction` takes a
  `database` argument and the resulting buffer is scoped to that
  database.
- **Cross-DB transactions are explicitly OUT OF SCOPE** for Phase 1.
  Rationale: our RocksDB setup is one instance per database (one
  `DB` per column family). A cross-DB transaction would require
  `OptimisticTransactionDB` or a global `WriteBatchWithSavePoint`
  wrapper, which we are not willing to maintain for this phase.
  The block importer (Phase 2) can work around the lack of cross-DB
  transactions by committing per-database serially and accepting that
  a process crash in the middle of apply rolls back to the last
  committed state of each database individually.
- Explicitly labelled: **we are not building a generic database
  product.** The transaction API is a narrow helper for execution
  atomicity, not a replacement for RocksDB transactions or a
  distributed DB abstraction.

### Read-your-writes visibility

For transaction-scoped reads, Phase 1 locks the following:

| API                | Visibility required                                  |
| ------------------ | ---------------------------------------------------- |
| `get(db, key)`     | Optional for Phase 1. If the caller needs read-your-writes, the caller must pass `transactionId` AND the engine must layer the buffer over the base DB read. Until implemented, reads go directly to the base DB. |
| `has(db, key)`     | Same as `get`.                                       |
| `batchGet`         | Same as `get`. Not required to honor the buffer in Phase 1. |
| `iterator` / prefix / range | **Out of scope** for transaction-scoped visibility. Iterators read from the committed base DB only. Attempting to use an iterator inside a transaction is allowed but will not see uncommitted writes. |

Rationale: execution does not need read-your-writes inside the storage
transaction API because the `EngineBackedEvmStateStore` buffer already
provides it at a higher level. The block importer (Phase 2) will
process writes as a bulk apply on already-computed state, so it also
does not need transaction-scoped iterators during the apply step.

If we discover a Phase 1 consumer that needs read-your-writes, we
upgrade this table; we do **not** upgrade it preemptively.

### What execution actually needs right now

Execution does **not** need the storage crate's transaction API for
atomicity — it uses its own buffer. What execution needs from storage is:

- Fast hot-path `get` / `put` / `delete` — already works.
- Atomic commit of a batch — already provided by `batch_write`.
- A narrow, explicit way to say "everything I'm about to write is part
  of one transaction" for the block importer or for parity-testing
  tools that replay fixtures — **this** is what the transaction API
  will eventually serve, and Section 3.2 is where we wire the buffer
  through.

Everything else (cross-DB transactions, transactional iterators,
MVCC reads, savepoints) is explicitly deferred.

### Behavior when `transaction_id` is absent on a write

If a `put` / `delete` / `batch_write` request arrives with an empty
`transaction_id`:

- The engine performs the write **immediately against the base DB**,
  exactly as `StorageEngine::put` / `delete` / `batch_write` do today.
- This matches the current behavior and is the "direct mode" default.
- Logs emitted by the gRPC handler must make it obvious that the write
  is a direct (non-transactional) write — so a human reading diagnostic
  logs can tell whether a test was exercising the buffer path or the
  direct path. Section 3.1 is where this logging lands.

If a write arrives with a `transaction_id` that is **not** present in
the active transaction map:

- The engine MUST return an explicit error (`transaction not found` or
  equivalent) and MUST NOT silently fall back to a direct write.
  Silent fallback hides bugs in the Java/Rust coordination code and is
  banned.

## Definition of "locked"

Section 1.3 is locked once:

- This file exists and is referenced from `close_loop.todo.md`.
- `StorageSPI.java` javadoc and `engine.rs` module-level doc both
  reference this file as the source of truth.
- Section 3 (storage semantic hardening) does its implementation work
  *within* these semantics — no Phase 1 code path should rely on
  cross-DB transactions, transactional iterators, or MVCC reads.

## Follow-up implementation items

Rust-side work is now done (iter 3):

- [x] Wire `put` / `delete` / `batch_write` through the
      `TransactionInfo::operations` buffer when a caller passes a
      `transaction_id`. The engine has new methods `put_in_tx`,
      `delete_in_tx`, `batch_write_in_tx`, and the gRPC handlers in
      `service/grpc/mod.rs` branch on the `transaction_id` field.
- [x] Return an explicit "transaction not found" error for writes
      bearing a stale `transaction_id`. No silent fallback.
- [x] Add storage-crate tests that exercise begin / write-in-buffer /
      commit and begin / write-in-buffer / rollback / verify-nothing-
      landed. Section 3.4. `cargo test -p tron-backend-storage` now
      runs 22 tests, all green — the crate is no longer `0 tests`.

Still open:

- [ ] Java-side plumbing: `RemoteStorageSPI` put/delete/batchWrite
      does not yet thread a `transaction_id` through. Java production
      callers must be audited to decide where transactions are opened
      and which writes join which transaction. See Section 3.1
      "still open" sub-items in `close_loop.todo.md`.
- [ ] Java-side tests that prove the bridge actually carries the
      `transaction_id` end to end. Depends on the Java audit above.
- [ ] Route buffered reads through the buffer — only if (and when)
      we decide to upgrade the read-your-writes table above. Not
      required for Phase 1.

## Anti-goals (durable)

Phase 1 explicitly **does not** provide:

- Cross-database transactions.
- Transactional iterators / range scans / prefix scans.
- Savepoints inside a transaction.
- Read-your-writes on `get` / `has` / `batchGet` until Section 3.2 or
  a later phase chooses to add it.
- A generic "DB product" abstraction suitable for arbitrary third-party
  consumers. The SPI is shaped for the block importer and parity
  tooling, and only those consumers.

If a future task tries to reach for any of these, first update this
file to explain why the anti-goal needs to be relaxed.
