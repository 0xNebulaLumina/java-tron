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
| `RuntimeSpiImpl.apply*` calls  | No-op — handled by Java EVM internally |
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
| `RuntimeSpiImpl.apply*` calls  | **Skipped** — Java sees `write_mode=PERSISTED` |
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

## Role of `RuntimeSpiImpl` Java-side apply

`framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
currently contains two families of logic:

1. **Java-side apply family** — `applyStateChangesToLocalDatabase`,
   `applyFreezeLedgerChanges`, `applyTrc10Changes`, `applyVoteChanges`,
   `applyWithdrawChanges`.
2. **Mirror family** — `postExecMirror`.

Classification for Phase 1:

- In `EE`, neither family is exercised meaningfully: Java actuators drive
  their own state updates, and `RuntimeSpiImpl` is a transparent wrapper.
- In `RR` (canonical, `rust_persist_enabled=true`), the apply family is
  **legacy / off**. Java sees `write_mode=PERSISTED` and skips apply.
  The mirror family runs so Java stores stay consistent with Rust state.
- In the transitional `RR-compute-only` profile (`rust_persist_enabled=false`),
  the apply family is **transitional**: Rust computes state changes only,
  Java applies them to `chainbase`. This is the developer/diagnostic
  profile and **not** the Phase 1 acceptance path. It exists so the
  execution lane can be validated separately from the storage lane.

Rule for Phase 1: do not introduce new code paths that depend on Java
apply being the canonical writer in `RR`. All new work should treat
`apply*` as eventually-removable and add mirror-side handling instead.

## `rust_persist_enabled` policy

The flag lives in `rust-backend/crates/common/src/config.rs` as
`RemoteExecutionConfig::rust_persist_enabled` and is checked in
`rust-backend/config.toml`.

Decision:

| Profile                          | `rust_persist_enabled` | Status                 |
| -------------------------------- | ---------------------- | ---------------------- |
| `EE` baseline                    | ignored (Rust not hit) | —                      |
| `RR` canonical (Phase 1 target)  | `true`                 | **Acceptance profile** |
| `RR` compute-only (development)  | `false`                | Development / diagnostic, not acceptance |
| `SHADOW`                         | `false`                | Legacy, not acceptance |

- **Never** means: not at all during Phase 1. **We do allow it** — canonical
  `RR` requires it.
- **Development only**: `false` is OK for local debugging where you want to
  inspect state in Java stores without Rust writes interfering.
- **Targeted experiments only**: `false` is fine for per-contract execution
  lane work where you explicitly want Java-side state to be the outcome.
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
- Stop calling `false` the universal recommendation. `false` is the
  development / compute-only path.
- Reference this file (`close_loop.write_ownership.md`) as the source
  of truth for the policy.
- Call out that the double-write risk is mitigated by the `write_mode`
  guard in `RuntimeSpiImpl`: when Rust returns `PERSISTED`, Java never
  runs `apply*`, so the two writers do not collide.

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

### Experimental / compute-only profile

For developers debugging a single contract type without Rust persistence:

```
# rust-backend/config.toml
[execution.remote]
system_enabled = true
rust_persist_enabled = false             # Rust computes, Java applies
emit_freeze_ledger_changes = true
emit_global_resource_changes = true
```

In this profile, `WriteMode.COMPUTE_ONLY` is returned, Java runs
`applyStateChangesToLocalDatabase`, and the Rust RocksDB stays empty.
This profile is explicitly not the acceptance profile, and results
from it are not citable as `RR` parity.

## Answering the key question

> Who writes the final state in this mode?

- `EE`: **Java** (via actuators + chainbase).
- `RR` canonical (`rust_persist_enabled=true`): **Rust** (via its own
  buffered storage engine). Java is a read-side mirror.
- `RR` compute-only (`rust_persist_enabled=false`): **Java** (via
  `RuntimeSpiImpl.apply*`). Developer profile only.
- `SHADOW`: not a Phase 1 acceptance path.

Any engineer encountering ambiguity should first check the active
`rust_persist_enabled` value and the `write_mode` field on the
execution response. Those two together determine the writer.

## Follow-up implementation items

These remain **open** — closing them is a coding task, not a doc task:

- [ ] Align `rust-backend/config.toml` and `rust-backend/crates/common/src/config.rs`
      comments so both point at this file as the policy source of truth.
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
