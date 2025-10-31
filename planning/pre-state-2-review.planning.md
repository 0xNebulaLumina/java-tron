• Summary

  - Short answer: Partially correct. Java-side post‑exec flush and dirty‑marking largely match the plan; Rust overlay seeding deviates in a way that won’t fix the reported stale read and also has a compile
    issue.

  Matches Plan

  - Post‑exec TRC‑10 flush hook
      - Added and invoked after applyTrc10LedgerChanges(...): framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:85
      - Guarded by remote mode and -Dremote.resource.sync.postexec flag (default “true”): framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:438
      - Uses ResourceSyncContext.flushPreExec() and logs: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:469
      - Resource context still finishes at end of Manager pipeline: framework/src/main/java/org/tron/core/db/Manager.java:1600
  - Dirty‑marking during TRC‑10 apply
      - Owner, blackhole, dynamic keys: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:661, 675, 668, 725, 857, 888
      - AssetIssue V1/V2 store keys: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:631, 639
  - Config toggles (Rust)
      - Added overlay_seed_shadow_trc10 and storage_block_barrier with defaults: rust-backend/crates/common/src/config.rs:136, 145, 274, 301
  - Track TRC‑10 touched addresses
      - Backend state field added: rust-backend/crates/core/src/service/mod.rs:34
      - Insert owner/blackhole on Issue, owner/issuer on Participate: rust-backend/crates/core/src/service/mod.rs:1764, 2006
      - Clear at block boundary: rust-backend/crates/core/src/service/mod.rs:179

  Deviations / Issues

  - Overlay seeding source is wrong (doesn’t use prior overlay)
      - Plan/todo: seed new overlay with prior block’s overlay contents for addresses touched by shadow TRC‑10; optionally compare storage vs prior overlay before deciding.
      - Actual: seeds from storage engine only, not from prior overlay, so it can re‑introduce the stale pre‑AssetIssue balance and will not fix the 3188→3189 mismatch.
      - Code: rust-backend/crates/core/src/service/mod.rs:119 (capture set), :138 (new overlay), :156 (iterate and read from storage)
  - Missing heuristic and prior overlay fallback
      - Todo item mentions comparing storage vs prior overlay and using prior overlay when storage lags. This is not implemented but was marked [x] in the todo.
  - Compile error in overlay seeding
      - Uses non‑existent StorageModule::get_engine() instead of engine(), and type mismatch with helper signature expecting &Arc<StorageEngine>: rust-backend/crates/core/src/service/mod.rs:149; storage module
        provides engine(&self) -> Result<&StorageEngine> (rust-backend/crates/storage/src/lib.rs:26)
  - Observability gap (minor)
      - Todo asks for INFO summarizing dirty counts/flush decision in applyTrc10LedgerChanges; current INFO logs show TRC‑10 change count, but not dirty set counts. Dirty counts only appear at DEBUG within
        ResourceSyncContext.flushPreExec() (framework/src/main/java/org/tron/core/storage/sync/ResourceSyncContext.java:221). The checkbox was marked [x] in the todo but the INFO-level summary is not present.

  What To Change

  - Seed from prior overlay (then fallback to storage)
      - Before replacing the overlay, snapshot the previous overlay (read lock), and for each last_block_trc10_touched address:
          - If prior overlay has AccountInfo, put that into the new overlay.
          - Else read from storage and seed if present.
      - Implement the optional comparison heuristic: when both prior overlay and storage have data, prefer prior overlay if it reflects the TRC‑10 delta (e.g., storage balance < overlay balance after an
        expected credit).
  - Fix storage engine access and helper signature
      - Use engine() and pass &StorageEngine (or adjust helper to accept &StorageEngine instead of &Arc<...>), e.g.:
          - let storage_engine = storage_module_any.engine().map_err(|e| ...)?;
          - Change read_account_from_storage(&StorageEngine, ...) accordingly.
  - Keep seeding in get_or_create_overlay(...)
      - The location is fine since grpc path calls it early: rust-backend/crates/core/src/service/grpc/mod.rs:1035.
  - Add INFO log for dirty flush summary
      - Emit a single INFO with dirty counts and “will_postexec_flush=true/false” after TRC‑10 apply to match todo intent.

  Verdict

  - 14ec601: Mostly aligns with planning/todo on Java side; minor logging delta from todo.
  - 4f0e147: Not aligned with the critical seeding requirement (uses storage instead of prior overlay) and contains a compile error; todo boxes for seeding/heuristic were marked completed prematurely.

  If you want, I can patch the Rust seeding to copy from the previous overlay (with storage fallback) and fix the engine access, plus add the INFO log on the Java side.

---

