• Here’s the first divergence and why it happens.

First Mismatch

- Tx: f0b6b6afc8bb55f4b1d8b4084f33095eabb2a5f6761de21e7609907c672c1a9a
- Block: 3189, index 0, WitnessCreateContract
- Embedded row: output-directory/execution-csv/20251030-055539-b3bf2f9c-embedded-embedded.csv:1125
- Remote row: output-directory/execution-csv/20251031-022223-a880835b-remote-remote.csv:1125
- Difference: Account 4177944d19c052b73ee2286823aa83f8138cb7032f (blackhole) oldValue/newValue
    - Embedded oldValue → newValue: 0x8000000be7406f80 → 0x8000000e3b3d1140
    - Remote oldValue → newValue: 0x8000000baa376f80 → 0x8000000dfe341140
    - The remote oldValue is smaller by exactly 1,024,000,000 SUN (0x000000003fa00000)

Immediate Preceding Tx

- Block 3188, index 0: AssetIssueContract
- Remote CSV: output-directory/execution-csv/20251031-022223-a880835b-remote-remote.csv:1124
    - Blackhole oldValue → newValue: 0x8000000baa376f80 → 0x8000000be7406f80 (+1,024,000,000 SUN)
- Embedded CSV: output-directory/execution-csv/20251030-055539-b3bf2f9c-embedded-embedded.csv:1124
    - Same blackhole delta, same newValue.

So the embedded WitnessCreate correctly reads blackhole’s post-AssetIssue balance as its starting oldValue; the remote WitnessCreate reads the pre-AssetIssue balance.

Corroborating Logs

- Rust (remote backend) shows AssetIssue shadows the TRC‑10 fee into the overlay only (not DB):

This proves remote execution of the next block (3189) used the pre‑AssetIssue state for blackhole.

Root Cause
- AssetIssue shadow deltas were applied to the overlay for block 3188 (not persisted by the backend).
- Java correctly applies the TRC‑10 deltas to storage after the AssetIssue call, but the remote backend’s next call (block 3189) read blackhole from its own storage view that did not yet include that
In short: shadow TRC‑10 overlay is block‑scoped and the storage view used by remote execution for the next block isn’t seeing the Java-applied TRC‑10 update in time, so oldValue is stale for the blackhole
entry.

What To Fix

    - Rust side: consider persisting AssetIssue “shadow” deltas to the overlay only for the current block (as now), but also make sure new block reads see Java’s applied state. If propagation latency is
    expected, a pragmatic mitigation is to “seed” the next‑block overlay with the prior block’s overlay contents for addresses touched by shadow TRC‑10 (until a proper write‑through guarantee is in place).
- CSV‑only workaround (if you need immediate parity): adjust CSV building for non‑VM fees (like WitnessCreate) to recompute blackhole old/new from the AccountStore after TRC‑10 application. This keeps CSVs
consistent but doesn’t fix execution ordering.

- Turn on debug for RuntimeSpiImpl.applyTrc10AssetIssue to log the concrete backend/store implementation used.
- In the rust backend, enable storage engine debug to confirm Java’s TRC‑10 writes arrive before the 3189 WitnessCreate call. Look for “Verified account write … TLsV52sR…” near 02:24:39.156; it’s absent until
~02:24:39.217, which aligns with the stale read.


---

think harder.

No need to start implementing, just read through and understand the codebase and give me a detailed plan on:

- Ensure remote execution reads the storage state that reflects Java‑applied TRC‑10 updates before executing the next block:
  - Java side: confirm that in remote storage mode, applyTrc10LedgerChanges writes via the remote AccountStore (not an embedded store), and is synchronous. We see it logs success, but the remote backend still read stale state, which implies the write didn’t hit the backend’s storage view before the next call.
  - Rust side: consider persisting AssetIssue “shadow” deltas to the overlay only for the current block (as now), but also make sure new block reads see Java’s applied state. If propagation latency is expected, a pragmatic mitigation is to “seed” the next‑block overlay with the prior block’s overlay contents for addresses touched by shadow TRC‑10 (until a proper write‑through guarantee is in place).

---


• Goal

- Remote execution reads must see Java‑applied TRC‑10 effects (e.g., AssetIssue fee credit to blackhole) before executing the next block, so CSV oldValue/newValue match embedded.

Why It’s Stale

- Java applies TRC‑10 after execution in RuntimeSpiImpl.applyTrc10LedgerChanges(...), writing via revoking DB (snapshots) that flush later at block checkpoints, not immediately.
- Remote backend starts the next block before those revoking snapshot mutations are flushed to remote storage; it reads the pre‑AssetIssue state.
- Evidence: AssetIssue (block 3188) credits blackhole; WitnessCreate (block 3189) remote reads blackhole oldValue that’s 1,024,000,000 SUN lower than embedded.

Java‑Side Plan

- Confirm remote storage path and sync semantics
    - Chainbase stores are backed by StorageBackendDB when storage.mode=REMOTE: chainbase/src/main/java/org/tron/core/db/TronStoreWithRevoking.java:53 and chainbase/src/main/java/org/tron/core/db/
    TronDatabase.java:33.
    - Storage backend uses StorageSpiBackendAdapter over RemoteStorageSPI, all calls block on .get(): framework/src/main/java/org/tron/core/storage/spi/StorageSpiBackendAdapter.java:31, framework/src/main/
    java/org/tron/core/storage/spi/RemoteStorageSPI.java:120.
- Mark and flush TRC‑10 mutations immediately after apply
    - In framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:518 (applyTrc10AssetIssue):
        - After accountStore.put(ownerAddress, ownerAccount) and any blackhole credit, call:
            - org.tron.core.storage.sync.ResourceSyncContext.recordAccountDirty(ownerAddress).
            - recordAccountDirty(blackholeAddress) if not burning.
            - recordDynamicKeyDirty("BURN_TRX_AMOUNT".getBytes()) when burning.
        - Immediately flush these to remote storage in‑band with the current tx, before returning:
            - Reuse existing service: ResourceSyncContext.flushPreExec() flushes the thread‑local dirty sets to StorageSPI.batchWrite(...) in deterministic order: framework/src/main/java/org/tron/core/
            storage/sync/ResourceSyncService.java:183.
            - If we want separation from pre‑exec flush, add a guarded post‑exec hook (e.g., remote.resource.sync.postexec=true) and call a small helper that invokes flushResourceDeltas(ctx, ...) with only
            TRC‑10 keys collected in this method.
    - Mirror for ParticipateAssetIssue in framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:675 (issuer/owner TRX movements): record dirty owner/issuer and flush.
- Ordering and safety
    - The Manager pipeline already calls ResourceSyncContext.begin(ctx) before execution and flushPreExec() before VM: framework/src/main/java/org/tron/core/db/Manager.java:1556.
    - Add a second, conditional flush right after applyTrc10LedgerChanges(...) returns in execute(...) so all TRC‑10 DB deltas are visible before the next tx is scheduled:
        - Site: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:219 (after “Successfully applied TRC-10 ledger changes…”), call ResourceSyncContext.flushPreExec() and then
        ResourceSyncContext.finish() at method end as today.
    - Gate via property -Dremote.resource.sync.enabled=true (already honored) and add -Dremote.resource.sync.postexec=true to isolate this behavior if needed.
- Observability
    - Add a one‑line INFO when post‑exec flush runs with counts and txId (similar to existing debug in ResourceSyncService).
    - Optional confirmation reads (remote.resource.sync.confirm=true) already exist; enable for first diagnosis.

Rust‑Side Plan (Hardening)

- Maintain overlay behavior but ensure new block reads are up‑to‑date
    - The per‑block overlay resets at block boundary: rust-backend/crates/core/src/service/mod.rs:104 via get_or_create_overlay(...) in gRPC path rust-backend/crates/core/src/service/grpc/mod.rs:1035.
    - If Java post‑exec flush is temporarily disabled or slow, seed next block’s overlay with shadow TRC‑10 effects from the prior block:
        - On new block creation, copy these addresses from the previous overlay into the new overlay iff the underlying storage still returns the pre‑delta value (simple compare) to avoid double application.
        - Guard with remote.overlay.seed_shadow_trc10=true in config, default off once Java post‑exec flush is proven.
- Add a light “read‑your‑writes” barrier
    - Expose a storage “sync”/barrier on the backend: the existing gRPC server for storage already applies writes synchronously to the engine. If there is an internal cache, add an explicit “flush engine
    view” or “reopen snapshot” at block boundary in storage engine and call it from get_or_create_overlay.
    - Alternatively, for the execution service, open a fresh engine snapshot per block to ensure visibility of last committed state.
- Diagnostics
    - Log block‑boundary balance for blackhole when executing AssetIssue/WitnessCreate (“pre‑read from storage: balance=X”) to catch pre/post parity and accelerate regression checks.

Verification

- Reproduce 3188→3189 sequence:
    - Before change: remote CSV row for block 3189 shows blackhole oldValue 0x8000000baa376f80; embedded shows 0x8000000be7406f80.
    - After change: remote CSV oldValue must match embedded (both 0x8000000be7406f80) and state digest identical.
- Additional checks
    - TRC‑10 Participate cases in same/next block pairs (owner debit, issuer credit).
    - Burn vs blackhole modes (dynamic property supportBlackHoleOptimization).
- Performance and safety
    - Post‑exec flush batches only the keys mutated; uses existing batchWrite asynchronous interface, awaited via .get() in the adapter, so it is synchronous from Java’s perspective.
    - Keep failures non‑fatal (service already has a circuit breaker and logs); if flush fails, execution continues but logs warn.

Decision Points / Flags

- remote.resource.sync.enabled (default true in remote mode) controls sync feature generally.
- Introduce remote.resource.sync.postexec to isolate post‑exec TRC‑10 flush path during rollout.
- remote.overlay.seed_shadow_trc10 guard on the Rust overlay seeding fallback.

Where To Touch

- Java:
    - framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:410 (applyTrc10LedgerChanges entry) and :518 (applyTrc10AssetIssue) — record dirty keys; mirror in :675 Participate.
    - framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:219 — call a post‑exec flush before returning from execute(...).
    - Optional: small helper in framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java to flush “post‑exec” deltas with context.
- Rust:
    - Overlay seeding list: collect at rust-backend/crates/core/src/service/mod.rs:1624 (AssetIssue) and :1856 (Participate).
    - Seed in new overlay at rust-backend/crates/core/src/service/grpc/mod.rs:1035 after get_or_create_overlay(context) and before any reads for the first tx of the new block, guarded by config.
    - If needed, add storage “sync”/flush semantics in the storage module and invoke from get_or_create_overlay.

This approach keeps execution semantics unchanged, ensures remote reads see Java‑applied TRC‑10 updates across blocks, and provides a robust fallback on the backend side until read‑your‑write is guaranteed.


---