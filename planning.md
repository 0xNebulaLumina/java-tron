Goal

- Unify TRON bandwidth/TRX fee semantics in Rust and emit authoritative state deltas.
- Make CSV/digests comparable by normalizing account encoding on the embedded (Java) path to match remote format.

Phase 1: Rust Bandwidth Semantics (Authoritative Fees)

- 
Scope: Implement full non-VM TRX transfer bandwidth/fee handling in Rust and return all resulting state changes in ExecutionResult.
- 
Ownership: rust-backend/ (execution + core service). Java only applies returned deltas.
- 
Data Sources:
    - Dynamic properties: read from RocksDB properties DB via existing StorageEngine (StorageModule::engine()).
    - Per-account resource usage: read/write from DBs mirroring Java (e.g., account, DelegatedResource, DelegatedResourceAccountIndex, possibly resource-like keys if present).
    - Genesis resource defaults: from Java’s dynamic properties or hardcoded genesis if not present (avoid hardcoding; read from DB).
- 
Parameters to read (names depend on Java key scheme; wire up via mapping table):
    - Free bandwidth per account and window size.
    - Tx size accounting rules (base payload, signatures, and TRC10/TRC20 special cases; start with serialized tx byte length from request).
    - Bandwidth from frozen TRX (net), delegations (obtained via DelegatedResource* DBs).
    - Fee mode: burn|blackhole from config.toml (already present: [execution.fees]).
    - Blackhole address if blackhole mode (already in config).
    - Support black-hole optimization flag (already present).
- 
Algorithm (Non-VM Transfer):
    - Compute bandwidth_used from tx bytes (replicate Java’s calculation; include signatures/refs).
    - Determine free bandwidth available to sender (per 24h rolling window):
    - Load sender’s current `freeNetUsage` and `latest_op_time`.
    - If window expired, reset usage; else apply remaining quota.
- Determine staked/delegated net bandwidth:
    - Sum own staked net + incoming delegations (minus outgoing).
    - Compute available vs used for this window; apply to tx after free bandwidth.
- Compute TRX fee if still insufficient:
    - Fee = remaining bytes * `bandwidth_price` (from dynamic properties).
    - If sender balance < value + fee → error (insufficient).
- Apply state changes:
    - Sender: balance -= (value + fee); update resource usage structs; update `latest_op_time`.
    - Recipient: balance += value; create if not exists.
    - Blackhole or burn:
      - If mode=blackhole: credit account with `fee` (create if not exists).
      - If mode=burn: no account delta (supply reduced notionally).
- 
Sort state changes deterministically (already done) and return in ExecutionResult.
- 
Do NOT rely on Java post-processing anymore for non-VM fees.
- 
Implementation Touchpoints:
    - crates/core/src/service.rs:
    - Remove/disable prior “post-processing” placeholder that added synthetic fee state (keep VM path behavior off).
    - Route non-VM tx through a new resource manager.
- crates/execution/src/:
    - Add `resource` module (bandwidth accounting):
      - `ResourceConfig` (limits, prices).
      - `ResourceStateStore` backed by StorageEngine (load/save usage records).
      - `ResourceCalculator` (free, staked, delegated).
      - `ResourceApplier` (mutate accounts + resource records).
- crates/execution/src/storage_adapter.rs:
    - Ensure account read/write parity with Java (already aligned with 21-byte address keys).
    - Add helpers to read/write resource keys (prefix-encoded like Java’s DB).
- proto:
    - Keep current `StateChange` union. Optionally extend `ExecutionResult` with `fee_applied` and `bandwidth_used` (the latter already present) for diagnostics. Not required for parity.

- Edge Cases:
    - Zero-balance recipients: ensure account creation deltas are emitted.
    - Very small txs fully covered by free bandwidth: fee=0, apply usage only.
    - Blackhole address invalid/missing while mode=blackhole: fallback to burn; warn.
    - Replay/Idempotence: ensure resource usage keyed by time window; avoid double counting if execution retried—Rust service should be stateless per-call; Java commits the deltas once.
    - Replay/Idempotence: ensure resource usage keyed by time window; avoid double counting if execution retried—Rust service should be stateless per-call; Java commits the deltas once.
- 
Config and Flags:
    - Drive everything via config.toml and dynamic properties from DB.
    - Add execution.fees.mode support as implemented; consider use_dynamic_properties=true toggle to resolve prices/limits from DB vs static config.
- 
Validation:
    - Unit tests (Rust): calculator edge cases, account creation, blackhole credit.
    - Integration test: seed small storage (mock engine) with resource props and accounts; run a tx across scenarios: free-only, staked-only, fee-required.
    - On live compare: The very first mismatched tx should now have identical sender old/new between runs.

Phase 2: Embedded CSV Normalization (Digest Parity)

- 
Scope: Keep embedded runtime storage unchanged, but when producing CSV/digest, serialize account changes in the same layout used by RemoteExecutionSPI: [balance(32)][nonce(8)][codeHash(32)][codeLen(4)][code].
- 
Ownership: framework/ (Java). No storage format changes, only the CSV export path.
- 
Touchpoints:
    - framework/src/main/java/org/tron/core/execution/report/ExecutionCsvLogger (or wherever CSV rows are built): normalize state_changes_json for account-level changes.
    - RuntimeSpiImpl (or the layer assembling state change entries) to distinguish account vs storage changes and pass normalized value bytes for accounts.
- 
Serialization Rules:
    - Balance: 32-byte big-endian.
    - Nonce: 8-byte big-endian (0 for TRON nonces).
    - Code hash: 32 bytes. For non-contract EOAs: keccak256(empty) = c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470.
    - Code length: 4-byte big-endian (0 for EOAs).
    - Code: empty for EOAs; existing code left as-is for contracts.
    - Key for account change: still empty key "" to indicate account-level change (matches remote).
- 
Backward Compatibility:
    - Only affects CSV export and digest fields; underlying RocksDB content remains in legacy Java format.
    - Add a config flag (e.g., csv.normalizedAccountEncoding=true) default ON for parity runs. Allow OFF to debug legacy format diffs.
- 
Validation:
    - Re-run CSV export on embedded and remote runs for early blocks; verify state_changes_json and state_digest_sha256 now match for non-contract cases.
    - Spot-check a contract account change (if present) to ensure code fields are included consistently.

Phase 3: Incremental Rollout and Observability

- Rollout Order:
    1. Ship Rust resource/bandwidth implementation behind feature flag execution.fees.use_dynamic_properties=true.
    2. Enable Java CSV normalization flag in dev, compare outputs.
    3. Turn on Rust-side fee handling in staging, confirm no mismatches across sampled blocks.
    4. Roll out to prod-like environment.
    4. Roll out to prod-like environment.
- 
Metrics and Logs:
    - Rust:
    - Emit structured logs: bandwidth_used, free_applied, staked_applied, fee_applied, fee_mode, blackhole_credit.
    - Counters: `resource.free_applied.bytes`, `resource.staked_applied.bytes`, `resource.fee_applied.sun`.
- Java:
    - CSV logger notes when normalized encoding is active.
    - Runtime logs note “remote-authoritative fee mode enabled” once full shift happens.

- Safeguards:
    - Consistency checks in Java before applying remote deltas:
    - If local old ≠ remote old by > epsilon (e.g., > 0), log warning with address and both olds (helps catch any residual drift).
- Kill-switch in config to revert Rust fee logic (execution.fees.use_dynamic_properties=false).

Phase 4: Parity Hardening

- Expand parity surface:
    - Confirm order of state changes: account changes sorted lexicographically by 20-byte EVM address; storage changes ordered after account changes for same address, as done in Rust.
    - Normalize recipient-first vs sender-first ordering; enforce same comparator in embedded runtime before emitting CSV.
- Handle VM path later:
    - Keep VM fee handling off in Rust for now (experimental_vm_blackhole_credit=false).
    - When enabling, ensure Java does not double-apply VM fees; make Rust authoritative for both paths.

Testing Plan

- Unit tests (Rust):
    - Free-only: one tx consuming ≤ free bandwidth → fee=0; account changes only.
    - Mixed: free+staked consumption; fee=0.
    - Fee-required: not enough free+staked → fee>0, blackhole mode credit present.
    - Edge: window rollover resets usage.
- Integration tests:
    - Mock storage with dynamic properties and resource usage keys; run batch of transfers; assert expected deltas.
- Cross-run CSV comparison:
    - Execute blocks 300–1800 on embedded and remote-remote; assert first tx’s sender balances match exactly; ensure state_digest_sha256 equivalence for non-contracts.

Deliverables

- Rust:
    - crates/execution/src/resource/ module (calculator + applier + store).
    - Extended handling in core::service non-VM execution path.
    - Config: document flags in config.toml.
- Java:
    - CSV normalization path for account changes.
    - Optional comparator for state change ordering to match Rust.

Success Criteria

- The first mismatched tx (block 342) shows identical sender old/new and identical state_changes_json and state_digest_sha256 across embedded-embedded vs remote-remote.
- Subsequent sampled txs maintain parity; any remaining diffs are attributable to contract code paths (to be addressed in later phase).
