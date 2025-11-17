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