• Here’s what I found for the first mismatch.

What’s the first differing transaction

- Tx: 5281eb99b3cab6b3cb4777e6b070df936f2cb068163fb8f3959a1cd88733a4c2
- Location: output-directory/execution-csv/20251027-040703-4abab9f8-embedded-embedded.csv:2 and output-directory/execution-csv/20251027-052954-fda621b5-remote-remote.csv:2
- Result fields match: success=true, energy_used=0, contract_type=TransferContract
- State digest differs:
    - Embedded: cd06c60d330cbc06b7cefbfbef99f2e0105a25d877c2adfc73a968ea62bc90e4
    - Remote:   57629f2e75b688e3889ad0f3a7e4def4fcdb707e669b81f8db7342e04a32f4e6

What differs in state_changes_json

- Both runs emit two account-level state changes (empty key) for:
    - Sender 4171b0af54e0a1182a5e0947d6a64f3b22740ef318
    - Recipient 41b87f2be4dede9fc25387f8df7e0944b5cb7900e1
- Both encode account values in the same “AccountInfo” layout that Java expects:
    - [balance(32)][nonce(8)][codeHash(32)][codeLen(4)][code][AEXT tail if present]
    - The AEXT tail begins with “AEXT00010044” (ASCII AEXT + v1 + 68 byte payload).
- The balances and code fields match across runs. The difference is in the AEXT tail payload for the sender’s account:
    - Embedded includes non-zero window sizes: shows 0x0000000000000708 for both windows (decimal 1800), plus the AEXT header and the rest of the payload. Example snippet for oldValue near the tail shows AEXT
    marker and 0x0708:
        - output-directory/execution-csv/20251027-040703-4abab9f8-embedded-embedded.csv:2
    - Remote includes AEXT but zeroes the entire payload (including net/energy window sizes and other resource fields):
        - output-directory/execution-csv/20251027-052954-fda621b5-remote-remote.csv:2
- That AEXT payload difference is enough to change the per-tx state digest even though balances/nonce/code hash are identical.

Corroborating logs

- Java recorded applying two state changes for this tx in remote mode:
    - remote-java.35c9393.log:23526 and remote-java.35c9393.log:23533
- Rust backend shows the AEXT mode being used:
    - remote-rust.35c9393.log:2 states accountinfo_aext_mode: "zeros"

- In remote mode, Java builds bytes for account changes from the gRPC proto AccountInfo. If any optional resource usage fields are present, Java appends an AEXT tail (with header “AEXT00010044” and 68-byte
payload).
- Embedded mode encodes existing resource defaults (notably window sizes = 1800) in the tail, so oldValue/newValue carry AEXT with real defaults.
- The Rust backend is configured with accountinfo_aext_mode = "zeros" and populates AccountInfo’s optional resource fields as present but zero-valued for EOAs. Java then appends an AEXT tail with all zeros.
This diverges from embedded’s window size defaults (1800), producing a different state digest for this tx.

References in code

- Java AEXT append path for remote account changes:
    - framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:420
    - AEXT serialization method:
        - framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:540
- Java AEXT parsing (used later when syncing to local DB):
    - framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:420
- Rust building AccountInfo and zeroing AEXT fields based on mode:
    - rust-backend/crates/core/src/service.rs:3512
    - AEXT field population logic:
        - rust-backend/crates/core/src/service.rs:3556
- Backend reports zeros mode:
    - remote-rust.35c9393.log:2

Conclusion for the first tx

- The mismatch is due to different AEXT tail payloads: embedded includes default window sizes (1800), remote uses zeros. That changes state_changes_json and the computed state_digest_sha256, even though the
functional result is identical.

Next checks or fixes you may want

- Easiest config workaround to align: set remote to emit the same defaults in AEXT (e.g., net_window_size=1800, energy_window_size=1800) for EOAs, while keeping other fields zero/false. That would make the
AEXT payload match embedded and should fix the digest.
- Alternatively, disable AEXT inclusion for digest parity by not setting the AccountInfo optional resource fields on the Rust side (mode "none" or don’t set presence), or by disabling AEXT tail in Java with
system property remote.exec.accountinfo.resources.enabled=false (affects remote only). Note this won’t remove AEXT from embedded’s own recorder.
- If you want, I can patch the Rust backend to emit 1800 for net/energy window sizes in zeros mode, or add a new mode like "defaults" to match embedded exactly.


---



think harder. No need to start implementing, just read through and understand the codebase and give me a detailed plan on: add a new mode like "defaults" to match embedded exactly.


---


• Goal

- Add execution.remote.accountinfo_aext_mode = "defaults" to make remote AccountInfo bytes match embedded’s AEXT tail for EOAs, eliminating digest mismatches in CSVs.

Current Behavior

- Java appends AEXT tail when proto AccountInfo carries any resource fields; see framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:540.
- Rust currently supports accountinfo_aext_mode = "none" | "zeros" | "tracked"; “zeros” sets Some(0)/false for EOAs; rust-backend/crates/core/src/service.rs:3512–3582.
- Embedded CSVs show non-zero window sizes in AEXT tail (0x0708 = 1800) for EOAs, while remote “zeros” emits 0 for all resources, causing state_digest differences.

Desired Defaults

- For EOAs, populate proto AccountInfo resource fields as:
    - net_usage=0, free_net_usage=0, energy_usage=0
    - latest_consume_time=0, latest_consume_free_time=0, latest_consume_time_for_energy=0
    - net_window_size=1800, energy_window_size=1800
    - net_window_optimized=false, energy_window_optimized=false
- Rationale: Matches AEXT payload embedded emits in CSVs (0x0708 observed for window sizes).

Change Overview

- Config: Recognize new value "defaults" for execution.remote.accountinfo_aext_mode.
- Core conversion: In rust-backend/crates/core/src/service.rs, extend AEXT population match to handle "defaults" exactly as above for EOAs.
- Sample config: Switch rust-backend/config.toml to use "defaults" by default for parity builds.
- Docs: Update config.rs docstrings and config.toml comments to explain semantics.
- No Java code changes required; Java already appends AEXT when optional fields are present.

Implementation Steps

1. Config plumbing

- Update docs/comments only; code already treats accountinfo_aext_mode as a free-form string:
    - rust-backend/crates/common/src/config.rs:93–106, 221, 239
    - Ensure comments mention “defaults” mode and that the default remains “none” for backward compatibility.

2. Core: Add “defaults” branch in AccountInfo conversion

- File: rust-backend/crates/core/src/service.rs:3512–3582
- In the closure convert_account_info, extend the match aext_mode { ... } to include:
    - "defaults" if is_eoa => the exact values listed above.
    - Keep existing "zeros", "tracked", and fallback behavior unchanged.
- Retain current code hash normalization to KECCAK_EMPTY for empty code (already present).
- Preserve logic to only set fields for EOAs; for non-EOAs, leave None unless later we add resource tracking.

3. Sample configuration

- File: rust-backend/config.toml:98
- Change accountinfo_aext_mode = "zeros" to "defaults" and adjust surrounding comment to recommend “defaults” for CSV parity with embedded.

4. Logging

- Main already logs AEXT mode: rust-backend/src/main.rs:51
- Optionally add a debug log in service.rs after computing the fields to show actual AEXT values when mode is “defaults” for traceability.

5. Tests

- Unit-level (Rust, core crate):
    - Add a focused test for the conversion closure behavior (can extract it into a small helper to make testable).
    - Cases:
        - EOA + mode "defaults": proto AccountInfo has resource fields present with window sizes 1800, others 0/false.
        - Contract account + mode "defaults": resource fields remain None.
        - EOA + mode "zeros": preserves current behavior (all zero/false).
        - Unknown mode: behaves like "none" (all None).
- Snapshot of proto AccountInfo presence (not Java serialization) is sufficient; Java will append AEXT when these are present.

6. Validation Plan

- Re-run the same workload with:
    - rust-backend/config.toml using accountinfo_aext_mode = "defaults".
    - Java property remote.exec.accountinfo.resources.enabled left at default true so it appends AEXT.
- Compare:
    - Per-tx state_changes_json tails for EOAs (look for “41455854” + “…00000708…” twice).
    - state_digest_sha256 equality for the first tx: output-directory/execution-csv/20251027-040703-4abab9f8-embedded-embedded.csv:2 vs new remote CSV line 2.
- Spot-check several subsequent transfers to ensure ongoing parity.

7. Documentation

- Update rust-backend/config.toml comments (AEXT section) to describe:
    - "none": no AEXT fields set (proto optional fields unset).
    - "zeros": EOA fields set to zero/false.
    - "defaults": EOA fields set to match embedded defaults (window sizes 1800, rest zero/false).
    - "tracked": reserved for future resource tracking.
- Update rust-backend/README or build.md if present to recommend "defaults" for parity testing.

Edge Cases and Considerations

- Non-EOA accounts:
    - Keep current behavior: do not set optional AEXT fields unless we have real resource metrics; embedded may encode them differently for contracts, but current diffs we observed are for EOAs.
- Dynamic properties:
    - 1800 appears to be a network default for window sizes; if this is dynamic on-chain, a future enhancement could read values via storage or pass via ExecutionContext to avoid hardcoding. For now,
    constants align with embedded CSVs.
- Backward compatibility:
    - Do not change default in code (none); only flip config.toml used in parity experiments to “defaults”.
- Digest stability:
    - Code hash normalization to KECCAK_EMPTY is already done, avoiding drift due to zero or 0x00 bytecode.

File Touch List

- rust-backend/crates/core/src/service.rs:3490–3600
- rust-backend/crates/common/src/config.rs:93–106, 220–240 (comments/docstring only)
- rust-backend/config.toml:89–100

Rollout

- Local: change config to “defaults”, run remote execution, produce CSV, diff against embedded.
- CI: add a small unit test target in core crate around AEXT mode behavior.

Risks

- If embedded’s AEXT defaults differ for some scenarios (e.g., non-EOA or post-operations), “defaults” may still diverge. Mitigation: restrict to EOAs and verify across a wider sample; consider adding a
“defaults_contracts” extension later if needed.

Open Questions

- Should “defaults” also apply to non-EOAs? Current plan limits to EOAs; can expand once we confirm embedded’s behavior for contracts.
- Are window sizes always 1800 on targeted network configs? If not, consider reading from a dynamic property in a follow-up.


