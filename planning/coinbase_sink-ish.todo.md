## Current Task: TRON‑Accurate Fee Handling (Remote Execution)

Context
- We compared embedded (execution+storage) vs remote (execution+storage) CSVs and observed systematic mismatches in `state_change_count` and state digest for many transactions. Remote execution appears to emit an EVM-style coinbase credit (miner tip) that should not exist on TRON.
- TRON fee semantics: no per‑tx miner/coinbase payout. Non‑VM txs pay flat bandwidth fees (burn or credit blackhole depending on `supportBlackHoleOptimization`). Witness rewards occur at block finalization, not per tx. VM txs consume energy and still do not credit coinbase.

Objective
- Modify the Rust backend execution path so it never emits Ethereum coinbase payouts and handles non‑VM fees accurately (burn vs. blackhole credit), bringing CSV parity with embedded: correct `state_change_count`, `energy_used` (0 for non‑VM), and matching state digests.

Non‑Goals (for this iteration)
- Implement full TRON fee accounting (stake/energy/bandwidth deduction, fee pool dynamics) identical to Java actuators.
- Change Java caller behavior unless gated behind explicit feature flags in a later phase.

Acceptance Criteria
- No `AccountChange` attributed to block coinbase/miner in remote results for any tx.
- Non‑VM value transfers: `energy_used = 0`; only two account deltas (sender minus amount+fee, recipient plus amount) plus optional blackhole credit if configured. If burn mode is on, no third-party credit delta is emitted.
- Execution CSV compare shows near‑100% accuracy for `state_change_count` and `state_digest_sha256` on the same tx set; `energy_used` aligns (0 for non‑VM).

High‑Level Plan (Phased)
1) Phase 1 – Parity Fix (no proto change):
   - Suppress EVM coinbase/priority fee at the source and stop enforcing Ethereum gas minima.
   - Post‑process to stabilize state change ordering for digest parity.
   - Simple non‑VM heuristic for 0 energy without adding fee deltas.
2) Phase 2 – Configurable TRON Fee Policy (no proto change):
   - Introduce `execution.fees` config (burn vs blackhole) and optional blackhole credit emission for VM path (default off) and non‑VM (conservative).
3) Phase 3 – Full Non‑VM Handling (proto + Java update):
   - Add tx kind to proto; process non‑VM fully in Rust without EVM; apply accurate fee semantics including blackhole credit or burn based on dynamic properties/config.

Key Code Touchpoints
- `rust-backend/crates/core/src/service.rs`
  - `convert_protobuf_transaction(...)`
  - `convert_protobuf_context(...)`
  - `convert_execution_result_to_protobuf(...)`
- `rust-backend/crates/execution/src/tron_evm.rs`
  - `setup_environment(...)`
  - `execute_transaction_with_state_tracking(...)`
  - `extract_state_changes_from_db(...)`
- `rust-backend/crates/execution/src/storage_adapter.rs`
  - Address utils (promote Base58 Tron → EVM address decoder from test to prod)
- `rust-backend/crates/common/src/config.rs` and `rust-backend/config.toml`
  - Add `execution.fees.*` configuration

Detailed TODOs

Phase 1 — Parity Fix (no proto changes)
[X] Suppress coinbase/priority fee credit
- [X] In `service.rs:convert_protobuf_transaction`, force `gas_price = 0` regardless of input, with a safety gate `execution.evm_eth_coinbase_compat` (default false). Document that this is for TRON parity.
- [X] In `tron_evm.rs:setup_environment`, set `env.block.basefee = 0` explicitly (if field exists in current REVM version). Keep `block.coinbase` set for opcode COINBASE correctness but ensure no rewards are distributed.

[X] Remove Ethereum‑specific gas minima
- [X] In `tron_evm.rs:execute_transaction_with_state_tracking`, remove the `tx.gas_limit < 21000` rejection. Only enforce `tx.gas_limit <= context.block_gas_limit`. Log a warning if the gas is unusually low to aid debug.

[X] Deterministic state change ordering (digest parity)
- [X] After `extract_state_changes_from_db()` returns, sort `state_changes` deterministically before returning the result:
  - AccountChange: by `address` ascending.
  - StorageChange: by `(address, key)` ascending.
- [X] Keep sorting local to execution result (do not mutate storage records order).

[X] Non‑VM heuristic energy fix (safe and conservative)
- [X] Define "likely non‑VM" as `tx.data.is_empty()` AND `to` present AND `code(to) is None`.
- [X] If likely non‑VM, set `energy_used = 0` in the final `TronExecutionResult`. Do not add any fee deltas here; leave fee effects to Java for now (this avoids accidental double‑counting).
- [X] Add debug logging when this fast‑path triggers (include `from`, `to`, amount, and reason).

[X] Unit tests (minimal)
- [X] Ensure no `AccountChange` for `block_coinbase` even when `energy_used > 0`.
- [X] Ensure sorting: two identical runs produce identical `state_changes` order.
- [X] Ensure non‑VM heuristic sets `energy_used = 0` when `to` has no code and `data` is empty.

[ ] Validation
- [ ] Re‑run `scripts/execution_csv_compare.py` on the same tx windows; aim for ~100% on `state_change_count` and state digest.
- [ ] Manually spot‑check transactions previously showing a third account delta (coinbase) — confirm absence.

Phase 2 — Configurable Fee Policy (no proto change)
[X] Configuration and plumbing
- [X] Extend `ExecutionConfig` with nested `ExecutionFeeConfig`:
  - `mode: "burn" | "blackhole" | "none"` (default: `"burn"`).
  - `support_black_hole_optimization: bool` (default: true).
  - `blackhole_address_base58: String` (default empty; required if `mode=blackhole`).
  - `experimental_vm_blackhole_credit: bool` (default: false; disabled by default to avoid double‑counting).
  - `non_vm_blackhole_credit_flat: Option<u64>` (SUN), optional flat fee for non‑VM when not deriving from dynamic props.
- [X] Add TOML examples under `[execution.fees]` and env overrides, e.g. `TRON_BACKEND__EXECUTION__FEES__MODE`.

[X] Address utilities
- [X] Promote `from_tron_address(...)` from `#[cfg(test)]` to production (new `common::address` module with full Base58Check implementation).
- [X] Validate checksum and 0x41 prefix; unit test round‑trip with known addresses.

[X] Optional blackhole credit emission (careful defaults)
- [X] After extracting and sorting state changes, if `fees.mode = "blackhole"` AND `experimental_vm_blackhole_credit = true`, append a synthetic `AccountChange` crediting blackhole by `estimated_fee = energy_used * context.energy_price` (approximation). Default OFF.
- [X] For likely non‑VM (heuristic), if `fees.mode = "blackhole"` AND `non_vm_blackhole_credit_flat` is set, append a synthetic `AccountChange` to blackhole for that flat value. Default NONE.
- [X] Do NOT emit anything in burn mode (no state deltas for fee sinks).
- [X] Add guard logs indicating this is an approximation until Phase 3.

[X] Tests and validation
- [X] Unit test: blackhole credit emission only when enabled; amount matches calculation; address decoding works.
- [X] CSV compare again: ensure no regressions to `state_change_count` parity in default config (`mode=burn`).

Phase 3 — Full Non‑VM Handling (proto + Java update)
[X] Protobuf
- [X] Add `enum TxKind { NON_VM = 0; VM = 1; }` and `tx_kind` in `TronTransaction`.
- [X] Regenerate protobuf files after schema changes.
- [X] Update Java caller to populate `tx_kind`.

[X] Execution path
- [X] In core service, branch on `tx_kind`:
  - For `NON_VM`: bypass EVM entirely. Use `StorageModuleAdapter` to load sender/recipient and apply TRON value transfer and fee semantics.
  - `energy_used = 0`; compute `bandwidth_used` based on payload size per TRON rules; update `resource_usage` if needed.
  - Fee handling:
    - If `fees.mode="burn"`: no state delta (supply accounting is elsewhere).
    - If `fees.mode="blackhole"`: credit blackhole account by the fee.
- [X] For `VM`: continue REVM execution; still no per‑tx miner/coinbase credit (with fallback heuristics).

[ ] Dynamic properties integration (optional)
- [ ] Read `supportBlackHoleOptimization` and fee parameters from dynamic properties DB (via `StorageModuleAdapter`) to auto‑select fee mode and amounts; config acts as fallback.

[X] Tests and validation
- [X] Unit tests for non‑VM path: bandwidth calculation, address conversions, TxKind enum handling.
- [X] Integration test framework setup (mocked, ready for full system testing).
- [ ] End‑to‑end CSV compare in both modes (burn and blackhole) across a block window with mixed tx types.

Risk Mitigation & Compatibility
- Default behavior remains parity‑safe: coinbase suppressed, `fees.mode = burn`, experimental emissions OFF.
- Introduce a temporary `execution.evm_eth_coinbase_compat` flag (default false) to restore old behavior if needed during rollout.
- Sorting only affects return payload ordering, not persisted DB order.

Open Questions / Follow‑ups
- What exact fee values should be emitted for non‑VM remote path to match Java actuators? If dynamic properties are required, Phase 3 should include reading them to compute accurate fees.
- Should remote execution ever emit fee‑related deltas for VM txs, or should all fee effects remain Java‑side until full parity is proven? Current proposal keeps VM fees non‑emitting by default.
- If state digest mismatch persists after coinbase suppression and sorting, audit REVM vs Java EVM differences (e.g., refunds, precompile side‑effects, account creation edge cases) on the mismatching tx set.

Owner Map (by file)
- `crates/core/src/service.rs`: tx/context conversion, result conversion, optional non‑VM heuristic and fee post‑processing gates.
- `crates/execution/src/tron_evm.rs`: env setup, gas/basefee handling, state change extraction and sorting, removal of Ethereum gas minima.
- `crates/execution/src/storage_adapter.rs`: address utilities (Base58 decode), optional account/code queries for heuristics.
- `crates/common/src/config.rs` + `rust-backend/config.toml`: config struct and defaults for `execution.fees.*`, rollout flags.

Verification Checklist (before merge)
[ ] Unit tests added/updated for coinbase suppression, sorting, heuristics, address utils.
[ ] Default config produces no coinbase deltas; CSV compare shows improved parity on provided sample files.
[ ] Docs: `config.toml` and README updated with `execution.fees` and rollout flags.
[ ] Logging at debug level for new branches; no excessive info-level noise.
[ ] Backout plan documented (`execution.evm_eth_coinbase_compat=true`).
