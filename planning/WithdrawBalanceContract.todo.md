WithdrawBalanceContract Remote Execution — Detailed Plan

Context
- Repo: java-tron with unified Rust backend under `rust-backend/`
- Scope: Implement non-VM handling for `WithdrawBalanceContract` in the Rust backend, wire it through gRPC, and apply effects in Java via ExecutionSPI, keeping embedded fallback possible.
- Strategy: Mirror the existing system-contract patterns (freeze/unfreeze/vote) with a new withdraw handler, add a dedicated sidecar payload (WithdrawChange) for the Java side to update Account.allowance and latestWithdrawTime reliably, and produce the expected AccountChange balance delta.

Goals (Definition of Done)
- Remote execution supports `WithdrawBalanceContract` when enabled by config.
- Validation rules match embedded: owner exists, is witness, cooldown respected, allowance positive, and no overflow.
- Effects match embedded: owner balance += allowance, owner allowance set to 0, `latestWithdrawTime = latest_block_header_timestamp`.
- Zero energy; bandwidth accounted; deterministic state change ordering.
- Java applies sidecar updates (allowance, latestWithdrawTime) after state changes.
- Feature flag allows fallback to Java when disabled.

Non‑Goals / Phase 1 Limitations
- Delegate/mortgage reward recalculation (`mortgageService.queryReward()`/`withdrawReward`) stays unported. Phase 1 uses Account.allowance only (consistent with current vote witness note about skipping withdrawReward).
- Guard Representative block (Genesis GR cannot withdraw) is optional via config (no embedded Args in Rust).

Architecture Overview
- Dispatch: `BackendService.execute_non_vm_contract` routes `WithdrawBalanceContract` to a new handler `contracts/withdraw.rs`.
- Storage: read account protobuf fields for allowance (field 11) and latest_withdraw_time (field 12); read dynamic props: `LATEST_BLOCK_HEADER_TIMESTAMP`, `WITNESS_ALLOWANCE_FROZEN_TIME`.
- Result: emit one `AccountChange` (balance delta) and a sidecar `WithdrawChange {owner, amount, latest_withdraw_time}`. Energy=0, logs empty.
- Apply (Java): `RuntimeSpiImpl` applies `WithdrawChange` by setting Account.allowance=0 and latestWithdrawTime=now.

High‑Level TODOs (Checklists)

1) Proto Schema: backend.proto
- [ ] Add message `WithdrawChange { bytes owner_address; int64 amount; int64 latest_withdraw_time; }`.
- [ ] Add `repeated WithdrawChange withdraw_changes = 14;` in `ExecutionResult`.
- [ ] Regenerate Rust gRPC bindings via `crates/core/build.rs` (tonic-build). Ensure no field number conflicts.
- [ ] Regenerate Java gRPC model at build-time (done by Gradle); adjust client mapping if required.

2) Execution Types (Rust)
- Files: `rust-backend/crates/execution/src/tron_evm.rs`, `rust-backend/crates/execution/src/lib.rs`
- [ ] Add `pub struct WithdrawChange { owner_address: Address, amount: i64, latest_withdraw_time: i64 }`.
- [ ] Extend `TronExecutionResult` with `pub withdraw_changes: Vec<WithdrawChange>`.
- [ ] Re-export `WithdrawChange` in `lib.rs` for consumers.
- [ ] Ensure default constructors/initialization fill `withdraw_changes`.

3) Storage Adapter Helpers (Rust)
- File: `rust-backend/crates/execution/src/storage_adapter/engine.rs`
- [ ] `get_latest_block_header_timestamp() -> Result<i64>`: read key `latest_block_header_timestamp` from `properties` DB (see Java key `LATEST_BLOCK_HEADER_TIMESTAMP`).
- [ ] `get_witness_allowance_frozen_time() -> Result<i64>`: read key `WITNESS_ALLOWANCE_FROZEN_TIME` (default 1 if absent). Multiply by `FROZEN_PERIOD` (86,400,000 ms) in handler.
- [ ] `get_account_allowance(address: &Address) -> Result<i64>`: parse Account protobuf field 11 (varint) from `account` DB.
- [ ] `get_account_latest_withdraw_time(address: &Address) -> Result<i64>`: parse Account protobuf field 12 (varint) from `account` DB.
- [ ] Ensure `is_witness(address)` and `get_witness(address)` already exist (present). Reuse them.
- Notes:
  - Keep parse routines robust; reuse existing lightweight protobuf parsing helpers if available.
  - Do not write allowance/time in Rust (avoid lossy `set_account`); Java applies via sidecar.

4) Core Service: Withdraw Handler (Rust)
- New file: `rust-backend/crates/core/src/service/contracts/withdraw.rs`
- Export in `rust-backend/crates/core/src/service/contracts/mod.rs`.
- Wire in dispatcher in `rust-backend/crates/core/src/service/mod.rs`:
  - [ ] Add match arm for `TronContractType::WithdrawBalanceContract` gated by `execution.remote.withdraw_balance_enabled`.
  - [ ] Error string on disabled: "WITHDRAW_BALANCE_CONTRACT execution is disabled - falling back to Java".
- Handler signature:
  - `fn execute_withdraw_balance_contract(&self, storage: &mut EngineBackedEvmStateStore, tx: &TronTransaction, ctx: &TronExecutionContext) -> Result<TronExecutionResult, String>`
- Validation in handler:
  - [ ] Owner account exists.
  - [ ] Owner is a witness (`is_witness(owner)`).
  - [ ] Guard Rep optional: if configured, check and reject with same message as embedded.
  - [ ] Cooldown: `now_ms - latest_withdraw_time >= witnessAllowanceFrozenTimeDays * FROZEN_PERIOD`.
  - [ ] Allowance > 0 (Phase 1: skip `withdrawReward` delegation). Error: "witnessAccount does not have any reward".
  - [ ] Overflow check when adding allowance to balance.
- Effects:
  - [ ] New balance = old_balance + allowance; persist via `set_account(owner, new_account)`.
  - [ ] Produce `TronStateChange::AccountChange { old_account, new_account }`.
  - [ ] Compute bandwidth_used via existing calculator; energy_used=0, logs=[]; aext_map remains empty unless global tracked mode is used (optional parity).
  - [ ] Emit `WithdrawChange { owner_address, amount=allowance, latest_withdraw_time=now_ms }` in `withdraw_changes`.
  - [ ] Do NOT modify allowance / latestWithdrawTime here (Java applies sidecar for parity and to avoid lossy serialization).
  - [ ] Deterministic sort of state changes (same logic as others).

5) gRPC Conversion (Rust)
- File: `rust-backend/crates/core/src/service/grpc/conversion.rs`
- [ ] Convert `TronExecutionResult.withdraw_changes` to protobuf `withdraw_changes` in `ExecuteTransactionResponse`.
- [ ] Address conversions: use `add_tron_address_prefix` for owner_address.
- [ ] No changes needed for request conversion (contract has no fields; `from` is the owner).

6) Config & Feature Flag (Rust)
- File: `rust-backend/config.toml`
- [ ] Add `[execution.remote] withdraw_balance_enabled = true` (default per rollout decision).
- [ ] Optional: `guard_representatives_base58 = []` to enforce GR restriction if provided.
- File: `rust-backend/src/main.rs`
- [ ] Log flag on startup for visibility.

7) Java SPI & Runtime Apply (Java)
- Interface: `framework/src/main/java/org/tron/core/execution/spi/ExecutionSPI.java`
  - [ ] Add nested `class WithdrawChange` similar to others (fields: `ownerAddress`, `amount`, `latestWithdrawTime`).
  - [ ] Extend `ExecutionResult` to carry `List<WithdrawChange> withdrawChanges` with accessors.
- Result wrapper: `framework/src/main/java/org/tron/core/execution/spi/ExecutionProgramResult.java`
  - [ ] Add `List<ExecutionSPI.WithdrawChange> withdrawChanges`.
  - [ ] Populate from Remote ExecutionResult in the remote SPI conversion path (usually in RemoteExecutionSPI; if not present, add mapping wherever the gRPC response is converted to `ExecutionProgramResult`).
  - [ ] Ensure `toExecutionResult()` includes an empty list for withdraw in embedded conversions to avoid NPE.
- Runtime apply: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
  - [ ] Implement `applyWithdrawChanges(ExecutionProgramResult, TransactionContext)`:
        - For each change: load account; set allowance = 0; set latestWithdrawTime = latest_withdraw_time; store.
        - Log applied amount and timestamp.
  - [ ] Call `applyWithdrawChanges()` after `applyStateChangesToLocalDatabase()` and before building final result (order consistent with freeze/trc10/vote patterns).

8) Testing Plan
- Rust unit tests:
  - [ ] Happy path: witness with allowance > 0 and cooldown satisfied → balance increases, `withdraw_changes` emitted, energy=0, bandwidth>0.
  - [ ] Not witness → validation error.
  - [ ] No allowance → validation error.
  - [ ] Cooldown not elapsed → validation error with message: "The last withdraw time is <ts>, less than 24 hours" (match embedded message where feasible).
  - [ ] Feature flag disabled → returns specific disabled message (fallback).
  - [ ] Overflow guard → proper error.
- Java tests (smoke/integration):
  - [ ] With remote execution enabled, execute WithdrawBalance; verify Account.balance delta, allowance==0, latestWithdrawTime set, and receipt success.
  - [ ] Verify `RuntimeSpiImpl` applied changes even when state change had only AccountChange.
- Proto compatibility:
  - [ ] Build Rust and Java to ensure generated types align.

9) Logging & Observability
- [ ] Add `info!` start/end logs for handler, including owner (base58), amount, and cooldown decision.
- [ ] Warn when GR list is configured and matches (blocked).
- [ ] Warn when delegation reward path is skipped (Phase 1 limitation).
- [ ] Trace state change ordering and bandwidth for parity debugging.

10) Rollout & Backward Compatibility
- Default behavior via flag: start disabled on mainnet‑like deployments; enable in controlled testing.
- Remote → Java fallback preserved by error string/flag check.
- New proto fields are additive; older nodes ignore unknown fields.
- Embedded mode unaffected; `ExecutionProgramResult` additions remain optional.

Validation Rules & Error Messages (Parity‑aligned)
- Invalid address or missing account: reuse existing address/account checks (error text consistent with other handlers).
- Not a witness: "account <addr> not exist as witness" or concise equivalent used elsewhere (match existing style in vote handler).
- Cooldown: "The last withdraw time is <ts>, less than 24 hours" (compute 24h from dynamic store `WITNESS_ALLOWANCE_FROZEN_TIME * FROZEN_PERIOD`).
- No reward: "witnessAccount does not have any reward" when allowance <= 0 (Phase 1).
- Feature disabled: "WITHDRAW_BALANCE_CONTRACT execution is disabled - falling back to Java".

Open Questions / Decisions
- Guard Representative restriction: implement via optional `[execution.remote].guard_representatives_base58` list; otherwise, log and skip.
- AEXT tracking: do we track bandwidth AEXT for this contract? Default to no unless globally in "tracked"; keep behavior consistent with other non‑VM handlers.
- Receipt `withdrawAmount`: embedded actuator sets it in `TransactionResultCapsule`; remote path currently doesn’t emit per‑receipt fields—OK to omit for now; can be added later through SPI if needed.

File Map & Edits
- Proto: `framework/src/main/proto/backend.proto` (+WithdrawChange, +ExecutionResult.withdraw_changes)
- Rust execution types: `rust-backend/crates/execution/src/tron_evm.rs`, `rust-backend/crates/execution/src/lib.rs`
- Rust storage adapter: `rust-backend/crates/execution/src/storage_adapter/engine.rs`
- Rust core service (new): `rust-backend/crates/core/src/service/contracts/withdraw.rs`
- Rust contracts mod: `rust-backend/crates/core/src/service/contracts/mod.rs`
- Rust dispatcher: `rust-backend/crates/core/src/service/mod.rs`
- Rust grpc conversion: `rust-backend/crates/core/src/service/grpc/conversion.rs`
- Rust config: `rust-backend/config.toml`, `rust-backend/src/main.rs` (log flags)
- Java SPI: `framework/src/main/java/org/tron/core/execution/spi/ExecutionSPI.java`
- Java result wrapper: `framework/src/main/java/org/tron/core/execution/spi/ExecutionProgramResult.java`
- Java runtime apply: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`

Acceptance Criteria (Quick Checklist)
- [ ] With flag enabled, a test WithdrawBalance tx executes remotely and returns success, energy=0, correct bandwidth, and an AccountChange reflecting balance += allowance.
- [ ] Java applies allowance reset to 0 and latestWithdrawTime update from `withdraw_changes`.
- [ ] With flag disabled, Java path handles WithdrawBalance (no remote changes attempted).
- [ ] Unit/integration tests pass; no regressions in existing system-contract handlers.
- [ ] Logs show clear enablement and operation details.

Implementation Notes
- Keep changes minimal and consistent with existing style in freeze/vote handlers.
- Reuse dynamic property keys and existing protobuf parsing helpers to avoid adding heavy dependencies.
- Avoid writing non‑essential Account fields from Rust to prevent clobbering—use sidecar for allowance/time.
- Sort state changes deterministically (address, and AccountChange before StorageChange) for CSV parity.

