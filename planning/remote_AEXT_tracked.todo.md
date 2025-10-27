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

1) Config and Modes
- [ ] Extend RemoteExecutionConfig aext mode to accept "tracked" (doc already hints at it); default remains current value.
- [ ] Startup log: "AccountInfo AEXT mode: tracked".

2) Storage: Account Resource DB
- [ ] Add a dedicated DB: "account-resource" (or reuse existing account DB with a namespaced key) to persist AEXT fields per address.
- [ ] EngineBackedEvmStateStore API:
  - [ ] get_account_resource(address) -> AccountAext
  - [ ] set_account_resource(address, AccountAext)
  - [ ] get/set global props: PUBLIC_NET_USAGE, PUBLIC_NET_TIME (and retain existing getters for limits/weights)
- [ ] Serialization format: a compact, versioned struct; document field order and endianness.

3) ResourceTracker (Bandwidth Phase)
- [ ] Implement increase(lastUsage, usage, lastTime, now, windowSize) and recovery(...) per ResourceProcessor.
- [ ] Implement calculateGlobalNetLimit(accountFrozenBW, totals) parity with Java.
- [ ] Implement path selection for owner account:
  - [ ] ACCOUNT_NET: decay to newNetUsage; if bytes <= (netLimit - newNetUsage), apply, set latestConsumeTime=now
  - [ ] else FREE_NET: decay newFreeNetUsage and PUBLIC_NET_USAGE; check limits; apply, set latestConsumeFreeTime=now and PUBLIC_NET_TIME=now
  - [ ] else FEE: mark as fee path (no AEXT usage changes)
- [ ] Compute “before” (decayed, pre‑apply) and “after” (post‑apply) AEXT snapshots.
- [ ] Update per‑account state in account-resource DB; update global counters if FREE_NET path used.
- [ ] Deterministic “now”: choose context.block_number (slot) for latestConsumeTime fields to align with Java’s slot‑based clocks.

4) Execution Integration
- [ ] In execute_transfer_contract / execute_witness_create_contract / execute_vote_witness_contract:
  - [ ] Compute `bandwidth_used` (already present), feed to ResourceTracker for `from` address.
  - [ ] Receive AextBefore/AextAfter for owner.
  - [ ] Persist AextAfter via set_account_resource.
  - [ ] Attach AEXT sidecar map into TronExecutionResult: addr → (before, after).

5) Protobuf Conversion (tracked mode)
- [ ] In convert_execution_result_to_protobuf:
  - [ ] If aext_mode == "tracked":
    - [ ] For each AccountChange:
      - [ ] Find AextBefore for old_account, fill AccountInfo optional fields if old_account.is_some()
      - [ ] Find AextAfter for new_account, fill optional fields if new_account.is_some()
  - [ ] Keep existing code hash/code/nonce/balance behavior.
- [ ] Ensure RemoteExecutionSPI sees presence of AEXT fields and appends tail.

6) Dynamic Properties Support
- [ ] Add getters for: FREE_NET_LIMIT, PUBLIC_NET_LIMIT, PUBLIC_NET_USAGE, PUBLIC_NET_TIME, TOTAL_NET_LIMIT, TOTAL_NET_WEIGHT.
- [ ] Add setters for PUBLIC_NET_USAGE, PUBLIC_NET_TIME when FREE_NET path applied.
- [ ] Use safe defaults if keys are absent (e.g., zeros) matching early-chain behavior.

7) Tests
- Unit tests (Rust):
  - [ ] increase/recovery parity with Java for a matrix of (lastUsage, lastTime, now, window).
  - [ ] Path selection: craft owners with sufficient ACCOUNT_NET vs FREE_NET and assert chosen path and updated fields.
  - [ ] Protobuf conversion populates AEXT when mode=tracked; absent otherwise.
- Integration checks:
  - [ ] For a WitnessCreate with expected bytesUsed (~212), ensure FREE_NET path, free_net_usage increases by 212 and timestamps set.
  - [ ] Round-trip through Java RemoteExecutionSPI (optional) or verify by inspecting serialized AccountChange lengths (presence of AEXT tail).

8) Observability
- [ ] Log per-tx summary: "AEXT tracked owner=.. path=FREE_NET bytes=.. before={...} after={...}".
- [ ] Log global updates: "PUBLIC_NET_USAGE updated to .., PUBLIC_NET_TIME=..".
- [ ] Toggleable verbosity via config.

9) Rollout and Safety
- [ ] Default remain "defaults" or "none" until validated; enable "tracked" in experiments.
- [ ] Add a kill-switch to fall back to previous aext_mode at runtime if discrepancies are detected.

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

