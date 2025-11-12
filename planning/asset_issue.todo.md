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

- [x] Java: Add AssetIssueContract mapping in RemoteExecutionSPI
- [x] Rust: Add dispatch arm for AssetIssueContract in execute_non_vm_contract
- [x] Rust: Implement AssetIssue handler (fee, AEXT/bandwidth, state changes)
- [x] Rust: Add storage adapter getters for ASSET_ISSUE_FEE (and future TRC‑10 props)
- [x] Config: Keep disabled by default; add/confirm flags (using existing trc10_enabled flag)
- [x] Tests: Java mapping unit test; Rust handler unit tests; e2e toggle test
- [x] Documentation: Feature flags, rollout notes, parity caveats

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

- [x] Insert switch arm with classification and payload.
- [x] Read `remote.exec.trc10.enabled` (and/or `.asset_issue.enabled`) and gate accordingly.
- [x] Add concise debug logs: inputs (name length, total_supply etc. if cheap to extract), classification, toggle state.
- [x] Extend unit test: validate `TxKind`, `ContractType`, and `data` passthrough for AssetIssueContract.

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

- [x] Add dispatch arm for AssetIssueContract with config gating.
- [x] Implement `execute_asset_issue_contract` as per Phase 1 responsibilities.
- [x] Add `EngineBackedEvmStateStore::get_asset_issue_fee()` to read `ASSET_ISSUE_FEE` (8‑byte big endian) from `properties`.
- [x] Unit tests:
     - Disabled flag returns error → Java fallback path.
     - Fee burn vs blackhole credit (two `state_changes` when not burning; one when burning).
     - Insufficient balance error.
     - AEXT tracked mode updates persisted and echoed.
- [x] Deterministic ordering of state_changes by address.

---

## Phase 2: Full TRC‑10 Ledger Semantics (Option A - Proto Extension) [COMPLETED]

We will proceed with Option A. Rust emits high‑level TRC‑10 semantic diffs; Java applies them to existing stores.

### Phase 2 Progress Summary:

**✅ Completed - Proto and Rust Side:**
1. Extended backend.proto with Trc10AssetIssued message and Trc10Change oneof
2. Added trc10_changes field to ExecutionResult in backend.proto
3. Updated Rust types (Trc10AssetIssued, Trc10Change) in tron_evm.rs
4. Extended AssetIssueInfo struct with Phase 2 fields (free_asset_net_limit, etc.)
5. Updated execute_asset_issue_contract to emit Trc10Change with all parsed fields
6. Added protobuf conversion logic in conversion.rs
7. All Rust code compiles successfully
8. Added comprehensive Rust tests for Trc10Change emission

**✅ Completed - Java Side (Parsing):**
1. Added Trc10AssetIssued and Trc10Change classes to ExecutionSPI.java
2. Updated ExecutionResult to include trc10Changes field
3. Added parsing logic in RemoteExecutionSPI.convertExecuteTransactionResponse
4. Updated all ExecutionResult constructor calls across the codebase
5. All Java code compiles successfully

**✅ Completed - Java Side (Application):**
1. Implemented applyTrc10Changes() in RuntimeSpiImpl.java
2. Implemented applyAssetIssuedChange() with full store application logic:
   - Create AssetIssueCapsule V1/V2 entries in stores
   - Manage TOKEN_ID_NUM (read, increment, save)
   - Update issuer account asset maps (assetV1/assetV2)
   - Handle ALLOW_SAME_TOKEN_NAME toggle (V1 vs V2 behavior)
3. Added comprehensive Java tests for TRC-10 changes:
   - testTrc10AssetIssuedChangeParsing: Verify parsing from ExecutionProgramResult
   - testTrc10AssetIssuedApplicationWithV1: Test V1+V2 store creation (ALLOW_SAME_TOKEN_NAME=0)
   - testTrc10AssetIssuedApplicationWithoutV1: Test V2-only store creation (ALLOW_SAME_TOKEN_NAME=1)
   - testTokenIdNumManagement: Verify TOKEN_ID_NUM read/increment/save
4. All tests compile successfully (require Spring context to run)
5. Java implementation feature-gated with `-Dremote.exec.apply.trc10=true` (default true)

Proto changes (backend.proto):

- Add new messages:
  - `message Trc10AssetIssued {` (fields mirror AssetIssueContract; bytes where Java expects bytes)
    - `bytes owner_address = 1;`
    - `bytes name = 2;`
    - `bytes abbr = 3;`
    - `int64 total_supply = 4;`
    - `int32 trx_num = 5;`
    - `int32 precision = 6;`
    - `int32 num = 7;`
    - `int64 start_time = 8;`
    - `int64 end_time = 9;`
    - `bytes description = 10;`
    - `bytes url = 11;`
    - `int64 free_asset_net_limit = 12;`
    - `int64 public_free_asset_net_limit = 13;`
    - `int64 public_free_asset_net_usage = 14;`
    - `int64 public_latest_free_net_time = 15;`
    - `string token_id = 16; // optional; if empty, Java computes via TOKEN_ID_NUM`
  - `message Trc10Change { oneof kind { Trc10AssetIssued asset_issued = 1; } }`
- Extend `ExecutionResult` with:
  - `repeated Trc10Change trc10_changes = <next_field_number>;`

Rust emission:

- In `execute_asset_issue_contract`, after fee/bandwidth/AEXT, build a `Trc10Change.asset_issued` with parsed fields.
- For `token_id`:
  - Leave empty and let Java compute from `DynamicPropertiesStore.getTokenIdNum() + 1`, unless we later add a safe query/increment RPC.
- Add to `ExecutionResult.trc10_changes` and return.

Java application:

- Extend `RemoteExecutionSPI.convertExecuteTransactionResponse(...)` to parse `trc10_changes`.
- For each `asset_issued`:
  - Read/compute `token_id`:
    - If present in message, trust it; else: `id = dynamicStore.getTokenIdNum() + 1` and then `dynamicStore.saveTokenIdNum(id)`.
  - Create AssetIssueCapsule V1 or V2 (both):
    - V1 key by `name` when `ALLOW_SAME_TOKEN_NAME == 0` (legacy paths), V2 by `token_id` string.
    - Set `precision`, `trx_num`, `num`, `start_time`, `end_time`, `description`, `url`, and free asset net limits.
  - Update issuer `AccountCapsule` asset balances:
    - V1: `owner.asset[name] = total_supply` when same‑name disabled.
    - V2: `owner.assetV2[token_id] = total_supply`.
  - Put updated account and asset issue entries to their stores; log deterministic outcomes.
- Ensure ordering of operations and store writes is deterministic; reuse existing helpers where possible.

Testing (Phase 2):

- Java
  - Tests ensuring `trc10_changes` produce correct store mutations and TOKEN_ID_NUM increments.
  - ALLOW_SAME_TOKEN_NAME toggles respected between V1 vs V2 addressability.
- Rust
  - Emission test: `trc10_changes` contains correctly populated `Trc10AssetIssued` from the input bytes.

Notes:

- Option B (Rust storage persistence) is not selected. Keep this as future consideration if we want Rust‑side TRC‑10 storage.

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

- [x] Keep feature disabled by default (both Java and Rust), document how to enable.
- [x] (Optional - decided to reuse existing trc10_enabled flag for simplicity)
- [x] Config documented in planning docs.

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

- [x] SPI mapping test: build a `Transaction` with `AssetIssueContract` and assert:
  - `TxKind == NON_VM`
  - `ContractType == ASSET_ISSUE_CONTRACT`
  - `data` equals `contract.toByteArray()`
  - Fallback behavior when `remote.exec.trc10.enabled=false` throws `UnsupportedOperationException`.
  - Test file: `framework/src/test/java/org/tron/core/execution/spi/RemoteExecutionSPIAssetIssueTest.java`

Rust

- [x] Unit tests for handler:
  - Disabled flag → returns Err; dispatch falls back to Java.
  - Owner balance ≥ fee; mode=burn → one AccountChange (owner −fee), energy_used=0, bandwidth_used>0.
  - Owner balance ≥ fee; mode=blackhole and optimization disabled → two AccountChanges (owner −fee, blackhole +fee) with deterministic ordering.
  - Insufficient owner balance → Err.
  - AEXT tracked mode → AEXT persisted and echoed for owner.
  - Deterministic execution across multiple runs.
  - Test location: `rust-backend/crates/core/src/tests.rs` (AssetIssueContract Tests section)

Integration (optional)

- [ ] End‑to‑end over gRPC with local backend and minimal Java harness; verify ExecuteTransactionResponse shape.
  - NOTE: Integration tests require a running Rust backend and are covered separately in end-to-end testing.

---

## Rollout and Parity Notes

- Keep feature off by default; roll out behind flags.
- Determinism: sort state_changes; use 0 energy for non‑VM; log key decisions with addresses and amounts.
- Parity caveats while Phase 2 is pending:
  - No TRC‑10 ledger mutations (stores, TOKEN_ID_NUM); value‑add only is fee deduction/credit and bandwidth/AEXT.
  - CSV/state diffs will reflect balance changes; TRC‑10 specific rows will still be produced by Java fallback when disabled.

### Implementation Summary (Phase 1 Complete)

**What Was Implemented:**
1. Java RemoteExecutionSPI mapping for AssetIssueContract (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:326-346)
2. Rust dispatch arm and handler (rust-backend/crates/core/src/service/mod.rs:231-235, 1233-1412)
3. Asset issue fee storage accessor (rust-backend/crates/execution/src/storage_adapter/engine.rs:405-418)
4. Protobuf parsing for AssetIssueContract fields (name, abbr, total_supply, precision, etc.)
5. Fee handling (burn mode and blackhole credit mode)
6. Bandwidth usage calculation and AEXT tracking
7. State change emission with deterministic ordering

**Testing Coverage:**
- Java: 5 unit tests covering classification, feature flags, serialization (RemoteExecutionSPIAssetIssueTest.java)
- Rust: 6 unit tests covering disabled flag, insufficient balance, fee burn, blackhole credit, AEXT tracking, deterministic execution (tests.rs)
- All tests use in-memory storage and mock data to avoid requiring running services

**Feature Flags:**
- Java: `-Dremote.exec.trc10.enabled=true` (default: false)
- Rust: `execution.remote.trc10_enabled = true` in config.toml (default: false)
- Both must be enabled for AssetIssueContract remote execution

**Known Limitations (Phase 1):**
- Does not create AssetIssue V1/V2 database entries (requires Phase 2 proto extension or storage support)
- Does not update account asset maps (assetV1/assetV2)
- Does not increment TOKEN_ID_NUM counter
- Does not validate name uniqueness (ALLOW_SAME_TOKEN_NAME)
- Only fee charging and balance updates are performed

**Phase 2 Requirements:**
- Extend backend.proto with Trc10Change message for semantic changes
- Java-side parsing and application to AssetIssueStore/AssetIssueV2Store
- TOKEN_ID_NUM management
- Name uniqueness validation
- Full TRC-10 asset lifecycle support

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
