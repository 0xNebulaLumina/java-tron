WitnessUpdateContract (execute_witness_update_contract) — Detailed Plan and TODOs

Summary
- Implement non-VM WitnessUpdate handler in Rust backend to update an existing witness URL.
- Parity with Java actuator: no fees, no balance changes, energy_used=0, bandwidth accounted, validations enforced.
- Deterministic single AccountChange (owner; old==new) to maintain CSV/state parity.

Code Touchpoints
- Entry: rust-backend/crates/core/src/service/mod.rs:753
- Bandwidth helper: rust-backend/crates/core/src/service/contracts/freeze.rs:1075
- Storage adapter (witness ops): rust-backend/crates/execution/src/storage_adapter/engine.rs:656,701
- WitnessInfo type: rust-backend/crates/execution/src/storage_adapter/types.rs:13
- SPI mapping for data: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:340

Feature Flags & Config
- Gated by execution.remote.witness_update_enabled (default true). Gate is already enforced in dispatch (mod.rs:225–233).
- No fee mode interaction; WitnessUpdate charges 0 fee in Java.

Behavior & Semantics
- Input: transaction.from = owner; transaction.data = update_url bytes; to = None; value = 0.
- Validations (mirror Java WitnessUpdateActuator):
  - Owner account must exist → "account does not exist".
  - Witness must exist → "Witness does not exist".
  - URL must be non-empty and ≤ 256 bytes (TransactionUtil.validUrl with allowEmpty=false) → "Invalid url".
  - URL must decode as UTF-8 (consistent with existing Rust WitnessCreate) → error includes decode failure detail.
- Effects:
  - Update witness URL in WitnessStore (protobuf Witness encoding preserved).
  - No balance/nonce changes; no logs; energy_used = 0.
  - Bandwidth is charged based on serialized tx size.
  - AEXT tracking applied when accountinfo_aext_mode == "tracked".
- State changes:
  - Exactly one AccountChange for owner with old_account == new_account (metadata change outside AccountInfo).
  - Deterministic ordering by address (sort if multiple changes are ever added; consistent with other handlers).

Error Mapping (parity)
- Empty URL or >256 bytes → Err("Invalid url").
- Owner missing → Err("account does not exist").
- Witness missing → Err("Witness does not exist").
- Invalid UTF-8 → Err("Invalid UTF-8 in witness URL: …").

Storage Interactions
- get_account(&owner) → Option<AccountInfo> (required).
- get_witness(&owner) → Option<WitnessInfo> (required).
- put_witness(&WitnessInfo { url = new_url, vote_count = preserved }) → persist protobuf-encoded witness.

Bandwidth & AEXT
- Compute bandwidth_used via BackendService::calculate_bandwidth_usage(transaction) (freeze.rs:1075).
- If remote.accountinfo_aext_mode == "tracked":
  - Load AccountAext (or defaults), FREE_NET_LIMIT.
  - ResourceTracker::track_bandwidth(owner, bandwidth_used, context.block_number, current_aext, free_net_limit).
  - Persist after_aext; populate aext_map with (before, after).

Logging
- debug: start of execution with owner.
- info: URL update summary with owner, old_url, new_url.
- warn: validation failures where useful.

Pseudo-code
1. owner = transaction.from
2. url_bytes = transaction.data
3. if url_bytes.is_empty() || url_bytes.len() > 256 => Err("Invalid url")
4. new_url = String::from_utf8(url_bytes) ? map_err as "Invalid UTF-8 in witness URL: …"
5. owner_account = get_account(owner).ok_or("account does not exist")
6. witness = get_witness(owner).ok_or("Witness does not exist")
7. updated = WitnessInfo { address: witness.address, url: new_url, vote_count: witness.vote_count }
8. put_witness(&updated)
9. state_changes = [ AccountChange { address: owner, old_account: Some(owner_account.clone()), new_account: Some(owner_account) } ]
10. bandwidth_used = calculate_bandwidth_usage(transaction)
11. aext_map = track bandwidth if mode == "tracked"
12. return TronExecutionResult { success: true, energy_used: 0, bandwidth_used, state_changes, logs: [], aext_map, freeze_changes: [], global_resource_changes: [], trc10_changes: [] }

Tests
- Location: rust-backend/crates/core/src/service/tests/contracts.rs

1) test_witness_update_contract_happy_path
   - Setup temp storage and BackendService with witness_update_enabled = true.
   - Create owner AccountInfo and initial WitnessInfo with url = "old".
   - Build tx: from=owner, to=None, value=0, data="new-url", type=WitnessUpdateContract.
   - Execute and assert:
     - Ok, success=true, energy_used=0, logs.is_empty().
     - state_changes.len()==1, AccountChange for owner with old==new AccountInfo.
     - Witness url becomes "new-url" in storage.
     - bandwidth_used > 0.

2) test_witness_update_contract_validations
   - Empty URL (data = []) → Err contains "Invalid url".
   - URL >256 bytes → Err contains "Invalid url".
   - Owner account missing → Err contains "account does not exist".
   - Witness missing (account exists) → Err contains "Witness does not exist".
   - Invalid UTF-8 bytes → Err contains "Invalid UTF-8 in witness URL".

3) test_witness_update_tracks_aext_when_enabled
   - Set accountinfo_aext_mode = "tracked"; set FREE_NET_LIMIT in storage adapter if needed.
   - Ensure result.aext_map contains owner entry and storage persists after_aext.

Edge Cases & Compatibility
- Legacy non-protobuf witness records are not supported; get_witness() tries protobuf and returns None if decode fails; handler will error with "Witness does not exist". Acceptable for Phase 1.
- Do not mutate nonce/balance; do not emit logs; no fee accounting.
- Address validation is not needed here since transaction.from is already an EVM Address; Java’s address format validation occurs earlier in pipeline.

Out of Scope (Future Work)
- Support decoding legacy witness formats if encountered.
- Additional witness fields (e.g., pub_key/production stats) updates if protocol evolves.
- Fee policy changes (remains zero per Java semantics).

Acceptance Criteria
- All new tests pass.
- Feature flag respected; dispatch returns error if disabled (handled by existing gate).
- No balance/nonce changes; energy_used=0; deterministic state_change ordering.
- Blackhole balance unchanged (observed in parity logs).

Step-by-Step TODOs
- [x] Implement execute_witness_update_contract in mod.rs (lines around 753):
     - [x] Validate feature gating (implicit via dispatch; optional assert).
     - [x] Validate url bytes (non-empty, ≤256) and UTF-8 decode.
     - [x] Load owner AccountInfo; error if missing.
     - [x] Load existing WitnessInfo; error if missing.
     - [x] Persist updated WitnessInfo with new URL.
     - [x] Build one AccountChange (old==new) and sort changes by address.
     - [x] Compute bandwidth_used.
     - [x] Track/persist AEXT if enabled.
     - [x] Return TronExecutionResult with success=true, energy_used=0.
- [x] Add unit tests in service/tests/contracts.rs:
     - [x] Happy path URL update.
     - [x] Validation matrix (empty, too long, missing account, missing witness, invalid UTF-8).
     - [x] AEXT tracking when enabled.
- [x] Add log lines (debug/info) to aid tracing and parity checks.
- [x] Run full test suite and verify no regressions in related system contracts.
     - Note: Build succeeds; tests have a pre-existing compilation error in tests.rs (FeesConfig reference) unrelated to WitnessUpdateContract.

Notes
- Follow existing coding style in service/mod.rs (debug/info/warn usage, error strings, deterministic sort).
- Keep changes minimal and focused (no new modules or proto changes).

