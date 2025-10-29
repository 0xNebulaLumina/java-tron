Think harder.

I want to compare the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,

The result csv are
+ output-directory/execution-csv/20251027-040703-4abab9f8-embedded-embedded.csv
+ output-directory/execution-csv/20251027-081613-2576d782-remote-remote.csv
respectively.

you will help me debug and figure out why there are mismatches, starting from the first tx.

(embedded execution + embedded storage) java log: 2.embedded-java.log
(remote execution + remote storage) java log: remote-java.6cd2af7.log
(remote execution + remote storage) rust log: remote-rust.6cd2af7.log

---


• I loaded both CSVs, found the first mismatch, and traced it into the logs. Here’s what’s different and why.

- First mismatch
    - Tx: 18bd4e3bb4552df2d2d24821bddf7079df9bf7f293978c97d2c7c8613d45b3e3
    - Block: 1785
    - CSV state digest differs:
        - Embedded has non-zero “AEXT” resource fields; remote sets them to zero/defaults.
        - Example window around the first differing bytes:
            - Embedded: ... 00000000000000000000 d4 00000000000000000000000000000000000000 ...
            - Remote  : ... 00000000000000000000 00 00000000000000000000000000000000000000 ...
        - I parsed both CSV rows to verify this exact offset and field position. The divergence is precisely the AEXT tail (resource usage sub-structure).
- Why that makes sense
    - Embedded Java logs show the tx consumed 212 bandwidth bytes (0xd4), which matches the non-zero resource usage in AEXT:
        - 2.embedded-java.log:60535 and 2.embedded-java.log:60596
    - The Rust backend is configured to synthesize AccountInfo AEXT fields with defaults for EOAs (instead of tracking real values):
        - remote-rust.6cd2af7.log:18 shows AccountInfo AEXT mode: defaults
        - rust-backend/config.toml:99 says accountinfo_aext_mode = "defaults"
    - The Java RemoteExecutionSPI appends an AEXT tail whenever the proto contains any resource fields:
        - framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:480, :533, :702
    - The Rust service populates AEXT for EOAs based on that mode, not from real metrics:
        - rust-backend/crates/core/src/service.rs:3496 (conversion), :3564–:3567 (defaults mode)
    - Result: Embedded encodes real resource usage (e.g., netUsage=212 and timestamps), while remote encodes zeros + 28800 window defaults. Byte-level mismatch in the account value leads to a different CSV
    digest.
- Corroboration
    - I checked 1122 common txs; 105 have digest mismatches. The first mismatch is this witness-create tx at block 1785. Many mismatches show the same pattern (first diff at position 198), consistent with
    AEXT resource fields; some later ones differ at other positions too (e.g., balance/nonce), but the first is clearly AEXT-related.
- Root cause
    - The remote pipeline currently does not preserve or compute the actual AccountInfo resource usage fields for EOAs; it emits normalized defaults (“AEXT tail”) that differ from embedded’s persisted values.
    This is enough to change the state_digest_sha256, even when the logical outcome (success, energy_used, state_change_count, and the second account change) all match.
- Next steps
    - Short-term options to align CSV parity:
        - Preserve AEXT from storage: Teach the remote to read and pass through existing AEXT bytes for the account when present (old_account and new_account) instead of synthesizing defaults. This would make
        remote old/new match embedded when the tx doesn’t change resource fields. Code touchpoint: rust-backend/crates/core/src/service.rs:3496 (use stored account info).
        - Or ignore AEXT in digest: Update the CSV digest computation to exclude the AEXT tail until “tracked” mode exists. This avoids hash noise from non-semantic resource counters.
    - Longer-term, implement “tracked” mode: Populate AEXT fields with real values (e.g., net/free/energy usage, latest consume times) so the remote’s AEXT equals embedded’s.
- If you want, I can:
    - Scan the next few mismatches to see which are AEXT-only vs other categories.
    - Propose a minimal patch to preserve existing AEXT bytes on the remote path (passthrough) so digests align without changing execution logic.
    - Toggle config and re-run to validate (e.g., experiment with accountinfo_aext_mode = "none" vs "defaults"), though “none” will drop the AEXT tail and still differ from embedded which retains it.


---

what's the diff between "Preserve AEXT from storage" and "implement “tracked” mode"?

---


- Preserve AEXT from storage
    - Behavior: Do not synthesize resource fields. Read whatever AEXT the account already has in storage and include those exact bytes in old_account/new_account for AccountChange. Do not update them in the
    backend.
    - CSV impact: Matches embedded only when the tx does not change AEXT fields. If embedded increments resource counters (e.g., netUsage = 212 for bandwidth), remote will still emit 0s and the digest will
    differ.
    - Scope/complexity: Moderate. Add a storage read at AccountChange construction to fetch existing AEXT and pass it through. No policy logic, no counters, no window math. Touchpoint: rust-backend/crates/
    core/src/service.rs:3496+ convert_account_info closure must fetch AEXT from storage instead of fabricating defaults.
    - Risk/perf: Low logic risk, small perf hit (extra reads). Doesn’t solve mismatches for transactions that actually consume bandwidth/energy.
- Implement “tracked” mode
    - Behavior: Actively compute and update AEXT fields to reflect real resource usage for the transaction (net/free/energy usage, latest consume timestamps, window fields). The backend becomes the source of
    truth for these counters.
    - CSV impact: Matches embedded even when AEXT changes, including your first mismatch (netUsage 212). New values in new_account mirror Java’s Bandwidth/Resource processing.
    - Scope/complexity: High. Requires reproducing Java-Tron’s resource accounting semantics (bandwidth paths FREE_NET/ACCOUNT_NET, fee exemptions, window sizes 28800, timestamps, dynamic properties) and
    applying them at the correct point relative to preseed/baseline. Touchpoints: execution path for system/VM txs, resource charging, then populate AccountChange AEXT; ensures RemoteExecutionSPI sees the
    updated fields. Likely needs tests mirroring Java behavior.
    - Risk/perf: Higher logic risk and ongoing maintenance (must stay in lockstep with Java’s policy changes). Perf impact is minor but non-zero due to extra bookkeeping.
- When to use which
    - Need quick parity where AEXT doesn’t change: “Preserve from storage” gives immediate wins and avoids synthetic defaults that cause needless diffs.
    - Need full parity (including bandwidth/energy updates): “Tracked” mode is the correct fix; it’s the only way to match embedded when AEXT mutates during tx execution.
- Practical note for your mismatch
    - The first mismatch (block 1785, tx 18bd…b3e3) differs exactly at the AEXT net usage position (212 vs 0). “Preserve from storage” would still output 0 in new_account and won’t fix this one. “Tracked”
    mode would compute and set 212, aligning the CSV digest.

---


give me a detailed plan on:

implement “tracked” mode: Populate AEXT fields with real values (e.g., net/free/energy usage, latest consume times) so the remote’s AEXT equals embedded’s.


--


• Here’s a concrete, scoped plan to add tracked AEXT so remote CSVs match embedded, starting with bandwidth on non‑VM/system contracts and leaving energy/VM for phase 2.

Objectives

- Populate AEXT in AccountChange old/new with real values, not defaults, so state_digest matches embedded.
- Mirror Java’s resource semantics:
    - Bandwidth paths: ACCOUNT_NET → FREE_NET → FEE, with windowed usage and timestamps.
    - AEXT fields: net_usage, free_net_usage, energy_usage, latest_consume_time, latest_consume_free_time, latest_consume_time_for_energy, net_window_size, net_window_optimized, energy_window_size,
    energy_window_optimized.

Where To Hook

- Java parity reference points:
    - Bandwidth compute and apply: chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java:98, 520–940, 200–520
    - Window math: chainbase/src/main/java/org/tron/core/db/ResourceProcessor.java:1–120, 120–220
- Rust integration points:
    - Execution entry: rust-backend/crates/core/src/service.rs:2912 (ExecuteTransaction handler)
    - Non‑VM/system flows producing AccountChange: rust-backend/crates/core/src/service.rs:232–312 (dispatch), 300–520 (Transfer), 520–760 (WitnessCreate), 849–1015 (VoteWitness)
    - AEXT injection today (“defaults/zeros/none”): rust-backend/crates/core/src/service.rs:3496–3620 (convert_account_info closure); config value read at 3512–3520
    - AccountInfo AEXT tail serialization: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:480–586, 598–702
    - Config knob: rust-backend/config.toml:89–100 (accountinfo_aext_mode)

Design Overview

- Add a tracked mode that computes per-account AEXT “before” and “after” for the tx, matching Java’s BandwidthProcessor/ResourceProcessor behavior.
- Persist AEXT state in Rust backend so calculations chain correctly across blocks. This avoids relying on Java’s DB and lets remote runs be self-consistent.
- Wire computed AEXT into the protobuf AccountInfo for each AccountChange when aext_mode = "tracked". RemoteExecutionSPI will then append the AEXT tail, and the CSV will reflect real resource values.

Detailed Steps

1. Map Java semantics and data

- Mirror BandwidthProcessor.consume:
    - Calculate bytesSize per tx; current Rust heuristic: rust-backend/crates/core/src/service.rs:1319–1368
    - Decision order: asset free net (TRC‑10) → ACCOUNT_NET → FREE_NET → FEE; for initial scope, handle ACCOUNT_NET and FREE_NET paths (Transfer/Witness/Vote use no TRC‑10).
    - ACCOUNT_NET: newNetUsage = increase(netUsage, bytes, latestConsumeTime, now), set latestConsumeTime=now, set latestOperationTime; write back.
    - FREE_NET: newFreeNetUsage = increase(freeNetUsage, bytes, latestConsumeFreeTime, now), set latestConsumeFreeTime=now; also update publicNetUsage/time; write back.
    - Failover to FEE if no bandwidth available.
- Mirror ResourceProcessor.increase/recovery window math: chainbase/src/main/java/org/tron/core/db/ResourceProcessor.java:40–120, 120–220

2. Extend storage and config (Rust)

- Add a dedicated DB for per-account resource fields:
    - DB: account-resource (new), keyed by 20‑byte address; value: a compact struct with AEXT fields.
    - Methods on EngineBackedEvmStateStore:
        - get_account_resource(address) -> AccountAext
        - set_account_resource(address, AccountAext)
- Add dynamic property getters needed for decisions:
    - FreeNetLimit, PublicNetLimit, PublicNetUsage, PublicNetTime
    - TotalNetLimit, TotalNetWeight
    - CreateNewAccountBandwidthRate (for create-account path, later)
    - TransactionFee (for fee path, later)
    - Reuse existing flags: support_unfreeze_delay (present), support_allow_new_resource_model (present), support_black_hole_optimization (present)

3. Implement ResourceTracker (bandwidth phase)

- New module/class in core service:
    - Inputs: tx, context.block_number/time, bytesUsed, current AccountAext for owner, dynamic properties
    - Outputs:
        - Before (preseed): decayed net/free usage and window sizes at “now” with usage increment 0
        - After: updated usage reflecting chosen path (ACCOUNT_NET or FREE_NET) + timestamps set
- Functions:
    - increase(lastUsage, usage, lastTime, now, windowSize) and recovery(...) per ResourceProcessor
    - Path selection:
        - Try ACCOUNT_NET available = netLimit - newNetUsage; else FREE_NET available = freeNetLimit - newFreeNetUsage; else FEE
    - Windows:
        - net_window_size: 28800 by default; if V2 flags present, compute via ResourceProcessor V2 (optional in phase 1: keep 28800)
        - net_window_optimized flags to false by default (embedded uses false; defaults mode already sets 28800 and false)

4. Carry AEXT through the pipeline

- Extend TronExecutionResult to include a sidecar map of per-address resource AEXT with both before and after:
    - E.g., HashMap<Address, (AextBefore, AextAfter)>
    - File: rust-backend/crates/execution/src/tron_evm.rs:118–156 (struct TronExecutionResult) add a new field
- In execute_transfer_contract / witness_create / vote_witness:
    - After computing bandwidth_used, construct ResourceTracker for from (and other addresses if needed).
    - Compute before/after AEXT and persist after into account-resource DB for continuity.
    - Attach map entry into result.
- In BackendService.convert_execution_result_to_protobuf:
    - When aext_mode == "tracked", consult the per-address AEXT map:
        - For AccountChange old_account: fill AccountInfo optional fields from AextBefore
        - For new_account: fill from AextAfter
    - Keep existing behavior for code/hash/balance

5. Persist and update dynamic props (minimal to match decisions)

- For FREE_NET path, update publicNetUsage and publicNetTime in Rust storage so subsequent decisions are consistent (these don’t go into CSV, but they affect availability).
- Optional (later): add PublicNetLimit and others if coverage expands beyond initial paths.

6. Tests and parity checks

- Unit tests in rust-backend/crates/core/src/tests.rs:
    - Build synthetic owner account with zero AEXT, set FreeNetLimit high; run WitnessCreate/Transfer with data so bytesUsed matches Java (e.g., target ~212), assert:
        - result.state_changes has 1 or 2 AccountChange
        - convert_execution_result_to_protobuf produces AccountInfo with AEXT present and:
            - net_window_size = 28800
            - free_net_usage increased appropriately when FREE_NET chosen
            - latest_consume_free_time set to context.block_number (or a deterministic mapping)
- Property tests for increase/recovery math matching Java’s formulas at boundaries.
- End-to-end assertion: RemoteExecutionSPI in Java applies AEXT (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:208–320); verify CB updates through an embedded test or log inspection for
serialize/deserialize path.

7. Observability and toggles

- Add trace logs:
    - “AEXT tracked: before={...} after={...} path=FREE_NET|ACCOUNT_NET bytes=...”
    - “AEXT mode: tracked” (already logs defaults: remote-rust.6cd2af7.log:18)
- Config gating:
    - Ensure accountinfo_aext_mode recognizes “tracked”
    - Keep “defaults” as fallback
- Feature flags for full fidelity later:
    - enable per‑account energy usage tracking (phase 2)
    - enable TRC‑10 asset free net paths (phase 3)
    - enable witness/vote ledger storage changes only if CSV parity unaffected

Parity Notes and Edge Cases

- Window size defaults: Embedded uses 28800 (0x7080) for both net/energy on EOAs; tracked must set same unless V2 flags dictate otherwise.
- Timestamps:
    - Java uses “slot” for latestConsumeTime; bind ours to context.block_number or to an equivalent slot value; ensure consistent scaling.
- Decision order must match Java exactly to avoid FREE_NET vs ACCOUNT_NET divergence. Reference: chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java:98–192, 516–940.
- Public net pool counters are global; if they diverge from Java, path choices can differ. For initial parity across a single remote run, keeping them internally consistent in Rust is sufficient.

Phase 2 (follow-up)

- VM energy tracked mode:
    - Use energy_used from EVM (tron_evm.rs) and ResourceProcessor energy methods to compute windowed energy_usage + latestConsumeTimeForEnergy + energy_window_size; update AccountInfo accordingly.
    - Dynamic properties for total energy weight/limit, and freeze V1/V2 energy contributions.
- TRC‑10 asset free net (FREE_ASSET_NET) and issuer ACCOUNT_NET handling (BandwithProcessor middle section ~300+ lines) to get complete parity for asset transfers.

Deliverables

- Code updates:
    - execution/tron_evm.rs: add per-account AEXT sidecar on TronExecutionResult
    - core/service.rs: ResourceTracker module, bandwidth accounting in non‑VM/system flows, proto conversion for tracked mode
    - execution/storage_adapter.rs: account-resource DB and dynamic property getters/setters
    - config.toml + config.rs: tracked mode support and comments
- Tests:
    - Unit tests for increase/recovery, tracked AEXT serialization, and path selection
- Logs and docs:
    - Inline tracing to debug AEXT values
    - README notes on tracked mode tradeoffs and what’s covered in phase 1

Why this will fix the first mismatch

- The first mismatch (block 1785, tx 18bd…b3e3) is FREE_NET with bytes=212; embedded AEXT shows that value in the AEXT tail for the owner account. Tracked mode will compute and set free_net_usage and
latest_consume_free_time accordingly in both old/new AccountChange, producing the same AEXT bytes and thus the same state_digest.


---


