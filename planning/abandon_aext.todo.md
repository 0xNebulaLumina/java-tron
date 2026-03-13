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
- [ ] Confirm “Phase 1 migration scope = bandwidth only”:
  - [ ] Migrate: `net_usage`, `free_net_usage`, `latest_consume_time`, `latest_consume_free_time`
  - [ ] Window fields:
    - [ ] Decide if migrator writes `net_window_size/net_window_optimized` at all, or only when proto fields are unset/0.
  - [ ] Do NOT migrate energy fields in v1 (unless explicitly enabled by a flag).

### 0.2 Define canonical time-domain representation
- [ ] Canonicalize all consume times to **slot domain**:
  - [ ] `slot = block_timestamp_ms / 3000`
  - [ ] Store slots in `protocol::Account.latest_consume_time`, `latest_consume_free_time`, `account_resource.latest_consume_time_for_energy`

### 0.3 Define conflict resolution rules (proto vs aext)
Pick deterministic rules to avoid ping-pong:
- [ ] For each migrated field, define one of:
  - [ ] “AEXT overwrites proto always”
  - [ ] “Proto wins if non-zero/non-default”
  - [ ] “Max(last_time), max(usage)”, etc (only if semantics are sound)
- [ ] Recommend: for usage + consume-times, **prefer AEXT when proto looks default/empty**, otherwise keep proto (minimize unintended overwrite).

### 0.4 Decide migration marker
- [ ] Choose marker location:
  - [ ] Option A: `db="properties"` key `RUST_MIGRATION_ACCOUNT_RESOURCE_TO_ACCOUNT_V1`
  - [ ] Option B: `data_dir/.migrations/account-resource-to-account.v1`
- [ ] Define “migration complete” meaning (e.g., scanned all keys, wrote N updates, optionally deleted AEXT keys).

### 0.5 Define operator workflow (runbook)
- [ ] Offline-first: run migrator with backend stopped.
- [ ] Provide `--dry-run` and `--backup` as the default safe path.

Acceptance for Phase 0
- [ ] Written-down field list, time-domain rule, and conflict-resolution rule.
- [ ] Written-down migration marker choice + meaning.

---

## TODOs — Phase 1: One-Time Offline Migrator (`account-resource` → `account`)

### 1.1 Entry point + packaging
- [ ] Decide binary location:
  - [ ] `rust-backend/src/bin/tron-backend-migrate-aext.rs` (simple)
  - [ ] or a new `crates/tools` workspace crate (clean separation)
- [ ] CLI args checklist:
  - [ ] `--data-dir <path>` (defaults to config)
  - [ ] `--chunk-size <N>` (default 256/1024)
  - [ ] `--dry-run`
  - [ ] `--delete-aext` (off by default)
  - [ ] `--backup-aext-db` (default on: rename dir or copy)
  - [ ] `--write-window-fields` (off by default, or “only-if-missing”)
  - [ ] `--enable-energy-migration` (default off; future)
  - [ ] `--force` (ignore migration marker)

### 1.2 Data iteration plan (must handle large DBs)
- [ ] Iterate `db="account-resource"` using `StorageEngine::get_next(...)` in key order.
- [ ] Key format validation:
  - [ ] Require `key.len()==20` (EVM address)
  - [ ] Record and skip invalid keys (metrics)
- [ ] For each key:
  - [ ] Deserialize `AccountAext` (skip on error, but count)
  - [ ] Map to `account` key = `[prefix_byte] + key` (21 bytes)
  - [ ] Load `protocol::Account` from `db="account"` (skip if missing; never create phantom accounts by default)

### 1.3 Prefix byte detection (0x41 vs 0xa0)
- [ ] Reuse existing heuristic from `EngineBackedEvmStateStore::detect_address_prefix`:
  - [ ] Scan `account`/`witness`/`votes` DB for any 21-byte keys with first byte in {0x41, 0xa0}
  - [ ] Default to 0x41 only if nothing found
- [ ] Log the detected prefix and warn if ambiguous.

### 1.4 Slot-domain conversion heuristic (legacy compatibility)
Goal: safely convert “block-number-like” consume times into slots.

Inputs:
- [ ] `head_ts_ms = properties["latest_block_header_timestamp"]` (note: lowercase key in current code)
- [ ] `head_slot = head_ts_ms / 3000`
- [ ] `head_block = properties["LATEST_BLOCK_HEADER_NUMBER"]`
- [ ] `slot_offset = head_slot - head_block` (expected positive on mainnet-like chains)

Conversion rule (document + test):
- [ ] For each consume-time `t`:
  - [ ] If `t <= 0` → leave as-is
  - [ ] Else if `t` “looks like a block number” → rewrite `t = t + slot_offset`
  - [ ] Else leave as-is
- [ ] Define “looks like block number” precisely (example options):
  - [ ] `t <= head_block && head_slot > head_block && slot_offset > 0`
  - [ ] `t < slot_offset/2` (guard against already-slot values)
- [ ] Log how many fields were rewritten vs left intact.
- [ ] Add a `--no-time-conversion` escape hatch for debugging only.

### 1.5 Write rules (idempotent)
- [ ] Only write the Account proto if at least one target field changes.
- [ ] Never touch non-bandwidth fields (unless explicitly enabled).
- [ ] When writing:
  - [ ] Use `put_account_proto(...)` to preserve java-compat encoding behavior.
  - [ ] (Optional) record “changed fields” counters for observability.

### 1.6 Optional deletion of `account-resource` keys
- [ ] Default behavior: do not delete anything (dry-run safe).
- [ ] If `--delete-aext`:
  - [ ] Delete the `account-resource` entry only after a successful proto write.
  - [ ] Keep a counter of deletions.

### 1.7 Mark completion + emit report
- [ ] Write migration marker (Phase 0 decision).
- [ ] Print summary:
  - [ ] scanned keys
  - [ ] decoded ok / decode errors
  - [ ] accounts missing in `account` DB
  - [ ] proto updates applied
  - [ ] time-fields rewritten (block→slot)
  - [ ] aext deletes performed

Acceptance for Phase 1
- [ ] On a fixture DB containing `account-resource`, migrator updates `protocol::Account` fields and is re-runnable with no further changes.
- [ ] Delegate-resource validation no longer depends on `account-resource` being present.

---

## TODOs — Phase 2: One-Release Lazy Backfill (Upgrade Safety Net)

Rationale: not all operators will run the offline migrator immediately.

### 2.1 Lazy backfill trigger (read-time or write-time)
- [ ] Choose trigger point:
  - [ ] Option A: on `get_account_proto(...)` (expensive; avoid doing work on every read)
  - [ ] Option B (preferred): on first tracked bandwidth update per account during execution
  - [ ] Option C: on startup, run a bounded incremental migration (still can be heavy)

### 2.2 Lazy backfill algorithm (bandwidth-only)
- [ ] If `protocol::Account` resource fields are “default/empty” (define exact predicate) AND `account-resource` has non-default:
  - [ ] Apply AEXT → proto (with time conversion)
  - [ ] Delete `account-resource` key
  - [ ] Continue execution using proto as the source of truth

### 2.3 Feature gate + logging
- [ ] Add config flag: `execution.remote.lazy_aext_backfill = true` (default true for one release).
- [ ] Add structured log line when lazy backfill happens (address + counts).

Acceptance for Phase 2
- [ ] Upgrading without running the offline migrator does not cause undercount/overcount in delegate validation due to stale resource fields.

---

## TODOs — Phase 3: Stop Reading/Writing `account-resource` in Normal Execution

### 3.1 Replace all `get_account_aext/set_account_aext` call sites
Identify and update the tracked-mode sites in:
- [ ] `rust-backend/crates/core/src/service/mod.rs` (all `aext_mode == "tracked"` blocks)
  - [ ] TransferContract handler
  - [ ] WitnessUpdateContract handler
  - [ ] VoteWitnessContract handler
  - [ ] Any other Non-VM handler using ResourceTracker

Target behavior:
- [ ] Build an in-memory “AEXT view” from `protocol::Account` (not from `account-resource`).
- [ ] Run tracking using that view.
- [ ] Persist updates directly into `protocol::Account`.
- [ ] Still populate `result.aext_map` for response/debugging (but do not persist `AccountAext`).

### 3.2 Introduce helpers: derive/update from `protocol::Account`
- [ ] Helper: `AccountAextView::from_account_proto(account: &protocol::Account) -> AccountAext`
  - [ ] Normalize window sizes into logical slots for tracker inputs.
  - [ ] Ensure energy fields are read from `account.account_resource` if present.
- [ ] Helper: `apply_bandwidth_view_to_account_proto(account: &mut protocol::Account, view: &AccountAext)`
  - [ ] Update only bandwidth fields + window fields (if needed).
  - [ ] Keep energy untouched.

### 3.3 Align “now” basis everywhere
- [ ] Ensure tracked bandwidth uses `now_slot = block_timestamp_ms / 3000` consistently (already in `33d487e`).
- [ ] Ensure consume times stored in proto are always in slot domain.

### 3.4 Update tests that currently assert persisted AEXT
- [ ] Replace `get_account_aext(...)` persistence assertions with proto assertions:
  - [ ] e.g. `rust-backend/crates/core/src/service/tests/contracts/witness_update.rs`

Acceptance for Phase 3
- [ ] `rg get_account_aext` shows no production call sites (tests/migrator only).
- [ ] All tracked-mode resource updates persist into `protocol::Account` only.

---

## TODOs — Phase 4: Remove Persisted AEXT Store (Code + Data)

### 4.1 Remove storage adapter APIs (after a deprecation window)
- [ ] Delete (or feature-gate) `EngineBackedEvmStateStore::{get_account_aext,set_account_aext,get_or_init_account_aext}`.
- [ ] Delete `db_names::account::ACCOUNT_RESOURCE` if unused outside migration tooling.
- [ ] Remove in-memory AEXT store from `InMemoryEvmStateStore` unless needed for tests.

### 4.2 Keep protobuf snapshot structs (do NOT remove)
- [ ] Keep `AccountAext`/`AccountAextSnapshot` in `framework/src/main/proto/backend.proto` (still useful for “hybrid” request snapshots).
- [ ] Keep Rust parsing helpers in `rust-backend/crates/core/src/service/grpc/aext.rs`.

### 4.3 Data cleanup options
- [ ] Provide an explicit operator command:
  - [ ] `tron-backend-migrate-aext --delete-aext` (deletes keys)
  - [ ] or `reset_db("account-resource")` equivalent
- [ ] Decide whether backend should refuse to start if `account-resource` is non-empty after marker indicates completion (probably “warn only”).

Acceptance for Phase 4
- [ ] No new `account-resource` directory is created by normal runs.
- [ ] Removing the directory does not change behavior (all reads/writes are to `account`).

---

## TODOs — Phase 5: Verification / Rollout Checklist

### 5.1 Unit/integration coverage (Rust)
- [ ] Add tests for migration time-domain conversion heuristic (head_slot/head_block synthetic).
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
- [ ] Pre-upgrade:
  - [ ] stop backend
  - [ ] backup data dir
  - [ ] run `--dry-run` migrator and inspect summary
- [ ] Upgrade:
  - [ ] run migrator with writes enabled
  - [ ] (optional) delete/rename `account-resource`
  - [ ] start backend
  - [ ] watch logs for lazy-backfill counts (should trend to 0)

Acceptance for Phase 5
- [ ] Migration is repeatable and safe (dry-run, backup, marker).
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

