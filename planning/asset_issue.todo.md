# TRC-10 AssetIssueContract: Remote Execution Plan

Owner: Core Runtime/Remote SPI + Rust Backend
Scope: Java mapping in RemoteExecutionSPI; Rust non-VM handler in execute_non_vm_contract; fee/bandwidth/AEXT accounting; optional proto extension for TRC‑10 semantic diffs.

Status: Design/Planning (do not implement yet)

---

## Goals

- Map TRC-10 AssetIssueContract into the Java RemoteExecutionSPI request with correct classification and payload.
- Implement non-VM dispatch + handler in Rust for AssetIssueContract:
  - Charge asset‑issue fee (burn or credit blackhole based on settings/dynamic properties).
  - Emit account changes (owner fee deduction; blackhole credit when applicable) and bandwidth/AEXT updates.
  - Keep energy_used = 0; compute bandwidth_used; deterministic ordering of state changes.
- Ensure Java applies returned account changes to AccountStore and flushes to DB.
- Prepare path for full TRC‑10 persistence (AssetIssue[V1/V2] stores, TOKEN_ID_NUM bump) via a Phase 2 proto extension or storage support.

Non-goals (Phase 1):
- Do not create TRC‑10 ledger entries (AssetIssueStore/AssetIssueV2Store) in Rust.
- Do not alter existing Java TRC‑10 actuators unless toggled to fallback.

Flags: Default disabled; opt-in via JVM system property and Rust config.

---

## Checklist (High-level)

- [ ] Java: Add AssetIssueContract mapping in RemoteExecutionSPI
- [ ] Rust: Add dispatch arm for AssetIssueContract in execute_non_vm_contract
- [ ] Rust: Implement AssetIssue handler (fee, AEXT/bandwidth, state changes)
- [ ] Rust: Add storage adapter getters for ASSET_ISSUE_FEE (and future TRC‑10 props)
- [ ] Config: Keep disabled by default; add/confirm flags
- [ ] Tests: Java mapping unit test; Rust handler unit tests; e2e toggle test
- [ ] Documentation: Feature flags, rollout notes, parity caveats

---

## Java Mapping (RemoteExecutionSPI)

File: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`

Add new `case AssetIssueContract` in `buildExecuteTransactionRequest(...)` switch:

- Classification
  - `txKind = TxKind.NON_VM`
  - `contractType = tron.backend.BackendOuterClass.ContractType.ASSET_ISSUE_CONTRACT`
  - `fromAddress = contract.getOwnerAddress()` (already inferred from tx capsule)
  - `toAddress = empty` (system contract)
  - `value = 0`
  - `data = full Any.unpack(AssetIssueContract).toByteArray()` to allow Rust-side proto parsing

- Gating/Toggles
  - Reuse existing property: `-Dremote.exec.trc10.enabled=true`
  - Optionally add more granular: `-Dremote.exec.trc10.asset_issue.enabled=true`
  - If disabled, throw `UnsupportedOperationException` to force fallback to Java actuators.

- AEXT snapshots
  - Pre-execution snapshots already collected in `collectPreExecutionAext(...)`; ensure owner address is included.

Todo items (Java)

- [ ] Insert switch arm with classification and payload.
- [ ] Read `remote.exec.trc10.enabled` (and/or `.asset_issue.enabled`) and gate accordingly.
- [ ] Add concise debug logs: inputs (name length, total_supply etc. if cheap to extract), classification, toggle state.
- [ ] Extend unit test: validate `TxKind`, `ContractType`, and `data` passthrough for AssetIssueContract.

---

## Rust Backend: Dispatch + Handler

Entry: `rust-backend/crates/core/src/service/mod.rs`

Dispatch arm (in `execute_non_vm_contract`):

```rust
Some(tron_backend_execution::TronContractType::AssetIssueContract) => {
    // Gate behind config.remote.trc10_enabled (and optionally asset_issue_enabled)
    if !remote_config.trc10_enabled { // or asset_issue_enabled
        return Err("ASSET_ISSUE_CONTRACT execution is disabled - falling back to Java".to_string());
    }
    self.execute_asset_issue_contract(storage_adapter, transaction, context)
}
```

Handler: `execute_asset_issue_contract(...)` (new function)

Responsibilities (Phase 1 / MVP):

1) Parse minimal fields from `transaction.data` (protobuf `AssetIssueContract` bytes):
   - owner_address (for sanity; must equal `transaction.from`)
   - `name`, `abbr`, `total_supply`, `precision`, `trx_num`, `num`, `start_time`, `end_time`, `description`, `url` (parse and log; not persisted in Phase 1)
   - Structure defined in `protocol/src/main/protos/core/contract/asset_issue_contract.proto`
   - Use existing varint helper `contracts::proto::read_varint` and length-delimited decoding (mirror freeze parsers).

2) Fee handling (asset issue fee):
   - Read dynamic property `ASSET_ISSUE_FEE` from `properties` DB (add accessor in storage adapter).
   - Validate owner balance ≥ fee; error out if insufficient.
   - Deduct fee from owner; emit `TronStateChange::AccountChange { old_account, new_account }`.
   - If blackhole optimization is disabled (credit mode), credit blackhole account:
     - Read `ALLOW_BLACKHOLE_OPTIMIZATION` (engine accessor exists) and get blackhole address (engine accessor exists).
     - Load blackhole account (default empty if absent) and add fee; emit account change.
   - Sort `state_changes` deterministically by address (ascending bytes) for CSV parity.

3) Bandwidth/AEXT tracking:
   - Compute `bandwidth_used = calculate_bandwidth_usage(transaction)` (existing helper).
   - If `accountinfo_aext_mode == "tracked"`, update owner’s AEXT via `ResourceTracker::track_bandwidth(...)` using `get_free_net_limit()`. Persist via `set_account_aext` and populate `aext_map`.

4) Result shape:
   - `success: true`
   - `energy_used: 0`
   - `bandwidth_used: computed`
   - `logs: []`
   - `state_changes: [owner (-fee), (optional) blackhole (+fee)]`
   - `freeze_changes: []`, `global_resource_changes: []`
   - `aext_map: populated when tracked`

Not in Phase 1:
   - Persisting AssetIssue V1/V2 records; updating account asset maps; bumping TOKEN_ID_NUM.

Parsing cheat sheet (proto field numbers):

- owner_address (1, bytes)
- name (2, bytes)
- abbr (3, bytes)
- total_supply (4, int64)
- frozen_supply (5, repeated message { frozen_amount, frozen_days })
- trx_num (6, int32)
- precision (7, int32)
- num (8, int32)
- start_time (9, int64)
- end_time (10, int64)
- order (11, int64, unused)
- vote_score (16, int32)
- description (20, bytes)
- url (21, bytes)
- free_asset_net_limit (22, int64)
- public_free_asset_net_limit (23, int64)
- public_free_asset_net_usage (24, int64)
- public_latest_free_net_time (25, int64)

Todo items (Rust)

- [ ] Add dispatch arm for AssetIssueContract with config gating.
- [ ] Implement `execute_asset_issue_contract` as per Phase 1 responsibilities.
- [ ] Add `EngineBackedEvmStateStore::get_asset_issue_fee()` to read `ASSET_ISSUE_FEE` (8‑byte big endian) from `properties`.
- [ ] Unit tests:
     - Disabled flag returns error → Java fallback path.
     - Fee burn vs blackhole credit (two `state_changes` when not burning; one when burning).
     - Insufficient balance error.
     - AEXT tracked mode updates persisted and echoed.
- [ ] Deterministic ordering of state_changes by address.

---

## Phase 2: Full TRC‑10 Ledger Semantics (Optional Path)

Two approaches; choose based on storage maturity:

Option A — Proto extension (recommended for fast parity)

- Extend `framework/src/main/proto/backend.proto`:
  - Define `message Trc10Change` with oneof variants:
    - `AssetIssued { name, abbr, total_supply, precision, trx_num, num, start_time, end_time, description, url, owner_address, token_id (v2), allow_same_token_name }`
  - Add `repeated Trc10Change trc10_changes = <next_field>` to `ExecutionResult`.
- Rust handler populates `AssetIssued` with parsed fields; computes next `TOKEN_ID_NUM` if doing ID assignment in Rust (or leave unset and let Java compute).
- Java `RemoteExecutionSPI.convertExecuteTransactionResponse(...)` parses `trc10_changes` and applies to stores:
  - Create AssetIssueCapsule in `AssetIssueStore` and `AssetIssueV2Store`.
  - Update issuer `AccountCapsule` asset maps (V1 by name; V2 by id).
  - Increment/persist `TOKEN_ID_NUM` in `DynamicPropertiesStore`.
  - Recompute and persist TRC‑10 free bandwidth fields if needed.

Option B — Rust storage support (longer path)

- Add TRC‑10 stores in storage adapter (asset_issue, asset_issue_v2, account_asset maps) mirroring Java DB layout.
- Perform all persistence in Rust and emit only account changes (plus optional explicit TRC‑10 changes for observability).

Todo items (Phase 2)

- [ ] Decide between Option A and B.
- [ ] If Option A: design proto; implement result conversion on both sides.
- [ ] If Option B: design DB schemas and implement storage adapter methods; add end‑to‑end tests.

---

## Configurations and Flags

Java (JVM flags)

- `-Dremote.exec.trc10.enabled=true` → enables TRC‑10 over remote backend (TransferAsset and AssetIssue).
- Optional: `-Dremote.exec.trc10.asset_issue.enabled=true` → more granular toggle.

Rust (config.toml / env)

- `execution.remote.trc10_enabled = false` (default false)
- Optional: add `execution.remote.asset_issue_enabled = false` for finer control.
- Fee mode: `execution.fees.mode = "burn" | "blackhole" | "none"` (defaults to "burn" in code; config.toml uses "blackhole" in repo)
- Blackhole optimization: `support_black_hole_optimization` dynamic property → when true, burn; when false, credit blackhole.
- AEXT echo: `execution.remote.accountinfo_aext_mode = "hybrid"` recommended for parity.

Todo items (Config)

- [ ] Keep feature disabled by default (both Java and Rust), document how to enable.
- [ ] (Optional) Add granular `asset_issue_enabled` in `RemoteExecutionConfig` with defaults and config builder wiring.
- [ ] Update `rust-backend/config.toml` docs/comments with the new flag.

---

## Pre/Post-exec Effects and Flushing

Context from constraints:

1) Emit account changes due to AssetIssueContract, send back to Java, Java apply to AccountStore and flush to DB
   - Achieved via `ExecutionResult.state_changes` with `AccountChange` entries; Java applies via `RuntimeSpiImpl.applyStateChanges...` (puts to AccountStore).

2) If there’s any pre-exec effect to Account/DynamicProperty store (fee/bandwidth/energy), flush to DB before executing tx
   - Pre-exec bandwidth in Java is not run during remote path; bandwidth/AEXT is computed in Rust and echoed back; flush happens right after remote exec (post‑exec) when Java applies state changes.
   - If we later rely on dynamic properties for bandwidth calculations within the same block (e.g., FREE_NET vs PUBLIC_NET usage), consider emitting explicit dynamic property changes in the result (like Phase 2 freeze flow uses `GlobalResourceTotalsChange`). For AssetIssue specifically, dynamic properties are read (ASSET_ISSUE_FEE), not modified.

3) Even if there’s no tx, if there’s any change to Account/DynamicProperty store (fee/bandwidth/energy), we still need to flush to DB
   - For remote exec, all emitted changes are applied immediately by Java (account `put`, dynamic `save*`).
   - For non-tx updates (maintenance windows), consider a small tracker to ensure `ResourceSyncContext` marks dirty keys and flushes at block boundaries. Current Phase 2 freeze flow already updates dynamic totals synchronously.

Todo items (Flush semantics)

- [ ] Confirm no dynamic property changes are expected during AssetIssue beyond fee charging (which is account-level only).
- [ ] Ensure AEXT tracking is applied and persisted by Java immediately after exec.
- [ ] If we later emit dynamic property updates for bandwidth pools, add a result field and Java applier similar to `GlobalResourceTotalsChange`.

---

## Testing Plan

Java

- [ ] SPI mapping test: build a `Transaction` with `AssetIssueContract` and assert:
  - `TxKind == NON_VM`
  - `ContractType == ASSET_ISSUE_CONTRACT`
  - `data` equals `contract.toByteArray()`
  - Fallback behavior when `remote.exec.trc10.enabled=false` throws `UnsupportedOperationException`.

Rust

- [ ] Unit tests for handler:
  - Disabled flag → returns Err; dispatch falls back to Java.
  - Owner balance ≥ fee; mode=burn → one AccountChange (owner −fee), energy_used=0, bandwidth_used>0.
  - Owner balance ≥ fee; mode=blackhole and optimization disabled → two AccountChanges (owner −fee, blackhole +fee) with deterministic ordering.
  - Insufficient owner balance → Err.
  - AEXT tracked mode → AEXT persisted and echoed for owner.

Integration (optional)

- [ ] End‑to‑end over gRPC with local backend and minimal Java harness; verify ExecuteTransactionResponse shape.

---

## Rollout and Parity Notes

- Keep feature off by default; roll out behind flags.
- Determinism: sort state_changes; use 0 energy for non‑VM; log key decisions with addresses and amounts.
- Parity caveats while Phase 2 is pending:
  - No TRC‑10 ledger mutations (stores, TOKEN_ID_NUM); value‑add only is fee deduction/credit and bandwidth/AEXT.
  - CSV/state diffs will reflect balance changes; TRC‑10 specific rows will still be produced by Java fallback when disabled.

---

## Open Questions

- Java flag naming: reuse `remote.exec.trc10.enabled` vs. add more granular `remote.exec.trc10.asset_issue.enabled`?
- Prefer proto extension (Option A) or Rust storage adapter (Option B) for full TRC‑10 persistence? Timeline?
- Should we emit explicit dynamic property updates for public bandwidth pools for AssetIssue? (Likely no.)
- Any additional invariant validations required for AssetIssue in Phase 1 (beyond asset issue fee and basic time sanity)?

---

## References (in-repo)

- Java SPI switch and AEXT snapshots: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- Backend proto enums: `framework/src/main/proto/backend.proto` (ContractType, TxKind, TronTransaction)
- Rust dispatch: `rust-backend/crates/core/src/service/mod.rs` (`execute_non_vm_contract`)
- Rust varint helper: `rust-backend/crates/core/src/service/contracts/proto.rs`
- Rust AEXT tracking and bandwidth examples: witness/transfer handlers in `service/mod.rs`
- Dynamic property keys (Java): `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java`
- TRC‑10 contract proto: `protocol/src/main/protos/core/contract/asset_issue_contract.proto`

