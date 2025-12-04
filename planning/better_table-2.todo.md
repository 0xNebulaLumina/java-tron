Title: Single-CSV Expanded Execution Table — Detailed Plan and TODOs

Context
- Current CSV holds only limited state_changes (effectively account fields) and misses: EVM storage, TRC‑10 balances/issuance, votes, freeze records, global resource totals, account resource usage (AEXT), and EVM logs.
- Goal: Keep one CSV file; keep the original base columns; add per‑domain triplets: X_changes_json, X_change_count, X_digest_sha256.
- No need for backward compatibility. We will replace the CSV header/order as specified here.

Base Columns (kept in this exact order)
- exec_mode, storage_mode, block_num, block_id_hex, is_witness_signed, block_timestamp, tx_index_in_block, tx_id_hex, owner_address_hex, contract_type, is_constant, fee_limit, is_success, result_code, energy_used, return_data_hex, return_data_len, runtime_error

Domain Triplets (to append after base columns, in this order)
- state_changes_json, state_change_count, state_digest_sha256            (legacy aggregate; union of account + evm_storage)
- account_changes_json, account_change_count, account_digest_sha256      (account core: balance, nonce, code_hash, code_len)
- evm_storage_changes_json, evm_storage_change_count, evm_storage_digest_sha256
- trc10_balance_changes_json, trc10_balance_change_count, trc10_balance_digest_sha256
- trc10_issuance_changes_json, trc10_issuance_change_count, trc10_issuance_digest_sha256
- vote_changes_json, vote_change_count, vote_digest_sha256
- freeze_changes_json, freeze_change_count, freeze_digest_sha256
- global_resource_changes_json, global_resource_change_count, global_resource_digest_sha256
- account_resource_usage_changes_json, account_resource_usage_change_count, account_resource_usage_digest_sha256
- log_entries_json, log_entry_count, log_entries_digest_sha256
- ts_ms  (retain trailing timestamp metadata)

Canonicalization & Digest (applies to every *_changes_json)
- JSON: array of objects. Only include fields that actually changed; omit unchanged fields.
- Key normalization: object keys sorted lexicographically at all depths.
- Hex encoding: lowercase hex, no 0x prefix, 21‑byte TRON addresses (41 + 20 bytes).
- Numbers: decimal strings, no floats. Timestamps in milliseconds as decimal strings.
- Array sort order per domain (stable):
  - account: address_hex
  - evm_storage: contract_address_hex, then slot_key_hex
  - trc10_balance: token_id (as string), then owner_address_hex
  - trc10_issuance: token_id, then field
  - vote: voter_address_hex, then witness_address_hex
  - freeze: owner_address_hex, resource_type, then recipient_address_hex (if any)
  - global_resource: field
  - account_resource_usage: address_hex
  - logs: contract_address_hex, then index
- Digest: sha256 over UTF‑8 of the canonical JSON array string; lowercase hex. For empty arrays, use the sha256 of empty string and set *_change_count=0 and *_changes_json="[]".

JSON Shapes (concise)
- account_changes_json: [{ address_hex, op: "create|update|delete", balance_sun:{old,new}, nonce:{old,new}, code_hash_hex:{old,new}, code_len_bytes:{old,new} }]
- evm_storage_changes_json: [{ contract_address_hex, slot_key_hex, op:"set|delete", old_value_hex, new_value_hex }]
- trc10_balance_changes_json: [{ token_id, owner_address_hex, op:"increase|decrease|set|delete", old_balance, new_balance }]
- trc10_issuance_changes_json: [{ token_id, field, op:"create|update|delete", old, new }]
- vote_changes_json: [{ voter_address_hex, witness_address_hex, op:"increase|decrease|set|delete", old_votes, new_votes }]
- freeze_changes_json: [{ owner_address_hex, resource_type:"ENERGY|BANDWIDTH|TRON_POWER", recipient_address_hex?, op:"freeze|unfreeze|update", old_amount_sun, new_amount_sun, old_expire_time_ms, new_expire_time_ms }]
- global_resource_changes_json: [{ field:"total_energy_limit|total_net_limit|total_energy_weight|total_net_weight|next_maintenance_time|…", op:"update", old, new }]
- account_resource_usage_changes_json: [{ address_hex, op:"update", net_usage:{old,new}, energy_usage:{old,new}, storage_usage:{old,new}, net_limit:{old,new}, energy_limit:{old,new} }]
- log_entries_json: [{ contract_address_hex, index, topics_hex:[...], data_hex }]

Behavioral Semantics
- Constant calls or failed tx: emit empty arrays, counts=0, digests=sha256("") for all domains.
- Account creation: op=create; old fields omitted or set to zero/empty by type; deletion: op=delete.
- Multi‑update within a tx: squash per key; old from first observation, new from last state at tx end.

Implementation Plan (High‑Level)
1) Extend CSV model (single file) with domain triplets and new header order.
2) Add DomainCanonicalizer util for per‑domain JSON canonicalization + digest.
3) Embedded mode: add DomainChangeJournal + recorder context, hook actuators/stores to record pre/post deltas.
4) Remote mode: map ExecutionProgramResult payloads to domain JSON; pre/post read from stores where needed for old/new.
5) Integrate into ExecutionCsvRecordBuilder to populate all triplets for each tx, then write one CSV row.
6) Tests: unit + integration; golden vectors for digests.

Detailed TODOs

A. CSV Record and Header
- [x] Update header builder to new column order (keep requested base columns order). File: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecord.java
  - [x] Add fields and getters for each domain triplet.
  - [x] Update getCsvHeader() to include all new columns in the specified order.
  - [x] Update toCsvRow() to emit values in the same order; JSON must be quoted and escaped per RFC‑4180.
- [x] Keep legacy state_change_count/state_changes_json/state_digest_sha256 right after runtime_error to preserve prior analytics.
- [x] Tests: adjust header expectations. File: framework/src/test/java/org/tron/core/execution/reporting/ExecutionCsvRecordTest.java
  - [x] Validate column count and presence of new columns.

B. Domain Canonicalization & Digest
- [x] Create DomainCanonicalizer with methods like:
  - accountToJsonAndDigest(List<AccountDelta>)
  - evmStorageToJsonAndDigest(List<StorageDelta>)
  - trc10BalancesToJsonAndDigest(...)
  - trc10IssuanceToJsonAndDigest(...)
  - votesToJsonAndDigest(...)
  - freezesToJsonAndDigest(...)
  - globalsToJsonAndDigest(...)
  - accountAextToJsonAndDigest(...)
  - logsToJsonAndDigest(...)
- [x] Sorting + tuple rules per domain (see Canonicalization section above).
- [x] Reuse StateChangeCanonicalizer.computeEmptyStateDigest() for empty arrays.
- [x] Unit tests per domain for stable ordering and digest determinism.

C. Embedded Mode Collection (DomainChangeJournal)
- [ ] Introduce DomainChangeJournal (thread‑local, similar to StateChangeJournal) in framework/src/main/java/org/tron/core/execution/reporting/.
- [ ] Introduce DomainChangeRecorderContext in chainbase/src/main/java/org/tron/core/db/ to decouple actuators from framework.
- [ ] Bridge implementation (DomainChangeRecorderBridge) in framework to forward to DomainChangeJournal.
- [ ] Initialize/clear alongside StateChangeJournal in Manager. File: framework/src/main/java/org/tron/core/db/Manager.java
  - [ ] initializeForCurrentTransaction() at tx start; clear at tx end.
- [ ] Hook points (pre/post reads with squash):
  - TRC‑10 balances: AccountCapsule.addAssetAmountV2/reduceAssetAmountV2; also V1 methods if relevant. File: chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java
  - TRC‑10 issuance: AssetIssueActuator.execute() to emit issuance metadata. File: actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java
  - Votes: VoteWitnessActuator.execute() — diff Account.votes pre/post and also persist VotesStore parity. File: actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java
  - Freezes: FreezeBalanceActuator/UnfreezeBalanceActuator — capture absolute frozen amounts (V1/V2) and expirations. Files: actuator/src/main/java/org/tron/core/actuator/FreezeBalanceActuator.java, .../UnfreezeBalanceActuator.java
  - Global resource: DynamicPropertiesStore.saveTotal(Net|Energy)(Weight|Limit) and saveNextMaintenanceTime — record old/new at first touch per tx. File: chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java
  - Account resource usage (AEXT): derive from account_changes old/new fields; no extra hooks required if AEXT tail is already serialized in StateChangeJournal.
  - Logs: from ProgramResult.getLogInfoList() in builder; no journal needed.
- [ ] Gating: reuse System property -Dexec.csv.stateChanges.enabled=true to enable both StateChangeJournal and DomainChangeJournal recordings (single switch).

D. Remote Mode Mapping (ExecutionProgramResult)
- [x] For VM txs (remote):
  - Account + EVM storage: split ExecutionProgramResult.getStateChanges() by empty vs non‑empty key.
  - Logs: map from ProgramResult.getLogInfoList() (already populated by remote conversion).
- [x] TRC‑10: use getTrc10Changes() for issuance/transfers; still compute actual old/new balances via pre/post reads against AccountStore before/after Java apply for parity.
- [x] Votes: getVoteChanges(); reconstruct per‑witness deltas using AccountStore pre/post if needed.
- [x] Freezes/global: getFreezeChanges(), getGlobalResourceChanges(); old/new via DynamicPropertiesStore pre/post.
- [ ] Account resource usage (AEXT): derive from account_changes old/new (AEXT tail decoding logic already in StateChangeJournal.serializeAccountInfo).
- [x] Ensure RuntimeSpiImpl.apply* methods continue to persist these changes before the builder reads post‑state. File: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java

E. CSV Builder Integration
- [x] In ExecutionCsvRecordBuilder.extractExecutionResults():
  - [x] Collect domain deltas from either ExecutionProgramResult (remote) or from journals (embedded).
  - [x] Call DomainCanonicalizer per domain to obtain JSON, count, and digest.
  - [x] Populate the new fields in ExecutionCsvRecord.Builder.
  - [x] Maintain legacy state_changes_* fields (aggregate union of account + evm_storage) for continuity.
- [ ] Tests creating synthetic ExecutionProgramResult with diverse domain payloads and asserting CSV row correctness.

F. Configuration & Flags
- [x] Keep existing writer controls: exec.csv.enabled, exec.csv.sampleRate, exec.csv.rotateMb, exec.csv.queueSize.
- [ ] Use exec.csv.stateChanges.enabled to toggle both journals (state + domain) in embedded mode.
- [ ] Remote backend flags (rust-backend/config.toml): ensure emitting freeze/vote/global/trc10 for full CSV; document recommended settings.

G. Testing Strategy
- [x] Unit tests: canonicalization per domain, digest stability for permuted inputs.
- [x] Unit tests: CSV header and row escaping with quotes/commas/newlines.
- [ ] Integration: embedded path — simulate hooks via DomainChangeRecorderContext and StateChangeRecorderContext; finalize journals; verify JSON/digests.
- [ ] Integration: remote path — craft an ExecutionProgramResult with non‑empty vectors for all domains; verify builder output.
- [ ] Golden vectors: add a small suite that covers each domain and check digests.

H. Acceptance Criteria
- [x] CSV contains all new columns in specified order; base columns preserved exactly.
- [x] For a tx that exercises each domain, *_change_count > 0 and *_changes_json arrays sorted deterministically with valid *_digest_sha256.
- [x] Constant or failed tx produce zero counts, [] arrays, and sha256("") for all digests.
- [ ] Embedded and remote modes yield identical JSON/digests for the same block/tx set.

I. Rollout Plan
- [x] Implement behind existing exec.csv.enabled switch; journaling remains opt‑in via exec.csv.stateChanges.enabled.
- [ ] Validate on a small block range; compare embedded vs remote CSV rows for digest equality per domain.
- [ ] Monitor CSV writer metrics (enqueued/written/dropped) to ensure no regressions.

Notes & Edge Cases
- For TRC‑10 V1/V2, normalize token_id consistently (V1 name → resolved ID string) and store token_key_hex as the raw key to ensure uniqueness across forks.
- For votes, if backend omits old state, compute old from AccountStore before applying remote changes.
- For freezes, V2 lacks expiration; use 0 for expire fields.
- For account core, nonce is 0 in TRON; still include field for parity/clarity.

Deliverables Checklist
- [x] New DomainCanonicalizer with tests.
- [ ] DomainChangeJournal + DomainChangeRecorderContext + Bridge.
- [x] ExecutionCsvRecord updated with new fields/header/row serialization + tests.
- [x] ExecutionCsvRecordBuilder integration logic + tests.
- [ ] Embedded hooks (minimal, focused) for TRC‑10, votes, freeze, globals.
- [x] Remote mapper usage of ExecutionProgramResult, with pre/post store reads for old/new.
- [ ] Golden vectors and documentation updates.

Quick Reference: Primary Files to Touch
- ExecutionCsvRecord: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecord.java
- ExecutionCsvRecordBuilder: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java
- StateChangeCanonicalizer (reused) + new DomainCanonicalizer: framework/src/main/java/org/tron/core/execution/reporting/
- Journals/Bridges/Contexts: framework/src/main/java/org/tron/core/execution/reporting/, chainbase/src/main/java/org/tron/core/db/
- Manager (journal lifecycle): framework/src/main/java/org/tron/core/db/Manager.java
- Actuators and stores hooks (embedded):
  - TRC‑10: chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java, actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java, actuator/src/main/java/org/tron/core/actuator/TransferAssetActuator.java
  - Votes: actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java
  - Freezes: actuator/src/main/java/org/tron/core/actuator/FreezeBalanceActuator.java, actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceActuator.java
  - Globals: chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java
- Remote apply (ensures post‑state visible): framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java

End

