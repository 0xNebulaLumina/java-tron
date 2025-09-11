## TRON‑Accurate Fee Handling: Phase 3 Implementation (COMPLETED)

**Status: Phase 3 Critical Fixes Implemented**

The Phase 3 fixes have been successfully implemented to address the "Insufficient balance" halts and parity gaps identified in the planning document. The following critical issues have been resolved:

### Implemented Fixes

1. **Fixed non-VM TRX fee deduction** (rust-backend/crates/core/src/service.rs:213-224)
   - Removed forced TRX fee calculation that was causing "Insufficient balance" errors
   - Default fee is now 0 unless explicitly configured via `non_vm_blackhole_credit_flat`
   - Properly implements TRON's free bandwidth semantics

2. **Removed nonce increment for NON_VM transactions** (rust-backend/crates/core/src/service.rs:238)
   - Non-VM TRX transfers no longer increment EVM nonce (TRON-accurate behavior)
   - EVM nonce is preserved for legitimate VM transactions only

3. **Made blackhole credit optional behind config** (rust-backend/crates/core/src/service.rs:272-328)
   - Blackhole credits only apply when fee_amount > 0 and properly configured
   - Supports both "burn" (default) and "blackhole" fee modes
   - Prevents unnecessary state deltas when no fees are involved

4. **Fixed deterministic context** (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:337-340)
   - Removed 0/now fallbacks in Java `RemoteExecutionSPI.buildExecuteTransactionRequest()`
   - Now requires `BlockCapsule` and fails fast if missing to ensure deterministic replay
   - Eliminates non-deterministic timestamp/block data during CSV generation

5. **Kept TRC-10 on Java path** (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:284-291)
   - Added `-Dremote.exec.trc10.enabled=false` (default) system property gate
   - Prevents TRC-10 `TransferAssetContract` from routing to Rust backend
   - Maintains correct TRC-10 balance updates via Java actuators until Rust storage supports TRC-10 ledgers

6. **Enhanced proto for future TRC-10 support** (framework/src/main/proto/backend.proto)
   - Added `ContractType` enum matching TRON Protocol.ContractType
   - Added `contract_type` and `asset_id` fields to `TronTransaction`
   - Updated Java code to populate these fields for better transaction classification

### Expected Behavior Changes

With these fixes, the Phase 3 remote execution should:
- **No longer halt** at block 2040 with "Insufficient balance" errors
- **Produce CSV parity** with embedded execution for `state_change_count` and `state_digest_sha256`  
- **Generate 0 energy_used** for non-VM TRX transfers (TRON-accurate)
- **Only emit fee deltas** when explicitly configured (burn mode = no deltas by default)
- **Maintain TRC-10 correctness** by keeping asset transfers on proven Java actuators

### Testing Recommendations

The implementation should now allow:
- Re-running the halted Phase 3 execution past block 2040
- Comparing CSV results with `scripts/execution_csv_compare.py` for improved parity
- Validating that non-VM transactions have `energy_used = 0` and correct state change counts

## TRON‑Accurate Fee Handling: Phase 3 Addendum (Original Plan)

Context
- Recent remote runs halted due to enforced non‑VM TRX fee deduction ("Insufficient balance …") and parity gaps. This addendum documents next steps to restore parity and correctness without starting implementation.

Behavioral Invariants
- No per‑transaction coinbase/miner credits on TRON (both VM and non‑VM).
- Non‑VM TRX transfers: energy_used = 0; only sender/recipient account deltas; fee is burn (no account delta) or blackhole credit (optional, config‑gated). Do not increment EVM nonce.
- TRC‑10 (TransferAssetContract) is non‑VM; never run it through TVM/EVM for state updates.
- Context must be deterministic (block number, timestamp, hash, coinbase) — no 0/now fallbacks during replay.

High‑Impact Decisions
- TRC‑10 routing: Keep TRC‑10 on Java actuators (still using remote storage) until Rust storage/execution can update TRC‑10 ledgers correctly. Do not treat TRC‑10 as TRX or VM.
- Proto enrichment (recommended): Add `contract_type` and `asset_id` to requests; keep `tx_kind` for coarse NON_VM vs VM classification.

Detailed TODOs
1) Proto & Classification
- Extend `backend.proto`:
  - Add `contract_type` enum aligned with TRON Protocol.ContractType.
  - Add `asset_id` for TRC‑10 `TransferAssetContract`.
  - Preserve `tx_kind` (NON_VM/VM).
- Java `RemoteExecutionSPI`:
  - Populate `contract_type` and `asset_id` (when applicable).
  - Route: TransferContract → NON_VM to Rust; TransferAssetContract → NON_VM but stay on Java path by default; gate with `-Dremote.exec.trc10.enabled=true`.

2) Rust — Non‑VM TRX (Safe Defaults)
- Fee deduction:
  - Default: no forced TRX fee deduction. Only deduct when `execution.fees.non_vm_blackhole_credit_flat` is set.
  - If `fees.mode = "burn"` and no flat fee: fee_amount = 0 (no account delta, no balance check for fees).
  - If `fees.mode = "blackhole"` and flat fee is set: credit blackhole by flat amount; do not block tx for fee if transfer value is affordable.
- Nonce: do not increment EVM nonce for NON_VM TRX.
- Reporting: `energy_used = 0`; keep bandwidth for reporting only; do not map to TRX fee unless configured.
- Deterministic state change sort: AccountChange by address; StorageChange by (address, key).
- Logging: debug when fee is skipped or blackhole credit applied; only error on real state inconsistencies.

3) Rust — TRC‑10 (Planned)
- Storage: expose `account-asset`, `asset-issue-v2` via storage engine + adapter.
- Execution: implement TRC‑10 non‑VM processor that updates TRC‑10 balances (not TRX), handles account creation rules, and emits deterministic deltas.
- Rollout: behind `execution.non_vm.trc10.enabled` (default false) until parity is validated.

4) Deterministic Context
- Java `RemoteExecutionSPI#buildExecuteTransactionRequest`:
  - Remove fallbacks to 0/now/zero for context.
  - Require `BlockCapsule`; populate block_number, block_timestamp, block_hash, coinbase strictly from it.
  - If absent: fail fast (warn + skip) to avoid non‑deterministic CSV.
- Rust context conversion: keep `basefee=0`, `gas_price=0` (unless `evm_eth_coinbase_compat=true`).

5) Fee Policy Configuration
- `execution.fees.mode`: `"burn" | "blackhole" | "none"` (default `"burn"`).
- `execution.fees.blackhole_address_base58`: required only in blackhole mode.
- `execution.fees.support_black_hole_optimization`: bool (default true).
- `execution.fees.experimental_vm_blackhole_credit`: bool (default false) — optional VM approximation.
- `execution.fees.non_vm_blackhole_credit_flat`: Option<u64> SUN (default None) — optional NON_VM flat credit.
- Defaults ensure parity: burn mode + no non‑VM flat fee = no extra deltas or halts.

6) VM Path Hygiene
- Keep `gas_price = 0`, `basefee = 0` to avoid coinbase payouts.
- No Ethereum gas minima; enforce only `gas_limit <= block_gas_limit`.
- Optional VM blackhole credit behind `experimental_vm_blackhole_credit` (default off).

7) CSV & Parity Validation
- Non‑VM TRX in burn mode: expect exactly two account deltas; +1 blackhole delta only if configured.
- TRC‑10: leave on Java path until Rust is ready; CSV must match embedded before enabling.
- No coinbase deltas in any tx type.
- Re‑run `scripts/execution_csv_compare.py` and target ~100% for `state_change_count` and `state_digest_sha256`.

8) Rollout & Flags
- `execution.evm_eth_coinbase_compat` (default false): emergency toggle for legacy gas semantics.
- `execution.non_vm.trx.enabled` (default true): non‑VM TRX path on/off.
- `execution.non_vm.trc10.enabled` (default false): TRC‑10 path gate.
- `execution.fees.experimental_vm_blackhole_credit` (default false): VM approximation gate.

9) Tests
- Rust unit:
  - NON_VM TRX with zero balance + burn mode succeeds, `energy_used=0`, 2 deltas, no nonce++.
  - NON_VM TRX with flat blackhole fee credits blackhole; still `energy_used=0`.
  - No coinbase AccountChange in VM when `gas_price=0`.
  - Deterministic ordering stable across runs.
- Java unit: `RemoteExecutionSPI` fills context strictly from `BlockCapsule`.
- Integration: CSV parity restored; no halts.

10) Risks & Backout
- Risks: TRC‑10 misrouting corrupting TRX balances; mitigated by keeping TRC‑10 on Java until ready. Flat blackhole credit may mislead analysis; disabled by default.
- Backout: flip `execution.evm_eth_coinbase_compat=true` or disable `execution.non_vm.trx.enabled` to return to Java actuators temporarily.

Owner Map (delta)
- Proto & Java: backend.proto; RemoteExecutionSPI (classification, context hygiene, feature flags)
- Rust core: crates/core/src/service.rs (non‑VM TRX behavior, blackhole gating, context)
- Rust storage: storage engine + adapter for TRC‑10 databases
- Config: rust-backend/config.toml; crates/common/src/config.rs (fees + flags)

Rationale
- The prior halt was caused by unconditional TRX fee enforcement for non‑VM. TRON uses free bandwidth first; fee deductions must not be forced by default. This plan restores parity safely, keeps TRC‑10 correct, and provides clear rollout gates.

