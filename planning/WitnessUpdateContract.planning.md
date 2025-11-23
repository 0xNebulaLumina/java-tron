• I compared the two CSVs, then traced mismatches through the Java and Rust logs.

What I Compared

- Files: output-directory/execution-csv/20251117-102033-01aa61f2-embedded-embedded.csv vs output-directory/execution-csv/20251123-090003-68fed2e4-remote-remote.csv
- Common transactions by tx_id_hex: 1287
- Mismatches found: 3

Mismatches Found

- All 3 are WitnessUpdateContract and differ only in is_success/result_code:
    - bc3284e33345d681a9b49bde07b878a45c9ea01172213ec06407d5d3d67e8467 (block 4386, idx 1): embedded=SUCCESS, remote=REVERT
    - f7670dea96c3c1094af18a9395c5f652ff4959c5c706d578f4f32a15ef222051 (block 4564, idx 0): embedded=SUCCESS, remote=REVERT
    - a3b4b0ed4979a742bc944226c322c744e573cd82466119b08d1258288a1e72f3 (block 4649, idx 0): embedded=SUCCESS, remote=REVERT
- For all 3: energy_used=0, state_change_count=0, state_digest_sha256=e3b0c442... (empty digest) in both runs.

Root Cause

- The Rust backend has witness_update_enabled = false, so it rejects WitnessUpdateContract (marks as REVERT), even though embedded Java treats these as successful no-ops.
    - Config flag: rust-backend/config.toml:69
    - Confirmed in backend startup: remote-rust.a14322a.log:2 shows witness_update_enabled: false
    - Runtime errors for these txs:
        - remote-rust.a14322a.log:21177 Non-VM contract execution failed: WITNESS_UPDATE_CONTRACT execution is disabled - falling back to Java
        - remote-rust.a14322a.log:21412 same for f7670...
        - remote-rust.a14322a.log:21441 same for a3b4b0...
- The Java CSV for the remote run still records REVERT, i.e., it does not reflect a successful fallback result for these system contracts.

---

• Here’s a concrete, code-aware plan to implement execute_witness_update_contract in the Rust backend with parity to the Java actuator.

Entry Points

- Implement in rust-backend/crates/core/src/service/mod.rs:753:
    - fn execute_witness_update_contract(&self, storage_adapter, transaction, context) -> Result<TronExecutionResult, String>
- No proto or SPI changes required: RemoteExecutionSPI already packs WitnessUpdateContract.update_url into transaction.data and leaves to empty.

Behavior

- Update the existing witness’s URL.
- No balance/energy changes, no logs, energy_used=0.
- Charge no fee (matches Java calcFee() = 0).
- Bandwidth counting applies, and AEXT tracking if enabled.

Validation Rules

- Use transaction.from as the owner (SPI already sets this).
- URL:
    - Non-empty and ≤ 256 bytes (mirror TransactionUtil.validUrl which uses allowEmpty=false and maxLength=256).
    - Decode as UTF‑8. If invalid UTF‑8, return an error (match existing WitnessCreate handler style).
- Owner account must exist.
- Witness must exist.

Suggested error messages (aiming for parity with Java):

- Empty/too long: "Invalid url"
- Missing account: "account does not exist"
- Missing witness: "Witness does not exist"
- Bad UTF-8: "Invalid UTF-8 in witness URL: …" (consistent with Rust-side patterns elsewhere)

State Changes

- Emit exactly one AccountChange for the owner with old_account == new_account (no structural changes). This keeps CSV/state-diff parity with other system contracts where metadata updates live outside
AccountInfo.
- Sort state_changes deterministically by address (pattern used elsewhere).

Bandwidth and AEXT

- Compute bandwidth_used = Self::calculate_bandwidth_usage(transaction).
- If execution.remote.accountinfo_aext_mode == "tracked":
    - Load AccountAext for owner or default.
    - Load FREE_NET_LIMIT from dynamic properties.
    - Apply ResourceTracker::track_bandwidth(owner, bandwidth_used, context.block_number, current_aext, free_net_limit).
    - Persist new AEXT via storage_adapter.set_account_aext(&owner, &after_aext).
    - Put (before_aext, after_aext) into aext_map.

Storage I/O

- Read owner account: storage_adapter.get_account(&transaction.from) → required.
- Read witness: storage_adapter.get_witness(&transaction.from) → required.
- Update witness URL:
    - Create updated WitnessInfo by cloning old and replacing url.
    - Persist with storage_adapter.put_witness(&updated_witness).
- No balance updates; no blackhole/fee accounting.

Logging

- Add helpful logs:
    - debug: "Executing WITNESS_UPDATE_CONTRACT for owner {tron_address}"
    - info: "Updated witness URL: owner={tron}, old_url='{..}', new_url='{..}'"
    - debug/warn around validation failures for troubleshooting.

Implementation Steps

1. Parse update URL
    - let url_bytes = &transaction.data;
    - Reject empty or >256 bytes → "Invalid url".
    - let new_url = String::from_utf8(url_bytes.to_vec())?; (propagate a clear error on decode failure).
2. Validate preconditions
    - get_account(&owner): if None → "account does not exist".
    - get_witness(&owner): if None → "Witness does not exist".
3. Apply mutation
    - Build updated WitnessInfo with same address and vote_count; update url.
    - put_witness(&witness_info) (protobuf serialized).
4. Build state_changes
    - Owner AccountChange where old == new AccountInfo (no-op).
    - Sort by address.
5. Bandwidth/AEXT
    - Compute bandwidth_used.
    - If tracked mode, compute and persist AEXT and populate aext_map.
6. Return TronExecutionResult
    - success=true, energy_used=0, logs=[], state_changes=[1 no-op], bandwidth_used, aext_map (possibly empty), no freeze/global/trc10 changes.

Test Plan
Add unit tests in rust-backend/crates/core/src/service/tests/contracts.rs:

- Happy path: test_witness_update_contract_happy_path
    - Setup:
        - Temp storage engine and EngineBackedEvmStateStore.
        - Register execution module with RemoteExecutionConfig { system_enabled: true, witness_update_enabled: true, .. }.
        - Create owner account (non-zero balance) and initial witness entry: WitnessInfo { url: "old", vote_count: 0 }.
    - Execute:
        - Build TronTransaction with from=owner, to=None, value=0, data="new-url", contract_type=WitnessUpdateContract.
        - Call service.execute_witness_update_contract.
    - Assert:
        - Ok result, success=true, energy_used=0.
        - Exactly one AccountChange for owner with old==new AccountInfo.
        - Witness in storage now has url="new-url".
        - bandwidth_used > 0, logs empty.
- Validation: test_witness_update_contract_validations
    - Empty URL (data=empty) → Err("Invalid url").
    - URL >256 bytes → Err("Invalid url").
    - Owner account missing → Err("account does not exist").
    - Witness missing (account exists but no witness) → Err("Witness does not exist").
    - Invalid UTF-8 → Err containing "Invalid UTF-8 in witness URL".
- AEXT tracking: test_witness_update_tracks_aext_when_enabled
    - With accountinfo_aext_mode = "tracked", assert aext_map contains owner and AEXT persisted with increased net usage.

Code References

- Stub to implement: rust-backend/crates/core/src/service/mod.rs:753
- Witness create (for style/parity): rust-backend/crates/core/src/service/mod.rs:540
- Vote witness (for CSV parity/no fee): rust-backend/crates/core/src/service/mod.rs:927
- Account update (pattern for one no-op AccountChange): rust-backend/crates/core/src/service/mod.rs:1200
- Storage adapter witness helpers: rust-backend/crates/execution/src/storage_adapter/engine.rs:656 (get_witness), :701 (put_witness)
- WitnessInfo type: rust-backend/crates/execution/src/storage_adapter/types.rs:13
- Bandwidth calc helper: rust-backend/crates/core/src/service/contracts/freeze.rs:1075
- Remote SPI mapping for data: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:394

Compatibility Notes

- SPI packs only update_url into data, not owner_address. Use transaction.from for owner.
- No fee handling; preserves blackhole balance (logs elsewhere show delta 0).
- Deterministic state change order keeps CSV parity with Java-side analysis tools.

Risks and Edge Cases

- Legacy-stored witnesses (non-protobuf) would be unseen by get_witness (it tries protobuf first). The engine currently returns Ok(None) if decode fails; this will surface as "Witness does not exist". This is
acceptable for modern stores and genesis created via Java protobuf.
- If you want stricter parity with Java for URL encoding (Java’s ByteString.toStringUtf8 tolerates invalid data), consider using String::from_utf8_lossy as a follow-up. For now, match existing Rust create
handler behavior and require valid UTF‑8.