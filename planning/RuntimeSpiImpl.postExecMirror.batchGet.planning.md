• Here’s a concrete plan to refactor Phase B mirror (RuntimeSpiImpl.postExecMirror) from per-key get() to per-db batchGet(), with the gotchas in this repo called out.                                            
                                                                                                                                                                                                                  
  ———                                                                                                                                                                                                             

  ## 0) What Phase B mirror is doing today (and why it’s expensive)

  In framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java the postExecMirror(...) path runs when write_mode=PERSISTED:

  - For every tx, it gets result.getTouchedKeys()
  - Groups by dbName
  - For each touched key:
      - If delete: store.delete(key)
      - Else: storageSPI.get(dbName, key).get() (gRPC call) then store.putRawBytes(key, value) or delete if missing

  This is 1 gRPC read per touched key, and your logs show most txs have 2–3 touched keys → 2–3 gRPC reads per tx.

  ———

  ## 1) Goal

  Reduce remote reads from:

  - O(#touched keys) gRPC calls per tx

  to:

  - O(#touched DBs) gRPC calls per tx (usually 1), using StorageSPI.batchGet(dbName, keys).

  ———

  ## 2) Key API + correctness constraints (important)

  ### 2.1 StorageSPI.batchGet returns Map<byte[], byte[]>

  This is tricky because byte[] keys in a HashMap are reference-identity keys.

  - EmbeddedStorageSPI.batchGet uses the same input key arrays as map keys → lookups via the same key references work.
  - RemoteStorageSPI.batchGet currently builds the map using kv.getKey().toByteArray() → new arrays, so result.get(originalKey) will NOT work.

  So your mirror implementation must not assume map.get(originalKey) works unless you also fix RemoteStorageSPI.batchGet.

  ### 2.2 Backend “success=false” must not be treated as “not found”

  In backend.proto, BatchGetResponse has success + error_message.
  Rust returns success=false with empty pairs on failure.
  RemoteStorageSPI.batchGet currently ignores success, which could cause mirror to interpret “empty response” as “delete everything”.

  Any plan must include: treat success=false as an exception, and either fallback or abort mirror for that tx/db.

  ———

  ## 3) Implementation plan (Java-side mirror), step-by-step

  ### Step A — Normalize + dedupe touched keys per DB (last-writer-wins)

  In postExecMirror before calling storage:

  1. Build Map<String, Map<KeyId, Boolean isDelete>> where KeyId is content-based (not byte[] identity).
      - Use org.tron.core.db.ByteArrayWrapper as KeyId.
      - Iterate touchedKeys in list order; for each (db, key, isDelete) do perDb.put(new ByteArrayWrapper(key), isDelete).
      - This makes duplicates cheap and resolves mixed delete/update by “last occurrence wins”.
  2. Also keep Map<String, Map<ByteArrayWrapper, byte[]>> that stores the actual byte[] to apply (use the latest occurrence’s byte[] so you’re consistent).

  Result: per DB you now have a unique set of keys and final ops.

  ### Step B — For each DB: apply deletes locally, batchGet the rest

  For each (dbName -> keyOps):

  1. Resolve store = getStoreByDbName(dbName, chainBaseManager).
      - If null: log once per dbName (or throttle), add errorCount += keyOps.size(), continue.
  2. Split keys into:
      - deleteKeys: isDelete=true → apply store.delete(keyBytes) immediately (no remote read)
      - readKeys: isDelete=false → need remote read
  3. For readKeys, call storageSPI.batchGet(dbName, readKeysAsByteArrays) in chunks (see Step C).
  4. For each key in readKeys:
      - If remote returned value: store.putRawBytes(key, value)
      - If remote returned null/not found: store.delete(key) (same behavior as current per-key get path)

  ### Step C — Chunking policy (avoid giant gRPC messages)

  Add a constant in RuntimeSpiImpl (or a helper) like:

  - MAX_BATCH_KEYS = 256 (conservative; your observed sizes are tiny anyway)

  Then:

  - Split readKeys into chunks of up to MAX_BATCH_KEYS
  - Call batchGet per chunk
  - Merge results

  (If later you see RESOURCE_EXHAUSTED / “message too large”, you can shrink chunk size dynamically.)

  ### Step D — Map lookup strategy (choose one of two approaches)

  #### Option 1 (local change only): wrap returned keys for lookup

  Don’t rely on Map<byte[], byte[]> key identity. Instead:

  - Convert each batchGet result map into Map<ByteArrayWrapper, byte[]> by iterating entries and wrapping the entry key.
  - Then for each requested key wrapper: lookup by ByteArrayWrapper.

  Pros: doesn’t require changing RemoteStorageSPI.
  Cons: extra wrapping + hashing (small overhead).

  #### Option 2 (recommended for consistency): fix RemoteStorageSPI.batchGet to preserve input key identity

  Change RemoteStorageSPI.batchGet to return a map keyed by the original input byte[] objects (matching EmbeddedStorageSPI behavior), by iterating response pairs in-order and doing result.put(keys.get(i),
  valueOrNull).

  Also: check response.getSuccess() and throw if false.

  Pros: fastest + simplest for all callers; mirror can do value = map.get(keyBytes).
  Cons: touches RemoteStorageSPI, not just mirror.

  I’d do Option 2 + keep Option 1 fallback (verify ordering; if response length mismatch or keys don’t match, fallback to wrapper map).

  ### Step E — Error handling + fallback strategy

  For each db chunk batchGet:

  - If gRPC throws OR success=false:
      - Log a single WARN (include dbName + chunk size + tx id)
      - Fallback choices:
          1. Fallback to per-key get for that chunk (keeps correctness, slower but rare)
          2. Or skip mirroring for that db/tx and count errors (risky: you may reintroduce overwrite bugs)

  I’d pick (1) to preserve correctness.                                                                                                                                                                           
                                                                                                                                                                                                                  
  ———                                                                                                                                                                                                             

  ## 4) Rust-side follow-up (optional but high value)

  Even after Java batches, Rust engine.batch_get currently loops db.get(key) per key. That’s better than many gRPC calls, but you can push further:

  - Replace loop with RocksDB multi_get (or equivalent) to reduce internal overhead.
  - Keep response order identical to request order (so Java Option 2 is safe long-term).

  This is a separate PR, but it multiplies the benefit of Java batching.

  ———

  ## 5) Testing plan (so you don’t ship a “delete everything” bug)

  ### Unit tests (Java)

  Create tests around a new helper (recommended) rather than private postExecMirror:

  1. Extract the core logic into a package-private helper class, e.g. PhaseBMirror (same package), with a method:
      - mirror(StorageSPI storageSPI, Map<String, List<TouchedKey>> touched, ChainBaseManager cbm)
  2. Mock StorageSPI and TronStoreWithRevoking:
      - Verify batchGet called once per db (or per chunk)
      - Verify deletes never call storage
      - Verify “not found” turns into store.delete
  3. Add a regression test for RemoteStorageSPI semantics:
      - Ensure batchGet throws when backend returns success=false

  ### Integration check

  Run your existing script and compare:

  - Remote progress in 30 minutes (block height, tx rows)
  - gRPC call volume (optional: add counters/logging around mirror batchGet count)

  ———

  ## 6) Expected payoff

  Given your observed distribution (mostly 2–3 touched keys), this change should reduce mirror read RPCs roughly ~2–3× for the common case, and remove per-key blocking latency inside the tx loop. If mirror
  reads are a big slice of tx handling, you should see a noticeable block/tx throughput bump.

  If you want, I can next produce a “design sketch” pseudocode for the refactor (still no implementation), including the exact data structures and where they’d live.