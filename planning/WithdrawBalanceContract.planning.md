• Here’s a concrete, code‑first plan to add WithdrawBalanceContract handling to the Rust backend, including all glue needed for end‑to‑end parity with the Java path.

Goal

- Execute WithdrawBalanceContract remotely with correct validation and effects.
- Update balance, zero allowance, set latestWithdrawTime, and respect cooldown/eligibility rules.
- Keep embedded fallback possible via a feature flag.

Key Design

- Add a system‑contract handler at the Rust “core service” layer (non‑VM path), mirroring freeze/vote patterns.
- Extend the execution result with a dedicated sidecar WithdrawChange so Java can apply allowance/time updates locally (like freeze ledger changes).
- Add storage adapter helpers to read Account.allowance and latest_withdraw_time and dynamic property witnessAllowanceFrozenTime.
- Gate behavior behind a config flag, with a pragmatic approach to “guard representative” detection.

Core Changes (Rust)

- Contract handler
    - Add execute_withdraw_balance_contract() under rust-backend/crates/core/src/service/contracts/withdraw.rs and export via contracts/mod.rs.
    - Wire into dispatcher in rust-backend/crates/core/src/service/mod.rs:195 (match arm for TronContractType::WithdrawBalanceContract).
- Validation rules in handler
    - Owner exists (get_account).
    - Owner is a witness (is_witness).
    - Cooldown check: now − latestWithdrawTime ≥ witnessAllowanceFrozenTime × FROZEN_PERIOD (86,400,000 ms).
    - Positive withdrawable amount:
        - Phase 1: use Account.allowance only (skip queryReward until delegation is ported).
    - Overflow check: balance + allowance.
- Effects in handler
    - Emit one AccountChange with balance += allowance, energy_used=0, bandwidth via existing calculator.
    - Add a “withdraw change” sidecar to set latestWithdrawTime and zero the allowance on Java side.

Execution Result + gRPC

- Extend execution result types
    - In rust-backend/crates/execution/src/tron_evm.rs define:
        - pub struct WithdrawChange { pub owner_address: Address, pub amount: i64, pub latest_withdraw_time: i64 }
        - Add pub withdraw_changes: Vec<WithdrawChange> to TronExecutionResult.
    - Re-export in rust-backend/crates/execution/src/lib.rs:16.
- Update gRPC proto
    - Edit framework/src/main/proto/backend.proto:
        - Add message WithdrawChange { bytes owner_address = 1; int64 amount = 2; int64 latest_withdraw_time = 3; }
        - Add repeated WithdrawChange withdraw_changes = 14; to ExecutionResult.
- Map to protobuf
    - In rust-backend/crates/core/src/service/grpc/conversion.rs:
        - Convert result.withdraw_changes into crate::backend::WithdrawChange in ExecuteTransactionResponse builder.

Storage Adapter Helpers (Rust)

- In rust-backend/crates/execution/src/storage_adapter/engine.rs add:
    - get_latest_block_header_timestamp() → read key latest_block_header_timestamp from properties DB.
    - get_witness_allowance_frozen_time() → read WITNESS_ALLOWANCE_FROZEN_TIME, default to 1 if missing.
    - get_account_allowance(&Address) → parse Account protobuf field 11 (varint).
    - get_account_latest_withdraw_time(&Address) → parse Account protobuf field 12 (varint).
- Keep writes for allowance/time on Java side via sidecar to avoid lossy set_account() (which serializes minimal fields).

Java Apply Path

- Apply withdraw changes after state changes:
    - In framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:
        - Add applyWithdrawChanges(ExecutionProgramResult, TransactionContext) (after applyStateChangesToLocalDatabase, alongside freeze/trc10/vote).
        - For each WithdrawChange: load account, set latestWithdrawTime, set allowance=0, store. Balance delta already applied by AccountChange.
    - No need to touch receipts for now (Actuator previously set withdraw amount into TransactionResultCapsule; remote path can omit).

Feature Flags

- Config
    - Add withdraw_balance_enabled to [execution.remote] in rust-backend/config.toml, default true or false per rollout preference.
    - In dispatcher at rust-backend/crates/core/src/service/mod.rs:195, gate the contract execution accordingly with a clear fallback error message to use Java path.

Guard Representatives

- Embedded checks “genesis guard rep cannot withdraw” use Args/Genesis config not available in Rust.
- Plan options:
    - Phase 1: Skip this check; log a warning about parity diff.
    - Optional: Add [execution.remote].guard_representatives_base58 = ["..."] and enforce if provided.

Parsing Input

- WithdrawBalanceContract carries only owner_address; use transaction.from as owner and allow empty data.

Bandwidth + AEXT

- Treat as system contract: energy_used=0.
- Use existing bandwidth calculator and tracked AEXT mode only if already enabled globally (no special changes here).

Testing

- Unit tests (Rust):
    - Happy path: witness with allowance, cooldown satisfied → balance increases; withdraw sidecar emitted; bandwidth > 0; energy 0.
    - No witness → validation error.
    - No allowance → validation error.
    - Cooldown not satisfied → validation error.
    - Feature flag disabled → returns “disabled, fallback to Java” error.
- Proto regen and compile checks in Rust and Java.

Risks / Parity Notes

- Delegation/mortgage queryReward is not ported; Phase 1 uses allowance only, matching current remote vote path note (“Skipping withdrawReward...”).
- Guard representative block needs either config or deferral; document as a known gap.
- set_account() is lossy; rely on sidecar for allowance/time to prevent wiping unrelated Account fields.

Step‑By‑Step Impl Order

1. Proto: add WithdrawChange and field in ExecutionResult, regen code.
2. Execution types: add WithdrawChange and withdraw_changes to TronExecutionResult.
3. Storage adapter: add getters for timestamp, frozen time, allowance, latestWithdrawTime.
4. Contract handler: implement execute_withdraw_balance_contract() in new contracts/withdraw.rs; wire into dispatcher and feature flag.
5. gRPC conversion: map withdraw_changes.
6. Java Runtime: add applyWithdrawChanges() and call it after state changes.
7. Tests: add Rust unit tests and basic integration check; build both Rust and Java.

