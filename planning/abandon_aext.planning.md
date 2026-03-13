• Using protocol::Account fields directly is generally the right direction—those fields (net_usage, latest_consume_time, net_window_size, account_resource.energy_usage, etc.) are the canonical java-tron state,
so it improves parity, makes APIs/debugging saner, and avoids inventing a Rust-only persistence model.

The big caveat is don’t end up with two sources of truth:

- In 3cc155d, delegate validation reads usage from the Account proto; if an account’s “real” usage was only ever persisted in AccountAext, validation can silently undercount and allow over-delegation.
- 33d487e mitigates this for bandwidth going forward by mirroring AEXT → Account proto (apply_bandwidth_aext_to_account_proto()), but it’s still a hybrid: you can get drift (and you still have legacy state
  until touched unless you migrate/backfill).

If you keep AccountAext, make it explicitly one of:

- Temporary bridge (plan to delete it), or
- Pure cache/derived view (proto is authoritative; AEXT can be rebuilt), or
- Authoritative store (then all reads must use AEXT, and proto must be treated as derived/output).

Given your commits, you’re already leaning “proto is authoritative”, so I’d finish that: either migrate/backfill existing AEXT into Account protos (or add read-time fallback) and then phase out the persisted
AEXT to avoid long-term consistency risk.`

---

currently how do we use AccountAext?

---

• Right now AccountAext is used as a resource-usage sidecar in two places:

- gRPC snapshot/echo (Java → Rust → Java): Java collects current resource fields from AccountCapsule and sends them as pre_execution_aext (framework/src/main/java/org/tron/core/execution/spi/
RemoteExecutionSPI.java:1651, proto in framework/src/main/proto/backend.proto:461). Rust parses them (rust-backend/crates/core/src/service/grpc/aext.rs:15, called in rust-backend/crates/core/src/service/
grpc/mod.rs:962) and, in "hybrid" mode, uses them to populate the AccountInfo AEXT tail in state changes (rust-backend/crates/core/src/service/grpc/conversion.rs:344).
- Rust-side tracked bandwidth accounting + persistence: When execution.remote.accountinfo_aext_mode == "tracked", system-contract handlers load the current AEXT from the custom RocksDB CF "account-
resource" (rust-backend/crates/execution/src/storage_adapter/db_names.rs:37, accessed via rust-backend/crates/execution/src/storage_adapter/engine.rs:2584), run windowed bandwidth tracking (rust-backend/
crates/execution/src/storage_adapter/resource.rs:69), then persist the updated AEXT and also mirror the bandwidth fields into the protocol::Account record (rust-backend/crates/core/src/service/mod.rs:909,
mirroring in rust-backend/crates/execution/src/storage_adapter/engine.rs:2659). In "tracked" mode the before/after AEXT is also returned via aext_map and used to populate AccountInfo fields in the response
(rust-backend/crates/core/src/service/grpc/conversion.rs:375).

Today it’s effectively bandwidth-only tracking (energy fields exist on AccountAext but aren’t updated by the tracker yet; see rust-backend/crates/execution/src/storage_adapter/types.rs:166 + rust-backend/
crates/execution/src/storage_adapter/resource.rs:69).

---

› think harder.

No need to start implementing, just read through and understand the codebase and give me a detailed plan on:

migrate/backfill existing AEXT into Account protos and then phase out the persisted AEXT to avoid long-term consistency risk.

---

• Goal                                                                                                                                                                                                            
                                                                                                                                                                                                                  
- Make protocol::Account in the account DB the only persisted source of truth for resource usage/window fields.                                                                                                 
- Treat AccountAext as an in-memory “view/snapshot” (for gRPC pre_execution_aext + optional tracked reporting), not a persisted store (account-resource).                                                       
                                                                                                                                                                                                                
Phase 1: Specify Exact Field Mapping + Normalization                                                                                                                                                            
                                                
- Bandwidth (top-level Account):                                                                                                                                                                                
    - AccountAext.net_usage → Account.net_usage                                                                                                                                                                 
    - AccountAext.free_net_usage → Account.free_net_usage                                                                                                                                                       
    - AccountAext.latest_consume_time → Account.latest_consume_time
    - AccountAext.latest_consume_free_time → Account.latest_consume_free_time                          
    - AccountAext.net_window_optimized → Account.net_window_optimized                                                                                                                                           
    - AccountAext.net_window_size (logical slots) → Account.net_window_size (raw):                                                                                                                              
        - if optimized: raw = slots * 1000 (WINDOW_SIZE_PRECISION)
        - else: raw = slots                                                                            
- Energy (nested Account.account_resource):                                                                                                                                                                     
    - AccountAext.energy_usage → Account.account_resource.energy_usage                                                                                                                                          
    - AccountAext.latest_consume_time_for_energy → Account.account_resource.latest_consume_time_for_energy
    - AccountAext.energy_window_optimized → Account.account_resource.energy_window_optimized                                                                                                                    
    - AccountAext.energy_window_size (logical slots) → Account.account_resource.energy_window_size (raw, same scaling rules)                                                                                    
    - Create account_resource only when at least one energy field is non-default (or if it already exists).                                                                                                     
- Critical legacy fix (timestamps): pre-33d487e AEXT used context.block_number as “now”; post-33d487e uses head_slot = block_timestamp_ms/3000. If you backfill old AEXT consume-times without conversion,
delegate validation (which uses head_slot) will see enormous deltas and effectively “fully recover” usage.
    - Migration must normalize AEXT consume-times into the slot domain.                                                                                                                                         
    - Practical conversion approach:                                                                                                                                                                            
        - Read head_slot = latest_block_header_timestamp/3000 and head_block = LATEST_BLOCK_HEADER_NUMBER from dynamic properties.                                                                              
        - Compute slot_offset = head_slot - head_block (mainnet-scale large positive number).                                                                                                                   
        - For each AEXT consume-time t:                                                                                                                                                                         
            - If it “looks like a block number” (e.g., t > 0 and t < slot_offset/2, or t <= head_block while t << head_slot), rewrite t = t + slot_offset.                                                      
        - Apply the same rule to latest_consume_time, latest_consume_free_time, latest_consume_time_for_energy.                                                                                                 
                                                                                                                                                                                                                
Phase 2: One-Time Backfill/Migration (Offline Tool or Startup Hook)                                                                                                                                             
                                                                                                                                                                                                                
- Recommended shape: a dedicated Rust binary (e.g. tron-backend-migrate) so you can run it with the backend stopped; startup hook is fine too, but offline avoids race-with-Java writes.                        
- Steps:
    1. Load config.storage.data_dir; open a StorageEngine on it.
    2. Confirm account-resource DB exists and is non-empty (Size/IsEmpty).
    3. Detect the network address prefix using existing logic (EngineBackedEvmStateStore::detect_address_prefix) so Address(20) → account key prefix+20.
    4. Read dynamic properties: latest_block_header_timestamp and LATEST_BLOCK_HEADER_NUMBER; compute head_slot and slot_offset.
    5. Iterate account-resource in sorted order in chunks:
        - Use StorageEngine::get_next("account-resource", start_key, limit); set next start_key = last_key + [0x00] to avoid repeating (keys are fixed 20 bytes).
    6. For each (key,value):
        - Validate key.len()==20; AccountAext::deserialize(value); if bad, log+skip.
        - Convert consume-times to slot domain (legacy fix above).
        - Load the matching Account proto from account DB:
            - If missing: decide policy (default: skip + keep AEXT or skip + delete AEXT, but do not create phantom accounts).
        - Apply mapping into the proto; only write if any targeted field actually changes (avoids rewriting every account byte blob).
        - Persist via put_account_proto (so the existing java-compat encoding path is used).
        - Optionally delete the AEXT entry after successful write (preferred once you trust the migrator).
    7. Record an idempotent migration marker:
        - Option A: custom key in properties DB like RUST_MIGRATION_ACCOUNT_RESOURCE_TO_ACCOUNT_V1.
        - Option B: a small file under data_dir/ (keeps Java-tron dynamic properties “pure”).
    8. Emit a final report: scanned / migrated / skipped-missing-account / decode-failed / already-up-to-date / deleted-aext.
- Safety knobs to include in the plan:
    - --dry-run (no writes; prints counts + sample diffs).
    - --chunk-size to tune memory/IO.
    - --delete-aext and/or --backup-account-resource-dir (rename directory after success, don’t hard-delete by default).

Phase 3: Stop Writing Persistent AEXT (Eliminate Dual-Truth)

- In rust-backend/crates/core/src/service/mod.rs, remove the pattern:
- get_account_aext → ResourceTracker → set_account_aext → apply_*_to_account_proto
- Replace with:
- Read “current AEXT view” from Account proto (normalize window sizes back to logical slots for the tracker).
- Run tracking logic to compute before/after (still useful for accountinfo_aext_mode="tracked" responses).
- Write only the updated fields back into the Account proto (single store).
- Transitional fallback (one release, strongly recommended):
- If proto resource fields look unset/default but an account-resource entry exists, do a lazy backfill (apply once, then delete AEXT). This protects upgrades where the operator didn’t run the offline
migrator.

Phase 4: Remove Legacy Store

- After the transition window:
    - Delete account-resource persistence APIs (EngineBackedEvmStateStore::{get_account_aext,set_account_aext,get_or_init_account_aext}) and the DB name constant if unused.
    - Keep AccountAext type only as gRPC snapshot payload (from framework/src/main/proto/backend.proto) and/or as an internal “view” struct derived from Account.

Tests / Verification

- Extend the existing apply_bandwidth_aext_to_account_proto test pattern to cover:
    - Energy mapping into account_resource.
    - Slot-domain conversion heuristic using synthetic head_slot/head_block.
- Add an integration test that seeds:
    - account DB entry + account-resource AEXT entry in a temp dir
    - runs migrator
    - asserts proto fields updated and (if enabled) account-resource key removed.
- Update the one place that asserts persistence today: rust-backend/crates/core/src/service/tests/contracts/witness_update.rs should assert proto fields changed (and/or that account-resource stays empty), not
that get_account_aext() returns Some.

Rollout

- Release N: ship migrator + lazy fallback + stop writing account-resource going forward.
- Release N+1: remove fallback + optionally auto-clean account-resource only when migration marker is present.