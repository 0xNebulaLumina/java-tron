Title: better_table-2 Review — Detailed Plan and TODOs

Context
- Goal: One expanded CSV with base columns unchanged and per-domain triplets: X_changes_json, X_change_count, X_digest_sha256; ensure deterministic canonicalization and identical outputs across embedded and remote modes.
- Current status (last 3 commits):
  - CSV header updated to include domain triplets and ts_ms.
  - DomainCanonicalizer implemented with deterministic ordering + digest; AEXT parsing; split account vs EVM storage.
  - Embedded journaling added for TRC‑10 (balances, issuance), votes, freezes (V2), and global totals; Manager wires DomainChangeJournal lifecycle.
  - Builder fills domains for both modes (remote via ExecutionProgramResult; embedded via journals/state journal).
- Gaps vs earlier plan/TODO:
  - Remote path uses placeholders for old/new in several domains (TRC‑10 balances, votes, global totals).
  - Empty-array digest policy mismatch (TODO calls sha256(""); code uses empty string).
  - Freeze V1/Unfreeze hooks not yet present (only V2 covered).
  - TRC‑10 transfer actuator-specific hooks not explicitly added (relies on AccountCapsule balance mutators; verify coverage).
  - Minor: return_data_len not set when return data is provided as hex string in builder; not critical but easy to align.

Schema (kept)
- Base columns (unchanged): exec_mode, storage_mode, block_num, block_id_hex, is_witness_signed, block_timestamp, tx_index_in_block, tx_id_hex, owner_address_hex, contract_type, is_constant, fee_limit, is_success, result_code, energy_used, return_data_hex, return_data_len, runtime_error
- Domain triplets (in order):
  - state_changes_json, state_change_count, state_digest_sha256
  - account_changes_json, account_change_count, account_digest_sha256
  - evm_storage_changes_json, evm_storage_change_count, evm_storage_digest_sha256
  - trc10_balance_changes_json, trc10_balance_change_count, trc10_balance_digest_sha256
  - trc10_issuance_changes_json, trc10_issuance_change_count, trc10_issuance_digest_sha256
  - vote_changes_json, vote_change_count, vote_digest_sha256
  - freeze_changes_json, freeze_change_count, freeze_digest_sha256
  - global_resource_changes_json, global_resource_change_count, global_resource_digest_sha256
  - account_resource_usage_changes_json, account_resource_usage_change_count, account_resource_usage_digest_sha256
  - log_entries_json, log_entry_count, log_entries_digest_sha256
  - ts_ms

Decisions Needed
1) Empty-array digest policy
   - Option A (current code): for empty arrays, emit digest="".
   - Option B (current TODO): for empty arrays, digest = sha256("") while *_changes_json="[]" and count=0.
   - Recommendation: pick one and align both code + docs. Default to Option B if cross‑tooling expects non-empty digests.

2) Remote old/new derivation strategy
   - Approach 1: Snapshot pre-state in a journal prior to applying remote changes; compute new from post-state.
   - Approach 2: Derive old from ExecutionProgramResult payload if it carries pre-state (not currently exposed for all domains).
   - Recommendation: Adopt Approach 1: add a lightweight PreStateSnapshot to cache required keys (TRC‑10 balances, votes per voter, global totals) before RuntimeSpiImpl.apply* mutates stores.

Canonicalization & Digest (All Domains)
- Keep: lowercase hex, no 0x; numbers as decimal strings; JSON object keys sorted lexicographically at all depths; arrays sorted by stable per-domain keys.
- Action: finalize empty-array digest policy and update DomainCanonicalizer.computeDigest accordingly.
- Action: extend tests to include input-permutation invariants and empty-array digest expectations.

Remote Mode — Detailed Plan
Source: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java (extractFromExecutionProgramResult) and Java SPI wrappers; apply path in RuntimeSpiImpl.

Pre-apply snapshot (new):
- Add a pre-apply snapshot facility in RuntimeSpiImpl to capture old values before mutating stores:
  - Votes: AccountStore.get(owner).getVotesList() keyed by (owner, witness)
  - TRC‑10 balances: AccountCapsule assetV2 for (owner, token_id)
  - Global totals: DynamicPropertiesStore: total_net_limit, total_energy_limit, total_*_weight, total_tron_power_weight, next_maintenance_time (if emitted)
- Store snapshots in a thread-local (e.g., reuse DomainChangeJournalRegistry or a dedicated PreStateSnapshotRegistry) and clear after CSV logging.

Domain mappings (post-apply, using snapshot for old):
- TRC‑10 balances
  - For each Trc10AssetTransferred: compute absolute old/new per (owner, token_id) using snapshot + post-apply store values; op: increase/decrease/set/delete based on old/new.
  - File: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java
- TRC‑10 issuance
  - Creation already mapped; for updates (if backend emits), include old/new via snapshot of AssetIssueStore or by reading before apply if provided.
  - File: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java
- Votes
  - Build per (voter, witness) entries comparing snapshot old vs new Account.votes; include deletes when a witness disappears.
  - Files: framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java (conversion helper) and/or ExecutionCsvRecordBuilder
- Freezes
  - If backend emits FreezeLedgerChange, include old from snapshot (owner/resource[/recipient]) and new from post-apply; V2: expiration=0.
  - Files: framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java
- Global resources
  - Use snapshot for old values and post-apply DynamicPropertiesStore for new; op=update.
  - Files: framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java
- Account resource usage (AEXT)
  - Derive from account state changes if provided; else omit or read post-state (new only) and skip if identical to snapshot (to avoid partial old/new without state bytes).
  - Files: framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java
- Logs
  - Already mapped from execResult.getLogInfoList(); keep as is.

Embedded Mode — Detailed Plan
- Journaling is in place; extend coverage to remaining actuators and ensure pre/post semantics:
  - TRC‑10 transfer actuators: verify hooks fire via AccountCapsule.addAssetAmountV2/reduceAssetAmountV2 across all paths (including TransferAssetActuator); add explicit hooks in TransferAssetActuator if any bypasses exist.
    - Files to verify/add: actuator/src/main/java/org/tron/core/actuator/TransferAssetActuator.java
  - Freeze V1 + Unfreeze actuators: record (owner, resource, recipient?), old/new amounts and expirations per V1 semantics.
    - Files to add/verify: actuator/src/main/java/org/tron/core/actuator/FreezeBalanceActuator.java, actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceActuator.java
  - Global totals: DynamicPropertiesStore hooks added for core totals; review any additional fields emitted by backend and add if needed.
    - File: chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java
  - Journaling lifecycle: ensure DomainChangeJournalRegistry is initialized for all tx types; current Manager wiring covers this.

Builder Integration
- ExecutionCsvRecordBuilder should:
  - Remote: use snapshot-derived old/new for TRC‑10, votes, globals; keep parsing AEXT from state changes when available.
  - Embedded: continue to read from StateChangeJournal + DomainChangeJournal.
  - Set return_data_len when setting returnDataHex from string (if easily derivable or add a helper to compute length from hex).
  - Keep legacy state_changes triplet (aggregate of account + evm_storage) and domain triplets.

Configuration & Flags
- Gating (unchanged):
  - CSV logging: -Dexec.csv.enabled, rotation/sampling/queue as-is.
  - Journaling (embedded): -Dexec.csv.stateChanges.enabled=true enables both state and domain journals.
- Remote backend config (docs only): confirm rust-backend emits all domains; document rust-backend/config.toml flags and recommended values for full CSV.

Testing Strategy
Unit tests (Java):
- DomainCanonicalizer
  - Per-domain canonicalization ordering and digest stability for permuted inputs.
  - Empty-array digest behavior (based on final policy).
  - AEXT parsing roundtrips and field omission when unchanged.
  - Files: framework/src/test/java/org/tron/core/execution/reporting/DomainCanonicalizerTest.java
- ExecutionCsvRecord
  - Header includes all fields and column count = 50.
  - CSV escaping quotes/commas/newlines.
  - return_data_len consistency.
  - Files: framework/src/test/java/org/tron/core/execution/reporting/ExecutionCsvRecordTest.java
- DomainChangeJournal
  - Merge semantics for multiple updates within a tx per domain (keep first old, last new; correct op selection).
  - Files: new tests under framework/src/test/java/org/tron/core/execution/reporting/

Integration tests (Java):
- Embedded path:
  - Simulate TRC‑10 transfer, vote change, freeze/unfreeze, and global total updates; assert *_changes_json, *_change_count, *_digest_sha256.
- Remote path:
  - Fabricate ExecutionProgramResult with domain payloads; capture pre-state snapshot; apply; assert builder produces identical outputs to embedded for the same scenario.
- Golden vectors:
  - Create small fixture set (one tx per domain) with expected JSON and digest; assert stability.

Acceptance Criteria
- Header and column count exactly match plan; base columns unchanged; order identical.
- For each exercised domain, *_change_count > 0; *_changes_json arrays sorted deterministically; *_digest_sha256 matches golden vectors.
- Constant or failed tx produce [] arrays, *_change_count=0, and digest behavior per chosen policy (Option A/B).
- Embedded and remote outputs are identical per tx (JSON + digest).
- Checkstyle and tests pass.

Risks & Mitigations
- Remote old/new correctness depends on reliable pre-state snapshots: mitigate by capturing snapshot immediately before apply, and clearing after CSV logging.
- Digest policy inconsistency with downstream tools: surface the decision explicitly; provide a migration note.
- Performance overhead from snapshots/journals: keep snapshots minimal (only keys needed for CSV), guard by exec.csv.enabled and stateChanges flag.

Rollout Plan
- Implement behind existing flags; keep journaling opt-in.
- Validate on a small block range: produce CSV for embedded and remote and diff per-domain digests.
- Monitor CSV writer metrics (enqueued/written/dropped) to ensure no regressions.

Detailed TODOs

A) Canonicalization & Digest
- [x] Decide empty-array digest policy and update code/docs accordingly.
  - [x] If Option B: update DomainCanonicalizer.computeDigest to return sha256("") for "[]"; adjust tests.
    - File: framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java
    - File: framework/src/test/java/org/tron/core/execution/reporting/DomainCanonicalizerTest.java
- [x] Add test vectors covering empty arrays per domain and ordering invariants.

B) Remote Pre-State Snapshot
- [x] Add PreStateSnapshotRegistry (thread-local) with minimal getters/setters.
  - File: framework/src/main/java/org/tron/core/execution/reporting/PreStateSnapshotRegistry.java (new class)
- [x] In RuntimeSpiImpl, before applying ExecutionProgramResult, capture pre-state into snapshot for:
  - [x] Votes: AccountStore.get(owner).getVotesList() → Map<(owner,witness), votes>
  - [x] TRC‑10 balances: AccountCapsule assetV2 → Map<(owner,token_id), balance>
  - [x] Global totals: DynamicPropertiesStore fields
  - File: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java
- [x] Clear snapshot after CSV logging (Manager already clears journals; extend to snapshot).
  - File: framework/src/main/java/org/tron/core/db/Manager.java

C) Remote Builder Mappings (absolute old/new)
- [x] TRC‑10 balances: compute absolute old/new using snapshot (old) and post-apply store (new); set op accordingly.
  - File: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java
- [x] Votes: compare snapshot vs post-apply votes per (owner,witness); add deletes where new=0.
  - File: framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java
- [x] Global resources: build deltas with old from snapshot and new from post-apply values.
  - File: framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java (conversion helper) or builder
- [ ] Freeze changes: use backend payload for new; get old from snapshot keyed by (owner, resource, recipient/null); V2 expiration=0.
  - File: framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java

D) Embedded Coverage & Hooks
- [ ] Ensure TRC‑10 balance hooks cover all update paths (including TransferAssetActuator).
  - File: actuator/src/main/java/org/tron/core/actuator/TransferAssetActuator.java
- [x] Add freeze/unfreeze V1 journaling similar to V2.
  - File: actuator/src/main/java/org/tron/core/actuator/FreezeBalanceActuator.java
  - File: actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceActuator.java
- [ ] Review additional DynamicPropertiesStore fields emitted by backend and add hooks if missing.
  - File: chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java

E) Builder polish
- [x] When using returnDataHex(String), set return_data_len from hex string length/2.
  - File: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecord.java
- [ ] Confirm ts_ms set in constructor and included in CSV row (already present); add test for ts_ms presence.
  - File: framework/src/test/java/org/tron/core/execution/reporting/ExecutionCsvRecordTest.java

F) Tests
- [x] Extend DomainCanonicalizerTest with:
  - [x] Empty-array digest policy check per domain.
  - [x] Sorting stability (varied input orders → same digest).
  - [ ] AEXT parse/serialize changes.
- [ ] Add DomainChangeJournal tests for merge semantics per domain.
- [ ] Add integration tests for remote/embedded parity with golden vectors.

G) Docs
- [ ] Update planning/better_table-2.todo.md to reflect final digest policy and snapshot approach.
- [ ] Add a short section to docs or README on enabling CSV, flags, and rust-backend config for full domain coverage.

Acceptance Checklist (go/no-go)
- [ ] Header == expected (50 columns) and base columns unchanged.
- [ ] Deterministic JSON + digest per domain; golden vectors pass.
- [ ] Empty tx or failed tx produce [] arrays, *_change_count=0, and correct empty digest per chosen policy.
- [ ] Embedded vs remote parity on a small block range (no diff in per-domain digests for same tx).
- [ ] Checkstyle + unit/integration tests pass.

End

