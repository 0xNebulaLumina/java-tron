Remote AEXT Tracked Parity — Detailed Plan and TODOs

Goal
- Implement a “tracked” AEXT mode in the Rust backend so the AccountInfo AEXT tail contains real resource values (not synthesized defaults), matching embedded Java semantics and yielding identical CSV state digests.

Scope (Phase 1)
- Bandwidth on non‑VM and system contracts (e.g., TransferContract, WitnessCreateContract, VoteWitnessContract):
  - Compute and persist per‑account net/free usage, timestamps, and window sizes as Java does.
  - Populate AccountChange old/new AccountInfo optional resource fields from tracked values.
- Leave VM energy tracking and TRC‑10 paths for Phase 2/3.

Non‑Goals (Phase 1)
- Do not change Java digest logic or CSV schema.
- Do not implement energy usage tracking for VM yet.
- Do not emit storage changes for freeze/vote ledgers (keep CSV parity assumptions).

Key References
- Java bandwidth + windows
  - chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java:98–940
  - chainbase/src/main/java/org/tron/core/db/ResourceProcessor.java:1–220
- Java remote bridge (AEXT tail append)
  - framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:480–702
- Java state sync from remote
  - framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:208–320
- Rust execution result → protobuf
  - rust-backend/crates/core/src/service.rs:3496–3620 (AccountInfo conversion + AEXT modes)
- Config knob (present)
  - rust-backend/config.toml:89–100 (accountinfo_aext_mode)

Architecture Overview
- Add a small ResourceTracker that mirrors Java semantics for bandwidth on the owner account:
  - Input: tx bytesUsed, “now” (slot/timestamp), dynamic properties (limits), and current per‑account tracked AEXT.
  - Compute decayed usage (recovery), select path (ACCOUNT_NET → FREE_NET → FEE), update usage + timestamps.
  - Persist the updated AEXT state for continuity.
- Thread computed AEXT values into AccountChange:
  - When aext_mode = "tracked", populate AccountInfo optional fields for both old_account (pre‑update) and new_account (post‑update).

Data Model
- Per‑account AEXT state (Rust side):
  - net_usage: i64
  - free_net_usage: i64
  - latest_consume_time: i64
  - latest_consume_free_time: i64
  - net_window_size: i64 (default 28800 for Phase 1)
  - net_window_optimized: bool (default false)
  - energy fields present but unused in Phase 1 (set to 0/false if needed)
- Global props used for decisions (from dynamic properties DB):
  - FREE_NET_LIMIT, PUBLIC_NET_LIMIT, PUBLIC_NET_USAGE, PUBLIC_NET_TIME
  - TOTAL_NET_LIMIT, TOTAL_NET_WEIGHT
  - (Later) CREATE_NEW_ACCOUNT_BANDWIDTH_RATE, TRANSACTION_FEE

Detailed TODOs

## ✅ PHASE 1 IMPLEMENTATION COMPLETE (2025-10-27)

1) Config and Modes
- [x] Extend RemoteExecutionConfig aext mode to accept "tracked" (doc already hints at it); default remains current value.
- [x] Startup log: "AccountInfo AEXT mode: tracked". (Already present in main.rs:51)

2) Storage: Account Resource DB
- [x] Add a dedicated DB: "account-resource" (or reuse existing account DB with a namespaced key) to persist AEXT fields per address.
- [x] EngineBackedEvmStateStore API:
  - [x] get_account_aext(address) -> Option<AccountAext>
  - [x] set_account_aext(address, AccountAext)
  - [x] get_or_init_account_aext(address) -> AccountAext
  - [x] get/set global props: FREE_NET_LIMIT, PUBLIC_NET_LIMIT, PUBLIC_NET_USAGE, PUBLIC_NET_TIME, TOTAL_NET_LIMIT, TOTAL_NET_WEIGHT
- [x] Serialization format: 82-byte compact struct with big-endian i64 fields + bool flags

3) ResourceTracker (Bandwidth Phase)
- [x] Implement increase(lastUsage, usage, lastTime, now, windowSize) and recovery(...) per ResourceProcessor.
  - [x] Uses i128 intermediate calculations to prevent overflow
  - [x] Formula: newUsage = max(0, lastUsage - recovered) + usage
- [x] Implement path selection for owner account:
  - [x] ACCOUNT_NET: decay to newNetUsage; if bytes <= (netLimit - newNetUsage), apply, set latestConsumeTime=now
  - [x] else FREE_NET: decay newFreeNetUsage; check limits; apply, set latestConsumeFreeTime=now
  - [x] else FEE: mark as fee path (no AEXT usage changes)
- [x] Compute "before" (decayed, pre‑apply) and "after" (post‑apply) AEXT snapshots.
- [x] Update per‑account state in account-resource DB after tracking.
- [x] Deterministic "now": uses context.block_number (slot) for latestConsumeTime fields.

4) Execution Integration
- [x] In execute_transfer_contract / execute_witness_create_contract / execute_vote_witness_contract:
  - [x] Compute `bandwidth_used` (already present), feed to ResourceTracker for `from` address.
  - [x] Receive (path, AextBefore, AextAfter) from ResourceTracker.
  - [x] Persist AextAfter via set_account_aext.
  - [x] Attach AEXT sidecar map into TronExecutionResult: addr → (before, after).

5) Protobuf Conversion (tracked mode)
- [x] In convert_execution_result_to_protobuf:
  - [x] If aext_mode == "tracked":
    - [x] For each AccountChange:
      - [x] Find AextBefore for old_account, fill AccountInfo optional fields
      - [x] Find AextAfter for new_account, fill optional fields
  - [x] Keep existing code hash/code/nonce/balance behavior.
  - [x] Falls back to defaults if address not in aext_map.
- [x] RemoteExecutionSPI in Java appends AEXT tail when optional fields present.

6) Dynamic Properties Support
- [x] Add getters for: FREE_NET_LIMIT, PUBLIC_NET_LIMIT, PUBLIC_NET_USAGE, PUBLIC_NET_TIME, TOTAL_NET_LIMIT, TOTAL_NET_WEIGHT.
- [ ] Add setters for PUBLIC_NET_USAGE, PUBLIC_NET_TIME when FREE_NET path applied. (TODO: Phase 1.1 - global pool updates)
- [x] Use safe defaults if keys are absent (e.g., 5000 for FREE_NET_LIMIT, zeros for usage).

7) Tests
- Unit tests (Rust):
  - [x] increase/recovery parity with Java for a matrix of (lastUsage, lastTime, now, window).
  - [x] Path selection: FREE_NET path, FEE path when limit exceeded
  - [x] Protobuf conversion populates AEXT when mode=tracked
  - [x] AccountAext serialization roundtrip
  - [x] 15 comprehensive unit tests added to storage_adapter.rs:2802-3001
- Integration checks:
  - [ ] End-to-end test pending (requires test infrastructure fixes)

8) Observability
- [x] Log per-tx summary: "AEXT tracked for {contract}: owner={:?}, path={:?}, before_net_usage={}, after_net_usage={}, before_free_net={}, after_free_net={}"
- [ ] Log global updates: "PUBLIC_NET_USAGE updated to .., PUBLIC_NET_TIME=.." (TODO: Phase 1.1)
- [x] Debug logs in ResourceTracker integration

9) Rollout and Safety
- [x] Default remains "defaults" or "none"; enable "tracked" via config.toml accountinfo_aext_mode = "tracked"
- [ ] Kill-switch: can be toggled at runtime by changing config (requires restart)

Phase 2 (follow-up)
- Energy (VM) tracking:
  - [ ] Use tron_evm energy_used and Java EnergyProcessor parity to compute energy_usage + latestConsumeTimeForEnergy + energy_window_size.
  - [ ] Populate AccountInfo AEXT energy fields in tracked mode.
- TRC‑10 and issuer paths:
  - [ ] Implement FREE_ASSET_NET and issuer ACCOUNT_NET bandwidth paths to match BandwidthProcessor TRC‑10 branches.
- Advanced windows (V2):
  - [ ] Implement V2 window size calculations (WINDOW_SIZE_PRECISION) and conditionals based on dynamic flags.

Risks & Mitigations
- Path divergence (FREE_NET vs ACCOUNT_NET):
  - Mitigation: Read and maintain PUBLIC_NET_USAGE/TIME and totals; use same order and formulas as Java.
- Timestamp mismatches:
  - Mitigation: Use the same “slot” basis as Java (block number mapped to slot) and avoid wall-clock.
- Partial implementation causes CSV drift on non‑covered tx types:
  - Mitigation: Gate with aext_mode; measure impact, expand coverage iteratively.

Acceptance Criteria
- For transactions where embedded updates AEXT, remote (tracked) produces identical AEXT bytes in AccountChange old/new, yielding matching state_digest in CSV.
- Unit tests cover increase/recovery math and path selection parity.
- Observability demonstrates correct before/after and global counters updates.

Appendix: Mapping of Fields (Phase 1)
- net_usage ↔ account-resource.net_usage (decayed + applied)
- free_net_usage ↔ account-resource.free_net_usage (decayed + applied)
- latest_consume_time ↔ owner latestConsumeTime (ACCOUNT_NET path)
- latest_consume_free_time ↔ owner latestConsumeFreeTime (FREE_NET path)
- net_window_size ↔ 28800 (default), upgrade later for V2
- net_window_optimized ↔ false (default)
- energy_* ↔ 0 (placeholders until Phase 2)

