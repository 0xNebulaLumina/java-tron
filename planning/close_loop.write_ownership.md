# Close Loop — 1.1 Canonical Write Ownership

This file closes Section 1.1 of `close_loop.todo.md`. Its job is to make
"who writes the final state?" unambiguous per mode, so no engineer has to
reason about overlapping write paths from partial code reading.

Authoritative reference. If this note and the code or config disagree,
fix the code/config unless there is an explicit planning update.

## Write-path matrix

We only recognize two target modes (`EE`, `RR`). Everything else is legacy.

### `EE` — embedded execution + embedded storage (canonical baseline)

| Layer                          | Owner                                |
| ------------------------------ | ------------------------------------ |
| Execution                      | `EmbeddedExecutionSPI` (Java EVM)    |
| Dynamic properties / stores    | Java actuators + `chainbase`         |
| State mutation path            | Actuator → `*Store.put(...)`         |
| RocksDB writer                 | `chainbase` / `TronStoreWithRevoking`|
| Historical Java apply calls    | Not present in the embedded runtime path |
| Remote (Rust) backend          | **Not used**. Must not be reached.   |

Canonical writer: **Java**. `RemoteExecutionSPI` is not in the loop in `EE`.
`RemoteStorageSPI` is not in the loop in `EE`.

### `RR` — remote execution + remote storage (Phase 1 target)

| Layer                          | Owner                                   |
| ------------------------------ | --------------------------------------- |
| Execution                      | `RemoteExecutionSPI` → Rust via gRPC    |
| Handler work                   | `rust-backend/crates/core/service/contracts/*` |
| Buffered state mutation        | Rust `EngineBackedEvmStateStore` with write buffer |
| Commit semantics               | Rust buffer commit on handler success; discard on failure |
| RocksDB writer                 | Rust `tron-backend-storage` → Rust-owned RocksDB |
| Response to Java               | `ExecutionResult { write_mode = PERSISTED, touched_keys, ... }` |
| Historical Java apply calls    | **Deleted** — Java fails fast on successful/effectful non-`PERSISTED` results |
| `postExecMirror` on Java side  | Refreshes Java's local revoking head from Rust touched keys so Java-side reads (consensus, RPC) stay coherent |

Canonical writer: **Rust** (backend process, its own RocksDB).

Java's local `chainbase` is a read-side mirror for code paths that still
consume Java stores (JSON-RPC, consensus apply glue, CSV/reporting). It is
*not* a redundant authoritative store. In `RR`, Java state must never be
treated as the source of truth.

### Other combinations

| Combination                                     | Status    |
| ------------------------------------------------ | --------- |
| Embedded execution + remote storage              | **Not a target mode.** Out of scope in Phase 1. No planning or optimization effort. |
| Remote execution + embedded storage              | **Not a target mode.** Same as above. |
| In-process `SHADOW` (embedded + remote)           | Legacy developer tool. Not acceptance path. See `close_loop.scope.md`. |

Fail-fast detection of "unsafe combination" (e.g., remote execution mode
with `rust_persist_enabled=false` but pointed at a storage backend that
doesn't persist Java-side either) is a follow-up implementation item
tracked under 1.1 acceptance.

## Role of `RuntimeSpiImpl`

`framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
now has one remote-state ownership role: it accepts persisted Rust writes
and refreshes Java's read-side mirror through `postExecMirror`.

The historical Java-side apply family has been deleted:
`applyStateChangesToLocalDatabase`, `applyFreezeLedgerChanges`,
`applyTrc10Changes`, `applyVoteChanges`, and `applyWithdrawChanges` are
not available runtime paths.

Classification for Phase 1:

- In `EE`, `RuntimeSpiImpl` is not the block-execution runtime; Java
  actuators drive state updates through the embedded path.
- In `RR` canonical (`rust_persist_enabled=true`), Rust writes the final
  state and returns `write_mode=PERSISTED` with touched keys. Java runs
  `postExecMirror` as read-side cache refresh only.
- `RR compute-only` is no longer a sanctioned runtime profile. Java fails
  fast if a non-`PERSISTED` remote result succeeds or carries remote state
  effects.

Rule for Phase 1: do not introduce new Java-side apply paths. All remote
state must be owned by Rust persistence and mirrored into Java through
B3 touched-key refresh.

## `rust_persist_enabled` policy

The flag lives in `rust-backend/crates/common/src/config.rs` as
`RemoteExecutionConfig::rust_persist_enabled` and is checked in
`rust-backend/config.toml`.

Decision:

| Profile                          | `rust_persist_enabled` | Status                 |
| -------------------------------- | ---------------------- | ---------------------- |
| `EE` baseline                    | ignored (Rust not hit) | —                      |
| `RR` canonical (Phase 1 target)  | `true`                 | **Acceptance profile** |
| `RR` compute-only                | `false`                | Legacy/unsupported; not sanctioned for canonical RR |
| `SHADOW`                         | `false`                | Legacy, not acceptance |

- **Never** means: not at all during Phase 1. **We do allow it** — canonical
  `RR` requires it.
- **Legacy/unsupported**: `false` is not a sanctioned canonical `RR`
  profile. Successful or effectful non-persisted results fail fast on the
  Java side because Java apply has been removed.
- **`RR` candidate mode**: `true`.

### Alignment actions

The current checked-in files have a direct contradiction we must fix:

- `rust-backend/config.toml` sets `rust_persist_enabled = true`, which is
  the correct value for the `RR` canonical profile.
- `rust-backend/crates/common/src/config.rs` defaults
  `rust_persist_enabled: false` and its doc comment calls `true` a
  "legacy mode, risk of double-write". This pre-dates the `write_mode`
  guard and is now wrong.

The Rust code default stays `false` (the safer value when no one has
opted into `RR`), but the surrounding comments and the `config.toml`
commentary must be updated to:

- Stop calling `true` legacy. `true` is the canonical Phase 1 `RR` path.
- Stop calling `false` the universal recommendation. `false` is a
  legacy/unsupported non-persisted path, not sanctioned canonical `RR`.
- Reference this file (`close_loop.write_ownership.md`) as the source
  of truth for the policy.
- Call out that the double-write risk is removed by deleting the
  Java-side apply family and by the `write_mode` guard in `RuntimeSpiImpl`:
  Java mirrors `PERSISTED` touched keys and rejects successful or effectful
  non-`PERSISTED` results.

## Recommended profiles

### Safe / canonical profile

For Phase 1 acceptance runs and any EE-vs-RR parity measurement:

```
# rust-backend/config.toml
[execution.remote]
system_enabled = true
rust_persist_enabled = true              # canonical RR writer = Rust
emit_freeze_ledger_changes = true
emit_global_resource_changes = true

# Java side
-Dexecution.mode=REMOTE
-Dstorage.mode=REMOTE                    # aspirational; see note below
```

Note: wiring `storage.mode=REMOTE` all the way through
`TronDatabase` / `TronStoreWithRevoking` is currently incomplete — the
Java stores still use hardcoded embedded RocksDB (see CLAUDE.md lesson
on "Main Application Integration"). Until that is fixed, `RR` runs still
keep a Java-local mirror, and `postExecMirror` is what keeps that mirror
consistent. This is a known gap, tracked via the sibling bridge-debt
work in Section 4.

### Non-persisted remote results

`rust_persist_enabled=false` is legacy/unsupported for canonical `RR`.
It is not a development profile that Java can complete by applying
sidecars locally.

A remote result with `WriteMode.COMPUTE_ONLY` may pass through only when
it represents an unsuccessful execution with no remote state effects. A
successful non-`PERSISTED` result, or any non-`PERSISTED` result carrying
touched keys, sidecars, or a contract address, fails fast because Java no
longer owns a local apply path.

## Answering the key question

> Who writes the final state in this mode?

- `EE`: **Java** (via actuators + chainbase).
- `RR` canonical (`rust_persist_enabled=true`): **Rust** (via its own
  buffered storage engine). Java is a read-side mirror.
- `RR` non-persisted (`rust_persist_enabled=false`): **unsupported for
  successful/effectful remote execution**. Java has no apply writer and
  fails fast instead.
- `SHADOW`: not a Phase 1 acceptance path.

Any engineer encountering ambiguity should first check the active
`rust_persist_enabled` value and the `write_mode` field on the
execution response. Canonical `RR` requires Rust persistence and a
`PERSISTED` response; Java only mirrors touched keys.

## Follow-up implementation items

These remain **open** — closing them is a coding task, not a doc task:

- [x] Align `rust-backend/config.toml` and `rust-backend/crates/common/src/config.rs`
      comments so both describe Rust as canonical `RR` writer and Java as read-side mirror.
- [ ] Add a fail-fast check at Rust startup that logs a clear warning when
      a user has chosen a combination we consider unsafe (e.g. running with
      `rust_persist_enabled=true` while the execution mode on the Java side
      is `EMBEDDED`).
- [ ] Add a fail-fast check at Java startup that warns when `execution.mode`
      is `REMOTE` but the active Rust config reports `rust_persist_enabled=false`
      while the node is expected to be in `RR` acceptance profile.
- [ ] Document the bridge-debt on `storage.mode=REMOTE` not being wired
      through the Java store constructors, so Phase 1 readers know the
      `RR` mirror is still Java-local.
