# Abandon `account-resource` (persisted `AccountAext`) — Migration + Deletion Plan (TODO)

Status: plan-only (no code changes yet)  
Owners: `rust-backend` (execution/storage_adapter/core service), optional `framework` follow-ups  
Last updated: 2026-02-10

## Problem Statement
As of commits `3cc155d` and `33d487e`, we now read resource-usage fields directly from `protocol::Account` for correctness-critical logic (e.g. DelegateResourceContract validation), **but** we still persist/consult a Rust-only sidecar store (`account-resource`) via `AccountAext`.

That creates a long-term “two sources of truth” hazard:
- Some code paths read `protocol::Account` (`account` DB).
- Other code paths read/write `AccountAext` (`account-resource` DB) and then *partially* mirror into `protocol::Account`.

The goal of this plan is to **backfill/migrate any existing `account-resource` data into `protocol::Account`** and then **stop using / remove** the persisted `AccountAext` store entirely.

Related context: `planning/abandon_aext.planning.md`, `planning/remote_AEXT_tracked.todo.md` (now needs to be superseded by this).

---

## Current Behavior (Quick Map)

### Persisted stores
- Canonical java-tron account record: `db="account"` storing `protocol::Account` (key = 21 bytes, prefix + 20-byte address).
- Rust-only sidecar: `db="account-resource"` storing serialized `AccountAext` (key = 20-byte EVM address).

### Read/write sites
- **Validation reads `protocol::Account`:**
  - DelegateResourceContract available-freeze validation uses `account.net_usage`, `account.latest_consume_time`, `account.net_window_*`, and energy equivalents under `account.account_resource`.
- **Tracked bandwidth accounting reads/writes `account-resource`:**
  - Multiple system-contract handlers do:
    - read `get_account_aext(...)` (or default)
    - `ResourceTracker::track_bandwidth(...)`
    - write `set_account_aext(...)`
    - mirror bandwidth fields into `protocol::Account` via `apply_bandwidth_aext_to_account_proto(...)` (bandwidth-only).

### “now” basis changed
- Older code used `context.block_number` as “now”.
- Newer code uses `head_slot = block_timestamp_ms / 3000` as “now”.

This matters for migration because persisted consume-times may be in **block-number domain** while new code expects **slot domain**.

---

## Desired End State
- `protocol::Account` (in `db="account"`) is the **only persisted source of truth** for:
  - bandwidth usage + timestamps + window fields
  - energy usage + timestamps + window fields (when implemented)
- `AccountAext` remains only as:
  - (A) request/response snapshot structure (`pre_execution_aext`, “tracked” response fields), and/or
  - (B) an internal derived “view” struct computed from `protocol::Account` (not persisted).
- No production code reads/writes `db="account-resource"`.
- `db="account-resource"` is empty/removed (post migration), preventing drift and reducing operational complexity.

---

## Key References (Implementation Touchpoints)

### Rust backend (persistence + conversion)
- `rust-backend/crates/execution/src/storage_adapter/db_names.rs` (`ACCOUNT_RESOURCE = "account-resource"`)
- `rust-backend/crates/execution/src/storage_adapter/engine.rs`
  - `get_account_proto(...)`, `put_account_proto(...)`
  - `get_account_aext(...)`, `set_account_aext(...)`, `get_or_init_account_aext(...)`
  - `apply_bandwidth_aext_to_account_proto(...)` (bandwidth-only mirroring)
- `rust-backend/crates/execution/src/storage_adapter/types.rs` (`AccountAext` serialization format)
- `rust-backend/crates/execution/src/storage_adapter/resource.rs` (`ResourceTracker`)
- `rust-backend/crates/core/src/service/mod.rs` (system-contract handlers; AEXT tracked mode call sites)

### Java side (not the target of this plan, but affected)
- `framework/src/main/proto/backend.proto` (defines `AccountAext`, `AccountAextSnapshot`)
- Java sends `pre_execution_aext` for “hybrid” mode; this plan does not remove that mechanism.

---

## Correctness Traps (Must Address)

### Trap 1: Slot vs block-number consume-time mismatch (critical)
`protocol::Account.latest_consume_time` and friends are now interpreted as `head_slot` (`timestamp_ms/3000`) in validation and resource window decay logic.

If legacy `AccountAext.latest_consume_time*` was stored as **block number**, naïvely copying it into the Account proto will cause:
- massive `now - last_time` deltas
- usage decays to ~0 (or fully recovers)
- validation undercounts usage (potentially allowing over-delegation)

Migration must normalize consume times into the **slot domain**.

### Trap 2: Energy fields in `AccountAext` are not authoritative today
Current `ResourceTracker` only updates bandwidth-related fields; energy fields in `AccountAext` are typically `0`.

Migration must **not overwrite** `protocol::Account.account_resource.energy_*` using `AccountAext` defaults unless we can prove the AEXT values are authoritative.

### Trap 3: Window size encoding (`*_window_size` + `*_window_optimized`)
Java uses:
- `optimized=false`: raw value is logical slots
- `optimized=true`: raw value is `logical_slots * 1000` (`WINDOW_SIZE_PRECISION`)

Migration must preserve semantics when writing window fields:
- Either write values consistent with the optimized flag, or
- Prefer existing proto window configuration if it is already set.

### Trap 4: “Compute-only vs persisted” execution model
Some execution paths are compute-only (Java applies changes), while others are persisted (Rust commits).

We must avoid a situation where:
- Rust stops persisting `account-resource`,
- but also fails to persist the equivalent fields into `protocol::Account` in persisted-mode paths,
leaving the chain state inconsistent for subsequent validation.

### Trap 5: Idempotency + crash safety
Migration/backfill must be:
- safe to re-run (idempotent)
- resumable after crash
- non-destructive by default (backup/rename before delete)

---

## Strategy Overview (High-Level)
1) **Backfill existing `account-resource` → `account`** (bandwidth fields only, plus safe time-domain conversion).
2) Add a **one-release lazy backfill fallback** for operators who upgrade without running the migrator.
3) Change tracked accounting to **read/update `protocol::Account` directly**, no longer touching `account-resource`.
4) Remove persisted AEXT APIs + optionally delete/reset the `account-resource` DB.

---

## TODOs — Phase 0: Decisions / Invariants (Design Gate)

### 0.1 Define exactly what gets migrated
- [x] Confirm "Phase 1 migration scope = bandwidth only":
  - [x] Migrate: `net_usage`, `free_net_usage`, `latest_consume_time`, `latest_consume_free_time`
  - [x] Window fields:
    - [x] **DECISION**: Write `net_window_size/net_window_optimized` only when proto fields are unset/0 (preserve existing proto values).
  - [x] Do NOT migrate energy fields in v1 (unless explicitly enabled by a flag).

### 0.2 Define canonical time-domain representation
- [x] Canonicalize all consume times to **slot domain**:
  - [x] `slot = block_timestamp_ms / 3000`
  - [x] Store slots in `protocol::Account.latest_consume_time`, `latest_consume_free_time`, `account_resource.latest_consume_time_for_energy`

### 0.3 Define conflict resolution rules (proto vs aext)
Pick deterministic rules to avoid ping-pong:
- [x] For each migrated field, define one of:
  - [x] **DECISION**: "AEXT overwrites proto when proto looks default/empty" - prevents overwriting intentional values
  - [x] For usage fields (`net_usage`, `free_net_usage`): AEXT wins if proto is 0
  - [x] For consume-time fields: AEXT wins if proto is 0
  - [x] For window fields: Only write if proto is 0 (preserve existing proto values)
- [x] Recommend: for usage + consume-times, **prefer AEXT when proto looks default/empty**, otherwise keep proto (minimize unintended overwrite).

### 0.4 Decide migration marker
- [x] Choose marker location:
  - [x] **DECISION**: Option B - `data_dir/.migrations/account-resource-to-account.v1` (keeps Java-tron dynamic properties "pure")
- [x] Define "migration complete" meaning: file contains JSON with `{completed: true, scanned: N, migrated: N, skipped: N, deleted: N, timestamp: ISO8601}`

### 0.5 Define operator workflow (runbook)
- [x] Offline-first: run migrator with backend stopped.
- [x] Provide `--dry-run` and `--backup` as the default safe path.

Acceptance for Phase 0
- [x] Written-down field list, time-domain rule, and conflict-resolution rule.
- [x] Written-down migration marker choice + meaning.

---

## TODOs — Phase 1: One-Time Offline Migrator (`account-resource` → `account`)

### 1.1 Entry point + packaging
- [x] Decide binary location:
  - [x] `rust-backend/src/bin/migrate_aext.rs` (simple) → builds as `tron-backend-migrate-aext`
  - [x] Added to Cargo.toml [[bin]] section
- [x] CLI args checklist:
  - [x] `--data-dir <path>` (defaults to ./data)
  - [x] `--chunk-size <N>` (default 256)
  - [x] `--dry-run`
  - [x] `--delete-aext` (off by default)
  - [x] `--backup-aext-db` (renames directory before migration)
  - [x] `--write-window-fields` (off by default - only writes when proto is 0)
  - [x] `--force` (ignore migration marker)
  - [x] `--no-time-conversion` (skip block→slot conversion)
  - [ ] `--enable-energy-migration` (deferred to v2; energy fields not migrated)

### 1.2 Data iteration plan (must handle large DBs)
- [x] Iterate `db="account-resource"` using `StorageEngine::get_next(...)` in key order.
- [x] Key format validation:
  - [x] Require `key.len()==20` (EVM address)
  - [x] Record and skip invalid keys (decode_errors counter)
- [x] For each key:
  - [x] Deserialize `AccountAext` (skip on error, but count)
  - [x] Map to `account` key = `[prefix_byte] + key` (21 bytes)
  - [x] Load `protocol::Account` from `db="account"` (skip if missing; accounts_missing counter)

### 1.3 Prefix byte detection (0x41 vs 0xa0)
- [x] Implemented `detect_address_prefix()`:
  - [x] Scan `account`/`witness`/`votes` DB for any 21-byte keys with first byte in {0x41, 0xa0}
  - [x] Default to 0x41 if nothing found
- [x] Log the detected prefix and warn if ambiguous.

### 1.4 Slot-domain conversion heuristic (legacy compatibility)
Goal: safely convert "block-number-like" consume times into slots.

Inputs:
- [x] `head_ts_ms = properties["latest_block_header_timestamp"]` (lowercase key)
- [x] `head_slot = head_ts_ms / 3000`
- [x] `head_block = properties["LATEST_BLOCK_HEADER_NUMBER"]` (uppercase key)
- [x] `slot_offset = head_slot - head_block` (expected positive on mainnet)

Conversion rule (document + test):
- [x] For each consume-time `t`:
  - [x] If `t <= 0` → leave as-is
  - [x] Else if `t` "looks like a block number" → rewrite `t = t + slot_offset`
  - [x] Else leave as-is
- [x] Define "looks like block number" precisely:
  - [x] `slot_offset > 0 && t <= head_block && t < head_slot / 2`
- [x] Log how many fields were rewritten (time_fields_rewritten counter).
- [x] Add `--no-time-conversion` escape hatch for debugging.

### 1.5 Write rules (idempotent)
- [x] Only write the Account proto if at least one target field changes (skipped_no_change counter).
- [x] Never touch non-bandwidth fields (energy fields excluded in v1).
- [x] When writing:
  - [x] Use `engine.put()` directly (migrator is offline, no java-compat encoding needed)
  - [x] Record "proto_updates" counter for observability.

### 1.6 Optional deletion of `account-resource` keys
- [x] Default behavior: do not delete anything (dry-run safe).
- [x] If `--delete-aext`:
  - [x] Delete the `account-resource` entry only after a successful proto write.
  - [x] Keep aext_deletes counter.

### 1.7 Mark completion + emit report
- [x] Write migration marker to `data_dir/.migrations/account-resource-to-account.v1` (JSON).
- [x] Print summary:
  - [x] scanned keys
  - [x] decoded ok / decode errors
  - [x] accounts missing in `account` DB
  - [x] proto updates applied
  - [x] time-fields rewritten (block→slot)
  - [x] aext deletes performed
  - [x] skipped (no change)

Acceptance for Phase 1
- [x] Binary compiles and includes unit tests for key increment, time conversion, and conflict resolution.
- [ ] On a fixture DB containing `account-resource`, migrator updates `protocol::Account` fields and is re-runnable with no further changes (integration test pending).
- [ ] Delegate-resource validation no longer depends on `account-resource` being present (requires Phase 3).

---

## TODOs — Phase 2: One-Release Lazy Backfill (Upgrade Safety Net)

Rationale: not all operators will run the offline migrator immediately.

### 2.1 Lazy backfill trigger (read-time or write-time)
- [x] Choose trigger point:
  - [x] **DECISION**: Option B - on first tracked bandwidth update per account
  - [x] Implemented `lazy_aext_backfill()` method in `EngineBackedEvmStateStore`
  - [x] Call at start of tracked accounting before running ResourceTracker

### 2.2 Lazy backfill algorithm (bandwidth-only)
- [x] If `protocol::Account` resource fields are "default/empty" (net_usage, free_net_usage, consume times all 0) AND `account-resource` has non-default:
  - [x] Apply AEXT → proto (bandwidth fields only)
  - [x] Apply window fields if proto is 0
  - [x] Delete `account-resource` key
  - [x] Continue execution using proto as the source of truth
- [x] Note: Time conversion is NOT done in lazy backfill (assumes already in slot domain or will be handled by first execution)

### 2.3 Feature gate + logging
- [x] Add config flag: `execution.remote.lazy_aext_backfill = true` (default true for one release).
- [x] Add structured log line when lazy backfill happens (tracing::info with address and field values).

### 2.4 Helper: derive AEXT view from Account proto
- [x] Implemented `aext_view_from_account_proto()` method:
  - [x] Normalize window sizes from raw proto value to logical slots
  - [x] Read energy fields from `account.account_resource` if present
  - [x] Use default window size of 28800 if not set

Acceptance for Phase 2
- [ ] Upgrading without running the offline migrator does not cause undercount/overcount in delegate validation due to stale resource fields.
- [ ] Lazy backfill is callable from tracked-mode handlers (integration with Phase 3).

---

## TODOs — Phase 3: Stop Reading/Writing `account-resource` in Normal Execution

### 3.1 Replace all `get_account_aext/set_account_aext` call sites
Identify and update the tracked-mode sites in:
- [x] `rust-backend/crates/core/src/service/mod.rs` (all `aext_mode == "tracked"` blocks)
  - [x] TransferContract handler (line ~910)
  - [x] WitnessCreateContract handler (line ~1406)
  - [x] WitnessUpdateContract handler (line ~1570)
  - [x] VoteWitnessContract handler (line ~1989)
  - [x] AccountUpdateContract handler (line ~2218)
  - [x] AccountCreateContract handler (line ~2564)
  - [x] TransferAssetContract (TRC-10) handler (line ~4362)
  - [x] AssetIssueContract handler (line ~4964)

Target behavior:
- [x] Build an in-memory "AEXT view" from `protocol::Account` via `aext_view_from_account_proto()`.
- [x] If lazy_aext_backfill enabled, use `lazy_aext_backfill()` to migrate stale data first.
- [x] Run tracking using that view.
- [x] Persist updates directly into `protocol::Account` via `apply_bandwidth_aext_to_account_proto()`.
- [x] Still populate `result.aext_map` for response/debugging (but do not persist to account-resource).

### 3.2 Introduce helpers: derive/update from `protocol::Account`
- [x] Helper: `aext_view_from_account_proto(&Address) -> AccountAext` (in storage_adapter/engine.rs)
  - [x] Normalize window sizes into logical slots for tracker inputs.
  - [x] Ensure energy fields are read from `account.account_resource` if present.
- [x] Existing helper: `apply_bandwidth_aext_to_account_proto(address, &AccountAext)`
  - [x] Update only bandwidth fields + window fields (if needed).
  - [x] Keep energy untouched.

### 3.3 Align "now" basis everywhere
- [x] Ensure tracked bandwidth uses `now_slot = block_timestamp_ms / 3000` consistently (all handlers use this).
- [x] Ensure consume times stored in proto are always in slot domain (via apply_bandwidth_aext_to_account_proto).

### 3.4 Update tests that currently assert persisted AEXT
- [ ] Replace `get_account_aext(...)` persistence assertions with proto assertions:
  - [ ] e.g. `rust-backend/crates/core/src/service/tests/contracts/witness_update.rs`

Acceptance for Phase 3
- [x] `rg get_account_aext` shows no production call sites in service/mod.rs.
- [x] All tracked-mode resource updates persist into `protocol::Account` only.

---

## TODOs — Phase 4: Remove Persisted AEXT Store (Code + Data)

### 4.1 Remove storage adapter APIs (after a deprecation window)
- [x] Deprecate `EngineBackedEvmStateStore::set_account_aext` with `#[deprecated]` attribute
- [x] Deprecate `EngineBackedEvmStateStore::get_or_init_account_aext` with `#[deprecated]` attribute
- [x] Add deprecation documentation to `get_account_aext` (kept for migrator/lazy backfill)
- [x] Add deprecation comment to `db_names::account::ACCOUNT_RESOURCE`
- [ ] Full deletion of these APIs (in future release after migration window)
- [ ] Remove in-memory AEXT store from `InMemoryEvmStateStore` (after tests are updated)

### 4.2 Keep protobuf snapshot structs (do NOT remove)
- [ ] Keep `AccountAext`/`AccountAextSnapshot` in `framework/src/main/proto/backend.proto` (still useful for “hybrid” request snapshots).
- [ ] Keep Rust parsing helpers in `rust-backend/crates/core/src/service/grpc/aext.rs`.

### 4.3 Data cleanup options
- [x] Provide an explicit operator command:
  - [x] `tron-backend-migrate-aext --delete-aext` (deletes keys after migration)
  - [x] `tron-backend-migrate-aext --backup-aext-db` (renames directory before migration)
- [ ] Decide whether backend should refuse to start if `account-resource` is non-empty after marker indicates completion (probably "warn only").

Acceptance for Phase 4
- [x] No new `account-resource` writes from production code (all writes removed).
- [x] All production reads use `aext_view_from_account_proto()` instead.

---

## TODOs — Phase 5: Verification / Rollout Checklist

### 5.1 Unit/integration coverage (Rust)
- [x] Add tests for migration time-domain conversion heuristic in migrator:
  - [x] `test_convert_time_domain` - tests no conversion when slot_offset=0
  - [x] `test_convert_time_domain` - tests block→slot conversion heuristic
  - [x] `test_convert_time_domain` - tests values already in slot domain are not converted
- [x] Add tests for conflict resolution:
  - [x] `test_apply_aext_conflict_resolution` - tests AEXT wins when proto is 0
  - [x] `test_apply_aext_conflict_resolution` - tests proto wins when non-zero
- [x] Add tests for key iteration:
  - [x] `test_increment_key` - tests byte key increment for iteration
- [ ] Add integration test:
  - [ ] seed temp dir with `account` proto + `account-resource` AEXT
  - [ ] run migrator
  - [ ] assert proto updated, and (if delete enabled) AEXT removed

### 5.2 Conformance / parity checks
- [ ] Ensure DelegateResourceContract validation behavior matches Java on fixtures where usage matters.
- [ ] Re-run any CSV/digest parity suite used by this repo (if applicable) with:
  - [ ] `accountinfo_aext_mode=tracked`
  - [ ] migration done vs not done (lazy backfill path)

### 5.3 Operator runbook (step-by-step)
- [x] Pre-upgrade:
  - [x] stop backend
  - [x] backup data dir (or use `--backup-aext-db`)
  - [x] run `--dry-run` migrator and inspect summary
- [x] Upgrade:
  - [x] run migrator with writes enabled
  - [x] (optional) use `--delete-aext` to remove migrated AEXT keys
  - [x] start backend
  - [x] watch logs for lazy-backfill counts (should trend to 0)

Acceptance for Phase 5
- [x] Migration is repeatable and safe (dry-run, backup, marker).
- [x] Migrator unit tests pass (3/3).
- [ ] After one release cycle, `account-resource` can be removed without correctness impact.

---

## Appendix A: Field Mapping (Bandwidth Only, v1)

From `AccountAext` (sidecar / derived view) → `protocol::Account`:
- `net_usage` → `Account.net_usage`
- `free_net_usage` → `Account.free_net_usage`
- `latest_consume_time` → `Account.latest_consume_time`
- `latest_consume_free_time` → `Account.latest_consume_free_time`
- `net_window_size` + `net_window_optimized` → `Account.net_window_size` + `Account.net_window_optimized`

Energy fields are explicitly excluded from v1 migration unless enabled by a flag.

## Appendix B: Suggested “Looks Like Block Number” Predicate (Concrete)

Given:
- `head_block >= 0`
- `head_slot = latest_block_header_timestamp_ms / 3000`
- `slot_offset = head_slot - head_block`

For a consume time `t`:
- If `t <= 0`: not convertible
- Else if `slot_offset > 0 && t <= head_block && t < head_slot / 2`: treat as block-number-domain, rewrite `t += slot_offset`
- Else: assume already slot-domain

This is intentionally conservative; correctness > converting everything.

