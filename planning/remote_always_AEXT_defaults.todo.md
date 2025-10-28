Remote AEXT "defaults" Mode — Detailed Plan and TODOs

Objective

- Add a new configuration mode execution.remote.accountinfo_aext_mode = "defaults" in the Rust backend so that EOAs (Externally Owned Accounts) carry AccountInfo resource fields that serialize to an AEXT tail identical to embedded Java-Tron defaults. This removes CSV state digest mismatches between embedded and remote for simple transfers and similar transactions.

Background and Findings

- Mismatch source: For tx 5281eb99b3cab6b3cb4777e6b070df936f2cb068163fb8f3959a1cd88733a4c2, both runs emit two account-level state changes. Balances and code hash match, but the AEXT tail differs.
- Embedded AEXT tail: includes net/energy window sizes set to 1800 (0x0708), while other resource fields appear as zero/false. This is visible in output-directory/execution-csv/20251027-040703-4abab9f8-embedded-embedded.csv:2 (look for "41455854 0001 0044 ... 0000000000000708 ... 0000000000000708").
- Remote AEXT tail (current): with accountinfo_aext_mode = "zeros", all AEXT payload fields, including window sizes, are zero; see output-directory/execution-csv/20251027-052954-fda621b5-remote-remote.csv:2.
- Java RemoteExecutionSPI appends AEXT when any optional resource fields are present in proto AccountInfo; see framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java (serializeAextTailFromProto).

Outcome Target

- New mode "defaults": For EOAs, set proto AccountInfo optional fields as follows:
  - net_usage = 0
  - free_net_usage = 0
  - energy_usage = 0
  - latest_consume_time = 0
  - latest_consume_free_time = 0
  - latest_consume_time_for_energy = 0
  - net_window_size = 1800
  - energy_window_size = 1800
  - net_window_optimized = false
  - energy_window_optimized = false
- Keep existing behaviors:
  - "none": do not set any optional fields (proto presence unset).
  - "zeros": set all optional fields to zero/false for EOAs.
  - "tracked": reserved for future; same as "none" for now.
- Restrict to EOAs (empty code). For contract accounts, continue emitting None for all resource fields until resource tracking exists.

Non-Goals (for this change)

- Do not implement real resource tracking (remain as TODO for "tracked").
- Do not alter Java serialization logic or StateChange canonicalization.
- Do not change the default code behavior in config.rs (default remains "none"). We may flip config.toml in repo to use "defaults" for parity experiments only.

Design

- Location of change: rust-backend/crates/core/src/service.rs — AccountChange conversion builds crate::backend::AccountInfo proto. Extend the match on aext_mode to support "defaults" for EOAs with the field values above.
- Code hash normalization: retain existing normalization to KECCAK_EMPTY for empty code, already implemented.
- Proto to bytes path:
  - Rust sets optional fields in AccountInfo proto depending on mode.
  - Java RemoteExecutionSPI.serializeAccountInfo detects presence and appends AEXT tail with header (AEXT 0x0001 0x0044) and a 68-byte payload encoding the above values.

Detailed TODOs

1) Config Plumbing and Documentation

- [x] Update rust-backend/crates/common/src/config.rs comments for RemoteExecutionConfig.accountinfo_aext_mode to document the new "defaults" option and its semantics (EOA-only).
- [x] Keep builder default as "none" (rust-backend/crates/common/src/config.rs). Do not change the runtime default.
- [x] Update rust-backend/config.toml comments (AEXT section) to describe modes: none, zeros, defaults, tracked.
- [x] In rust-backend/config.toml, set accountinfo_aext_mode = "defaults" for parity testing in this repo (optional, but recommended during validation).

2) Core Conversion Logic (service.rs)

- [x] Extend the AEXT population match in rust-backend/crates/core/src/service.rs within the AccountChange conversion closure (convert_account_info) to add:
  - Case "defaults" if is_eoa:
    - net_usage = Some(0)
    - free_net_usage = Some(0)
    - energy_usage = Some(0)
    - latest_consume_time = Some(0)
    - latest_consume_free_time = Some(0)
    - latest_consume_time_for_energy = Some(0)
    - net_window_size = Some(1800)
    - net_window_optimized = Some(false)
    - energy_window_size = Some(1800)
    - energy_window_optimized = Some(false)
  - All non-EOA accounts: fall through to None for all resource fields.
- [x] Keep existing branches: "zeros", "tracked", default -> None.
- [x] Add a debug log when mode == "defaults" for EOAs to print the selected values to aid traceability.

3) Sample Config and Developer Docs

- [x] rust-backend/config.toml:
  - Change accountinfo_aext_mode to "defaults".
  - Update narrative comments to state "defaults" is recommended for CSV parity with embedded, especially for benchmark/verification runs.
- [ ] If present, update rust-backend/README.md or build.md to describe the new mode and parity guidance.

4) Tests (Rust)

Unit tests (core crate):

- [ ] Extract/encapsulate the AEXT selection logic into a small helper (pure function) to facilitate direct unit testing without needing to construct the full service. Alternatively, test via a narrow surface creating a fake AccountInfo and observing the resulting proto fields from convert_account_info.
- [ ] Test: EOA + mode "defaults" — verify proto AccountInfo optional fields presence and values match the spec above (window sizes 1800, rest zero/false).
- [ ] Test: Contract account + mode "defaults" — verify all optional fields are None (not present).
- [ ] Test: EOA + mode "zeros" — verify all optional fields zero/false (current behavior preserved).
- [ ] Test: EOA + mode "none" — verify all optional fields None.
- [ ] Test: Unknown mode — verify fallback to None.

Optional serialization consistency check (integration-ish):

- [ ] Round-trip check that Java’s AEXT serializer would produce the expected header and payload sizes given these proto fields (document expectations; full cross-language test may be omitted here but is covered by acceptance validation below).

5) Logging and Observability

- [ ] service.rs: keep info!(...) logging of selected mode (already logs at startup, rust-backend/src/main.rs). Add debug! indicating when "defaults" branch is active per account and the key values being set.
- [ ] Optionally add a short-lived feature flag to force logging of first N AEXT emissions for easier triage.

6) Acceptance Validation (Manual)

Environment prep:

- [ ] Build Rust backend: cd rust-backend && cargo build --release
- [ ] Ensure rust-backend/config.toml has accountinfo_aext_mode = "defaults".
- [ ] Start backend: ./rust-backend/target/release/tron-backend
- [ ] Build Java and re-run the same workload using remote execution + remote storage.

CSV parity checks:

- [ ] Compare state_changes_json for the first few transfers; confirm AEXT tail present with:
  - Magic/version/length: 41 45 58 54 00 01 00 44
  - Two 64-bit window sizes equal to 0x0000000000000708
  - All other 64-bit resource metrics zero; booleans zero (false)
- [ ] Verify state_digest_sha256 equality for the first differing tx previously observed (line 2 in both CSVs).
- [ ] Spot check several subsequent blocks/txs for parity.

7) Edge Cases and Future Work

- Non-EOA behavior: remain None (no AEXT) in all modes except future "tracked". If embedded starts emitting defaults for contracts, revisit.
- Window size constants: 1800 is observed in embedded CSVs. If this is dynamic across networks/configs, consider a follow-up to source it from dynamic properties or a backend-configurable parameter (e.g., execution.remote.aext_defaults.net_window_size / energy_window_size).
- Backward compatibility: runtime default remains "none"; only the repo’s sample config.toml changes for parity-focused development. Document this clearly.
- Potential shadow verification improvement: make the AEXT inclusion consistent across modes to reduce digest drift when running shadow comparisons.

8) Risks and Mitigations

- Risk: Embedded may emit different defaults in scenarios not yet observed (e.g., after certain system contracts). Mitigation: restrict "defaults" to EOAs only; expand later if needed; expand acceptance sample set.
- Risk: Hidden reliance on dynamic properties for resource windows. Mitigation: parametrize defaults via config in a later PR if parity diverges across networks.
- Risk: Developers confuse "zeros" and "defaults". Mitigation: clear config.toml comments and README section with examples.

9) Rollout Plan

- [ ] Open PR: feat(backend): add AEXT defaults mode for parity
- [ ] Include rationale and CSV before/after diff for the first tx, highlighting digest fix.
- [ ] Land with unit tests; keep default = none; config.toml switched to defaults in repo for parity workflows.
- [ ] Track downstream parity results; if stable, consider making "defaults" the repo’s recommended mode for verification tasks.

References (for implementers)

- Java AEXT serialization: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java
- Java AEXT parsing (for local DB sync): framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java
- Rust AccountInfo conversion: rust-backend/crates/core/src/service.rs
- Config definition: rust-backend/crates/common/src/config.rs
- Sample config: rust-backend/config.toml

