I am debugging and fix why, for the pre-balance of executing a tx,
the rust-side may be different from the java-side.

Logs:
java log: remote-java.6751d25.log
rust log: remote-rust.6751d25.log

more spefically

java:
```
03:15:18.308 INFO  [sync-handle-block] [DB](Manager.java:1607) TxId: 007fe233fa1da28bc72a543a5a96d67e8d0579853888545ade21ab146aa74101, BlockNum: 2131, ContractType: TransferContract, Blackhole balance: before=9223372046956375808 SUN, after=9223372046956375808 SUN, diff=0 SUN

03:15:18.892 INFO  [sync-handle-block] [DB](Manager.java:1607) TxId: ce4873a63c75d6232e487bcd4ec4387b2e756ead44e5d22207acad5807b22852, BlockNum: 2140, ContractType: AccountUpdateContract, Blackhole balance: before=9223372046956375808 SUN, after=9223372046956375808 SUN, diff=0 SUN
```

rust
```
[2m2025-11-17T03:15:18.307793Z[0m [32m INFO[0m [2mtron_backend_core::service::grpc[0m[2m:[0m Blackhole balance BEFORE execution: 9223372046956375808 SUN (address: TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy) - block: 2131, txId: 007fe233fa1da28bc72a543a5a96d67e8d0579853888545ade21ab146aa74101, tx from: TSnjgPDQfuxx72iaPy82v3T8HrsN4GVJzW, contract_type: TransferContract
[2m2025-11-17T03:15:18.307968Z[0m [32m INFO[0m [2mtron_backend_core::service::grpc[0m[2m:[0m Blackhole balance AFTER execution: 9223372046956375808 SUN (address: TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy, delta: +0 SUN) - block: 2131, txId: 007fe233fa1da28bc72a543a5a96d67e8d0579853888545ade21ab146aa74101, tx from: TSnjgPDQfuxx72iaPy82v3T8HrsN4GVJzW, contract_type: TransferContract

[2m2025-11-17T03:15:18.891562Z[0m [32m INFO[0m [2mtron_backend_core::service::grpc[0m[2m:[0m Blackhole balance BEFORE execution: 9223372046947975808 SUN (address: TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy) - block: 2140, txId: ce4873a63c75d6232e487bcd4ec4387b2e756ead44e5d22207acad5807b22852, tx from: TMLbXW9wPy2PMHLvR9PfwTrWsvq9MTrRgE, contract_type: AccountUpdateContract
[2m2025-11-17T03:15:18.891949Z[0m [32m INFO[0m [2mtron_backend_core::service::grpc[0m[2m:[0m Blackhole balance AFTER execution: 9223372046947975808 SUN (address: TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy, delta: +0 SUN) - block: 2140, txId: ce4873a63c75d6232e487bcd4ec4387b2e756ead44e5d22207acad5807b22852, tx from: TMLbXW9wPy2PMHLvR9PfwTrWsvq9MTrRgE, contract_type: AccountUpdateContract

```

why in block 2131 the rust storage already update to a new value, but in block 2140 it "jumps back" to an old value?


you will read thru and understand the codes and analyze whether the explanation in planning/flush_race.planning.md is correct 

----


• The jump is caused by the snapshot flush writing an older blackhole value back into the remote RocksDB, after it had already been updated, and then block 2140 running without re‑syncing the blackhole.

Here’s the chain of events, tied to code and your logs.

1. What the logs show

- At block 2120, both Java and Rust see the same value:
    - Java: before=9223372046947975808 (remote-java.6751d25.log:103510)
    - Rust: Blackhole balance BEFORE execution: 9223372046947975808 (remote-rust.6751d25.log:16231)
- At block 2131, both sides see a higher value:
    - Java: before=9223372046956375808 (remote-java.6751d25.log:105968)
    - Rust: Blackhole balance BEFORE execution: 9223372046956375808 (remote-rust.6751d25.log:17686)
- At block 2140:
    - Java still sees before=9223372046956375808 (remote-java.6751d25.log:106574)
    - Rust suddenly sees BEFORE execution: 9223372046947975808 (remote-rust.6751d25.log:17703) – exactly the old 2120 value.

So Rust really does “jump back” from the newer 6956375808 SUN to the older 6947975808 SUN between 2131 and 2140, while Java does not.

2. What happens between 2131 and 2140

- Blocks 2132–2139 are empty on Java side:
    - PushBlock block number: 2132, ... txs: 0 through 2139 (remote-java.6751d25.log:106007–106550).
- Rust has no ExecuteTransaction logs for blocks 2132–2139:
    - rg "block: 213[2-9]" remote-rust.6751d25.log → no matches.
- Right after finishing block 2139, Java’s snapshot manager runs a flush:
    - 03:15:18.840 INFO [SnapshotManager] Flush cost: 51 ms, create checkpoint cost: 48 ms, refresh cost: 3 ms. (remote-java.6751d25.log:106556)
- Immediately after that flush, block 2140 is processed:
    - 03:15:18.788 – PushBlock block number: 2140 (remote-java.6751d25.log:106556+)
    - 03:15:18.888 – ResourceSync pre-exec flush: accounts=1, ..., includes_blackhole=false (remote-java.6751d25.log:106564)
    - 03:15:18.891562Z – Rust logs Blackhole balance BEFORE execution: 9223372046947975808 (remote-rust.6751d25.log:17703)

The key facts:

- There are no Rust executions between 2131 and 2140 that could legitimately change the blackhole.
- A SnapshotManager.flush() runs between them.
- The resource sync for the 2140 tx explicitly does not include the blackhole (includes_blackhole=false), so Rust uses whatever is in RocksDB at that moment.

3. How writes flow in code

There are two independent write paths into the remote “account” DB used by Rust:

1. ResourceSyncService (Java → remote)
    - Manager.processTransaction:
        - Creates TransactionContext and calls ResourceSyncContext.begin(...).
        - Pre-exec resource consumption (bandwidth, memo fee, multisig fee) mutates accounts and dynamic properties.
            - When blackhole is credited directly:
            consumeMultiSignFee / consumeMemoFee adjust the blackhole via AccountStore.put(...) and call ResourceSyncContext.recordAccountDirty(blackholeAddress) (Manager.java:902+).
        - Just before the gRPC ExecuteTransaction call:
        ResourceSyncContext.flushPreExec() → ResourceSyncService.flushResourceDeltas(...) (Manager.java:1556).
    - ResourceSyncService.flushResourceDeltas(...):
        - Reads current account values from AccountStore (which uses the TronStoreWithRevoking.revokingDB chain) via getUnchecked(...).
        - Calls storageSPI.batchWrite("account", accountBatch) (ResourceSyncService.java:140–207).
        - In REMOTE mode, StorageSPI is a gRPC client (RemoteStorageSPI) writing directly into the Rust backend’s RocksDB “account” column family.

    This is the path that made Rust see the new blackhole value at 2131.
2. SnapshotManager flush (Java snapshots → remote)
    - All chainbase stores (AccountStore, DynamicPropertiesStore, etc.) extend TronStoreWithRevoking<T> (TronStoreWithRevoking.java), whose constructor does:
        - Determine storageMode from config.
        - If embedded or remote, get a StorageBackend from StorageBackendFactory.getInstance().createStorageBackend(mode, dbName).
        - Wrap that in a StorageBackendDbSource and then StorageBackendDB.
        - Initialize the revokingDB as new Chainbase(new SnapshotRoot(this.db)).
    - In remote mode, StorageBackend is a StorageSpiBackendAdapter over a StorageSPI created with StorageSpiFactory.createStorage(StorageMode.REMOTE) (StorageBackendFactoryImpl.java).
    - SnapshotManager manages a chain of Snapshots over those DBs:
        - buildSession() → advance() appends a new SnapshotImpl layer and increments size.
        - When size > maxSize, it does:
            - flushCount += size - maxSize;
            - flush(); (SnapshotManager.java:120–139)
    - flush() calls refresh() which for each Chainbase db calls refreshOne(db):
        - refreshOne collects flushCount snapshots starting from root.getNext() and calls root.merge(snapshots) (SnapshotManager.java:304–330).
    - SnapshotRoot.merge(...):
        - Flattens the given snapshot(s) into a Map<WrappedByteArray, WrappedByteArray> batch.
        - Then calls ((Flusher) db).flush(batch) (SnapshotRoot.java:86–119).
    - For remote mode, db is StorageBackendDB, whose flush implementation is:
        - Convert wrapped keys/values to raw byte[].
        - Call dbSource.getStorageBackend().batchPut(convertedBatch) (StorageBackendDB.java:101–115).
        - That batchPut goes through StorageSpiBackendAdapter.batchPut → storageSPI.batchWrite(dbName, batch).get().

    So snapshot flush also writes directly into Rust’s RocksDB “account” DB, but using whatever values are stored in the older snapshots being merged, not necessarily the latest head snapshot that ResourceSync
used.

4. Why Rust’s blackhole balance “jumps back”

Putting this together:

- Up through block 2120, both Java and Rust agree on blackhole = 9223372046947975808 (call this value B₀).
- Between 2120 and 2131, some transactions (fees, etc.) credit additional SUN to the blackhole at the Java level. Those mutations go through AccountStore.put into the revoking snapshot chain and are sent to
Rust via ResourceSyncService.flushResourceDeltas when needed.
- By the time of block 2131, both Java and Rust read B₁ = 9223372046956375808:
    - Java directly from AccountStore.getBlackhole() (revoking head snapshot).
    - Rust from EngineBackedEvmStateStore::get_account(...) reading RocksDB, where ResourceSync has already pushed B₁.
- Blocks 2132–2139 are empty. Java still advances snapshots per block and calls updateDynamicProperties, but no new blackhole writes go to Rust.
- At block 2139, SnapshotManager decides there are too many snapshots: size > maxSize. It:
    - Increments flushCount.
    - Calls flush(), which:
        - Builds a batch from the oldest flushCount snapshot layers after root (these reflect earlier block states, where the blackhole was still B₀).
        - Writes that batch to the underlying StorageBackend for "account" via batchPut → storageSPI.batchWrite("account", batch).
- Because the blackhole account exists in those older snapshots with value B₀, that batch contains the TLs… key with balance B₀, and this write overwrites the previously correct B₁ in RocksDB.
    - Java’s current snapshot chain is untouched (it still has B₁ at the head), so AccountStore.getBlackhole() continues to see B₁.
    - Rust, which only sees the persisted RocksDB state, now sees B₀ again.
- For the AccountUpdateContract in block 2140:
    - Java’s ResourceSyncService logs: accounts=1, ..., includes_blackhole=false (remote-java.6751d25.log:106564–106566), so it does not re‑sync the blackhole account.
    - Rust’s ExecuteTransaction handler reads the blackhole via storage_adapter.get_account(...) and logs the old B₀: 9223372046947975808 (remote-rust.6751d25.log:17703).
    - Java logs before=9223372046956375808 from its head snapshot, so the pre-balance diverges.

So the “jump back” is:

- Not because Rust somehow rolled back the chain.
- Not because 2131 changed the blackhole but 2140 didn’t.
- But because the snapshot flush wrote an older snapshot value (B₀) into the remote "account" DB after it had already been advanced to B₁ by ResourceSync, and the 2140 transaction didn’t include the blackhole
in its pre-exec sync.

5. Is flush_race.planning.md’s explanation correct?

Broadly, yes. Point by point:

- “Empty Blocks 2132-2139” – correct: Java pushed those blocks with txs: 0, Rust has no executions.
- “No Blackhole Sync for 2140” – correct: includes_blackhole=false in ResourceSyncService logs for that tx.
- “Stale Read from RocksDB at 2140 matching an earlier log” – correct: Rust’s 2140 pre-balance equals the 2120 value.
- Architectural diagram:
    - AccountStore.put() → snapshot chain → snapshot flush → underlying DB → REMOTE backend → RocksDB – accurate in remote mode via StorageBackendDB and StorageSpiBackendAdapter.
    - ResourceSyncService uses a separate StorageSPI instance that also writes directly to the same remote DB via gRPC.
- “Two different paths write to Rust storage” – exactly:
    - Path 1: ResourceSyncService.flushResourceDeltas (pre-exec sync based on the head snapshot).
    - Path 2: SnapshotRoot.merge → StorageBackendDB.flush (checkpoint/refresh based on older snapshots).
- The only nuance I’d add:
    - The behaviour is less about a “race” in the threading sense (the flush completes before 2140’s ResourceSync starts), and more about write ordering between two unsynchronised writers using different
views of the state (older snapshots vs current head) to update the same remote DB.
    - The description “after transaction execution at block 2131 Java writes to AccountStore → snapshot chain” is true in general, but the actual blackhole bump from B₀ to B₁ happened in earlier fee‑charging
transactions (the 2131 tx itself has diff=0 in the logs). That doesn’t change the core diagnosis.

So, the planning document’s root-cause story is essentially correct: snapshot flush in REMOTE mode can and did overwrite Rust’s newer state with older snapshot data, and because the 2140 transaction didn’t
mark the blackhole as dirty, ResourceSync didn’t repair it before Rust executed and logged the pre-balance.

If you want, next step would be to design a concrete fix (e.g., never using REMOTE StorageBackend for the revoking/snapshot layer, or routing all remote writes through the ResourceSync/StorageSPI path) so
that snapshots can’t clobber Rust’s canonical state.

---

which fix do you recommend? compare the pros & cons of the fixes that you think of.

---

• Here’s how I’d think about fixes and their trade‑offs.

———

1. Stop snapshot flush from writing the account DB in REMOTE mode (single writer for accounts)

Idea

- In REMOTE storage mode, treat the Rust backend’s RocksDB "account" column family as canonical and ensure there is only one logical writer:
    - Remote execution (Rust) + pre‑exec ResourceSyncService writes.
- Java’s chainbase (SnapshotManager / SnapshotRoot) still maintains snapshots and rollback capability, but its flush/merge does not push account deltas into the remote DB.
- Practically: adjust StorageBackendDB.flush or the merge path so that for dbName == "account" and StorageMode.REMOTE, we either no‑op or only update local caches, not the remote RocksDB.

Pros

- Directly removes the root cause: two unsynchronised writers for the same remote table.
- Conceptually clean: remote execution + ResourceSync define the state of accounts; Java snapshots are an in‑memory overlay and rollback mechanism.
- Avoids subtle, order‑dependent bugs for all accounts, not just the blackhole.

Cons

- You must audit that no purely‑Java account mutations exist that must be persisted to remote but never go through:
    - Remote execution, or
    - ResourceSyncService (pre‑exec flush).
- If such paths exist (e.g., some Java‑only maintenance logic still touching balances/allowances), you either:
    - Need to route them via a ResourceSync‑like path, or
    - Keep a separate, explicit “Java→remote” write path for those cases.
- Changes are cross‑module: chainbase (Snapshot/Flusher) and framework (storage mode) must agree on the semantics.

When I’d choose this

- If you’re ready to treat the Rust backend as the canonical source of account state, and to enforce a single writer for the "account" DB.
- This is the most principled fix for the “flush race” class of bugs.

———

2. Make snapshot flush “forward‑only” (flush current head state, not per‑snapshot values)

Idea

- Keep snapshot flush writing to remote, but never write a value that is older than Java’s current head snapshot.
- Instead of merging raw SnapshotImpl.db entries into a batch and flushing them, compute the batch from the final view for each key:
    - E.g. for each key touched in the snapshots being merged, look up the value as seen at Chainbase.head() and flush that.
- Intuitively: flush is “commit the current state to disk”, not “replay the history of these N snapshots”.

Pros

- Fixes the general problem: snapshot flush cannot overwrite a more recent value in remote DB with an older one, because it always writes the head view.
- Keeps the dual‑storage architecture (remote StorageBackendDB) intact; fewer policy decisions about which DBs are “single writer”.
- Covers all keys, not just blackhole.

Cons

- Requires non‑trivial plumbing:
    - SnapshotRoot.merge(List<Snapshot>) and/or Chainbase.merge(List<Snapshot>) need access to the head snapshot view, not just each snapshot’s local hashdb.
    - For the account DB, you must preserve delete semantics and asset‑optimization logic while recomputing from head.
- More computation at flush time (lots of get calls per key) and more subtle correctness considerations.
- If you get the merge semantics wrong, you risk quiet state drift.

When I’d choose this

- If you want a robust fix that keeps the current dual‑storage design but are willing to do a deeper, careful refactor around snapshot merge semantics.

———

3. Always re‑sync the blackhole account before every remote execution

Idea

- In ResourceSyncService.flushResourceDeltas, ensure the blackhole account is always included in dirtyAccounts in REMOTE mode, even if it wasn’t touched in pre‑exec resource consumption for this transaction.
- Effect: right before every ExecuteTransaction RPC, Java pushes the current blackhole balance from AccountStore to remote RocksDB, overriding whatever the snapshot flush may have written.

Pros

- Very small, surgical change (one place, clear log signal).
- Directly fixes your observed mismatch:
    - Even if a snapshot flush wrote an old blackhole value, the next tx’s pre‑exec sync overwrites it with the Java head value, so Rust’s “BEFORE execution” log matches Java’s Manager log again.
- Low consensus risk: writing the current blackhole state to remote before exec is idempotent and monotonic.

Cons

- This is a symptom‑level fix:
    - The underlying “two writer” problem still exists for other accounts/dynamic properties.
    - It just keeps the one globally‑visible account you care about (blackhole) aligned.
- Slight extra load: one extra account in every pre‑exec batch in REMOTE mode, though that’s negligible compared to a tx execution.

When I’d choose this

- As a short‑term, low‑risk mitigation to stabilise remote vs Java pre‑balance logs while you design and implement a deeper architectural fix (like 1 or 2).
- If you’re primarily debugging parity and need predictable logs now.

———

4. Versioned / height‑aware writes (long‑term, more invasive)

Idea

- Embed a version (e.g. block number or logical timestamp) in account values or in a side metadata table.
- When writing to remote DB, a component only overwrites a key if its version ≥ the existing one.
- Snapshot flushes would include the block height they correspond to; remote exec would always write newer versions.

Pros

- Makes stale writes structurally impossible: even if snapshot flush “replays” an old state, the backend rejects it.
- Works even if multiple writers exist, as long as everyone respects the versioning rule.

Cons

- Requires changes on both Java and Rust sides:
    - How accounts are encoded or how metadata is stored.
    - How StorageEngine / StorageSPI decide to accept/reject writes.
- Harder to roll out on an existing chain without a migration plan.
- Adds complexity to every write path, not just the problematic ones.

When I’d choose this

- As a long‑term hardening step once the basic architecture has stabilised, not as the first fix.

———

My recommendation

- Short term (to fix your immediate discrepancy): implement option 3 — always include the blackhole in ResourceSyncService.flushResourceDeltas in REMOTE mode. It directly fixes the “2131 ok, 2140 jumps back”
behaviour with minimal blast radius and is easy to reason about.
- Medium term (to eliminate the class of bugs): move towards 1 (single writer for account in REMOTE mode), i.e.:
    - Treat Rust’s RocksDB as canonical for accounts.
    - Ensure Java’s snapshot flush never pushes account deltas to remote, and route any Java‑only account mutations through a well‑defined remote‑sync path.
- If you prefer to keep the current dual‑storage structure, invest in 2 (head‑based flush) instead, but that will be more intricate to get right.

If you want, I can sketch what the “always flush blackhole” change would look like in ResourceSyncService.flushResourceDeltas and how to guard it behind a feature flag so you can enable/disable it per
environment.


---

• High‑level goal for option 2

Make snapshot flush “forward‑only” so that when SnapshotManager collapses old snapshots into SnapshotRoot and writes to the underlying DB (which is the remote RocksDB in REMOTE mode), it never writes a value
that is older than Java’s current head snapshot. Concretely, for each key touched by the snapshots being merged, we want to flush the value as seen at Chainbase.head() (the latest logical state), not the
value as it appeared in an older snapshot.

Below is a detailed plan for how to do that safely in this codebase.

———

1. Understand the current snapshot / flush flow

Files:

- chainbase/src/main/java/org/tron/core/db2/core/Chainbase.java
- chainbase/src/main/java/org/tron/core/db2/core/SnapshotRoot.java
- chainbase/src/main/java/org/tron/core/db2/core/SnapshotImpl.java
- chainbase/src/main/java/org/tron/core/db2/core/SnapshotManager.java
- chainbase/src/main/java/org/tron/core/db/StorageBackendDB.java
- framework/src/main/java/org/tron/core/db/TronStoreWithRevoking.java

Key pieces:

- TronStoreWithRevoking constructs revokingDB as a Chainbase over a SnapshotRoot:
    - In dual mode (storage.mode = embedded|remote), db is a StorageBackendDB that wraps a StorageBackendDbSource, which in REMOTE mode is ultimately a StorageSpiBackendAdapter over StorageSPI → gRPC → Rust
    RocksDB.
    - revokingDB = new Chainbase(new SnapshotRoot(this.db));.
- Chainbase manages:
    - SnapshotRoot (the base, persisted view).
    - A chain of SnapshotImpl instances representing deltas (one per ISession / revoking window).
    - get(key) walks from head (latest snapshot) down to root, returning the first value found (SnapshotImpl.get delegates to get(this, key)).
- SnapshotManager:
    - Holds a list of Chainbase dbs (account, properties, etc.).
    - buildSession() creates new SnapshotImpls and advances head, increasing size.
    - When size > maxSize, it:

    flushCount = flushCount + (size - maxSize.get());
    updateSolidity(size - maxSize.get());
    size = maxSize.get();
    flush();
    - flush():

    if (shouldBeRefreshed()) {
        createCheckpoint();
        refresh();   // <-- this is the merge into root
        flushCount = 0;
        logger.info("Flush cost: ..., create checkpoint cost: ..., refresh cost: ...");
    }
    - refresh() iterates over dbs and calls refreshOne(db) on a per‑DB executor.
- SnapshotManager.refreshOne(Chainbase db):

if (Snapshot.isRoot(db.getHead())) return;

List<Snapshot> snapshots = new ArrayList<>();
SnapshotRoot root = (SnapshotRoot) db.getHead().getRoot();
Snapshot next = root;
for (int i = 0; i < flushCount; ++i) {
    next = next.getNext();
    snapshots.add(next);
}

root.merge(snapshots);     // <-- flush/merge logic
root.resetSolidity();
if (db.getHead() == next) {
    db.setHead(root);
} else {
    next.getNext().setPrevious(root);
    root.setNext(next.getNext());
}

So snapshots = [root.next, root.next.next, ..., snapshot_flushCount] (earliest layers after root). After merge, those snapshots are removed from the chain; only later snapshots remain.
- SnapshotRoot.merge(List<Snapshot> snapshots) (current behaviour):

public void merge(List<Snapshot> snapshots) {
    Map<WrappedByteArray, WrappedByteArray> batch = new HashMap<>();
    for (Snapshot snapshot : snapshots) {
    SnapshotImpl from = (SnapshotImpl) snapshot;
    Streams.stream(from.db)
        .map(e -> Maps.immutableEntry(
            WrappedByteArray.of(e.getKey().getBytes()),
            WrappedByteArray.of(e.getValue().getBytes())))
        .forEach(e -> batch.put(e.getKey(), e.getValue()));
    }
    if (needOptAsset()) {
    processAccount(batch);
    } else {
    ((Flusher) db).flush(batch);   // StorageBackendDB.flush
    putCache(batch);
    }
}
    - Later snapshots in the list overwrite earlier ones in batch for the same key.
    - For account DBs (isAccountDB), needOptAsset() true → processAccount(batch) applies asset optimization and then flushes accounts to db.
    - Flusher.flush for remote mode uses StorageBackendDB.flush:

    public void flush(Map<WrappedByteArray, WrappedByteArray> batch) {
        Map<byte[], byte[]> convertedBatch = new HashMap<>();
        batch.forEach((key, value) -> {
        convertedBatch.put(key.getBytes(), value.getBytes());
        });
        dbSource.getStorageBackend().batchPut(convertedBatch); // → remote RocksDB
    }

So in REMOTE mode, SnapshotRoot.merge(snapshots) is writing snapshot‑level values (from early snapshots) directly into the remote "account" / "properties" DBs via batchPut. That’s the second writer that can
overwrite newer state.

———

2. Define the desired semantics for Option 2

We want SnapshotRoot.merge(snapshots) in REMOTE mode to obey:

- For any key k that is affected by the snapshots being merged:
    - Let V_head(k) = value of k according to head (db.getHead().get(k)).
    - Let V_root(k) = value of k according to root (before merge).
    - Let V_remote_before(k) = whatever is currently in remote RocksDB (may have been advanced by ResourceSync and remote execution).
- After merge+flush:
    - The logical chain (root + remaining snapshots) seen by Java head must still reflect the same state as before merge (no semantic change).
    - The write we send to the remote DB for key k must be consistent with V_head(k)—in particular, we must not write a value that predates V_head(k).

In practice:

- For keys that only exist in the early snapshots (no later updates), V_head(k) equals the merged snapshot value; so behaviour is unchanged.
- For keys that have been updated again in later snapshots (e.g. blackhole after resource sync + remote exec), V_head(k) may differ from what snapshots 1..flushCount say. Today we flush the snapshot value; we
want instead to flush V_head(k).

This is what makes flush “forward‑only” with respect to the Java view.

———

3. Design outline for a head‑based merge

We’ll change the semantics of SnapshotRoot.merge(List<Snapshot> snapshots) (or add a new variant) so that it uses the head view for each key.

At a high level:

1. In SnapshotManager.refreshOne(Chainbase db), capture:
    - SnapshotRoot root = (SnapshotRoot) db.getHead().getRoot();
    - Snapshot head = db.getHead();
    - List<Snapshot> snapshotsToMerge = [...] as today.
2. Replace the call root.merge(snapshotsToMerge) with a new API, e.g.:

    root.mergeWithHead(head, snapshotsToMerge);
3. Implement SnapshotRoot.mergeWithHead(Snapshot head, List<Snapshot> snapshots) roughly as:
    - Compute the set of affected keys:

    Set<WrappedByteArray> mergedKeys = new HashSet<>();
    for (Snapshot snapshot : snapshots) {
        SnapshotImpl from = (SnapshotImpl) snapshot;
        Streams.stream(from.db)
            .forEach(e -> mergedKeys.add(WrappedByteArray.of(e.getKey().getBytes())));
    }
    - For each WrappedByteArray key in mergedKeys:
        - Extract byte[] rawKey = key.getBytes();.
        - Compute byte[] headValue = head.get(rawKey);:
            - head.get already walks snapshot chain down to root, so this reflects the latest logical value.
        - Decide what to put in the batch:
            - If headValue == null (key no longer exists at head):
                - For non‑account DBs: we should treat this as delete; we can either:
                    - Put value = null into the batch to trigger deletion via updateByBatch, or
                    - Call db.remove(rawKey) directly before/after the batch flush.
                - For account DBs (needOptAsset() == true), we need to feed the delete into processAccount the same way as today:
                    - batch.put(key, WrappedByteArray.of(new byte[0])), and let processAccount interpret ByteArray.isEmpty(...) as deletion + related asset clean‑up.
            - Else (headValue != null):
                - For all DBs: batch.put(key, WrappedByteArray.of(headValue));.
    - Now flush using the existing logic:

    if (needOptAsset()) {
        processAccount(batch);
    } else {
        ((Flusher) db).flush(batch);
        putCache(batch);
    }
4. After mergeWithHead returns, the rest of refreshOne can stay the same:
    - root.resetSolidity();
    - Fix up the snapshot chain (detach the merged snapshots).
    - db.setHead(...) remains as today.

Key property: even if some of these keys were updated in later snapshots (beyond snapshotsToMerge), we are using head.get(key) so we only ever write the latest logical value for each merged key into root/
remote.

Even if that means root “jumps ahead” of the snapshots being merged for some keys, the semantics remain correct because:

- For keys where later snapshots re‑apply the same value, they become redundant overlays.
- For keys where later snapshots set the same final value, root already has that value; snapshots still contain PUTs/DELETEs, but reading from head will find the topmost snapshot entry, which matches root, so
no behavioural change.

———

4. Detailed implementation steps

Below is the step‑by‑step plan, with the code we’ve just analysed in mind.

Step 1: Add a head‑aware merge API on SnapshotRoot

- In SnapshotRoot:
    - Add a new public method:

    public void mergeWithHead(Snapshot head, List<Snapshot> snapshots) { ... }
    - Internally:
        - Treat head as the current Chainbase.getHead() for this DB.
        - Build Set<WrappedByteArray> mergedKeys by scanning the HashDB of each SnapshotImpl in snapshots (similar to existing merge(List<Snapshot>) but we ignore values there).
        - For each key:
            - Call byte[] headValue = head.get(rawKey); (this uses the existing snapshot chain logic).
            - If headValue == null:
                - If needOptAsset():
                    - batch.put(key, WrappedByteArray.of(new byte[0])); – same pattern as existing processAccount deletion handling.
                - Else:
                    - We need a way to represent deletion to the underlying Flusher. For LevelDB/RocksDB, updateByBatch treats value == null as delete (RocksDbDataSourceImpl.updateByBatchInner). Right now
                    SnapshotRoot.merge(List<Snapshot>) never passes null, it always passes WrappedByteArray; so we may either:
                        - Allow batch to contain value = null for this key (and adjust Flusher to interpret that as deletion), or
                        - Call db.remove(rawKey) directly outside of the batch for non‑account DBs.
            - If headValue != null:
                - batch.put(key, WrappedByteArray.of(headValue));.
        - Then call the existing processAccount / ((Flusher) db).flush logic on that batch.
    - Keep the existing merge(List<Snapshot> snapshots) API for:
        - Non‑refresh use‑cases (e.g. checkpoint recovery) where using the snapshot values is acceptable or needed.
        - Gradual rollout: we can later redirect those call sites to the head‑aware variant if safe.

Step 2: Wire SnapshotManager.refreshOne to use the head‑aware merge

- In SnapshotManager.refreshOne(Chainbase db):
    - After computing root and snapshots:

    Snapshot head = db.getHead();
    root.mergeWithHead(head, snapshots);
    - Replace the old root.merge(snapshots) call with this.
- Ensure that flushCount and chain rewiring logic remain unchanged.

The merged snapshots are still removed from the chain; the difference is only which value for each key is written into root/remote.

Step 3: Handle deletion semantics carefully

- For account DB (“account”) with asset optimization:
    - processAccount(batch) already knows how to interpret an “empty value” as deletion:
        - It checks ByteArray.isEmpty(v.getBytes()) to decide if the account is being deleted and handles asset clean‑up accordingly.
        - It then calls ((Flusher) db).flush(accounts).
    - So for “head says this account no longer exists”, we can represent that as:

    batch.put(key, WrappedByteArray.of(new byte[0]));
    - This keeps behaviour consistent.
- For non‑account DBs:
    - Check how deletion is currently represented. For LevelDB & RocksDB via updateByBatch:
        - RocksDbDataSourceImpl.updateByBatchInner deletes when entry.getValue() == null.
        - Currently SnapshotRoot.merge(List<Snapshot>) never passes null (always WrappedByteArray), so deletes are represented as Value.Operator.DELETE in SnapshotImpl.db, not as null values in SnapshotRoot’s
        batch.
    - For our head‑based merge we have two options:
        - Option A: For “head says key is gone”, directly call db.remove(rawKey) for that key (bypassing the batch).
            - Simpler to reason about; uses existing DB.remove, which is already implemented for all backends (local and remote via StorageBackendDbSource.deleteData).
        - Option B: Allow batch to contain a special WrappedByteArray representing a delete, and teach Flusher.flush / StorageBackendDB.flush how to convert that to null and thus deletion.
    - For a first iteration, Option A is usually simpler and less invasive:
        - Collect a Set<byte[]> deletes while iterating keys where headValue == null.
        - After the main flush, call db.remove(k) for each k in deletes.
        - Be mindful that for account DBs you also have to call AccountAssetStore delete logic; but that is already handled by processAccount, so we don’t use db.remove there.

Step 4: Keep recovery / checkpoint semantics separate

- SnapshotManager.recover(TronDatabase<byte[]> tronDatabase) (at the bottom of SnapshotManager) uses:

dbs.forEach(db -> db.getHead().getRoot().merge(db.getHead()));

This calls SnapshotRoot.merge(Snapshot from) (single snapshot) rather than the multi‑snapshot merge.
- For recovery, we are replaying checkpointed data into an empty in‑memory chain at startup. In that scenario:
    - Remote DB and Java in‑memory state should be reconstructed from the checkpoint; there is no concurrent writer.
    - The current merge(Snapshot from) (snapshot‑based) semantics are fine.
- Plan:
    - Leave SnapshotRoot.merge(Snapshot from) untouched.
    - Only change merge(List<Snapshot> snapshots) / mergeWithHead used by flush/refresh.

Step 5: Condition behaviour on storage mode if desired

- Optionally, we can gate the head‑based merge behind a condition:
    - For example, only use mergeWithHead when the underlying db is a StorageBackendDB configured in REMOTE mode; otherwise use the old merge semantics.
- This can be done by:
    - Adding a helper on SnapshotRoot:

    private boolean isRemoteBackend() {
        return db instanceof StorageBackendDB &&
                CommonParameter.getInstance().getStorage().getStorageMode().equalsIgnoreCase("remote");
    }
    - In SnapshotManager.refreshOne, choose:

    if (isRemoteBackend(root)) {
        root.mergeWithHead(head, snapshots);
    } else {
        root.merge(snapshots);
    }
- This reduces risk for embedded / legacy setups while you validate the behaviour in REMOTE mode.

———

5. Testing and validation strategy

To be confident in Option 2, you’ll want both targeted unit tests and end‑to‑end behaviour checks.

Unit / integration tests in chainbase

- Add tests for SnapshotRoot.mergeWithHead in a new test class (e.g. SnapshotRootForwardMergeTest):
    1. Simple forward merge (no later snapshots)
        - root: x=0
        - snapshot1: x=1
        - head = snapshot1
        - snapshots = [snapshot1]
        - After mergeWithHead, assert:
            - root.get(x) == 1
            - No snapshots remain
            - Underlying DB received x=1.
    2. Later snapshot override
        - root: x=0
        - snapshot1: x=1
        - snapshot2: x=2
        - head = snapshot2
        - flushCount=1 → snapshots=[snapshot1]
        - After mergeWithHead:
            - root.get(x) == 2 (we want head’s value, not 1)
            - snapshot2 still exists, and head.get(x) == 2
            - Underlying DB ends up with x=2, not x=1.
    3. Delete + re‑add cases: combinations of PUT/DELETE across snapshots, verifying root+snapshots still read correctly and flush uses head view.
    4. Account DB with needOptAsset(): verify that deletions still trigger the correct asset store updates and that values are asset‑optimized.

End‑to‑end regression for blackhole case

- Re‑run the scenario that exposed the bug:
    - Build a short chain where:
        - At block B0, blackhole = B₀.
        - Later transactions (pre‑exec sync + remote exec + state sync) advance blackhole to B₁.
        - Snapshot flush is triggered with flushCount > 0 and includes older snapshots with the blackhole key.
        - A later tx at block B2 does not include blackhole in ResourceSync (includes_blackhole=false).
    - With the new head‑based merge, assert:
        - Java’s Manager logs before=B₁.
        - Rust logs Blackhole balance BEFORE execution: B₁, not B₀.
- Also verify:
    - For other accounts touched only in early snapshots (no later updates), remote still matches Java.

———

6. Pitfalls and things to watch carefully

- Performance: head‑based merge will call head.get(key) for every key in the merged snapshots.
    - These are in‑memory operations over SnapshotImpl + root; they do not hit the remote DB (they go through revokingDB).
    - Still, flush may be called with many keys; you may want basic benchmarking and perhaps limit mergedKeys cardinality if needed.
- Deletion semantics for non‑account DBs:
    - Today, deletes for DBs other than “account” come from SnapshotImpl’s Value.Operator.DELETE, not from null values.
    - When we switch to head‑based merge, we must ensure we still issue proper deletes, either via db.remove(key) or by mapping to null values in the batch.
- Redundant snapshots:
    - After head‑based merge, some later snapshots may contain PUTs that set the same value root already has.
    - This is semantically harmless but can leave some “dead” entries in those snapshots until they’re eventually merged; not a correctness issue, but something to be aware of.
- Behaviour difference in embedded mode:
    - In purely embedded LevelDB/RocksDB mode, merging snapshots has historically been the only way to persist incremental changes to disk.
    - Using head‑based values for flush is still logically correct (you’re essentially persisting the current head state), but you are changing which intermediate states get persisted.
    - Gating this behaviour to REMOTE mode first, and validating, is safer.
- Checkpoint / recovery:
    - Keep SnapshotRoot.merge(Snapshot from) semantics unchanged for checkpoint recovery, so startup behaviour doesn’t change.
    - Only the steady‑state flush (merge(List<Snapshot>) invoked from refreshOne) should switch to the head‑based semantics.

———

Summary

Option 2 can be implemented by making SnapshotRoot.merge(List<Snapshot>) head‑aware:

- When SnapshotManager flushes old snapshots into root, it should, for each key touched by those snapshots, write the value as seen at db.getHead() (Java’s latest view), not the raw snapshot values.
- That ensures flush is “forward‑only” from Java’s perspective and prevents a stale snapshot from overwriting a newer remote state (as happened with the blackhole account).
- The core mechanical change is localised to SnapshotManager.refreshOne and SnapshotRoot.merge(List<Snapshot>), with careful handling of deletions and asset optimization for the account DB.

If you’d like, I can next sketch concrete pseudo‑code for mergeWithHead including the delete handling and a small example showing how it behaves on the exact blackhole race you hit.