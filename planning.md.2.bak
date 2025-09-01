I want to compare the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,

So I run
```
nohup java -Xms9G -Xmx9G -XX:ReservedCodeCacheSize=256m \
     -XX:MetaspaceSize=256m -XX:MaxMetaspaceSize=512m \
     -XX:MaxDirectMemorySize=1G -XX:+PrintGCDetails \
     -XX:+PrintGCDateStamps  -Xloggc:gc.log \
     -XX:+UseConcMarkSweepGC -XX:NewRatio=2 \
     -XX:+CMSScavengeBeforeRemark -XX:+ParallelRefProcEnabled \
     -XX:+HeapDumpOnOutOfMemoryError \
     -XX:+UseCMSInitiatingOccupancyOnly  -XX:CMSInitiatingOccupancyFraction=70 \
     -Dexec.csv.enabled=true -Dexec.csv.stateChanges.enabled=true \
     -jar ./build/libs/FullNode.jar -c ./main_net_config_embedded.conf >> start.log 2>&1 &
```

and
```
nohup java -Xms9G -Xmx9G -XX:ReservedCodeCacheSize=256m \
     -XX:MetaspaceSize=256m -XX:MaxMetaspaceSize=512m \
     -XX:MaxDirectMemorySize=1G -XX:+PrintGCDetails \
     -XX:+PrintGCDateStamps  -Xloggc:gc.log \
     -XX:+UseConcMarkSweepGC -XX:NewRatio=2 \
     -XX:+CMSScavengeBeforeRemark -XX:+ParallelRefProcEnabled \
     -XX:+HeapDumpOnOutOfMemoryError \
     -XX:+UseCMSInitiatingOccupancyOnly  -XX:CMSInitiatingOccupancyFraction=70 \
     -Dexec.csv.enabled=true -Dexec.csv.stateChanges.enabled=true \
     -jar ./build/libs/FullNode.jar -c ./main_net_config_remote.conf \
     --execution-spi-enabled --execution-mode "REMOTE" >> start.log 2>&1 &
```

respectively.


The result csv are
+ output-directory/execution-csv/20250829-134317-a5713e7a-embedded-embedded.csv
+ output-directory/execution-csv/20250830-074028-d1b5315f-remote-remote.csv
respectively.


Then I run the compare tool:
```
python3 scripts/execution_csv_compare.py \
 --left output-directory/execution-csv/20250829-134317-a5713e7a-embedded-embedded.csv \
 --right output-directory/execution-csv/20250830-074028-d1b5315f-remote-remote.csv \
 --output reports/
```

The output is:
```
Starting execution CSV comparison...
Left file:  output-directory/execution-csv/20250829-134317-a5713e7a-embedded-embedded.csv
Right file: output-directory/execution-csv/20250830-074028-d1b5315f-remote-remote.csv
Output dir: reports/
Comparing fields: is_success, result_code, energy_used, return_data_hex, runtime_error, state_digest_sha256, state_change_count
Loading output-directory/execution-csv/20250829-134317-a5713e7a-embedded-embedded.csv...
Loaded 2116 rows from output-directory/execution-csv/20250829-134317-a5713e7a-embedded-embedded.csv
Loading output-directory/execution-csv/20250830-074028-d1b5315f-remote-remote.csv...
Loaded 888 rows from output-directory/execution-csv/20250830-074028-d1b5315f-remote-remote.csv
Performing join and comparison...

Reports generated in reports//
- Summary: comparison_summary.txt
- Mismatches: mismatches.csv
- JSON data: comparison_results.json

Comparison complete!
Matched transactions: 888
Mismatched transactions: 2116
Left-only transactions: 1228
Right-only transactions: 0
is_success: 99.3% accuracy (6/888 mismatches)
result_code: 99.3% accuracy (6/888 mismatches)
energy_used: 0.7% accuracy (882/888 mismatches)
return_data_hex: 100.0% accuracy (0/888 mismatches)
runtime_error: 99.3% accuracy (6/888 mismatches)
state_digest_sha256: 0.0% accuracy (888/888 mismatches)
state_change_count: 0.0% accuracy (888/888 mismatches)
Comparison completed successfully!
```

And I found the `state_change_count` in `20250829-134317-a5713e7a-embedded-embedded.csv` and `20250830-074028-d1b5315f-remote-remote.csv` are different.
I think it's due to coinbase/fee sink-ish account change in evm, which should not appear on TRON.

basically

Fee Semantics: EVM vs TRON

- EVM gas model:
    - Base fee: burned (EIP‑1559); pre‑1559 all gas went to miner.
    - Priority fee (“tip”): credited to the miner’s coinbase address in the same transaction state
transition.
    - Simple ETH transfer typically uses 21000 gas; coinbase gets the tip; this shows up as an account
delta to the miner.
- TRON resource model:
    - Resources: bandwidth (for non‑VM tx) and energy (only for VM tx).
    - Non‑VM (e.g., TransferContract): flat network fee plus optional “account creation” fee.
    - Fee destination:
    - Burn: if `supportBlackHoleOptimization` is enabled, the fee is burned — no account receives funds;
total supply decreases (no state change to a fee account).
    - Blackhole account: otherwise, fee is transferred to a designated “blackhole” account (an actual
account in state).
- Witness rewards: paid at block finalization in Manager#payReward (block/fee pool accounting), not
per transaction to a coinbase address. There is no EVM‑style miner tip credited in the same tx state
transitions.

What “TRON‑Accurate Fee Handling” Means

- Non‑VM transactions:
    - Do not credit any “coinbase/miner” account as part of the tx state changes.
    - Apply only:
    - Sender balance decrease by `amount + fee`.
    - Recipient balance increase by `amount`.
    - Fee handling:
      - If burning: no extra account delta (supply reduction handled by dynamic properties, not as an
account transfer).
      - If blackhole path: credit the blackhole account by `fee`.
- Energy used should be 0 (not 21000), because the EVM is not involved.
- VM transactions:
    - Charge energy according to TRON’s rules (caller vs origin share via consumeUserResourcePercent,
staking/freeze mechanics).
    - Still no per‑tx miner/coinbase credit. Fee burn or fee pool accounting follows TRON proposals and is
not an EVM miner payout.

Why the Remote Backend Should Not Emit Coinbase

- The Java actuator path (embedded) never credits a coinbase/miner address during a tx. It either burns or
credits blackhole (per config).
- The “extra” account change you saw in remote CSV is a carryover from Ethereum semantics. It should be
removed for TRON parity.
- If burning is enabled, the correct behavior is to emit no third account change at all.


Now, no need to code, think hard and  give me a plan on:

Modify Rust backend to emit TRON‑accurate fee handling for non‑VM (burn vs. blackhole credit), so it doesn’t report the extra “coinbase” change when it shouldn’t exist on TRON.


---


Goal

- Remove Ethereum-style coinbase credit from remote execution results and emit TRON-accurate fee effects
for non-VM txs (burn vs. blackhole), so state_change_count and state digest match embedded.

Behavioral Invariants

- No per-tx miner/coinbase credit on TRON (VM and non-VM).
- Non-VM (e.g., TransferContract): energy_used = 0; only sender/recipient deltas; fee is burn (no state
delta) or blackhole credit (1 extra delta).
- VM txs: charge/report energy used; no miner/coinbase credit; any fee burn/blackhole accounting should
not appear as EVM state changes.

Phase 1 – Parity Fix (no proto change)

- Suppress EVM coinbase payout:
    - service: set gas_price = 0 in convert_protobuf_transaction to prevent REVM from moving funds to
coinbase. File: rust-backend/crates/core/src/service.rs.
    - evm env: ensure block.basefee = 0 (if available) and keep enable_london_fork true or set basefee
explicitly to zero. File: crates/execution/src/tron_evm.rs.
- Remove Ethereum-specific gas checks:
    - Drop the tx.gas_limit < 21000 rejection. Only enforce tx.gas_limit <= context.block_gas_limit. File:
crates/execution/src/tron_evm.rs.
- Stabilize state change ordering for digest parity:
    - Before returning, sort state_changes deterministically:
    - AccountChange: by `address` ascending.
    - StorageChange: by `(address, key)` ascending.
- File: crates/execution/src/tron_evm.rs (post extract_state_changes_from_db).
- Heuristic non-VM energy fix (optional stopgap):
    - If data.is_empty() and to has no code (query via storage adapter), set energy_used = 0. Do not add
any fee deltas. Files: crates/core/src/service.rs (after result), or in tron_evm.rs post-process result.

Result: No coinbase state deltas; non-VM txs report energy 0; state_change_count should match embedded;
digest accuracy improves.

Phase 2 – TRON Fee Policy (configurable, still no proto change)

- Add execution fee policy config:
    - ExecutionConfig additions:
    - `fees.mode`: `"burn" | "blackhole" | "none"` (default `"burn"`).
    - `fees.blackhole_address_base58`: Base58 Tron address for credit path.
    - `fees.support_black_hole_optimization`: bool (default true).
- Wire env overrides TRON_BACKEND__EXECUTION__FEES__.... Files: crates/common/src/config.rs, rust-backend/
config.toml.
- Address parsing helper:
    - Add a production-safe Base58 Tron address → 20-byte EVM address decoder (lift from_tron_address out
of #[cfg(test)]). Files: crates/execution/src/storage_adapter.rs or a small crates/common/src/addr.rs.
- Fee post-processor (VM path only, optional and conservative):
    - Keep gas_price=0 to avoid coinbase deltas.
    - If fees.mode = "blackhole" and you want to reflect a fee delta, optionally credit blackhole with a
simple fee = energy_used * context.energy_price. Note: this is an approximation and not full TRON resource
settlement; guard behind fees.experimental_vm_blackhole_credit = false by default.
    - If fees.mode = "burn", do nothing (no state delta).
    - Implement as a post-processing step after extract_state_changes_from_db, appending a synthetic
AccountChange for blackhole if enabled. Files: crates/execution/src/tron_evm.rs (post-processing) or
crates/core/src/service.rs (before converting to protobuf).
- Non-VM stopgap (no proto change):
    - Keep non-VM heuristic (no code at to, empty data) to set energy_used=0.
    - Do not attempt to compute actual TRON non-VM flat fees here; leave fee effects to Java unless you
enable an optional fees.non_vm_blackhole_credit_flat config and can read dynamic fee parameters from
storage (future step).

Result: Configurable fee emission that defaults to “burn” (no extra deltas), ensuring parity; optional
blackhole credit only when explicitly enabled.

Phase 3 – Full Non‑VM Handling (requires proto + Java caller change)

- Extend proto to carry tx type:
    - Add enum TxKind { NON_VM = 0; VM = 1; } to TronTransaction and a tx_kind field.
    - Pass tx kind from Java (remote mode).
- Implement non‑VM processor (storage-native, no EVM):
    - For NON_VM: apply TRX transfer semantics with proper fee handling:
    - Update sender balance: `-(amount + fee)`, recipient `+amount`.
    - Fee:
      - If `fees.mode="burn"`: do not create an account delta (burn is supply change).
      - If `fees.mode="blackhole"`: credit blackhole account by `fee`.
    - `energy_used = 0`, `bandwidth_used` computed from payload size and TRON rules; add
`TronResourceUsage` if needed.
- Use StorageModuleAdapter to load/update accounts and push StateChangeRecord::AccountChange. Files: new
util in crates/core/src/service.rs or a new crates/execution/src/non_vm.rs.
- Dynamic properties:
    - Optionally read supportBlackHoleOptimization and network fee params from chain state (dynamic
properties DB) to auto-derive fee mode/amount; otherwise use config fallback.

Result: Remote backend applies TRON-accurate non‑VM fees and emits correct state deltas including
blackhole credit when configured.

Validation

- Unit tests:
    - VM: ensure no AccountChange for coinbase even when energy_used > 0. Files: crates/execution/src/
tests/*.
    - Sorting: ensure deterministic ordering of state_changes.
    - Addr parsing: Base58 ↔ 20-byte conversions round-trip.
- Integration checks:
    - Re-run your two nodes and scripts/execution_csv_compare.py:
    - Expect `state_change_count` and `state_digest_sha256` to converge to ~100% on VM txs.
    - `energy_used`: matches embedded; non‑VM → 0.
    - No coinbase-related mismatches.
- Performance: confirm no regression from sorting and post-processing; log only at debug.

Rollout

- Default config: fees.mode="burn", coinbase suppression on. No behavior change visible to Java except
removal of coinbase deltas.
- Backward-compat flags: execution.evm_eth_coinbase_compat=true (off by default) to temporarily restore
old behavior if needed.
- Docs: update rust-backend/config.toml and README about fee policy and TRON parity, note the proto
extension in Phase 3.

If you want, I can draft the exact config keys and the minimal code diffs for Phase 1 to get you parity
quickly, then outline the Phase 2/3 changes behind feature flags.


---



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
