I want to support tracking changes of EVM storage, chainbase tables (TRC‑10 maps, votes, freeze records).

help me redesign the csv table

---


• Here’s a scalable, parity‑friendly redesign that cleanly covers EVM storage and chainbase tables (TRC‑10, votes, freeze). It drops backward compatibility in favor of a normalized, multi‑CSV “execution
bundle.”

Design Goals

- Clear separation of domains (VM storage vs. chainbase ledgers).
- Deterministic, composable digests per category + overall.
- Efficient joins by transaction identity; easy to shard/rotate.
- Absolute old/new values (idempotent, replayable).

Bundle Layout

- executions.csv — one row per tx (summary, counts, digests).
- evm_storage_changes.csv — EVM SSTORE deltas.
- account_changes.csv — TRX account core fields (balance/nonce/code).
- trc10_asset_balances.csv — TRC‑10 owner/token balance deltas.
- trc10_asset_issuance.csv — TRC‑10 issuance metadata.
- vote_changes.csv — Account.votes deltas (per candidate).
- freeze_changes.csv — V1/V2 freezes/unfreezes (per resource).
- global_resource_totals.csv — net/energy totals deltas after freeze ops.
- logs.csv — EVM logs (topics, data).
- account_aext_snapshots.csv — before/after AEXT for touched accounts.

Optionally later: internal_txs.csv, return_values.csv, errors.csv.

Common Identity Columns

- schema_version: int (start at 2)
- run_id: string (execution batch)
- exec_mode: enum (embedded|remote|shadow)
- storage_mode: enum (embedded|remote)
- block_num: u64
- block_id_hex: hex
- tx_index_in_block: i32
- tx_id_hex: hex
- category_row_idx: u32 (stable order within category for this tx)

All category CSVs include these columns to enable joins.

executions.csv (tx summary)

- Tx info: is_witness_signed, block_timestamp, owner_address_hex, contract_type, is_constant, fee_limit
- Result: is_success, result_code, energy_used, return_data_hex, runtime_error
- Counts: evm_storage_count, account_change_count, trc10_balance_count, trc10_issuance_count, vote_change_count, freeze_change_count, global_resource_count, log_count, aext_snapshot_count
- Digests (hex): digest_evm_storage, digest_account, digest_trc10_balance, digest_trc10_issuance, digest_vote, digest_freeze, digest_global_resource, digest_logs, digest_aext, digest_overall
- ts_ms

Digest rule: category digests are SHA‑256 of canonical tuples (see below). digest_overall = SHA‑256 of the concatenation of category digests in a fixed order with labels, e.g. “evm:<d>;acct:<d>;trc10b:<d>;…”.

evm_storage_changes.csv

- Keys: contract_address_hex, slot_key_hex (32‑byte)
- Values: old_value_hex, new_value_hex (32‑byte)
- Canonical tuple: contract|key|old|new
- Sorting: by (contract_address, slot_key), ascending hex.

account_changes.csv

- address_hex
- old_balance, new_balance (SUN, decimal)
- old_nonce, new_nonce (decimal; TRON uses 0 for EOAs)
- old_code_hash_hex, new_code_hash_hex (32‑byte)
- code_len_old, code_len_new (u32)
- Canonical tuple: address|bal_old|bal_new|nonce_old|nonce_new|codehash_old|codehash_new|clen_old|clen_new
- Sorting: by address.

Note: code bytes aren’t dumped; only hash + lengths for scalability.

trc10_asset_balances.csv

- owner_address_hex
- Keying:
    - token_model: enum (v1|v2)
    - token_key_hex: raw bytes of key (V1=name bytes; V2=token_id ASCII)
    - token_id_str: normalized string for V2 (optional convenience)
- Values: old_amount, new_amount (i64)
- Canonical tuple: owner|token_model|token_key|old|new
- Sorting: by (owner, token_model, token_key)

trc10_asset_issuance.csv

- owner_address_hex
- token_model: v1|v2
- token_key_hex, token_id_str
- Metadata: name_hex, abbr_hex, total_supply, precision, trx_num, num, start_time, end_time, description_hex, url_hex, free_asset_net_limit, public_free_asset_net_limit, public_free_asset_net_usage,
public_latest_free_net_time
- Canonical tuple: owner|token_model|token_key|total_supply|precision|…|id_str
- Sorting: by (owner, token_model, token_key)

vote_changes.csv

- owner_address_hex
- candidate_address_hex
- old_vote_count, new_vote_count (i64)
- Canonical tuple: owner|candidate|old|new
- Sorting: by (owner, candidate)

freeze_changes.csv

- owner_address_hex
- resource: enum (bandwidth|energy|tron_power)
- model: enum (v1|v2)
- old_amount, new_amount (i64 absolute)
- expiration_ms_old, expiration_ms_new (only meaningful for v1; else 0)
- Canonical tuple: owner|resource|model|old_amount|new_amount|exp_old|exp_new
- Sorting: by (owner, resource, model)

global_resource_totals.csv

- total_net_weight_old, total_net_weight_new
- total_net_limit_old, total_net_limit_new
- total_energy_weight_old, total_energy_weight_new
- total_energy_limit_old, total_energy_limit_new
- Canonical tuple: net_w_old|net_w_new|net_l_old|net_l_new|eng_w_old|eng_w_new|eng_l_old|eng_l_new
- Sorting: exactly one row per tx; if multiple updates occur in a tx, emit final‑only.

logs.csv

- address_hex, topic0_hex..topic3_hex, data_hex
- log_index (in‑tx ordering)
- Canonical tuple: address|idx|topic0|…|topic3|data
- Sorting: by log_index.

account_aext_snapshots.csv

- address_hex
- Before/after AEXT (resource usage):
    - net_usage_old/new, free_net_usage_old/new, energy_usage_old/new
    - latest_consume_time_old/new, latest_consume_free_time_old/new, latest_consume_time_for_energy_old/new
    - net_window_size_old/new, net_window_optimized_old/new
    - energy_window_size_old/new, energy_window_optimized_old/new
- Canonical tuple: address|all_fields_old|all_fields_new
- Sorting: by address.

Determinism & Canonicalization

- Hex lowercase everywhere; addresses and keys are raw hex with 41‑prefix for TRON addresses.
- Category sorting rules as specified; no dependence on discovery order.
- Old/new must be absolute post‑image/pre‑image (never deltas). If “old” absent in store, use 0 (numbers) or empty hash (code), per table rules.
- Each category’s digest: SHA‑256 over newline‑joined canonical tuples.
- Overall digest: SHA‑256 over “cat_name:digest” lines in fixed order: evm_storage, account, trc10_balance, trc10_issuance, vote, freeze, global_resource, logs, aext.

Scalability

- Partition each CSV by block range or time window (e.g., per 1M blocks, or per 256MB rotate).
- Streaming writer with backpressure; per‑category files rotate independently.
- Optional gzip per file; digests computed on uncompressed content.
- Use category_row_idx to preserve stable row identity within a tx for incremental verification.

Filling the Tables (high‑level)

- Embedded:
    - Instrument Actuators and repository layers to emit to the appropriate category emitters (journal per category).
    - EVM storage: hook SSTORE; Account: balance/code updates; TRC‑10: asset map updates; Votes: Account.votes mutations; Freeze: Frozen/V2 and dynamic totals.
    - AEXT: snapshot before/after for accounts touched by bandwidth/energy processors.
- Remote:
    - Use backend ExecutionResult to populate:
        - evm_storage_changes (if provided from VM executor; else rebuild from journal if available).
        - account_changes from AccountInfo old/new.
        - trc10_asset_balances and trc10_asset_issuance from Trc10Change.
        - vote_changes and freeze_changes from dedicated result vectors.
        - global_resource_totals from backend totals.
        - logs from VM logs.
        - aext from pre‑exec snapshots + post‑exec tracked/hybrid values.

Why this beats a single “storage_changes”

- EVM storage has different invariants than chainbase tables; separating keeps each schema minimal, strongly typed, and indexable.
- Per‑category digests isolate mismatches and scale better than one giant blob.
- Readers can consume only what they need (e.g., asset analytics vs. VM diffing).

---

think harder.

no need to start implementing, just give me a detailed plan on how to fill the Tables

---

Below is a concrete, execution-ready plan for how to fill each table consistently in both embedded and remote modes. It focuses on when and where to capture old/new, how to dedupe within a tx, and how to
compute counts/digests deterministically.

Lifecycle

- Per-tx journal: Create TxBundleBuilder at tx start; it owns category collectors:
    - evm_storage, account, trc10_balance, trc10_issuance, vote, freeze, global_resource, logs, aext.
- Pre-snapshots:
    - AEXT “before”: capture only for accounts the resource processors will charge (typically tx owner) right before execution/bandwidth charging.
    - Account core “before” (balance/nonce/code): lazy-snapshot on first mutation.
- Execute tx.
- Apply side-effects:
    - Embedded: actuators update chainbase; collectors hook pre/post reads.
    - Remote: Java applies remote changes (state changes, freeze, trc10, votes) to local stores; collectors hook around these applies.
- Post-snapshots:
    - AEXT “after”: right after resource charging/ledger updates finish.
- Finalize:
    - Each collector dedupes intra-tx to final (old_at_tx_start, new_at_tx_end).
    - Build canonical tuples, compute category digests, write category CSV rows with category_row_idx.
    - Write executions.csv row with counts + category digests + overall digest.

Canonicalization Rules

- Address/keys: lowercase hex; TRON addresses 41-prefixed hex.
- Numbers: base-10 ASCII, no leading zeros (use “0” for zero).
- Bools: 0/1.
- Enums: lowercase string (resource: bandwidth|energy|tron_power; model: v1|v2).
- Sorting inside category: by the category’s key (defined below).
- Category digest: SHA-256 over newline-joined canonical tuples.
- Overall digest: SHA-256 over lines “evm:<hex>”, “acct:<hex>”, “trc10b:<hex>”, “trc10i:<hex>”, “vote:<hex>”, “freeze:<hex>”, “global:<hex>”, “logs:<hex>”, “aext:<hex>” in that fixed order.

Executions (executions.csv)

- Fill after all collectors finalize:
    - Result fields from ProgramResult/ExecutionProgramResult.
    - Counts from collectors.
    - Digests from collectors, plus overall digest.
    - tx identity and metadata from block context.
- category_row_idx is per-category, 0..N-1 in sort order used for that category.

EVM Storage (evm_storage_changes.csv)

- Embedded:
    - Hook repository SSTORE: on first write to (contract, slot) capture old=pre-image; always update “last_new” to most recent write. On finalize, emit one row per (contract, slot): old, new.
- Remote:
    - Use ExecutionResult SSTORE list if provided; if multiple writes to same slot, squash: old=first.old, new=last.new.
    - If remote engine doesn’t output SSTORE, instrument it to attach StorageChange events or mirror embedded journal logic.
- Keys: contract_address_hex, slot_key_hex.
- Values: old_value_hex/new_value_hex (32-byte hex).
- Sort key: (contract_address_hex, slot_key_hex).

Accounts (account_changes.csv)

- What fields: balance, nonce, code_hash, code_len (not code bytes).
- Embedded:
    - Before any balance/nonce/code write: snapshot old (if not already).
    - After final write in tx: snapshot new from AccountStore.
    - If none of those fields changed (strict equality), emit nothing.
- Remote:
    - Prefer ExecutionResult AccountInfo old/new when provided (VM txs).
    - For non-VM deltas applied by Java (fees, etc.), do the same pre/post reads against AccountStore.
- Keys: address_hex.
- Sort key: address_hex.

TRC‑10 Balances (trc10_asset_balances.csv)

- Keying:
    - token_model: v1|v2 based on ALLOW_SAME_TOKEN_NAME and presence of token_id.
    - token_key_hex: raw key bytes (V1: name bytes; V2: token_id ASCII bytes).
    - token_id_str: V2 convenience (numeric string), else empty.
- Embedded (TransferAssetActuator, ParticipateAssetIssue if applicable):
    - Before apply: read owner/recipient balances by correct map (v1 or v2).
    - After apply: read new balances.
    - Aggregate within tx: if multiple changes to same (owner, model, token_key), old = value at first touch; new = last value at tx end.
- Remote:
    - From Trc10Change::AssetTransferred, derive keys and amount. Still read actual old/new from AccountStore before/after Java applies the TRC-10 change (ensures consistency with embedded).
    - For fees that impact TRX only: no TRC-10 balance rows unless asset map changed.
- Emit one row per changed (address, model, token_key).
- Sort key: (owner_address_hex, token_model, token_key_hex).

TRC‑10 Issuance (trc10_asset_issuance.csv)

- When AssetIssue executed:
    - Determine v1/v2 behavior from ALLOW_SAME_TOKEN_NAME.
    - Resolve token_id (Java may compute when absent).
    - Capture metadata (name, abbr, precision, total_supply, urls, limits, times).
    - Emit exactly one issuance row (issuer, model, token_key, token_id_str).
    - Optionally, also emit an initial trc10_asset_balances row for issuer (old=0, new=total_supply) in the balances table (recommended).
- Remote:
    - Use Trc10Change::AssetIssued to fill metadata; reconcile token_id as Java stores it for v2.
- Sort key: (owner_address_hex, token_model, token_key_hex).

Votes (vote_changes.csv)

- Embedded (VoteWitnessActuator):
    - Before: read Account.votes (list).
    - After: read new list.
    - Per candidate: old_count (default 0 if missing), new_count; emit row only if changed.
- Remote:
    - Use VoteChange entries (owner + list of vote entries) and seed “old” from Account.votes if backend is configured to seed; otherwise compute old via pre-read from AccountStore before applying; new via
    post-read.
- Sort key: (owner_address_hex, candidate_address_hex).

Freezes (freeze_changes.csv)

- Model v1 (Frozen fields with expiration) and v2 (FrozenV2 list):
    - Embedded:
        - Before: snapshot per resource (bandwidth/energy/tron_power) absolute frozen amount (and expiration for v1).
        - After: snapshot same.
        - Emit one row per (owner, resource, model) if changed.
    - Remote:
        - Use FreezeLedgerChange entries to know what resource/model changed; still compute absolute old/new via pre/post reads from AccountStore for parity.
- Columns: owner_address_hex, resource, model, old_amount, new_amount, expiration_ms_old/new (v1; else 0).
- Sort key: (owner_address_hex, resource, model).

Global Resource Totals (global_resource_totals.csv)

- One row per tx if any totals changed:
    - Embedded: after freeze/unfreeze ops, read DynamicPropertiesStore totals (net/energy weights and limits) before first change and after final change; emit final old/new.
    - Remote: ExecutionResult global_resource_changes may carry new totals; still prefer computing old/new via pre/post DynamicPropertiesStore reads in Java for consistency.
- Sort: single row (no per-key), or use category_row_idx = 0.

Logs (logs.csv)

- Embedded: from ProgramResult.getLogInfoList(), enumerate with per-tx log_index in order emitted.
- Remote: from ExecutionResult.logs in order.
- Columns: address_hex, topic0..topic3_hex (empty if not present), data_hex, log_index.
- Sort key: log_index (0..N-1).

Account AEXT Snapshots (account_aext_snapshots.csv)

- Which accounts: those charged bandwidth/energy during this tx (at least the owner), or any account whose AEXT changed.
- Embedded: snapshot “before” right before BandwidthProcessor/energy accounting, “after” right after; take fields from AccountCapsule (net/energy usage, times, windows, optimized flags).
- Remote: use pre-exec AEXT passed to backend (hybrid mode) as “before”; “after” from tracked mode or from Java’s post-apply snapshot if backend isn’t tracking; prefer Java pre/post reads for determinism.
- Emit one row per address with any change (or emit for all touched accounts even if equal, if you want full observability).
- Sort key: address_hex.

Dedup/Squash Rules (per collector)

- Single-key squashing: keep old from first observation, new from last state at tx end:
    - EVM storage: key=(contract, slot).
    - Account: key=address; compare tuple of core fields for change.
    - TRC-10: key=(address, model, token_key).
    - Votes: key=(owner, candidate).
    - Freeze: key=(owner, resource, model).
- Only emit if old != new (except logs, issuance, globals):
    - Issuance: always emit once when issued.
    - Logs: always emit in order.
    - Global totals: emit if any field changed.
    - AEXT: your choice; recommended emit only on change to keep volume down.

Embedded vs Remote Sources

- Prefer Java pre/post reads against stores to produce old/new for chainbase-driven tables (trc10_balance, votes, freeze, globals, aext) after the Java applies remote deltas. This guarantees parity and
isolates backend representation differences.
- For EVM storage:
    - Embedded: repository SSTORE hooks.
    - Remote: require backend to report SSTORE deltas; if not, add instrumentation in backend’s EVM state layer.
- For Account core (balance/nonce/code):
    - VM txs: use backend AccountInfo old/new if available and consistent; else compute via pre/post Java reads.
    - Non-VM TRX fees: compute via pre/post reads.

Error Handling

- If a collector fails mid-tx:
    - Emit executions.csv with is_success/result_code and partial counts/digests for categories that finalized.
    - Log category writeErrors; do not block block processing.
- If remote returns incomplete info (e.g., missing Trc10Change), fallback to Java pre/post reads based on actual actuator paths taken (Java’s apply* methods).

Performance

- Use lazy pre-snapshot on first mutation key to avoid heavy pre-scans.
- Accumulate in memory; flush on tx boundary.
- Rotate files per size/time; maintain per-category row_idx counters per file for continuity (or reset per file and rely on tx_id).