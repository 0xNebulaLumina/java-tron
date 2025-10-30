# TRC-10 Remote Execution: Asset Issue + Participate — Detailed Plan

Status: Phase 1 Complete (Emit-and-Apply Implementation)

Goal: Add Java→Rust mapping for TRC‑10 contracts and implement remote handlers in Rust for AssetIssueContract and ParticipateAssetIssueContract. Keep TransferAssetContract stubbed for now. Deliver safe parity under feature flags with a clear upgrade path.

Contents
- Overview & Strategy
- Java Tasks (mapping + application)
- Proto Tasks (new TRC‑10 change type, codegen)
- Rust Tasks (dispatch + handlers + validation)
- Phase 2 (optional) full persistence in Rust
- Edge Cases & Parity Notes
- Test Plan
- Rollout & Config
- Acceptance Criteria

---

## Overview & Strategy

Two viable approaches exist for applying TRC‑10 changes during remote execution:

1) Emit TRC‑10 ledger changes from Rust; apply deltas in Java (recommended first)
   - Pros: Minimal storage complexity in Rust; reuse mature Java code for stores and invariants; simpler to reach parity quickly.
   - Cons: Needs a small backend.proto change and a new Java apply path.

2) Persist TRC‑10 ledgers entirely in Rust storage (Phase 2)
   - Pros: Fewer Java code paths at runtime; fully remote persistence.
   - Cons: Requires non‑trivial storage adapter work (Account proto mutation or AccountAssetStore integration), more risk to parity.

Plan: Implement (1) now, then (2) as an optional follow‑up behind flags.

---

## Java Tasks

- [x] J1: Map TRC‑10 contracts in RemoteExecutionSPI
  - File: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
  - Imports:
    - `org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract`
    - `org.tron.protos.contract.AssetIssueContractOuterClass.ParticipateAssetIssueContract`
  - Switch additions in `buildExecuteTransactionRequest()`:
    - AssetIssueContract
      - Gate with `-Dremote.exec.trc10.enabled` (default false): if disabled, throw `UnsupportedOperationException` to fall back to Java actuators.
      - Unpack `AssetIssueContract`, set: `fromAddress = owner`, `toAddress = empty`, `value = 0`, `data = full contract bytes`.
      - `txKind = NON_VM`, `contractType = ASSET_ISSUE_CONTRACT`, `assetId = empty`.
    - ParticipateAssetIssueContract
      - Same gate as above.
      - Unpack `ParticipateAssetIssueContract`, set: `fromAddress = owner`, `toAddress = to_address`, `value = 0`, `data = full contract bytes`, `assetId = asset_name` bytes.
      - `txKind = NON_VM`, `contractType = PARTICIPATE_ASSET_ISSUE_CONTRACT`.
  - Keep existing TRC‑10 transfer case gated as it is (remains stubbed in Rust); fallback if disabled.

- [x] J2: Pre‑exec AEXT snapshot coverage (optional parity aid)
  - Extend `collectPreExecutionAext(...)` to include `toAddress` for `ParticipateAssetIssueContract` (like TransferAssetContract) to improve resource usage parity in CSV/logs.

- [x] J3: Apply TRC‑10 ledger changes in Java runtime (Phase 1 approach)
  - File: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
  - Add method `applyTrc10LedgerChanges(ExecutionProgramResult result, TransactionContext context)`
    - Iterate `result.getTrc10Changes()` (new field) and apply per change (details in Proto Tasks):
      - ISSUE: mirror `AssetIssueActuator.execute`:
        - Read/derive token id number (increment `TOKEN_ID_NUM`), assign id on AssetIssueContract.
        - Persist to `AssetIssueStore` (name‑keyed) and `AssetIssueV2Store` (id‑keyed) depending on `ALLOW_SAME_TOKEN_NAME`.
        - Initialize issuer’s asset balance: `remainSupply = totalSupply - sum(frozen)`, set V2 and/or legacy maps accordingly.
        - Deduct `ASSET_ISSUE_FEE` from issuer; burn vs credit blackhole per `supportBlackHoleOptimization()`.
      - PARTICIPATE: mirror `ParticipateAssetIssueActuator.execute`:
        - Compute `exchangeAmount = floor(amount * num / trx_num)`; debit issuer asset balance, credit buyer asset balance; move TRX.
    - Preserve existing Java invariants and numerical behavior.
  - Invoke in `RuntimeSpiImpl.execute()` right after converting remote result, in the same area where freeze/global resource changes are applied.

- [ ] J4: Tests for mapping and application
  - Mapping: ensure that when `-Dremote.exec.trc10.enabled=false`, TRC‑10 falls back to Java actuators (UnsupportedOperationException is thrown and caught by caller to fallback path).
  - Application: minimal unit test to feed a fabricated `ExecutionProgramResult` with `Trc10LedgerChange` entries and assert stores (AssetIssueStore/V2 and account balances) update.

---

## Proto Tasks (backend.proto)

- [x] P1: Add TRC‑10 ledger change messages
  - File: `framework/src/main/proto/backend.proto`
  - New message and enum:
    - `enum Trc10Op { ISSUE = 0; PARTICIPATE = 1; TRANSFER = 2; }`
    - `message FrozenSupply { int64 frozen_amount = 1; int64 frozen_days = 2; }` (mirror fields used by Java)
    - `message Trc10LedgerChange {
         Trc10Op op = 1;
         bytes owner_address = 2;   // 21‑byte Tron address
         bytes to_address = 3;      // optional for ISSUE; required for PARTICIPATE/TRANSFER
         bytes asset_id = 4;        // V2 id if known (for ISSUE Java assigns); otherwise asset_name per contract input
         int64 amount = 5;          // for PARTICIPATE (TRX) or TRANSFER; ISSUE sets 0
         // ISSUE‑only fields below (copied from AssetIssueContract for Java side application)
         bytes name = 10;
         bytes abbr = 11;
         int64 total_supply = 12;
         int32 precision = 13;
         repeated FrozenSupply frozen_supply = 14;
         int32 trx_num = 15;
         int32 num = 16;
         int64 start_time = 17;
         int64 end_time = 18;
         bytes description = 19;
         bytes url = 20;
         int64 free_asset_net_limit = 21;
         int64 public_free_asset_net_limit = 22;
         // Fee hint: asset_issue_fee in SUN (Rust fills; Java computes preferred source from properties if absent)
         optional int64 fee_sun = 30;
       }`

- [x] P2: Extend `ExecutionResult` with TRC‑10 changes
  - Add: `repeated Trc10LedgerChange trc10_changes = <next_free_tag>;`
  - Ensure tag numbers don't collide (after 11 currently used; pick 12+ as available; adjust accordingly).

- [x] P3: Regenerate code
  - Rust: `crates/core/build.rs` already compiles `framework/src/main/proto/backend.proto`; no path change needed. Rebuild to generate new gRPC artifacts.
  - Java: Gradle build generates `tron.backend.BackendOuterClass.Trc10LedgerChange` types.

---

## Rust Tasks

- [x] R1: Enable and log TRC‑10 feature
  - File: `rust-backend/config.toml`
    - Set `[execution.remote] trc10_enabled = true` (for dev/test profile).
  - File: `rust-backend/src/main.rs`
    - Log flag on startup: `info!("  TRC-10 enabled: {}", config.execution.remote.trc10_enabled);`

- [x] R2: Dispatch TRC‑10 contract types in core service
  - File: `rust-backend/crates/core/src/service/mod.rs`
  - In non‑VM branch, add:
    - `Some(TronContractType::AssetIssueContract)` → require `remote.trc10_enabled`, call `execute_asset_issue_contract(...)`.
    - `Some(TronContractType::ParticipateAssetIssueContract)` → require flag, call `execute_participate_asset_issue_contract(...)`.
    - Leave `TransferAssetContract` returning `Err("TRC‑10 transfers not yet implemented in Rust backend")`.

- [x] R3: Decode TRC‑10 contract payloads (manual protobuf parsing)
  - Note: Instead of prost, used manual wire-format protobuf parsing following the established pattern from freeze contracts.
  - Leveraged `read_varint()` helper from `service/contracts/proto.rs` for field-by-field parsing.
  - This approach avoids additional dependencies while maintaining compatibility with TRON protobuf format.

- [x] R4: Implement `execute_asset_issue_contract`
  - Input: `EngineBackedEvmStateStore`, `TronTransaction`, `TronExecutionContext`.
  - Steps:
    1) Parse `AssetIssueContract` from `transaction.data`.
    2) Validate (parity‑oriented):
       - Owner account exists; `name`, `abbr`, `url`, `description` meet format (TODO: minimal checks first; exact Java `TransactionUtil` parity as follow‑up).
       - Time window: `start_time > ctx.block_timestamp` and `end_time > start_time`.
       - Totals: `total_supply > 0`, `trx_num > 0`, `num > 0`.
       - FrozenSupply: count <= `MAX_FROZEN_SUPPLY_NUMBER`; each `frozen_amount > 0`, `frozen_days` within `[MIN_FROZEN_SUPPLY_TIME, MAX_FROZEN_SUPPLY_TIME]`; sum(frozen) <= total_supply.
       - Name uniqueness when `ALLOW_SAME_TOKEN_NAME == 0`; reject `name == trx` case when same‑name allowed (case‑insensitive).
       - Fee coverage: owner TRX balance >= `ASSET_ISSUE_FEE`.
    3) Compute change:
       - Build a `Trc10LedgerChange` with `op=ISSUE`, `owner_address`, `name/abbr/...`, frozen list, limits, including `fee_sun`.
       - Do NOT assign `asset_id` here; Java assigns by incrementing `TOKEN_ID_NUM`.
    4) Energy/bandwidth: set bandwidth via existing calculator; energy_used=0.
    5) Return success with 1 AccountChange (owner old==new acceptable) and `trc10_changes` containing the ISSUE entry.

- [x] R5: Implement `execute_participate_asset_issue_contract`
  - Steps:
    1) Parse `ParticipateAssetIssueContract` from `transaction.data`.
    2) Validate:
       - Owner and `to_address` exist and are different; amount > 0.
       - Asset exists (lookup by `asset_name` or V2 id depending on `ALLOW_SAME_TOKEN_NAME`) - Phase 1: deferred to Java.
       - `to_address` matches asset issuer; `ctx.block_timestamp` within [start, end) - Phase 1: deferred to Java.
       - Compute `exchangeAmount = floor(amount * num / trx_num)`; ensure > 0 - Phase 1: computed in Java during apply.
       - Check balances: owner TRX >= amount; issuer asset >= exchangeAmount - Phase 1: deferred to Java.
    3) Build `Trc10LedgerChange` with `op=PARTICIPATE`, include `owner_address`, `to_address`, `asset_id` (asset name or id), `amount` (TRX paid).
    4) Emit 2 AccountChanges (owner, issuer; old==new acceptable), set bandwidth, energy_used=0.

- [x] R6: Attach TRC‑10 changes to protobuf result
  - File: `rust-backend/crates/core/src/service/grpc/conversion.rs`
  - Extend `convert_execution_result_to_protobuf` to map internal `trc10_changes` to `repeated Trc10LedgerChange` in the response.

- [ ] R7: Unit tests (core/service)
  - AssetIssue: valid contract → response `success=true`, 1 AccountChange, 1 Trc10LedgerChange(ISSUE) with expected fields.
  - Participate: valid contract → response `success=true`, 2 AccountChanges, 1 Trc10LedgerChange(PARTICIPATE).
  - Gating: when `trc10_enabled=false`, handlers return explicit `Err(...)` and Java falls back.

---

## Phase 2 (Optional): Full Persistence in Rust

- [ ] R8: Storage adapter extensions
  - File: `rust-backend/crates/execution/src/storage_adapter/engine.rs`
  - Implement dynamic property helpers: `get_asset_issue_fee()`, `get_max_frozen_supply_number()`, `get_min_frozen_supply_time()`, `get_max_frozen_supply_time()`, `get_one_day_net_limit()`, `get_latest_block_header_timestamp()`.
  - Implement `get_token_id_num()` and `save_token_id_num()` (keys: `TOKEN_ID_NUM`).
  - Implement AssetIssue persistence:
    - Persist AssetIssueContract bytes to `asset-issue` (name‑key) and/or `asset-issue-v2` (id‑key) DBs.
    - Initialize owner’s asset balance: write to `account-asset` DB or account proto assetV2 map.
  - Participate ledger persistence:
    - Debit/credit TRX balances via AccountInfo.
    - Adjust account assets in `account-asset` DB (requires `assetOptimized` or updating account proto maps — plan careful approach or add a Java apply shim for this field only).

- [ ] J6: Java flag to turn off `applyTrc10LedgerChanges` when Rust persistence is enabled
  - Keep a property (e.g., `-Dremote.exec.trc10.apply_in_java`) default true; set false when Rust takes over.

---

## Edge Cases & Parity Notes

- AllowSameTokenName handling:
  - 0: name‑keyed V1 store in addition to V2 id‑keyed; V2 precision forced to 0.
  - 1: V2 only; name ‘trx’ is forbidden.

- TOKEN_ID_NUM authority:
  - Phase 1: Java increments and assigns id during apply step.
  - Phase 2: Rust increments in storage and includes the id for reference.

- FrozenSupply and issuer account’s `frozen_supply` field:
  - Phase 1: apply in Java matching `AssetIssueActuator` (`account.addAllFrozenSupply(...)`).
  - Phase 2: either extend Rust account proto serialization to include frozen_supply or keep it in asset‑issue store (verify call‑sites).

- Fees:
  - Asset issue fee sourced from `ASSET_ISSUE_FEE` dynamic property; apply burn vs blackhole per `ALLOW_BLACKHOLE_OPTIMIZATION`. Phase 1 defers to Java.

- Bandwidth/Energy:
  - System contracts use energy_used=0; bandwidth computed from payload size.

- Error strings:
  - Keep messages close to Java actuators for consistent logs (e.g., “TotalSupply must greater than 0!”, “No asset named ...”).

---

## Test Plan

- Java
  - Mapping fallback test when `remote.exec.trc10.enabled=false`.
  - Apply step test: inject `ExecutionProgramResult` with a synthetic `Trc10LedgerChange` ISSUE and verify `AssetIssueStore/V2` and issuer account balances updated; PARTICIPATE likewise.

- Rust
  - Unit tests for handlers producing expected `Trc10LedgerChange` entries with correct fields and validations.
  - Gating tests for `trc10_enabled`.

- Integration (manual)
  - Start Rust backend with `trc10_enabled=true`.
  - Start Java full node with `-Dstorage.remote.host/port` and `-Dremote.exec.trc10.enabled=true`.
  - Use RPC `CreateAssetIssue2` and `ParticipateAssetIssue2`; verify through `Wallet` queries (`GetAssetIssueById`, account assets) that results match embedded execution.

---

## Rollout & Config

- Flags and defaults
  - Java mapping: disabled by default; enable via `-Dremote.exec.trc10.enabled=true`.
  - Rust backend: enable in `rust-backend/config.toml` (`execution.remote.trc10_enabled=true`) for dev/testing; leave code default as false in `RemoteExecutionConfig::default()` for safety.
  - Phase 1 apply in Java: default on; add property to disable when moving to Phase 2.

- Observability
  - Log TRC‑10 flag status on backend startup; log each handler’s main fields (name/id/amount) at info level; keep validation errors explicit.

---

## Acceptance Criteria

- With both flags enabled, TRC‑10 Create and Participate succeed via remote path:
  - Stores reflect new assets (AssetIssueStore/V2) and balances.
  - Account TRX deltas (fee and participation amount) are correct.
  - Wallet and HTTP APIs return consistent results compared to embedded execution.
- With either flag disabled, TRC‑10 contracts execute via embedded Java actuators unchanged.

