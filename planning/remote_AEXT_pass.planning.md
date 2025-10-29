› Think harder.

I want to compare the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,

The result csv are
+ output-directory/execution-csv/20251027-040703-4abab9f8-embedded-embedded.csv
+ output-directory/execution-csv/20251027-104445-73b9280d-remote-remote.csv
respectively.

you will help me debug and figure out why there are mismatches, starting from the first tx.

(embedded execution + embedded storage) java log: 2.embedded-java.log
(remote execution + remote storage) java log: remote-java.ea09d4e.log
(remote execution + remote storage) rust log: remote-rust.ea09d4e.log

---


• I’ve compared the two CSVs, found the first divergence, and traced it through the logs.

First Mismatch

- Block/Tx: 1785:0, tx_id 18bd4e3bb4552df2d2d24821bddf7079df9bf7f293978c97d2c7c8613d45b3e3
- Contract: WitnessCreateContract
- CSV rows: output-directory/execution-csv/20251027-040703-4abab9f8-embedded-embedded.csv:9 and output-directory/execution-csv/20251027-104445-73b9280d-remote-remote.csv:9
- Difference: state_digest_sha256 only
    - embedded: 3fa46980d360eea4fad9935025f2f1ed8a3191361e8ad44f9cb3d597d4eb7998
    - remote:   6baa0df34dda4b82d138d9c0ad46915a764c778af79abeb60e0bd7d1e3fd28b3

What Actually Differs

- Both runs report 2 state changes for the tx: the owner account and witness entry.
- The witness entry bytes are identical across runs. The difference is in the owner account’s serialized bytes.
- Owner address: 4116329c4b64920408342e66221a1d52a974901984 (matches CSV owner_address_hex).
- In the owner account “oldValue”, 32-byte chunk #03 differs:
- Embedded Java notes bandwidth 212 for this tx:
    - 2.embedded-java.log:60535 and 2.embedded-java.log:60537
- Remote Java does the same bandwidth accounting and applies 2 state changes:
    - remote-java.ea09d4e.log:65032, 65034 (bandwidth 212), 65041 (“Applying 2 state changes…”)
- Remote Rust backend is configured to zero AEXT fields by default:
    - remote-rust.ea09d4e.log:2 shows accountinfo_aext_mode: "defaults"
    - The tx is executed as WitnessCreate; see remote-rust.ea09d4e.log:162

Why The Digest Diverges

- In embedded mode, Java’s BandwidthProcessor updates the owner’s AEXT (e.g., free_net usage) before/around state change emission. Those non-zero AEXT bytes persist in the serialized account for the change.
- In remote mode, the Rust backend serializes AccountInfo with AEXT mode “defaults” (zeros for usage/time fields, 28800 window sizes). It does not see Java’s pre-updated AEXT and thus emits zeroed AEXT in
old/new values for unchanged AEXT fields.
- Since the tx does not modify those AEXT fields, each side “preserves” its own view of unchanged bytes (non-zero vs zero), producing different account bytes and thus a different state digest.

Evidence In Code

- AEXT mode behavior: rust-backend/crates/core/src/service.rs:3630
- “defaults” zeros usage/time and sets windows to 28800: rust-backend/crates/core/src/service.rs:3661–3671
- “tracked” mode exists to populate AEXT from tracked values when available: rust-backend/crates/core/src/service.rs:3672–3706
- Config default and recommendation comment: rust-backend/config.toml:89–101

Next Steps

- Option A (preferred for parity): Switch Rust backend to use AEXT tracking so old/new include resource usage values consistently.
    - Set rust-backend/config.toml:99 accountinfo_aext_mode = "tracked"
    - Re-run remote-remote and re-compare CSVs.
- Option B (align around Java’s view): If remote keeps “defaults”, change the CSV generation/digest to compute from the final applied state in Java (after Java merges bandwidth updates plus remote changes),
or normalize/ignore AEXT chunks when hashing.
- Option C (hybrid): Pass Java’s current AEXT values into the remote engine (context or side channel) so the remote returned old/new mirrors Java’s pre-execution resource usage. Then, state changes JSON will
match byte-for-byte.

Want me to switch the backend to “tracked” in rust-backend/config.toml and re-run the comparison, or would you prefer I prototype an AEXT-normalized digest to validate parity quickly?

---

give me a detailed plan on:

Option C (hybrid): Pass Java’s current AEXT values into the remote engine (context or side channel) so the remote returned old/new mirrors Java’s pre-execution resource usage.

---

• Here’s a precise design and rollout plan for Option C: push Java’s pre-execution AEXT into the remote execution request and have the backend echo those values into AccountInfo old/new so the CSV and digest
match embedded.

Overview

- Goal: Ensure remote AccountChange bytes include the same AEXT tail as embedded for unchanged resource fields (bandwidth/energy windows, usage, timestamps).
- Approach: Add an optional “pre-execution AEXT snapshot” field to gRPC; Java fills this per-address before the remote call; Rust prefers these values when serializing AccountInfo for state changes.

Wire Format

- Update framework/src/main/proto/backend.proto:
    - Add message definitions:
        - AccountAext { int64 net_usage = 1; int64 free_net_usage = 2; int64 energy_usage = 3; int64 latest_consume_time = 4; int64 latest_consume_free_time = 5; int64 latest_consume_time_for_energy = 6;
        int64 net_window_size = 7; bool net_window_optimized = 8; int64 energy_window_size = 9; bool energy_window_optimized = 10; }
        - AccountAextSnapshot { bytes address = 1; AccountAext aext = 2; }
    - Extend ExecuteTransactionRequest with repeated AccountAextSnapshot pre_execution_aext = 3; (do not renumber existing fields).
- Backwards compatibility: Field is optional; older clients/servers still work.

Java: Build Request With AEXT

- Where: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:440–520
- Add AEXT collection to buildExecuteTransactionRequest(...):
    - Get DB handle: context.getStoreFactory().getChainBaseManager().
    - Read AccountCapsule for relevant addresses. Minimum viable:
        - Always include tx.from (owner). For better coverage, also include tx.to (when present), and for known system contracts (e.g., WitnessCreate/Freeze), include the blackhole account from
        DynamicPropertiesStore if it exists.
    - For each account, fill AccountAextSnapshot using AccountCapsule:
        - net_usage: account.getNetUsage()
        - free_net_usage: account.getFreeNetUsage()
        - energy_usage: account.getAccountResource().getEnergyUsage()
        - latest_consume_time: account.getLatestConsumeTime()
        - latest_consume_free_time: account.getLatestConsumeFreeTime()
        - latest_consume_time_for_energy: account.getAccountResource().getLatestConsumeTimeForEnergy()
        - net_window_size: account.getWindowSize(BANDWIDTH)
        - net_window_optimized: account.getWindowOptimized(BANDWIDTH)
        - energy_window_size: account.getWindowSize(ENERGY)
        - energy_window_optimized: account.getWindowOptimized(ENERGY)
    - Attach to request: requestBuilder.addAllPreExecutionAext(aextSnapshots).
- Notes:
    - Use the already-available context; no new plumbing required.
    - If an account isn’t found, skip it; backend will fall back.

Rust: Consume And Use Pre-Exec AEXT

- Where to parse: rust-backend/crates/core/src/service.rs:3008–3180 (execute_transaction handler)
- Steps:
    - Parse req.pre_execution_aext to HashMap<Address, AccountAext>, converting Tron 0x41-prefixed bytes address to 20-byte revm::primitives::Address.
    - Add parameter to convert_execution_result_to_protobuf to accept this optional map.
        - Update calls at rust-backend/crates/core/src/service.rs:3169–3177 to pass the map.
- Where to apply: rust-backend/crates/core/src/service.rs:3630–3762
    - In convert_execution_result_to_protobuf, inside the convert_account_info closure:
        - Current logic picks AEXT based on aext_mode:
            - “tracked” uses result.aext_map (populated by backend when tracking).
            - “defaults/none” produce zeros or omit AEXT.
        - Add “hybrid” branch (or “prefer_pre_provided”): if an entry is present in the pre-exec AEXT map for the account address, populate all AccountInfo optional AEXT fields from this map, for both
        old_account and new_account. If not present, fall back to existing behavior.
- Config switch:
    - Extend config to add mode accountinfo_aext_mode = "hybrid" (default stays as-is).
        - When in “hybrid”, the serializer prefers the request-provided map and otherwise falls back to “defaults”. Do not alter/persist AEXT on the backend.
    - Logging:
        - Add debug logs when using pre-provided AEXT: “Using pre-exec AEXT for address X in hybrid mode.”

Semantics

- Scope: Non-VM transactions (system contracts) where Java already handles bandwidth updates and expects unchanged AEXT bytes to be preserved in AccountInfo serialization for that tx.
- Old/new behavior: For tx that do not modify AEXT, both old/new should carry the same AEXT values. This mirrors embedded behavior observed in the mismatch case.
- Persistence: Backend must not persist or mutate AEXT in “hybrid” (leave Java as source of truth). Leave “tracked” mode for a different path when backend tracks/persists AEXT itself.

Edge Cases

- Missing AEXT: If Java doesn’t provide a snapshot for an address, backend falls back to “defaults”/current logic; parity may be slightly degraded for that address but still correct for others.
- VM transactions: Keep existing logic; do not enable “hybrid” AEXT population for VM by default to avoid implying semantics not mirrored by Java.

Validation Plan

- Unit-level:
    - Add a targeted test in Rust core service (mocking a request with pre_execution_aext filled for owner) and assert that AccountInfo in the response has the expected optional AEXT fields set for both old/
    new.
- End-to-end:
    - Re-run the remote path on the same dataset. Expect the first mismatch at 1785:0 to resolve because the owner’s AEXT now matches Java’s old/new bytes.
    - Recompute mismatches across the CSVs. Expect a large drop in “state_digest_sha256-only” mismatches for non-VM contracts.
- Log verification:
    - Confirm remote-rust logs show “hybrid” mode and “Using pre-exec AEXT” lines for the affected addresses.
    - Confirm remote-java logs still show “Applying 2 state changes …” and no resource field deserialization errors.

Rollout

- Phase 1:
    - Implement proto + Java builder + Rust consume path.
    - Gate with config accountinfo_aext_mode = "hybrid".
- Phase 2:
    - Expand Java to send AEXT also for common side-effect addresses in system contracts:
        - Blackhole (when not burning), witnesses (for vote/witness contracts), freeze ledger owner for freeze/unfreeze, etc., keyed on ContractType.
- Phase 3:
    - Evaluate enabling “hybrid” by default for non-VM transactions only, keeping VM path untouched.
    - Optional: Add metrics counters in backend for “pre_exec_aext_used”.

Concrete Code Touchpoints

- Proto:
    - framework/src/main/proto/backend.proto:458 (ExecuteTransactionRequest) add field 3.
    - framework/src/main/proto/backend.proto add AccountAext and AccountAextSnapshot messages near AccountInfo definitions.
- Java:
    - framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:440–520 build and attach pre_execution_aext.
- Rust:
    - rust-backend/crates/core/src/service.rs:3008–3180 parse snapshots into a map.
    - rust-backend/crates/core/src/service.rs:3599–3762 modify convert_execution_result_to_protobuf to accept and prefer the map under “hybrid”.
    - rust-backend/crates/common/src/config.rs:93, 222, 240 add “hybrid” mode string handling.

If you want, I can draft the exact proto additions and the Java/Rust method signatures to minimize code churn when you implement.

---

