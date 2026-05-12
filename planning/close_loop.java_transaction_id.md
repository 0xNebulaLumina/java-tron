# Close Loop — 3.1 Java-side `transaction_id` Plumbing Audit

This file closes the Java-side half of Section 3.1 of
`close_loop.todo.md`. The Rust side was wired through in iter 3
(gRPC handlers branch on `transaction_id`; engine has real per-tx
buffer); this audit documents the Java side's current state and
freezes the design for the Phase 2 plumbing work.

Companion notes:

- `close_loop.storage_transactions.md` — the per-DB transaction
  contract this plumbing must respect.
- `close_loop.write_ownership.md` — write-ownership policy that
  determines when transactions even matter on the Java side.
- `close_loop.bridge_debt.md` — `RemoteStorageSPI` is currently
  not a production hot-path bridge, which limits the urgency.

## Audit

### Java SPI interface signatures

`framework/src/main/java/org/tron/core/storage/spi/StorageSPI.java`:

```java
CompletableFuture<Void> put(String dbName, byte[] key, byte[] value);
CompletableFuture<Void> delete(String dbName, byte[] key);
CompletableFuture<Void> batchWrite(String dbName, Map<byte[], byte[]> operations);
```

**No `transaction_id` parameter exists at the Java SPI surface.**
The `beginTransaction` / `commitTransaction` / `rollbackTransaction`
methods exist (they return / consume opaque transaction ids), but
there is no way for a caller to say "this `put` belongs to
transaction X" through the Java SPI as it stands. The
`RemoteStorageSPI` implementation builds its `PutRequest` /
`DeleteRequest` / `BatchWriteRequest` proto messages without
populating the `transaction_id` field that already exists in the
proto schema.

### `RemoteStorageSPI` request builders

`framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java`:

- `put(...)` line ~144 builds `PutRequest.newBuilder()
  .setDatabase(dbName).setKey(...).setValue(...).build()` —
  `transaction_id` field stays default (empty string), which is
  the iter 3 "direct write against the base DB" branch.
- `delete(...)` line ~169 — same shape, no `transaction_id`.
- `batchWrite(...)` line ~215 — same shape, no `transaction_id`.
- `beginTransaction(...)` line ~610 returns the opaque
  transaction id from the Rust engine, but nothing on the Java
  side consumes it for subsequent put/delete/batchWrite calls
  because there is no parameter to consume it through.
- `commitTransaction(...)` line ~635 and `rollbackTransaction(...)`
  line ~653 do thread the `transaction_id` through to the Rust
  side (these were the only methods that needed it pre-iter-3).

### Production callers

`grep RemoteStorageSPI` against `framework/src/main/java`:

- `RemoteStorageSPI` is only referenced from
  `StorageSpiFactory` (the construction site).
- `StorageSpiFactory.createStorage(StorageMode.REMOTE)` is in
  turn referenced from a small number of test classes
  (`StorageSPIIntegrationTest`, `StorageSpiFactoryTest`,
  `DualStorageModeIntegrationTest`, `RemoteTronWorkloadBenchmark`,
  `RemoteStoragePerformanceBenchmark`, `DualModePerformanceBenchmark`).
- **`RemoteStorageSPI` is NOT integrated into the main java-tron
  node's persistence path.** Per the lessons in `CLAUDE.md`:
  "The actual FullNode application still uses hardcoded storage
  initialization in `TronDatabase.java` and `TronStoreWithRevoking.java`
  constructors." The dual-storage-mode factory was implemented
  but never wired into the main node.

This is the key fact: **today there is no production hot-path
caller that would benefit from `transaction_id` plumbing on
the Java side**. Java's main node writes via
`chainbase`-backed `TronStoreWithRevoking`, NOT via the
`StorageSPI` `put` / `delete` / `batchWrite` API. So the Java
side of Section 3.1 is structurally absent, not just unwired.

## Decisions for Phase 1

Phase 1 freezes the following:

1. **Java SPI signature reshape is deferred to Phase 2 / block
   importer work.** The right shape is to add an overloaded
   trio:

   ```java
   CompletableFuture<Void> put(String txId, String dbName, byte[] key, byte[] value);
   CompletableFuture<Void> delete(String txId, String dbName, byte[] key);
   CompletableFuture<Void> batchWrite(String txId, String dbName, Map<byte[], byte[]> operations);
   ```

   leaving the existing 3-arg variants as the "direct write"
   convenience that delegates with `txId == ""`. This matches
   the Rust side's empty-string-direct convention without
   forcing every existing caller to thread an extra null
   parameter.

2. **No production-path Java caller is migrated in Phase 1.**
   Because no Java production caller uses `RemoteStorageSPI`
   today, there is nothing to migrate. The block importer
   work (Phase 2 milestone) is what will introduce the first
   production caller that opens a transaction, threads it
   through writes, and commits — and that's the right time
   to land the SPI signature change.

3. **Where transaction identifiers are owned**: the block
   importer Phase 2 work is the canonical owner. A block
   importer opens one transaction per block, threads its id
   through every write the block produces, and commits at
   end-of-block (or rolls back on validation failure). No
   other Java code path is expected to open transactions in
   Phase 1 — see `close_loop.storage_transactions.md`
   "What execution actually needs right now".

4. **Two missing test paths** (already tracked under Section
   3.4 Java-focused) are the Phase 1 deliverable that
   demonstrates the Java side can carry `transaction_id` to
   the Rust side at all:

   - A test that opens a transaction via `beginTransaction`,
     directly issues a `PutRequest` with that id (bypassing
     the current 3-arg `put` API), reads back via direct
     `get` to confirm read isolation, then commits and reads
     back to confirm visibility.
   - A test that proves the gRPC `transaction_id == ""`
     direct path round-trips correctly (the existing 3-arg
     `RemoteStorageSPI.put` already exercises this; the test
     is a contract assertion, not new wiring).

   Both tests can be written today against the existing iter
   3 Rust gRPC handler without any Java SPI signature change,
   because they construct the `PutRequest` proto directly via
   the gRPC client stub instead of going through
   `RemoteStorageSPI.put(...)`.

5. **The `audit + classify + plumb` items in the close_loop todo
   for Section 3.1 split into "audit done in Phase 1" and
   "plumb in Phase 2"**: the audit is what this file is. The
   plumbing is gated on the block importer work in Phase 2.

## Phase 1 acceptance

Section 3.1 acceptance is satisfied by:

- This audit documents every Java write call that could carry
  `transaction_id` (there are exactly three: `put` / `delete` /
  `batchWrite` on `RemoteStorageSPI`, all currently NOT
  passing the field).
- The reason no production migration is needed in Phase 1 is
  documented: `RemoteStorageSPI` is not on the production hot
  path (`CLAUDE.md` lesson on "Main Application Integration").
- The Phase 2 transaction-owner is named: the block importer.
- The two open test items in Section 3.4 cover the
  bridge-existence proof (one buffered-tx round-trip + one
  direct-path round-trip).
- Tracing/logging that distinguishes transactional vs direct
  writes already exists on the Rust side as of iter 3
  (gRPC handler debug lines).

The implementation work — adding the 4-arg overloaded SPI
methods and threading `transaction_id` from a real Java caller —
is Phase 2.

## Follow-up implementation items

Tracked as Phase 2 / block-importer work:

- [ ] Add the 4-arg overloaded `put` / `delete` / `batchWrite`
      methods to `StorageSPI` and implement them in both
      `EmbeddedStorageSPI` and `RemoteStorageSPI`. The Remote
      implementation populates `transaction_id` on the proto
      request; the Embedded implementation either errors out
      (transactions not supported in EE mode) or implements
      a Java-side per-tx buffer to mirror the Rust shape.
- [ ] Update `RemoteStorageSPI.beginTransaction` to return
      a typed `Transaction` handle that owns the id, instead
      of an opaque `String`, so Java callers cannot mix
      transaction ids across DBs accidentally.
- [ ] Add the two integration tests called out in Section
      3.4 Java-focused (transactional round-trip; direct
      round-trip). Both can be written today against the
      Rust side without waiting for Phase 2.
- [ ] When the block importer lands in Phase 2, make it the
      first Java production caller that opens a transaction,
      threads its id through every write the block produces,
      and commits at end-of-block.
