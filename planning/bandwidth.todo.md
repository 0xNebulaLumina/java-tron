## Plan: Parity for Bandwidth Semantics and CSV Encoding

Context: We observed mismatches between (embedded execution + embedded storage) and (remote execution + remote storage) runs starting from the first TransferContract. The deltas point to Java applying bandwidth/TRX adjustments locally that the Rust backend did not see. We will: (1) move TRON bandwidth/TRX fee semantics into the Rust backend so it emits authoritative state changes; (2) normalize the embedded CSV account-change encoding to match the remote format for digest parity.

### Goals
- Rust becomes the source of truth for non-VM TRX transfer resource/fee accounting and emits complete state deltas.
- Embedded CSV export serializes account changes in the same layout as RemoteExecutionSPI to align digests.
- Preserve existing RocksDB storage layouts in Java (only CSV/digest serialization changes on the embedded path).

### Phase 1 — Rust: Implement TRON Bandwidth/TRX Fee Semantics

Owner: rust-backend

Scope: Non-VM value transfers (no data, to EOA). VM paths remain unchanged for now.

Key Design Points
- Read dynamic properties and resource usage from RocksDB via StorageEngine (no hardcoded constants).
- Apply consumption order: free bandwidth → staked/delegated net → TRX fee (burn or blackhole), matching Java semantics.
- Return all resulting state changes (sender account, recipient account, resource usage updates, optional blackhole credit) in `ExecutionResult` for Java to persist.
- Deterministic state change ordering (address asc; account changes before storage for same addr).

Detailed TODOs
[X] Config: Introduce flag `execution.fees.use_dynamic_properties=true` to enable Rust-side fee semantics (default off until rollout).
[X] Resource store: Implement readers/writers for resource-related keys (freeNet usage, latest op time, staked net, delegations) mirroring Java DB namespaces (e.g., `properties`, `DelegatedResource`, `DelegatedResourceAccountIndex`).
[X] Calculator: Compute `bandwidth_used` from tx payload; determine available free bandwidth (windowed), staked/delegated bandwidth, remainder requiring TRX fee using `bandwidth_price` from dynamic properties.
[X] Applier: Produce state deltas:
    - Sender: balance -= (value + fee), update `latest_op_time`, update usage records.
    - Recipient: balance += value, create if needed.
    - Fee mode = burn: no account delta; mode = blackhole: credit blackhole account (create if needed).
[X] Core service: Replace current non-VM path post-processing with the new resource manager when the flag is enabled; ensure the gRPC `ExecutionResult` carries all state changes already sorted.
[X] Logging/metrics: Emit structured debug for `bandwidth_used`, `free_applied`, `staked_applied`, `fee_applied`, `fee_mode`, `blackhole_credit`. Add counters: `resource.free.bytes`, `resource.staked.bytes`, `resource.fee.sun`.
[ ] Edge cases: Window reset on expiry; insufficient funds (value + fee); invalid blackhole address fallback to burn; idempotency guarantee (execution is stateless; Java persists once).
[ ] Tests (unit): free-only, staked-only, fee-required, window rollover, blackhole credit creation.
[ ] Tests (integration, mock engine): seed minimal props/accounts; validate emitted state deltas for representative scenarios.

Out of Scope (Phase 1)
- VM fee semantics (remain disabled: `experimental_vm_blackhole_credit=false`).
- Changing Java storage layouts.

### Phase 2 — Java: Normalize Account-Change Encoding for CSV/Digests

Owner: framework (Java)

Scope: CSV/digest export only. Storage encoding stays as-is.

Target Encoding (to match RemoteExecutionSPI)
- Account value bytes in state change: `[balance(32)][nonce(8)][codeHash(32)][codeLen(4)][code]`.
- For EOAs: `nonce=0`, `codeHash=keccak256("") = c5d246...`, `codeLen=0`, `code=empty`.
- Account change key remains empty (`key_len=0`) to indicate account-level mutation.

Detailed TODOs
[ ] Add a `csv.normalizedAccountEncoding=true` config toggle (default ON for parity workflows).
[ ] In the CSV/logger path (ExecutionCsvLogger and friends), detect account-level state changes and serialize values with the normalized layout above instead of legacy `[balance][latestOpTime][flag]` layout when the flag is set.
[ ] Ensure deterministic ordering of state changes in CSV mirrors Rust’s comparator (address asc; account changes before storage for same address).
[ ] Keep legacy path available behind the toggle for A/B debugging.
[ ] Tests: Golden CSV rows for a few account-change cases (EOA transfer, contract account with code), verifying `state_changes_json` formatting and `state_digest_sha256` stability.

### Phase 3 — Rollout, Validation, and Observability

Rollout Plan
[ ] Dev: Enable `csv.normalizedAccountEncoding=true`; keep Rust fee flag OFF. Verify CSV parity shape.
[ ] Staging: Enable Rust fee flag `execution.fees.use_dynamic_properties=true`. Validate deltas and CSV parity on sampled ranges (e.g., blocks 300–1800).
[ ] Production-like: Gradual enablement; monitor metrics and mismatch alerts.

Cross-Run Validation
[ ] Compare embedded-embedded vs remote-remote for early blocks; confirm the first previously mismatched tx now matches balances and `state_digest_sha256`.
[ ] Spot-check contract account changes (if any) to ensure code hash/length are consistent across both.

Observability & Safeguards
[ ] Java safeguard: before applying remote deltas, if local-old differs from remote-old beyond 0 (expected to be equal post-migration), log a warning with address and both values for investigation.
[ ] Kill switch: both flags (`use_dynamic_properties`, `csv.normalizedAccountEncoding`) must be revertible at runtime/config restart.

### Phase 4 — Parity Hardening (Post-MVP)
[ ] Extend parity to VM paths when ready; move VM fee semantics into Rust with the same authoritative pattern.
[ ] Digest canonicalization (optional): define a semantic hash over fields (balance/nonce/codeHash/codeLen/code) to future-proof against representation drift.
[ ] Broaden test coverage to include delegated resource edge cases and blackhole optimization interactions.

### Acceptance Criteria
- The first mismatched transaction (block 342) shows identical sender/recipient balances across runs and identical `state_changes_json`/`state_digest_sha256`.
- Subsequent sampled transactions maintain parity; any residual mismatches are investigated via the logging/metrics added above.

### Notes & Assumptions
- Java remains the committer of state to local RocksDB; Rust returns authoritative deltas. No double application of fees post migration.
- Dynamic properties and resource usage locations must match Java’s DB schema; we will mirror the key strategy to avoid schema drift.

### Task Tracking (High-Level)
[ ] Phase 1: Rust resource manager (config, store, calculator, applier, service integration, tests)
[ ] Phase 2: Java CSV normalization (toggle, serializer, ordering, tests)
[ ] Phase 3: Rollout + validation (env toggles, comparisons, metrics)
[ ] Phase 4: Hardening (VM parity, canonical digests, extended tests)
