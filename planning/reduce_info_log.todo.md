# Reduce INFO Logging During Sync (remote-remote) — TODO

Status: **Phase 2.R implemented** (code changes done, validation pending)
Owners: Rust `rust-backend` (`crates/core` gRPC, `crates/execution`)
Target: improve **remote-remote** sync throughput by reducing hot-path **INFO** logging overhead (CPU + I/O) on Rust backend.

## Problem Statement
In remote-remote mode, the node can process blocks far faster than real-time. Today, several hot loops emit INFO logs **per transaction**.

That volume is expensive because:
- Rust backend logs multiple INFO lines per tx, and some INFO logs trigger **extra storage reads** purely to print debug fields.

Goal: keep INFO as **operator-facing progress/anomaly** logs, move per-tx internals to DEBUG/TRACE, and add throttled progress summaries to retain visibility.

## Evidence (current state)
### Rust backend (sample run)
From existing run artifacts:
- `remote-rust.7feee0d.log` contains ~**117,167 INFO** lines.
- Many are per-transaction in gRPC execution:
  - `rust-backend/crates/core/src/service/grpc/mod.rs:1065` (buffered writes mode)
  - `rust-backend/crates/core/src/service/grpc/mod.rs:1096` (blackhole balance BEFORE — includes storage reads + address conversions)
  - `rust-backend/crates/core/src/service/grpc/mod.rs:1107`, `rust-backend/crates/core/src/service/grpc/mod.rs:1111` (execution start + non-VM result)
  - `rust-backend/crates/core/src/service/grpc/mod.rs:1212` (blackhole balance AFTER — extra read via fresh adapter)
  - `rust-backend/crates/core/src/service/grpc/mod.rs:1222` (tx executed)
  - `rust-backend/crates/core/src/service/grpc/mod.rs:1236` (committed writes)
  - `rust-backend/crates/core/src/service/grpc/mod.rs:1248` (dropping buffer)
- EVM state-change dumping at INFO:
  - `rust-backend/crates/execution/src/tron_evm.rs:1059`, `rust-backend/crates/execution/src/tron_evm.rs:1124`
  - Per-change loop: `rust-backend/crates/execution/src/tron_evm.rs:1128` / `rust-backend/crates/execution/src/tron_evm.rs:1133`

## Constraints / Risks
- We must not reduce WARN/ERROR logs that surface correctness issues during sync.
- Some INFO logs were added to debug parity (Phase B / blackhole / touched keys). We still need a *fast* way to re-enable them for debugging.
- Avoid introducing correctness changes (logging-only PR): no behavior changes beyond log verbosity and progress summaries.
- Avoid hidden costs: any log that computes values before checking level must be guarded (`tracing::enabled!`).

## Design Principles
1) INFO = **progress + anomalies**, not inner-loop state.
2) DEBUG/TRACE = detailed per-block/per-tx internals.
3) Add **throttled progress** logs so operators still see liveness.
4) Make “verbose logging for debugging” a deliberate opt-in:
   - Rust: `RUST_LOG` target overrides and/or explicit config booleans.

## Baseline harness (must do first)
### TODOs — Phase 0: Measure & set acceptance targets
- [ ] Pick a fixed replay/sync window for benchmarking (e.g., block height A→B).
- [ ] Collect baseline remote-remote metrics:
  - [ ] wall-clock seconds, blocks/sec, tx/sec
  - [ ] CPU%, disk write MB/s
  - [ ] log line counts (INFO lines separately)
- [ ] Record baseline log sources:
  - [ ] Rust: `INFO` counts in gRPC execution path and EVM state-change dumping.
- [ ] Define success criteria (initial):
  - [ ] **Rust**: remove per-tx INFO logging from hot path (INFO becomes periodic summary only).
  - [ ] **Throughput**: measurable improvement (target TBD; expect “significant” if logging is currently blocking).

Acceptance for Phase 0
- [ ] A repeatable “before” benchmark with all metrics captured.

---

## Rust backend plan

### TODOs — Phase 1.R (no-code knobs, validate quickly)
- [ ] Run backend with explicit `RUST_LOG` for perf runs:
  - [ ] `RUST_LOG=warn` (global)
  - [ ] Or targeted: `RUST_LOG=warn,tron_backend_core::service::grpc=warn,tron_backend_execution=warn`
- [ ] Confirm perf changes vs baseline with the same sync window.

Acceptance for Phase 1.R
- [ ] With only `RUST_LOG` changes, log volume drops and throughput improves (if backend logging is on the hot path).

### TODOs — Phase 2.R (code changes: remove per-tx INFO + guard expensive work)

#### 2.R.1 gRPC execute hot-path logs (core)
File: `rust-backend/crates/core/src/service/grpc/mod.rs` (multiple callsites around per-tx execution)

Plan:
- [x] Convert per-tx INFO logs to DEBUG:
  - [x] "Using buffered writes …"
  - [x] "Executing NON_VM/VM …"
  - [x] "Transaction executed successfully …"
  - [x] "Committed … touched keys …"
  - [x] "Dropping buffer …"
- [x] Ensure expensive log work is behind a `tracing::enabled!(Level::DEBUG)` check:
  - [x] Blackhole BEFORE/AFTER balance logs (currently cause extra storage reads and conversions):
    - [x] Only compute and read balances when DEBUG is enabled or when an explicit config flag is set.
    - [x] Prefer "summary only" at INFO; reserve per-tx addresses/ids for DEBUG.
- [ ] Add periodic INFO summary (every T seconds) instead of per-tx INFO:
  - [ ] tx count, tx/sec, avg commit ops, touched keys histogram, last block number seen

#### 2.R.2 EVM state-change extraction logs (execution)
File: `rust-backend/crates/execution/src/tron_evm.rs:1059..1137`

Plan:
- [x] Demote "Extracting … state change records" and "Extracted and sorted …" from INFO to DEBUG.
- [x] Move the per-change loop to TRACE and guard it:
  - [x] `if tracing::enabled!(tracing::Level::TRACE) { for … }`
- [ ] (Optional) Add sampling: log only first N changes when DEBUG is enabled.

#### 2.R.3 Contract-specific logs
Files (examples; scan for `info!` in hot code paths):
- `rust-backend/crates/core/src/service/mod.rs` (contract execution logs)
- `rust-backend/crates/core/src/service/contracts/*`

Plan:
- [x] Identify any `info!` that runs per tx and demote to DEBUG (unless it's a rare anomaly).
- [x] Keep WARN/ERROR for exceptional situations unchanged.

**Files updated:**
- `rust-backend/crates/core/src/service/mod.rs` - all per-tx `info!` → `debug!`
- `rust-backend/crates/core/src/service/contracts/freeze.rs` - all `info!` → `debug!`
- `rust-backend/crates/core/src/service/contracts/delegation.rs` - all `info!` → `debug!`
- `rust-backend/crates/core/src/service/contracts/withdraw.rs` - all `info!` → `debug!`
- `rust-backend/crates/execution/src/storage_adapter/engine.rs` - all `tracing::info!` → `tracing::debug!`
- `rust-backend/crates/execution/src/storage_adapter/database.rs` - all `tracing::info!` → `tracing::debug!`
- `rust-backend/crates/execution/src/storage_adapter/in_memory.rs` - `tracing::info!` → `tracing::debug!`
- `rust-backend/crates/execution/src/tron_evm.rs` - per-tx logs demoted + TRACE guard added

Acceptance for Phase 2.R
- [ ] With default `RUST_LOG` (or default filter), backend INFO logs are not emitted per tx; only periodic summaries remain.
- [ ] No extra storage reads are performed solely for logging unless DEBUG/TRACE is enabled.

---

## Cross-cutting: “Sync progress” without spam
### TODOs — Phase X (operator-facing progress logs)
- [ ] Add a periodic summary log on Rust backend (if not already present):
  - [ ] tx/sec, average writes/touched keys, last block
- [ ] Ensure these are the *only* INFO logs that scale with sync speed.

Acceptance for Phase X
- [ ] Operators can confirm liveness during sync with minimal log noise.

---

## Validation checklist (after implementation)
- [ ] Re-run the fixed-height benchmark window.
- [ ] Verify improvements:
  - [ ] Rust INFO is periodic summary only (no per-tx INFO)
  - [ ] Throughput increases (blocks/sec, tx/sec)
- [ ] Regression safety:
  - [ ] No new WARN/ERROR introduced by logging changes.
  - [ ] Debug toggles restore deep logs when needed.
  - [ ] No functional behavior changes beyond logging.

