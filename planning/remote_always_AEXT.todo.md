Remote AEXT Presence Parity — Backend TODO

Summary
- Objective: Make the Rust backend always emit the AccountInfo AEXT tail for EOAs by populating proto optional resource fields with zero/false values so Java’s RemoteExecutionSPI appends AEXT consistently.
- Outcome: Remote path produces AccountInfo bytes with an AEXT tail (76 bytes) per account change where applicable, aligning the shape of serialized account changes with embedded and improving cross‑mode comparability.

Motivation
- Current mismatch cause: Embedded appends AEXT; remote omits it because the backend sets all AEXT proto fields to None (absent). This changes account serialization length and the state digest.
- Goal: Ensure presence parity first (tail exists in both paths); future work can tackle value parity (real resource usage) or digest-insensitive hashing.

Scope
- In scope: Rust backend conversion of internal AccountInfo → protobuf AccountInfo for state changes. Populate proto “optional” resource fields with Some(0)/false for EOAs (and optionally all accounts) to force AEXT presence.
- Out of scope: Implementing real resource usage accounting in Rust; changing Java digest algorithm; altering embedded serialization rules.

Definitions
- AEXT: Account EXTension tail, 76 bytes: magic("AEXT") + version(2) + length(2=0x44) + payload(68). Payload is resource usage/time/window fields.
- EOA: Externally Owned Account, determined here as account with empty code bytes (code_bytes.is_empty()).

Current Behavior (as of this repo)
- Proto type supports presence:
  - framework/src/main/proto/backend.proto: AccountInfo has proto3 optional fields net_usage..energy_window_optimized.
- Java (remote) appends AEXT only if proto presence flags are set:
  - framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:528–555, 581–589
- Java (embedded) appends AEXT when enabled via property; it serializes from AccountCapsule fields:
  - framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournal.java:269–337, 347–425
- Rust backend currently sets AEXT proto fields to None (absent), so Java omits the tail:
  - rust-backend/crates/core/src/service.rs:3440–3580, closure convert_account_info at ~3543

Design Overview
- Approach: In convert_account_info, detect EOAs (empty code). For EOAs, set all AEXT proto optional fields to Some(0) or Some(false) instead of None. This will make Java append a 76‑byte tail.
- Optional extension: Apply the same presence for contract accounts (unconditional presence) to avoid mixed presence when code emptiness is ambiguous.
- Optional config: Add a mode switch (none | zeros | tracked) to control behavior without code changes later.
- Backward compatibility: Default can remain “none” initially; enable “zeros” in tests/experiments to validate.

Detailed TODOs
1) Backend conversion changes (required)
   - File: rust-backend/crates/core/src/service.rs
   - Location: convert_execution_result_to_protobuf → match TronStateChange::AccountChange → closure convert_account_info (around lines 3520–3560).
   - Steps:
     - Keep existing code_bytes normalization and KECCAK_EMPTY behavior.
     - Compute is_eoa = code_bytes.is_empty().
     - If is_eoa, populate all resource fields as present with zeros/false:
       - net_usage: Some(0)
       - free_net_usage: Some(0)
       - energy_usage: Some(0)
       - latest_consume_time: Some(0)
       - latest_consume_free_time: Some(0)
       - latest_consume_time_for_energy: Some(0)
       - net_window_size: Some(0)
       - net_window_optimized: Some(false)
       - energy_window_size: Some(0)
       - energy_window_optimized: Some(false)
     - Apply the same fill for both old_account_proto and new_account_proto.
     - Add a debug log summarizing: address, is_eoa, aext_mode=“zeros”.

2) Optional: Unconditional presence for all accounts (nice to have)
   - Rationale: Avoid presence differences between EOA vs contracts; guarantees every AccountInfo has AEXT tail, making payload shape fully uniform.
   - Change: Ignore is_eoa and always set Some(0)/false for all accounts.

3) Optional: Config knob (future-proofing)
   - File: rust-backend/config.toml
   - Add execution.remote.accountinfo_aext_mode = "none" | "zeros" | "tracked" (default: none)
   - Wire into service.rs to choose behavior in convert_account_info.
   - Modes:
     - none: all None (current behavior)
     - zeros: Some(0)/false (this plan)
     - tracked: Some(real values) when backend supports resource metrics (future)

4) Tests
   - Unit (Rust): rust-backend/crates/core/src/tests.rs
     - Add test that builds a minimal AccountChange for an EOA and verifies the produced crate::backend::AccountInfo has all has_* flags true with zero/false values.
     - If unconditional mode is chosen, include a contract account test (code non-empty) and still expect presence.
   - Integration smoke:
     - Build backend and run a simple NON_VM transfer that creates a new account.
     - Capture Java remote CSV; verify account state_change newValue/oldValue have AEXT tail (length increases by 76 bytes).
     - Confirm Java remote logs show “Appending AEXT tail …” (RemoteExecutionSPI debug).
   - Regression check:
     - Ensure no change to execution status, energy, return data, or storage slot changes.

5) Observability
   - Add debug log in service.rs when constructing AccountInfo:
     - “AccountInfo AEXT presence: mode=zeros, is_eoa=true/false, address=<base58>”.
   - Consider a one-time startup log printing the configured AEXT mode.

6) Documentation
   - Keep this TODO file as the canonical implementation plan.
   - Optionally add a brief note in rust-backend/README.md explaining the config knob and AEXT behavior for analysts.

Validation Plan
- Build and run:
  - cd rust-backend && cargo build --release
  - Start backend and run Java node in remote mode.
- Generate CSVs for a small block range that includes EOAs and contract accounts.
- Verify:
  - Remote CSV account change entries now include the AEXT tail (hex length grows from 152 to 304 per account entry).
  - Remote Java log shows RemoteExecutionSPI appended AEXT (debug line).
  - No change to success/energy/storage_changes counts.
- Note: This achieves presence parity; digest parity will still differ if embedded includes non-zero resource values while remote emits zeros. Addressing digest equality requires either real values on backend or digest rules that ignore AEXT.

Risks & Mitigations
- Larger payloads: +76 bytes per AccountInfo when present. Mitigation: gate via config.
- EOAs vs contracts: If only EOAs get AEXT, mixed presence may remain. Mitigation: unconditional presence mode.
- Future behavior drift: When backend starts tracking real values, zeros → real may change digests again. Mitigation: version/config gating, document expectations.

Rollout Plan
- Phase 1 (dev): Implement zeros mode behind a flag; default to none.
- Phase 2 (test): Enable zeros in a sandbox run; confirm CSV shape and RemoteExecutionSPI behavior.
- Phase 3 (prod opt-in): Enable zeros in environments where shape parity aids analysis; keep fallback to none.

Acceptance Criteria
- For an EOA AccountChange produced by the backend, Java RemoteExecutionSPI logs that AEXT was appended and the resulting account serialization length increases by 76 bytes.
- Optional: For unconditional mode, the above holds for contract accounts as well.
- No regression in execution results (status, energy, logs, storage changes).

Open Questions
- Do we want unconditional AEXT presence for contracts to simplify parity?
- Should digest computation ignore the AEXT segment to compare “core” account fields only?
- Do we need a Java property to force append even without proto presence (fallback path)?

References
- Java remote AEXT gating: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:528–555, 581–589
- Java embedded AEXT serialization: framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournal.java:269–337, 347–425
- Rust backend AccountInfo conversion: rust-backend/crates/core/src/service.rs:3440–3580 (closure convert_account_info ~3543)

