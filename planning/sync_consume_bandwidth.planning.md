Think harder.

there're non-EVM, Java-side resource/bandwidth deduction that the Rust backend never saw.
we need to Sync Java-side resource updates to remote storage: After Java applies non-EVM resource/bandwidth/fee mutations locally, write those deltas to remote storage so the Rust backend sees the same “old” next time.

give me a detailed plan. no need to start implementing.

---

Here’s a concrete, end‑to‑end plan to sync Java-side resource/bandwidth/fee mutations to the remote Rust storage so the backend always sees the same “old” next time.

  Goal

  - Ensure every non‑EVM resource mutation that Java applies (bandwidth, free‑net, energy windows/usage, memo/multi‑sig fees, burn/pool, public net usage/time, dynamic price history counters, etc.) is visible to the Rust backend’s storage
  view before any subsequent remote execution or state query.

  Key Observations

  - Stores (e.g., AccountStore, DynamicPropertiesStore) in remote storage mode already write via StorageBackendDB → StorageBackendDbSource → StorageSpiBackendAdapter → RemoteStorageSPI gRPC to the Rust backend. So data persistence is “there”.
  - The mismatch is timing/visibility: Java mutates state pre‑exec (e.g., Manager.consumeBandwidth, consumeMemoFee, consumeMultiSignFee, BandwidthProcessor, EnergyProcessor, native resource processors). Rust execution may read stale state if:
      - It uses a cached/snapshotted view not refreshed after Java mutations, or
      - Java applies resource deltas outside the per‑transaction journaling currently used for execution-state reporting.
  - Current journaling is initialized after pre‑exec resource mutations (Manager.processTransaction calls consumeBandwidth/fees before StateChangeJournalRegistry.initializeForCurrentTransaction()), so we don’t capture those changes for
  coordination, metrics, or batch application.

  What to Sync

  - Account-level resource fields:
      - Bandwidth: netUsage, freeNetUsage, latestConsumeTime, latestConsumeFreeTime, latestOperationTime, asset free-net usage maps (v1/v2), issuer account netUsage for TRC‑10 paths.
      - Energy: energyUsage, latestConsumeTimeForEnergy, windowSize/windowSizeV2 as updated in V2 logic, “merged” usage updates during VM setup for freeze-v2 paths.
      - Balances: fee deductions (memo/multi-sign/creation), blackhole crediting, fee pool updates.
      - New account creations triggered by non‑VM contracts (transfer→create, etc.).
  - Dynamic properties:
      - publicNetUsage, publicNetTime
      - TOTAL_TRANSACTION_COST, TOTAL_CREATE_ACCOUNT_COST
      - BLOCK_ENERGY_USAGE, TOTAL_ENERGY_AVERAGE_USAGE, TOTAL_ENERGY_AVERAGE_TIME, and adaptive limit params when they’re changed in-block
      - TRANSACTION_FEE_POOL, BURN_TRX_AMOUNT
      - Bandwidth/Energy price histories if updated by proposals or maintenance steps

  Phased Plan

  - Phase 1: Map Mutation Points (inventory and confirm)
      - Bandwidth: chainbase/.../BandwidthProcessor.consume() → AccountStore.put(), DynamicPropertiesStore.savePublicNetUsage/savePublicNetTime, fee fallback path → consumeFeeForBandwidth.
      - Energy: EnergyProcessor.useEnergy(), VM pre‑exec adjustments in VMActuator.getAccountEnergyLimit*() and getTotalEnergyLimit*() when V2 windows are used → AccountStore.put(), DynamicPropertiesStore.saveBlockEnergyUsage.
      - Fees: Manager.consumeMemoFee, Manager.consumeMultiSignFee → Commons.adjustBalance, and then either DynamicPropertiesStore.burnTrx() or blackhole account credit.
      - Delegate/Undelegate: native processors adjust account windows/usage and persist.
      - Block-level: start-of-block saveBlockEnergyUsage(0) and post-block updateTotalEnergyAverageUsage()/updateAdaptiveTotalEnergyLimit().
      - Block-level: start-of-block saveBlockEnergyUsage(0) and post-block updateTotalEnergyAverageUsage()/updateAdaptiveTotalEnergyLimit().
  -
  Phase 2: Delta Schema and API
      - Define ResourceDelta: groups by DB:
      - accounts: list of `address` keys that have changed
      - dynamicProps: list of keys that have changed
      - assetIssue/assetIssueV2: if public free-asset net usage changed
  - API:
      - `ResourceSyncContext.begin(TransactionContext)`
      - `recordAccountDirty(byte[] address)`
      - `recordDynamicKeyDirty(byte[] key)` (keys are the byte[] used by `DynamicPropertiesStore`)
      - `flushPreExec()` to apply/batch-write/confirm visibility before remote exec
      - `finish()` end-of-tx clean up
  -
  Internals: when recording dirties, do not serialize in hot path; on flush, read from stores and build batches per DB.
  -
  Phase 3: Implement Sync Service (framework)
      - Add org.tron.core.storage.sync.ResourceSyncContext (thread-local, like StateChangeJournalRegistry).
      - Add ResourceSyncService to:
      - Resolve DB names: “account”, “properties”, “asset-issue”, “asset-issue-v2”
      - Build `Map<byte[], byte[]>` per DB from latest store values
      - Use `StorageSPI.batchWrite(dbName, ops)` to minimize gRPC round-trips
      - Optionally offer a “confirm read-back” for debugging/metrics (guarded by flag)
  - Add config flags:
      - `-Dremote.resource.sync.enabled=true` (default true when storage mode is REMOTE)
      - `-Dremote.resource.sync.debug=false` for extra logs
      - `-Dremote.resource.sync.confirm=false` to optionally batchGet keys post-flush for diagnostics

  - Phase 4: Instrument Hotspots (minimal, centralized hooks)
      - Manager.processTransaction:
      - Move `ResourceSyncContext.begin()` to before `consumeBandwidth/fees`.
      - After `consumeBandwidth/fees` and before `trace.exec()`:
        - `ResourceSyncService.flushPreExec()` so Rust sees latest account/dynamic mutations.
  - Manager.consumeMemoFee/consumeMultiSignFee:
      - After adjusting balances/burn/pool, call `recordAccountDirty(owner)`; if burn/pool changed, mark dynamic props dirty (e.g., `BURN_TRX_AMOUNT`, `TRANSACTION_FEE_POOL`).
  - BandwidthProcessor.consume path:
      - After each `accountStore.put(...)` for owner/issuer, call `recordAccountDirty(address)`.
      - After `DynamicPropertiesStore.savePublicNetUsage/Time` and `addTotalTransactionCost`, mark dynamic props keys dirty.
      - For token free-net usage updates, mark issuer and sender accounts dirty; mark affected asset issue key dirty (public free-asset net usage).
  - EnergyProcessor.useEnergy:
      - After `accountStore.put(...)`, mark account dirty.
      - After `saveBlockEnergyUsage`, mark dynamic key dirty.
  - VMActuator energy pre-merge (V2 windows):
      - After `rootRepository.updateAccount()`, mark creator/caller dirty as applicable.
  - Native Delegate/Undelegate processors:
      - After ownership/receiver window/usage updates, mark both accounts dirty.
  - Post‑exec:
      - No action needed for remote‑returned EVM changes (they’re applied by `RuntimeSpiImpl.applyStateChangesToLocalDatabase`). Resource deltas are Java‑driven and were flushed pre‑exec.

  - Phase 5: Ordering and Consistency
      - Ensure flush order:
      - Always flush pre‑exec account changes first, then dynamic props.
      - For token free-asset bandwidth, flush `asset-issue(-v2)` first (issuer/public fields), then account changes, then dynamic props.
  - Atomicity:
      - Current SPI doesn’t attach `transaction_id` to writes; we will rely on synchronous per‑put/batch consistency at remote side, and ordering before remote exec.
      - Keep an extension point for future: add `transaction_id` parameters to StorageSPI operations; wire RemoteStorageSPI to use `transaction_id` in gRPC when added.

  - Phase 6: Feature Flags, Metrics, and Logging
      - Flags:
      - Enable sync by default in remote storage mode only (`StorageSpiFactory.determineStorageMode()==REMOTE`).
  - Metrics:
      - Counters for “resource_sync.preexec.flush.count”
      - Timers for per‑flush latency
      - Gauges: number of keys batched per DB
  - Logs:
      - Debug log one line per tx summarizing: accounts N, dynamic keys M, assets K

  - Phase 7: Tests (dual-mode)
      - Unit tests:
      - ResourceSyncContext: records + flush with mocked SPI writes per DB
      - BandwidthProcessorTest/EnergyProcessorTest: enable remote mode flag; assert `recordAccountDirty` invocations and flush constructs final batch
  - Integration tests (Java-only wiring, no Rust network required in CI):
      - DualStorageModeIntegrationTest:
        - Set `STORAGE_MODE=remote` but use `EmbeddedStorageSPI` in test context as a stand-in; verify batches are applied and remote side “sees” correct values by reading via SPI.
        - Flow: submit tx that triggers bandwidth fee + memo fee; assert account balances and dynamic props pre‑exec flush match subsequent reads via SPI before executing a no-op remote call.
      - For EVM call path: pre‑merge energy usage V2 updates appear in SPI before remote execution is invoked.
  - Manual test plan (with Rust backend):
      - Start Rust `tron-backend`, run `:framework:test` focused on remote path or local integration runner; compare “old” resource snapshots across sequential txs by enabling backend debug logs.

  - Phase 8: Documentation
      - Document which keys/DBs get synced, order guarantees, and flags in docs/ or build.md.
      - Note limitations (no cross‑DB gRPC transaction binding yet; all flushes are best‑effort synchronous).
      - How to diagnose mismatches with remote-java.log and backend’s metrics.

  Design Notes and Rationale

  - Minimal invasive hooks: Centralize in Manager pre‑exec and in a handful of processors where changes are applied to stores. Avoid heavy store‑level interception (no wrapping of TronStoreWithRevoking).
  - Batch writes: Reduce gRPC calls; grouping by DB per transaction minimizes overhead.
  - Ordering: Asset/public usage before accounts before props prevents transient reads seeing a public limit without corresponding account usage updates (or vice versa).
  - Journaling separation: Keep EVM journaling for remote→Java sync distinct; this new pre‑exec “resource delta” path is Java→remote and must occur before remote execution.
  - Forward compatibility: The proto already supports transaction_id fields on storage ops; if/when StorageSPI supports transaction-scoped writes, we can bind all pre‑exec deltas into a single backend transaction and commit before remote EVM.

  Concrete Class/Method Touchpoints

  - New:
      - framework/src/main/java/org/tron/core/storage/sync/ResourceSyncContext.java
      - framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java
  - Update:
      - framework/src/main/java/org/tron/core/db/Manager.java
      - Begin/flush resource sync around `consumeBandwidth/consumeMemoFee/consumeMultiSignFee` and before `trace.exec()`.
  - chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java
      - After `accountStore.put()` and dynamic property updates → `ResourceSyncContext.record...`
  - chainbase/src/main/java/org/tron/core/db/EnergyProcessor.java
      - After `accountStore.put()` and `saveBlockEnergyUsage()` → record dirties
  - actuator/src/main/java/org/tron/core/vm/VMActuator.java
      - Where `rootRepository.updateAccount(...)` modifies energy windows in V2 paths → record dirties
  - Native processors for delegate/undelegate → record sender/receiver dirties

  Risk Mitigation

  - Double writes: Stores already hit remote; the sync is about timing/ordering and batching, not duplication. We won’t re‑write values; we only ensure pre‑exec mutations are completed and “observed” by the backend before execution. Batching
  here is an optional optimization; correctness remains with existing put calls.
  - Performance: Keep record* calls cheap (addresses/keys only). Do heavy lifting in flush with a single batch per DB.
  - Backward compatibility: Feature-flag the entire sync so embedded mode behavior remains unchanged.

  If you want, I can draft the exact hook points and the minimal method signatures for ResourceSyncContext/Service next, or propose a quick POC wiring in Manager + BandwidthProcessor to validate the approach.
  