Title: Execution CSV v2 — Scalable Multi-Table Design and Fill Plan

Status: draft
Owner: core-exec, storage, tooling
Scope: embedded + remote execution parity; EVM storage + chainbase ledgers (TRC‑10, votes, freeze, globals, logs, AEXT)

Goals
- Separate domains (VM storage vs. chainbase) into dedicated CSVs for clarity and scalability.
- Deterministic, per‑category digests and a single overall digest to validate parity quickly.
- Absolute old/new values (idempotent, replayable) with stable canonicalization and ordering.
- Embedded and remote produce byte‑identical results (for the same chain state).

Non‑Goals
- Backward compatibility with the existing single CSV (we will keep v1 side‑by‑side during rollout, but design v2 independently).

Bundle Layout (CSV set for each run)
- executions.csv — one row per transaction (summary, category counts, category digests, overall digest).
- evm_storage_changes.csv — EVM SSTORE deltas.
- account_changes.csv — TRX account core fields (balance/nonce/code hash/length).
- trc10_asset_balances.csv — TRC‑10 owner/token absolute balances (old/new).
- trc10_asset_issuance.csv — TRC‑10 issuance metadata.
- vote_changes.csv — Account.votes deltas (per candidate).
- freeze_changes.csv — Freeze/Unfreeze (v1/v2) deltas (per resource type).
- global_resource_totals.csv — net/energy totals before/after (single row if changed).
- logs.csv — EVM logs (ordered within tx).
- account_aext_snapshots.csv — before/after AEXT (resource usage) for accounts touched by bandwidth/energy.

Common Identity Columns (present in all category CSVs)
- schema_version: int (start at 2)
- run_id: string
- exec_mode: embedded|remote|shadow
- storage_mode: embedded|remote
- block_num: u64
- block_id_hex: hex (lowercase)
- tx_index_in_block: i32
- tx_id_hex: hex (lowercase)
- category_row_idx: u32 (stable, 0..N‑1 within the category for this transaction)

Canonicalization & Digest Rules
- Hex lowercase for all hex fields; TRON addresses are 41‑prefixed hex.
- Numbers are base‑10 ASCII, no leading zeros; zero is "0".
- Booleans emitted as 0/1. Enums lowercase strings.
- Each category defines its own canonical tuple and sort key.
- Category digest: SHA‑256 over newline‑joined canonical tuples (UTF‑8) in sorted order.
- Overall digest: SHA‑256 over the following lines in this fixed order:
  - evm:<digest_evm_storage>
  - acct:<digest_account>
  - trc10b:<digest_trc10_balance>
  - trc10i:<digest_trc10_issuance>
  - vote:<digest_vote>
  - freeze:<digest_freeze>
  - global:<digest_global_resource>
  - logs:<digest_logs>
  - aext:<digest_aext>

CSV Schemas (headers)

1) executions.csv
- schema_version,run_id,exec_mode,storage_mode,block_num,block_id_hex,tx_index_in_block,tx_id_hex,
  is_witness_signed,block_timestamp,owner_address_hex,contract_type,is_constant,fee_limit,
  is_success,result_code,energy_used,return_data_hex,runtime_error,
  evm_storage_count,account_change_count,trc10_balance_count,trc10_issuance_count,
  vote_change_count,freeze_change_count,global_resource_count,log_count,aext_snapshot_count,
  digest_evm_storage,digest_account,digest_trc10_balance,digest_trc10_issuance,
  digest_vote,digest_freeze,digest_global_resource,digest_logs,digest_aext,digest_overall,
  ts_ms

2) evm_storage_changes.csv
- schema_version,run_id,exec_mode,storage_mode,block_num,block_id_hex,tx_index_in_block,tx_id_hex,category_row_idx,
  contract_address_hex,slot_key_hex,old_value_hex,new_value_hex
- Canonical tuple: contract|key|old|new
- Sort key: (contract_address_hex, slot_key_hex)

3) account_changes.csv
- schema_version,run_id,exec_mode,storage_mode,block_num,block_id_hex,tx_index_in_block,tx_id_hex,category_row_idx,
  address_hex,old_balance,new_balance,old_nonce,new_nonce,old_code_hash_hex,new_code_hash_hex,code_len_old,code_len_new
- Canonical tuple: address|bal_old|bal_new|nonce_old|nonce_new|codehash_old|codehash_new|clen_old|clen_new
- Sort key: address_hex

4) trc10_asset_balances.csv
- schema_version,run_id,exec_mode,storage_mode,block_num,block_id_hex,tx_index_in_block,tx_id_hex,category_row_idx,
  owner_address_hex,token_model,token_key_hex,token_id_str,old_amount,new_amount
- token_model: v1|v2; token_key_hex: V1=name bytes; V2=token_id ASCII bytes
- Canonical tuple: owner|token_model|token_key|old|new
- Sort key: (owner_address_hex, token_model, token_key_hex)

5) trc10_asset_issuance.csv
- schema_version,run_id,exec_mode,storage_mode,block_num,block_id_hex,tx_index_in_block,tx_id_hex,category_row_idx,
  owner_address_hex,token_model,token_key_hex,token_id_str,name_hex,abbr_hex,total_supply,precision,trx_num,num,
  start_time,end_time,description_hex,url_hex,free_asset_net_limit,public_free_asset_net_limit,
  public_free_asset_net_usage,public_latest_free_net_time
- Canonical tuple: owner|token_model|token_key|total_supply|precision|trx_num|num|start|end|desc|url|limits|...
- Sort key: (owner_address_hex, token_model, token_key_hex)

6) vote_changes.csv
- schema_version,run_id,exec_mode,storage_mode,block_num,block_id_hex,tx_index_in_block,tx_id_hex,category_row_idx,
  owner_address_hex,candidate_address_hex,old_vote_count,new_vote_count
- Canonical tuple: owner|candidate|old|new
- Sort key: (owner_address_hex, candidate_address_hex)

7) freeze_changes.csv
- schema_version,run_id,exec_mode,storage_mode,block_num,block_id_hex,tx_index_in_block,tx_id_hex,category_row_idx,
  owner_address_hex,resource,model,old_amount,new_amount,expiration_ms_old,expiration_ms_new
- resource: bandwidth|energy|tron_power; model: v1|v2; expiration applicable to v1 else 0
- Canonical tuple: owner|resource|model|old_amount|new_amount|exp_old|exp_new
- Sort key: (owner_address_hex, resource, model)

8) global_resource_totals.csv
- schema_version,run_id,exec_mode,storage_mode,block_num,block_id_hex,tx_index_in_block,tx_id_hex,category_row_idx,
  total_net_weight_old,total_net_weight_new,total_net_limit_old,total_net_limit_new,
  total_energy_weight_old,total_energy_weight_new,total_energy_limit_old,total_energy_limit_new
- Canonical tuple: net_w_old|net_w_new|net_l_old|net_l_new|eng_w_old|eng_w_new|eng_l_old|eng_l_new
- Sort: single row (category_row_idx=0 when present)

9) logs.csv
- schema_version,run_id,exec_mode,storage_mode,block_num,block_id_hex,tx_index_in_block,tx_id_hex,category_row_idx,
  address_hex,topic0_hex,topic1_hex,topic2_hex,topic3_hex,data_hex,log_index
- Canonical tuple: address|idx|topic0|topic1|topic2|topic3|data
- Sort key: log_index (0..N‑1)

10) account_aext_snapshots.csv
- schema_version,run_id,exec_mode,storage_mode,block_num,block_id_hex,tx_index_in_block,tx_id_hex,category_row_idx,
  address_hex,
  net_usage_old,net_usage_new,free_net_usage_old,free_net_usage_new,energy_usage_old,energy_usage_new,
  latest_consume_time_old,latest_consume_time_new,latest_consume_free_time_old,latest_consume_free_time_new,
  latest_consume_time_for_energy_old,latest_consume_time_for_energy_new,
  net_window_size_old,net_window_size_new,net_window_optimized_old,net_window_optimized_new,
  energy_window_size_old,energy_window_size_new,energy_window_optimized_old,energy_window_optimized_new
- Canonical tuple: address|all_fields_old|all_fields_new
- Sort key: address_hex

Lifecycle (per‑transaction)
1) Initialize TxBundleBuilder at tx start (collectors for each category).
2) Pre‑snapshots:
   - AEXT "before" for accounts to be charged (at minimum tx owner) right before Bandwidth/Energy accounting.
   - Lazy account core "before": on first mutation (balance/nonce/code) per address.
3) Execute tx.
4) Apply side effects:
   - Embedded: actuators mutate chainbase; collectors hook pre/post reads.
   - Remote: Java applies remote changes (state_changes, TRC‑10, votes, freeze/globals) to local stores; collectors hook around these.
5) Post‑snapshots:
   - AEXT "after" right after resource charging and any freeze/unfreeze that changes windows/limits.
6) Finalize:
   - Each collector dedupes: old = first observed, new = final store value at tx end.
   - Build canonical tuples, compute per‑category digests, write per‑category CSV rows with category_row_idx.
   - Write executions.csv row with counts + digests + overall digest.

Dedup/Squash Rules (per category)
- EVM storage: key=(contract, slot). Coalesce multi‑writes; emit if old!=new.
- Account: key=address. Compare tuple (bal, nonce, codehash, codelen); emit if changed.
- TRC‑10 balances: key=(owner, model, token_key). Aggregate; emit if old!=new.
- Issuance: exactly one row on issuance (no dedup within tx).
- Votes: key=(owner, candidate). Emit if changed.
- Freeze: key=(owner, resource, model). Emit if old!=new (expiration compare for v1).
- Global totals: emit one row with pre/post (first/last in tx) if any field changed.
- Logs: emit all with log_index order.
- AEXT: emit if any field changed (or optionally always emit for touched accounts — configure via flag).

Data Sources — Embedded
- EVM storage: repository SSTORE hook (capture pre‑image at first write, keep last write value).
- Account core: AccountStore read before first change; after final change read from store.
- TRC‑10 balances: TransferAssetActuator (+ other asset mutators): pre/post reads against AccountStore (V1/V2 map according to ALLOW_SAME_TOKEN_NAME and token_id presence).
- Issuance: AssetIssueActuator: token_id resolution, metadata; emit issuance row; optionally also emit issuer balance creation as a trc10_asset_balances row (old=0,new=total_supply).
- Votes: VoteWitnessActuator: pre/post Account.votes; per candidate rows.
- Freeze: Freeze/Unfreeze (v1/v2): pre/post resource fields on account; expiration for v1; emit rows.
- Global totals: DynamicPropertiesStore: pre before first freeze change, post after last freeze change in tx; emit one row if changed.
- Logs: ProgramResult.getLogInfoList ordered emission.
- AEXT: Bandwidth/Energy processors: snapshot pre before charging; snapshot post after charging (and after freeze changes that affect windows).

Data Sources — Remote
- EVM storage: prefer backend SSTORE deltas; if missing, instrument backend EVM state to produce them.
- Account core: VM txs — backend AccountInfo old/new when consistent; non‑VM — compute via Java pre/post reads surrounding apply* methods.
- TRC‑10 balances: from Trc10Change::AssetTransferred plus Java pre/post reads (source of truth is AccountStore state before/after Java applies change).
- Issuance: Trc10Change::AssetIssued; reconcile token_id and metadata; emit issuance; optionally emit issuer balance creation in balances table.
- Votes: VoteChange entries; seed old from Account.votes (or pre‑read) and compute new via post‑read.
- Freeze: FreezeLedgerChange entries; compute absolute old/new via pre/post reads.
- Global totals: backend may supply; still compute via DynamicPropertiesStore pre/post for determinism.
- Logs: ExecutionResult.logs in order.
- AEXT: pre from Java (hybrid); post from backend tracked or Java post‑apply snapshot; prefer Java pre/post reads for determinism.

Feature Flags (rollout)
- csv.v2.enabled=true|false (default false)
- csv.v2.emit.aext.all_touched=true|false (default false — emit only on change)
- csv.v2.rotate_mb=256 (per‑file rotate)
- csv.v2.compress=gzip|none (default none)

TODO — Schema & Canonicalization
- [ ] Freeze schema_version=2 and headers for all CSVs.
- [ ] Implement canonical tuple builders for each category.
- [ ] Implement per‑category sorters (address/key ordering), and SHA‑256 digest helpers.
- [ ] Define type validators (hex casing, numeric formats, enum domains).

TODO — Writers & Orchestration (Java)
- [ ] Create TxBundleBuilder and category collectors (evm, account, trc10_balances, trc10_issuance, votes, freeze, globals, logs, aext).
- [ ] Implement lazy pre‑snapshot and final post‑snapshot per collector.
- [ ] Implement per‑category CSV writers with rotation and atomic file create/append.
- [ ] Implement executions.csv writer to aggregate counts/digests/overall digest.
- [ ] Add metrics: enqueued/written/dropped/writeErrors per category; expose periodic stats.

TODO — Embedded Hooks (Java)
- [ ] Repository SSTORE hook → evm_storage collector (pre/post capture and squash).
- [ ] Account core changes (balance/nonce/code) → account collector (pre on first mutation, post at end).
- [ ] TransferAssetActuator → trc10_balances collector (owner/recipient pre/post; V1/V2 keying).
- [ ] AssetIssueActuator → trc10_issuance (metadata) and optional balances (issuer old=0,new=total_supply).
- [ ] VoteWitnessActuator → vote_changes (per candidate pre/post).
- [ ] Freeze/Unfreeze(v1,v2) actuators → freeze_changes (pre/post per resource); expiration capture for v1.
- [ ] DynamicPropertiesStore integration → global_resource_totals (pre at first change, post at last change in tx).
- [ ] Bandwidth/Energy processors → account_aext_snapshots (pre/post snapshots for charged accounts).

TODO — Remote Integration (Java)
- [ ] RuntimeSpiImpl.applyStateChanges/applyTrc10Changes/applyVoteChanges/applyFreezeLedgerChanges: surround with pre/post reads to fill collectors (not just relying on backend payloads).
- [ ] Logs: collect from ExecutionProgramResult for VM txs.
- [ ] EVM storage: consume backend SSTORE or add a compatibility reconstruction path when absent.
- [ ] AEXT (hybrid/tracked): use Java pre/post snapshots as authoritative; only use backend AEXT for guidance.

TODO — Compare & Validation Tooling
- [ ] New comparator: verify per‑tx per‑category digests and counts between embedded and remote bundles.
- [ ] Produce summary (match %, mismatches by category), and drill‑downs for failing txs.
- [ ] Optionally reconstruct category tuples from stores to spot source of mismatch.

TODO — Tests
- [ ] Unit: canonicalization for each category (order‑insensitivity, stable digest).
- [ ] Unit: key encodings (TRC‑10 v1/v2), code hash/len capture, SSTORE squash.
- [ ] Golden vectors: sample txs covering each category (TRC‑10 transfer/issue; votes; freeze v1/v2; logs; EVM writes).
- [ ] E2E: run a fixed block window both modes; assert per‑category and overall digest equality.

TODO — Performance & Robustness
- [ ] Batch writes and bounded queues; backpressure strategy per category.
- [ ] File rotation and safe close on shutdown; resume semantics.
- [ ] Error handling: per‑category writeErrors logged but non‑blocking; executions.csv records counts for finalized categories only when partial.
- [ ] Metrics: periodic dump and optional Prometheus export (future).

TODO — Rollout Plan
- [ ] Ship behind csv.v2.enabled=false (default).
- [ ] CI job to generate bundles on small block window; gate on parity.
- [ ] Incrementally enable on parity pipelines; leave v1 CSV in place for reference.
- [ ] Document ops: disk sizing, rotation, cleanup.

Open Questions
- Should account_aext_snapshots emit for all touched accounts even when unchanged (full observability) vs. only when changed (lower volume)? Default proposed: only when changed.
- For global_resource_totals with multiple changes in a single tx, we plan to emit final‑only; do we also want intermediate points for debugging (a separate debug channel/log)?
- For EVM storage on remote: if backend cannot emit SSTORE soon, do we temporarily skip evm_storage_changes.csv for remote and mark digest as empty while still producing the other categories?

Notes
- All digests are computed over canonical tuples only; CSV row ordering or writer chunking does not affect digest.
- Absolute old/new semantics ensure idempotent and replay‑friendly outputs.

