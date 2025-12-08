think harder.

currently the csv only track account_changes (balance, nonce, code_hash, code_len)

but not other changes:  tracking changes of EVM storage, chainbase tables (TRC‑10 Balances, TRC‑10 Issuance, votes, freeze records),  Global Resource, account resources usage, Logs ...

no need to care about backward compability, help me redesign a new csv table, keep the original
`exec_mode,storage_mode,block_num,block_id_hex,is_witness_signed,block_timestamp,tx_index_in_block,tx_id_hex,owner_address_hex,contract_type,is_constant,fee_limit,is_success,result_code,energy_used,return_data_hex,return_data_len,runtime_error`

keep using 1 csv, no need to record different changes (EVM storage, chainbase tables, etc) in different csv.


btw when tracking these other changes, also track _changes_json, _change_count, _digest_sha256 like we did for state_changes.

---

• I’ll propose a single, expanded CSV schema that keeps your original base columns and adds change-tracking for EVM storage, Chainbase tables (TRC‑10 balances/issuance, votes, freeze records), global resources,
account resource usage, and EVM logs. Each domain gets three columns: X_changes_json, X_change_count, X_digest_sha256, with deterministic canonicalization for the digest.

Overview

- Keep the original base columns as-is.
- Add per-domain change triplets: X_changes_json, X_change_count, X_digest_sha256.
- All JSON fields are canonicalized before hashing: sorted keys, stable ordering for arrays, lowercase hex, numbers as decimal strings.
- Empty/no-change domains use [] for JSON, 0 for counts, and empty string for digest.

Column Order

- Base columns (unchanged): exec_mode, storage_mode, block_num, block_id_hex, is_witness_signed, block_timestamp, tx_index_in_block, tx_id_hex, owner_address_hex, contract_type, is_constant, fee_limit,
is_success, result_code, energy_used, return_data_hex, return_data_len, runtime_error
- Accounts: account_changes_json, account_change_count, account_digest_sha256
- EVM Storage: evm_storage_changes_json, evm_storage_change_count, evm_storage_digest_sha256
- TRC‑10 Balances: trc10_balance_changes_json, trc10_balance_change_count, trc10_balance_digest_sha256
- TRC‑10 Issuance: trc10_issuance_changes_json, trc10_issuance_change_count, trc10_issuance_digest_sha256
- Votes: vote_changes_json, vote_change_count, vote_digest_sha256
- Freezes: freeze_changes_json, freeze_change_count, freeze_digest_sha256
- Global Resources: global_resource_changes_json, global_resource_change_count, global_resource_digest_sha256
- Account Resource Usage: account_resource_usage_changes_json, account_resource_usage_change_count, account_resource_usage_digest_sha256
- EVM Logs: log_entries_json, log_entry_count, log_entries_digest_sha256

Canonicalization + Digest

- Key ordering: sort JSON object keys lexicographically at all depths.
- Array ordering:
    - account: by address_hex, then field name
    - evm_storage: by contract_address_hex, then slot_key_hex
    - trc10_balance: by token_id (numeric string), then owner_address_hex
    - trc10_issuance: by token_id, then field
    - votes: by voter_address_hex, then witness_address_hex
    - freezes: by owner_address_hex, then resource_type, then recipient_address_hex (if any)
    - global_resource: by field
    - account_resource_usage: by address_hex
    - logs: by contract_address_hex, then index
- Hex: lowercase, no 0x prefix. Addresses and hashes are hex strings.
- Numbers: decimal strings (e.g., "1000"), never floats.
- Timestamps: epoch milliseconds as decimal strings.
- Digest: sha256 over the UTF‑8 bytes of the canonical JSON array string; lowercase hex output. Empty arrays use empty string for digest.

JSON Shapes

- account_changes_json (only fields that changed are present)
    - [{
    "address_hex": "41abcd...",
    "op": "create|update|delete",
    "balance_sun": {"old": "123", "new": "456"},
    "nonce": {"old": "1", "new": "2"},
    "code_hash_hex": {"old": "…", "new": "…"},
    "code_len_bytes": {"old": "0", "new": "245"}
    }]
- evm_storage_changes_json
    - [{
    "contract_address_hex": "41abcd...",
    "slot_key_hex": "00…32bytes…",
    "op": "set|delete",
    "old_value_hex": "…",    // omit or "" if op=create/set-from-empty
    "new_value_hex": "…"
    }]
- trc10_balance_changes_json
    - [{
    "token_id": "1002001",
    "owner_address_hex": "41abcd...",
    "op": "increase|decrease|set|delete",
    "old_balance": "1000",
    "new_balance": "1500"
    }]
- trc10_issuance_changes_json (fields changed in token metadata/limits/supply)
    - [{
    "token_id": "1002001",
    "field": "total_supply|free_asset_net_limit|free_asset_net_usage|precision|description|url|owner_address|…",
    "op": "create|update|delete",
    "old": "…",
    "new": "…"
    }]
- vote_changes_json (per voter→witness pair)
    - [{
    "voter_address_hex": "41aa…",
    "witness_address_hex": "41bb…",
    "op": "increase|decrease|set|delete",
    "old_votes": "100",
    "new_votes": "0"
    }]
- freeze_changes_json (account freezes and delegations)
    - [{
    "owner_address_hex": "41aa…",
    "resource_type": "ENERGY|BANDWIDTH",
    "recipient_address_hex": "41rcp…"  // omit if self
    "op": "freeze|unfreeze|update",
    "old_amount_sun": "100000000",
    "new_amount_sun": "0",
    "old_expire_time_ms": "1712345678901",
    "new_expire_time_ms": "0"
    }]
- global_resource_changes_json (DynamicPropertiesStore deltas)
    - [{
    "field": "total_energy_limit|total_net_limit|total_energy_weight|total_net_weight|available_energy|available_net|next_maintenance_time|…",
    "op": "update",
    "old": "…",
    "new": "…"
    }]
- account_resource_usage_changes_json (per account)
    - [{
    "address_hex": "41abcd…",
    "op": "update",
    "net_usage": {"old": "123", "new": "200"},
    "energy_usage": {"old": "0", "new": "1000"},
    "storage_usage": {"old": "2048", "new": "4096"},
    "net_limit": {"old": "5000", "new": "5000"},
    "energy_limit": {"old": "300000", "new": "300000"}
    }]
- log_entries_json (EVM logs)
    - [{
    "contract_address_hex": "41abcd…",
    "index": "0",
    "topics_hex": ["a1…", "b2…"],
    "data_hex": "…"
    }]

Semantics

- Include only actual deltas. If a field did not change, omit it from that JSON object.
- On revert/failure or constant calls, all change arrays are [] and counts are 0. Digest fields are "".
- Creation vs update vs delete:
    - New account/contract/token: op=create; old fields omitted or "0"/"" per type.
    - Removal/clear: op=delete; new omitted or "0"/"" per type.
    - Set/change: op=update or domain-specific set/increase/decrease where meaningful.

CSV Formatting

- Header row includes all columns in the order shown.
- JSON columns are RFC‑4180 quoted; double quotes inside are doubled.
- Use [] for empty arrays; “0” for counts; empty string for digests with no entries.

Small Example (abbreviated)

- account_changes_json: [{"address_hex":"41aa…","op":"update","balance_sun":{"old":"100","new":"50"}}]
- evm_storage_changes_json: [{"contract_address_hex":"41cc…","slot_key_hex":"00…1f","op":"set","old_value_hex":"", "new_value_hex":"de…ad"}]
- log_entries_json: [{"contract_address_hex":"41cc…","index":"0","topics_hex":["aa…"],"data_hex":"bb…"}]
- Corresponding *_change_count: 1, 1, 1
- Corresponding *_digest_sha256: sha256 of each canonical JSON array


---

think harder.
now no need to start implementing, just give me a detailed plan on how to fill the tables for both embedded mode and remote mode.

---

• I reviewed how CSV logging and change capture work today and mapped concrete hook points for both execution modes. Below is a detailed, implementation‑ready plan to fill the expanded, single CSV with the new
per‑domain change triplets while keeping your original base columns.

What Exists Today

- CSV writer and record:
    - Record builder assembles base columns and single state_changes set with a digest: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:20 and framework/src/main/java/
    org/tron/core/execution/reporting/ExecutionCsvRecord.java:312
    - File rotation, queue, header writing: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvLogger.java:313
- State change collection:
    - Embedded journaling (account + EVM storage SSTORE): framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournal.java and .../StateChangeJournalRegistry.java:114
    - In-VM hooks calling recorder: actuator/src/main/java/org/tron/core/vm/program/ContractState.java:151 and actuator/src/main/java/org/tron/core/vm/repository/RepositoryImpl.java:810
    - Recorder plumbing: chainbase/src/main/java/org/tron/core/db/StateChangeRecorderContext.java:44 and framework/src/main/java/org/tron/core/execution/reporting/StateChangeRecorderBridge.java:21
    - Canonical digest for state_changes: framework/src/main/java/org/tron/core/execution/reporting/StateChangeCanonicalizer.java:17
- Remote execution returns richer data:
    - backend.proto includes logs, trc10_changes, vote_changes, freeze_changes, global_resource_changes: framework/src/main/proto/backend.proto:678
    - Java wrapper surfaces those in ExecutionProgramResult: framework/src/main/java/org/tron/core/execution/spi/ExecutionProgramResult.java:18
    - Remote Runtime applies them to local DB: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:44

CSV Schema Expansion

- Keep your base columns exactly as-is (present today) and add, for each domain, 3 columns: X_changes_json, X_change_count, X_digest_sha256.
- Domains to add:
    - evm_storage, trc10_balance, trc10_issuance, votes, freezes, global_resource, account_resource_usage, logs.
- Update header and row emitter in one place:
    - Extend: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecord.java:361, 312
    - Update tests expecting header/field count: framework/src/test/java/org/tron/core/execution/reporting/ExecutionCsvRecordTest.java:206

Canonicalization + Digest (all domains)

- Sort keys deterministically; lower-case hex; numbers as decimal strings; arrays sorted per domain key:
    - evm_storage: contract_address_hex, slot_key_hex
    - trc10_balance: token_id, owner_address_hex
    - trc10_issuance: token_id then field
    - votes: voter_address_hex, witness_address_hex
    - freezes: owner_address_hex, resource_type, recipient_address_hex
    - global_resource: field
    - account_resource_usage: address_hex
    - logs: contract_address_hex, index
- Compute digest as SHA‑256 of canonical JSON array (like state_changes), use empty‑state digest for zero entries (same behavior you use today): framework/src/main/java/org/tron/core/execution/reporting/
StateChangeCanonicalizer.java:37
- Add a DomainCanonicalizer helper with per-domain tuple builders and digest helpers in: framework/src/main/java/org/tron/core/execution/reporting/

Remote Mode: Sources and Filling Strategy

- State changes (accounts + EVM storage):
    - Split ExecutionProgramResult.stateChanges into:
        - account_changes: entries with key empty → parse serialized account bytes (balance, nonce=0, code_hash, code_len) to JSON rows; count and digest
        - evm_storage: entries with non-empty key → JSON per slot; count and digest
    - Code: extend builder: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:115
- TRC‑10:
    - trc10_issuance: map ExecutionProgramResult.getTrc10Changes().assetIssued → JSON rows; count and digest
    - trc10_balance: map ...assetTransferred → 2 rows or 1 “set” row per owner/recipient; include old/new by reading pre-state from AccountStore just before applying (cheap lookups) and computing deltas
- Votes:
    - From ExecutionProgramResult.getVoteChanges(): for each owner, compare new list vs pre-state Account.votes (AccountStore) to produce per witness old/new entries; count and digest
- Freezes:
    - From ExecutionProgramResult.getFreezeChanges(): rows contain owner, resource, absolute amount and expiration; count and digest
- Global resource:
    - From ExecutionProgramResult.getGlobalResourceChanges(): rows with fields (total_net_limit, total_energy_limit, total_*_weight, etc.); count and digest
- Account resource usage:
    - Derive from account_changes old/new by decoding AEXT tail you already serialize (net_usage, free_net_usage, energy_usage, latest timestamps, windows, optimized flags); count and digest
- Logs:
    - From ProgramResult.getLogInfoList() (already mapped from remote LogEntry): contract_address, topics, data; count and digest
- Notes:
    - No additional runtime hooks needed; only build/serialize at CSV build time.
    - If remote backend config currently disables some emissions for “CSV parity”, flip on for full tables: rust-backend/config.toml:87, 103.

Embedded Mode: Sources and Filling Strategy

- State changes (accounts + EVM storage):
    - Already captured by StateChangeJournal:
        - Accounts: AccountStore put/remove → StateChangeRecorderContext.recordAccountChange() chainbase/src/main/java/org/tron/core/store/AccountStore.java:67
        - EVM storage SSTORE: ContractState.putStorageValue() → recordStorageChange() actuator/src/main/java/org/tron/core/vm/program/ContractState.java:151
    - Use the same split logic as remote in the CSV builder.
- TRC‑10 balances:
    - Add recording in AccountCapsule asset mutators to DomainChange context:
        - addAssetAmountV2/reduceAssetAmountV2: chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java:733, 780
        - Emit rows with owner/token_id old/new (pre-read old before mutation)
- TRC‑10 issuance:
    - Record in AssetIssueActuator.execute() when creating AssetIssueCapsule and seeding issuer balance:
        - actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java:46
- Votes:
    - Record in VoteWitnessActuator.execute() after building new votes:
        - Compare accountCapsule.getVotesList() pre vs post to build per witness entries; actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java:34
- Freezes:
    - Record in FreezeBalanceActuator and UnfreezeBalanceActuator:
        - Emit owner, resource, absolute frozen amount, expiration using V1/V2 semantics: actuator/src/main/java/org/tron/core/actuator/FreezeBalanceActuator.java:37 and actuator/src/main/java/org/tron/core/
        actuator/UnfreezeBalanceActuator.java:39
- Global resource:
    - Record only the global totals/limits “save” calls:
        - DynamicPropertiesStore.saveTotalNetLimit, saveTotalEnergyLimit2, saveTotalNetWeight, saveTotalEnergyWeight, and next_maintenance_time:
        - chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java:1299, 1313, 1260, 1271, 2222
        - Emit old/new values; keep scope tight to avoid noise from unrelated properties
- Account resource usage:
    - Same derivation as remote: decode AEXT in account_changes old/new
- Logs:
    - From embedded ProgramResult.getLogInfoList() directly
- Notes:
    - Gate embedded domain journaling with the existing flag: -Dexec.csv.stateChanges.enabled=true (re-use this to initialize both StateChangeJournal and the new DomainJournal).
    - Initialize per-tx before exec: Manager processTransaction already does this for state journal: framework/src/main/java/org/tron/core/db/Manager.java:1540

Plumbing: Domain Recorder and Journal

- Add a peer to StateChangeRecorderContext for domain changes:
    - chainbase: new DomainChangeRecorderContext with methods:
        - recordTrc10BalanceChange(owner, tokenId, old, new)
        - recordTrc10Issuance(tokenId, owner, fields…)
        - recordVoteChange(owner, witness, old, new)
        - recordFreezeChange(owner, resource, amount, expiration, v2Flag)
        - recordGlobalResourceChange(field, old, new)
    - framework: DomainChangeRecorderBridge forwards to DomainChangeJournalRegistry (thread-local), similar to StateChangeRecorderBridge: framework/src/main/java/org/tron/core/execution/reporting/
    - Initialize/clear per tx alongside state journal in Manager: framework/src/main/java/org/tron/core/db/Manager.java:1540 and 1619
- Use this DomainJournal in the CSV builder when programResult is not ExecutionProgramResult (embedded mode) and as a fallback/complement if some domains are not emitted by remote backend.

Builder Changes (single place)

- Enhance ExecutionCsvRecordBuilder.extractExecutionResults():
    - Read from ExecutionProgramResult when available (remote/shadow); else read from StateChangeJournal + DomainJournal
    - Build each domain’s JSON array, count, digest via DomainCanonicalizer
    - Set fields on ExecutionCsvRecord and keep the original base columns logic: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:20
- Update ExecutionCsvRecord to hold new columns and row assembly: framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecord.java:312, 361

Config and Flags

- CSV on/off, rotation, sampling remain unchanged: -Dexec.csv.enabled, etc.
- Embedded capture: reuse -Dexec.csv.stateChanges.enabled=true to also enable DomainJournal.
- Remote backend: enable emissions for freeze/global/vote/trc10 in rust-backend/config.toml for full CSV coverage.

Validation Plan

- Unit tests:
    - Canonicalization per domain (ordering, hex casing, digest stability)
    - CSV header/row emitter updated field count: framework/src/test/java/org/tron/core/execution/reporting/ExecutionCsvRecordTest.java:206
    - AEXT parser roundtrip for account_resource_usage
- Integration tests:
    - Embedded: simulate storage/account changes using StateChangeRecorderContext and DomainChangeRecorderContext; verify JSON/digests in a constructed record
    - Remote: fabricate an ExecutionProgramResult with representative domain payloads; verify builder output
- Golden vectors: add a small set including at least one tx for each domain to lock digests

Edge Cases

- Constant calls or failed tx: emit empty arrays, 0 counts, and empty-state digests (same as current state_changes)
- Multi-updates within a tx: merge semantics in journals for final old/new (already done for storage; mirror for TRC‑10 and votes)
- TRC‑10 V1/V2: normalize token_id consistently; for V1 map name→id via stores before logging