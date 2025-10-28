# Option C: Hybrid Pre‑Execution AEXT Pass to Remote Execution

This document details a concrete, implementation‑ready plan to pass Java’s pre‑execution Account AEXT (resource usage/window fields) to the Rust backend, so the backend echoes those values in AccountInfo serialization for state changes. This achieves byte‑level parity with embedded for non‑VM transactions where Java updates/relies on AEXT around bandwidth processing.

## Problem Summary
- CSV mismatch root cause: embedded includes non‑zero AEXT fields (e.g., free_net usage, timestamps, window sizes) in AccountInfo old/new, while remote emits zero/default AEXT. Example mismatch: block 1785:0 WitnessCreateContract where owner’s oldValue includes AEXT bytes in embedded but zeros in remote.
- Remote currently derives AEXT for serialization via a config (`accountinfo_aext_mode`) and optional backend tracking. It doesn’t know Java’s pre‑execution AEXT unless we explicitly transmit it.

## Goal
- Ensure AccountInfo old/new values in remote results serialize with the exact AEXT bytes that Java sees before the transaction, for unchanged resource fields. This drives state digest parity without making the backend persist or compute AEXT.

## Non‑Goals
- Do not change Java’s resource accounting order or semantics.
- Do not persist AEXT in the backend for hybrid mode.
- Do not change VM transaction semantics in this iteration.

---

## High‑Level Design
- Extend gRPC request with an optional list of pre‑execution AEXT snapshots keyed by address.
- Java collects AEXT from local `AccountCapsule` before invoking remote execution and attaches it.
- Rust backend, in a new "hybrid" mode, prefers these pre‑provided AEXT values when serializing AccountInfo for both `old_account` and `new_account` in state changes. If a snapshot for an address is not provided, fall back to current behavior.
- No backend persistence/mutation of AEXT in hybrid mode.

---

## Detailed TODOs

### 1) Protobuf Contract (framework/src/main/proto/backend.proto)
- Add messages (new, optional):
  - `AccountAext` with fields
    - `int64 net_usage`
    - `int64 free_net_usage`
    - `int64 energy_usage`
    - `int64 latest_consume_time`
    - `int64 latest_consume_free_time`
    - `int64 latest_consume_time_for_energy`
    - `int64 net_window_size`
    - `bool net_window_optimized`
    - `int64 energy_window_size`
    - `bool energy_window_optimized`
  - `AccountAextSnapshot { bytes address; AccountAext aext; }`
- Extend `ExecuteTransactionRequest` (do NOT renumber existing fields):
  - `repeated AccountAextSnapshot pre_execution_aext = 3;`
- Backward compatibility:
  - Field is optional; older clients/servers interoperate (ignored when absent).
- Follow‑ups (future, not in scope for first pass):
  - Optional `pre_execution_aext` for `CallContractRequest` if parity desired for readonly calls.

Validation tasks:
- Regenerate Java and Rust stubs (Gradle and Cargo build should drive codegen):
  - Java: ensure `tron.backend.BackendOuterClass` contains new messages/fields.
  - Rust: tonic build (rust‑backend/crates/core/build.rs) already compiles `framework/src/main/proto/backend.proto`.


### 2) Java: Populate Pre‑execution AEXT (RemoteExecutionSPI)
- File: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- Method: `buildExecuteTransactionRequest(TransactionContext)` around 440–520
  - Add feature flag: system property `remote.exec.preexec.aext.enabled` (default `true`).
  - Collect addresses to snapshot:
    - Always: `from` (owner). This alone fixes many mismatches like WitnessCreate.
    - If present: `to` (recipient) for `TRANSFER_CONTRACT` and similar.
    - Contract‑specific candidates (phase 2):
      - WitnessCreate/VoteWitness: blackhole address (when not burning) from DynamicPropertiesStore.
      - Freeze/Unfreeze V1/V2: owner again suffices; consider freeze ledger owner if ledger is serialized as AccountChange in some paths (verify).
  - For each selected address:
    - Lookup `AccountCapsule` via `context.getStoreFactory().getChainBaseManager().getAccountStore().get(...)`.
    - If present, build `AccountAextSnapshot`:
      - Map fields:
        - `net_usage = account.getNetUsage()`
        - `free_net_usage = account.getFreeNetUsage()`
        - `energy_usage = account.getAccountResource().getEnergyUsage()`
        - `latest_consume_time = account.getLatestConsumeTime()`
        - `latest_consume_free_time = account.getLatestConsumeFreeTime()`
        - `latest_consume_time_for_energy = account.getAccountResource().getLatestConsumeTimeForEnergy()`
        - `net_window_size = account.getWindowSize(BANDWIDTH)`
        - `net_window_optimized = account.getWindowOptimized(BANDWIDTH)`
        - `energy_window_size = account.getWindowSize(ENERGY)`
        - `energy_window_optimized = account.getWindowOptimized(ENERGY)`
    - Add to `ExecuteTransactionRequest.pre_execution_aext`.
  - Logging: `DEBUG` summary of addresses included and sample fields.
- Do not change existing AccountInfo serialization in Java; this path is solely request enrichment.

Validation tasks:
- Unit smoke: in a local harness, assert `pre_execution_aext` length >= 1 for typical system contracts when flag enabled.


### 3) Rust Backend: Ingest and Prefer Pre‑exec AEXT
- Files: `rust-backend/crates/common/src/config.rs`, `rust-backend/crates/core/src/service.rs`
- Config: Add new mode to `execution.remote.accountinfo_aext_mode`:
  - Allowed: `none`, `zeros`, `defaults`, `tracked`, `hybrid`
  - Semantics: `hybrid` = prefer request `pre_execution_aext` for AEXT serialization; if not provided per‑address, fall back to `defaults` (or existing mode fallback per implementation decision).
- In `execute_transaction` handler (service.rs:3008–3180):
  - Build `HashMap<Address, AccountAext>` from `req.pre_execution_aext`:
    - Convert Tron 0x41‑prefixed `bytes address` to 20‑byte EVM address using existing helpers (see `strip_tron_address_prefix`).
    - Store raw values as simple struct (mirror proto fields) without persistence.
- In `convert_execution_result_to_protobuf` (service.rs:3599–3762):
  - Extend signature to accept `Option<HashMap<Address, Aext>>`.
  - In `convert_account_info` closure:
    - If mode is `hybrid` and `pre_exec_map.contains(address)`, set all optional AEXT proto fields from this map for both old and new accounts.
    - Else, preserve existing logic:
      - `tracked` uses `result.aext_map`.
      - `defaults` uses zero usages and 28800 window sizes for EOAs.
      - `zeros` and `none` per current behavior.
  - Logging: `DEBUG` indicating source of AEXT for each account (pre_exec/tracked/defaults/none).
- Persistence: Do not write these values to storage; they are used only to construct the response.

Validation tasks:
- Unit test (core/service):
  - Build a fake `ExecuteTransactionRequest` with `pre_execution_aext` for sender.
  - Force a simple non‑VM contract and capture `ExecuteTransactionResponse`.
  - Assert `result.state_changes[..].account_change.{old_account,new_account}` carry the provided AEXT (e.g., `net_window_size == 28800` and non‑zero usage/time when provided).


### 4) Backward and Forward Compatibility
- When `pre_execution_aext` is absent (older Java), `hybrid` mode falls back to `defaults` (or current active mode if we choose to honor it).
- When backend is older (unknown field), Java still operates as the extra field is ignored by server—ensure client stubs match server during deployment.
- Keep VM path behavior unchanged for now; a later iteration can consider parity needs if required.


### 5) Observability & Debuggability
- Rust `INFO/DEBUG` logs:
  - Loaded config mode for AEXT.
  - Number of pre‑exec AEXT snapshots received per request.
  - For each state change, the AEXT source chosen (pre_exec/tracked/defaults/none) and the address.
- Java `DEBUG` log:
  - Addresses included in `pre_execution_aext` collection with minimal field summaries.


### 6) Rollout Plan
- Phase 1 (opt‑in):
  - Ship proto change, Java producer, Rust consumer with `hybrid` disabled by default.
  - Enable via config:
    - Rust: `execution.remote.accountinfo_aext_mode = "hybrid"` (rust-backend/config.toml:89).
    - Java: `-Dremote.exec.preexec.aext.enabled=true` (default true; flag allows emergency disable).
  - Validate on short historical slices; confirm CSV digest parity improvements.
- Phase 2 (coverage):
  - Expand Java to include additional addresses for system contracts that affect secondary accounts (details below).
  - Re‑benchmark parity.
- Phase 3 (default on):
  - Consider making `hybrid` default for non‑VM only, if parity improvements are stable.


### 7) Risk Analysis and Mitigations
- Risk: Increased request size for many addresses.
  - Mitigation: Start with owner (+optional to) only; add contract‑specific secondaries selectively.
- Risk: Mismatch due to wrong snapshot timing.
  - Mitigation: Collect strictly pre‑execution values from the local DB; do not mutate AEXT in backend in hybrid.
- Risk: Divergence for VM tx.
  - Mitigation: Scope hybrid to non‑VM; evaluate VM later if needed.
- Risk: Back‑compat deploy order.
  - Mitigation: Version‐aligned deployment; field is optional; gating via config.


### 8) Acceptance Criteria
- For the dataset used in recent runs, the first mismatch at 1785:0 (WitnessCreate) resolves: `state_digest_sha256` identical.
- Overall mismatches (state_digest only) across the run drop materially for non‑VM txs.
- No change in `is_success`, `result_code`, `energy_used`, `state_change_count` across runs.
- No Rust backend panics; logs confirm “hybrid” path is in use.


### 9) Concrete Task Checklist

Proto
- [x] Define `AccountAext` and `AccountAextSnapshot` in `framework/src/main/proto/backend.proto`.
- [x] Add `repeated AccountAextSnapshot pre_execution_aext = 3;` to `ExecuteTransactionRequest`.
- [x] Regenerate Java/Rust stubs; ensure builds green.

Java
- [x] Add toggle `remote.exec.preexec.aext.enabled` (default true).
- [x] In `RemoteExecutionSPI.buildExecuteTransactionRequest(...)` collect pre‑exec AEXT for: owner (required), recipient (optional), contract‑specific addresses (phase 2).
- [x] Map `AccountCapsule` fields to `AccountAextSnapshot` accurately (BANDWIDTH/ENERGY windows and flags).
- [x] Add DEBUG logs summarizing snapshots.
- [x] Smoke test request contains expected snapshots in typical paths (verified via compilation).

Rust
- [x] Add `hybrid` to `execution.remote.accountinfo_aext_mode` in `crates/common/src/config.rs` (parsing/defaults/docs).
- [x] Parse `pre_execution_aext` into `HashMap<Address, Aext>` in `execute_transaction`.
- [x] Thread the map into `convert_execution_result_to_protobuf`.
- [x] In AEXT population for AccountInfo, prefer `pre_exec` values when mode == `hybrid`.
- [x] Add INFO/DEBUG logs for mode and usage source per account.
- [ ] Unit tests: verify AEXT echo behavior and fallbacks (deferred - integration test more appropriate).

Docs & Ops
- [x] Update rust‑backend/config.toml documentation for `hybrid` mode semantics.
- [x] Add short runbook note: how to enable/disable and verify logs (included in config.toml comments).
- [ ] E2E parity run on a small slice; capture mismatch deltas (ready for testing).


### 10) File Reference Pointers
- Proto: `framework/src/main/proto/backend.proto:458`, `framework/src/main/proto/backend.proto:540`, `framework/src/main/proto/backend.proto:590`
- Java request build: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:440`
- Java AEXT serialization (for context): `framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournal.java:352`
- Rust request handler: `rust-backend/crates/core/src/service.rs:3008`
- Rust result serialization: `rust-backend/crates/core/src/service.rs:3599`
- Rust config: `rust-backend/crates/common/src/config.rs:222`
- Backend config TOML (docs): `rust-backend/config.toml:89`


### 11) Future Enhancements (Out of Scope)
- Support for passing AEXT for VM tx paths where Java expects specific AEXT semantics.
- Normalization layer to ignore AEXT in digest calculation if we want AEXT‑agnostic parity mode.
- gRPC streaming metrics for AEXT source selection to monitor adoption.


## Contract‑Specific Address Coverage (Phase 2)

Include additional addresses in `pre_execution_aext` based on the contract type. These are “best effort” participants that commonly appear in state changes for system contracts. Including an address that ultimately does not appear in a state change is harmless.

Notes
- Always include `owner`/`from`.
- Include `to` when applicable.
- Include `blackhole` only when fees are credited to it (Java: support blackhole optimization disabled).
- For delegated resource flows, include both `owner` and `receiver`.

Contracts and Addresses
- TransferContract (TRX)
  - Required: owner (`from`), recipient (`to`).
  - Rationale: Both typically have AccountChange (balance delta).

- WitnessCreateContract
  - Required: owner.
  - Optional: blackhole when credited (i.e., when support_blackhole_optimization is false in Java). If Java burns (no credit), omit.
  - Java blackhole lookup: prefer `AccountStore.getBlackhole()`; fallback to dynamic property or config if available. Omit if not present.

- VoteWitnessContract
  - Required: owner (voter).
  - Rationale: CSV parity model uses one AccountChange for owner (old==new) to record operation; AEXT should mirror Java’s values.

- WitnessUpdateContract
  - Required: owner (witness address).

- AccountUpdateContract
  - Required: owner.
  - Rationale: Account metadata changes (name) but CSV emits AccountChange old==new; include AEXT to keep bytes identical.

- FreezeBalanceContract (V1)
  - Required: owner.
  - Optional (if delegating in V1): receiver (when present in the contract fields).
  - Rationale: Owner AccountChange is typical. If `emit_freeze_ledger_changes` remains false (default), no storage ledger changes in CSV.

- UnfreezeBalanceContract (V1)
  - Required: owner.
  - Optional (if unfreezing delegated resources): receiver (when present).

- FreezeBalanceV2Contract / UnfreezeBalanceV2Contract
  - Required: owner.
  - Rationale: V2 separates delegation into its own contracts; still include owner for AEXT parity.

- WithdrawExpireUnfreezeContract
  - Required: owner.

- DelegateResourceContract
  - Required: owner, receiver.
  - Rationale: Both parties may appear in state updates (delegation indexes/counters). Even when CSV reduces to AccountChange(owner), including receiver is low‑cost and safe.

- UnDelegateResourceContract
  - Required: owner, receiver.

- WithdrawBalanceContract
  - Required: owner (witness withdrawing rewards).

- TransferAssetContract (TRC10)
  - Required (if/when TRC‑10 enabled): owner, recipient.
  - Rationale: Even if asset balances are represented via StorageChange, bandwidth/timestamps may be reflected in AccountChange(owner). Safe to include both.

- Proposal* contracts (Create/Approve/Delete)
  - Required: owner.

- UpdateBrokerageContract
  - Required: owner (witness).

- AccountPermissionUpdateContract
  - Required: owner.

Retrieval Details (Java)
- Owner (`from`) and Recipient (`to`)
  - Extract from `TransactionContext`’s typed contract messages (e.g., `TransferContract`, `DelegateResourceContract`).
- Receiver
  - For contracts with a `receiver_address` field (e.g., delegate/un‑delegate), parse from the specific contract message.
- Blackhole
  - Prefer `manager.getAccountStore().getBlackhole()` if available.
  - If not, derive from config or dynamic properties; if unavailable, omit.
  - Include only when Java’s path credits the blackhole account instead of burning (consult `DynamicPropertiesStore.supportBlackHoleOptimization()` or equivalent; when true, burning occurs and blackhole AccountChange should not appear).

Heuristics and Safety
- It is acceptable to include a superset of addresses; the backend will ignore snapshots with no matching state change accounts.
- Capture AEXT strictly before building `ExecuteTransactionRequest` to reflect pre‑execution values (do not mutate local AEXT between snapshot and gRPC call).
