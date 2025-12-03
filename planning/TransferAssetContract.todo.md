# TRC-10 TransferAssetContract: Remote Execution Plan

Owner: Core Runtime/Remote SPI + Rust Backend
Scope: Java mapping in RemoteExecutionSPI; Rust non‑VM handler in `execute_non_vm_contract`; bandwidth/AEXT accounting; optional TRX fee deltas (config‑gated); Phase 2 TRC‑10 semantic change (Java applies to stores).
Status: Design/Planning (do not implement yet)

---

## Goals

- Classify and route `TransferAssetContract` to the Rust backend as a non‑VM contract when enabled.
- In Rust, validate inputs, compute bandwidth, track AEXT for the owner, and return a deterministic `ExecutionResult`:
  - `energy_used = 0`, non‑zero `bandwidth_used`, no logs.
  - Account changes: default is a single no‑op AccountChange for `owner` (old==new) so AEXT is carried; if TRX fee is configured, emit the real owner delta (and optional blackhole credit) instead.
  - Emit Phase 2 `Trc10Change::AssetTransferred` with owner, recipient, asset identifier (name or token_id), and amount. Java will apply TRC‑10 ledger deltas.
- Keep feature gated and off by default; preserve fallback to Java actuators when disabled.

## Non‑Goals (for this phase)

- Do not persist TRC‑10 balances or asset metadata in Rust storage (Java will continue to own TRC‑10 stores).
- Do not implement FREE_ASSET_NET/PUBLIC_NET path selection yet (will be a follow‑up to match BandwidthProcessor).
- Do not treat TRC‑10 as a VM/TVM execution path.

## Preconditions & Feature Flags

- Java mapping must carry `contract_type=TRANSFER_ASSET_CONTRACT`, `tx_kind=NON_VM`, and `asset_id` bytes.
- Gate end‑to‑end with JVM flag `-Dremote.exec.trc10.enabled=true` (existing) and Rust `execution.remote.trc10_enabled`.
- AEXT serialization mode stays configurable (`none|zeros|defaults|tracked|hybrid`) with recommended defaults:
  - For CSV parity without tracking, use `defaults`.
  - For resource parity, use `tracked` and persist AEXT updates.
- Fees config: `fees.non_vm_blackhole_credit_flat` optional; `fees.mode` in `{burn, blackhole}`; `fees.blackhole_address_base58` required only for blackhole mode.

## End‑to‑End Flow (High‑Level)

1) Java mapping (RemoteExecutionSPI)
- Contract type switch maps `TransferAssetContract` to NON_VM when `remote.exec.trc10.enabled=true`.
- Fields passed:
  - `from = ownerAddress`
  - `to = toAddress`
  - `value = amount`
  - `asset_id = asset_name` bytes (V1 name or V2 token_id string)
  - `data = []` (no need to include full proto for this contract)
- Pre‑execution AEXT snapshots collected for `from` and `to` (to allow hybrid/tracked AEXT passthrough if desired).

2) Rust handler (non‑VM path)
- Extract `owner = tx.from`, `to = tx.to.expect(..)`, `amount = tx.value (U256→u64)`, `asset_id = tx.metadata.asset_id.expect(..)`.
- Validate `amount > 0` and `asset_id` present.
- Compute `bandwidth_used = calculate_bandwidth_usage(tx)`.
- AEXT tracking (if `aext_mode == "tracked"`):
  - Load owner AEXT (`get_account_aext`), get `FREE_NET_LIMIT` dynamic prop, call `ResourceTracker::track_bandwidth`.
  - Persist `after_aext` and populate `aext_map[owner] = (before, after)`.
- State changes:
  - Default (no TRX fee configured): emit one AccountChange for `owner` with `old_account == new_account` (no balance/nonce change) to carry AEXT and ensure deterministic CSV presence.
  - If `fees.non_vm_blackhole_credit_flat = Some(fee)`:
    - Load owner, ensure `balance >= fee`, debit owner; emit owner AccountChange.
    - If `fees.mode == blackhole` and address configured, credit blackhole and emit its AccountChange; else burn (no extra state change).
- Emit one `Trc10Change::AssetTransferred` describing the TRC‑10 transfer.
- Deterministic ordering: sort AccountChanges by address; keep `energy_used=0`, no logs.

3) Java apply (RuntimeSpiImpl)
- Parse `Trc10Change.asset_transferred` and apply to TRC‑10 stores:
  - Read `ALLOW_SAME_TOKEN_NAME` to choose V1 (asset map by name) vs V2 (assetV2 by token_id).
  - Ensure recipient account exists (create if missing, as per actuator behavior).
  - Debit owner and credit recipient by `amount` in the correct map.
  - Persist accounts; mark dirty for resource sync; log deterministic outcomes.

---

## Detailed TODOs

### A. Protocol & Types

- [ ] Extend `framework/src/main/proto/backend.proto`:
  - [ ] Add `message Trc10AssetTransferred { bytes owner_address = 1; bytes to_address = 2; bytes asset_name = 3; string token_id = 4; int64 amount = 5; }`
  - [ ] Extend `message Trc10Change { oneof kind { Trc10AssetIssued asset_issued = 1; Trc10AssetTransferred asset_transferred = 2; } }`
- [ ] Rust types (execution):
  - [ ] Add `struct Trc10AssetTransferred` and enum variant `Trc10Change::AssetTransferred` in `rust-backend/crates/execution/src/tron_evm.rs`.
- [ ] Rust→proto conversion:
  - [ ] Update `rust-backend/crates/core/src/service/grpc/conversion.rs` to convert the new variant to/from proto.
- [ ] Java SPI parsing:
  - [ ] Update `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java` to parse `asset_transferred` into a new `ExecutionSPI.Trc10AssetTransferred` type.
- [ ] Java apply path:
  - [ ] Add `applyAssetTransferredChange(ExecutionSPI.Trc10AssetTransferred, ChainBaseManager, TransactionContext)` in `RuntimeSpiImpl` and hook it in the loop beside asset_issued.

### B. Java Mapping (RemoteExecutionSPI)

- [ ] Keep existing gate in place: if `remote.exec.trc10.enabled` is false, throw `UnsupportedOperationException` to fall back to Java actuator.
- [ ] Ensure we include `asset_id` bytes from `TransferAssetContract.asset_name`.
- [ ] Ensure AEXT pre‑snapshots include both `from` and `to` for this contract type.
- [ ] Add concise debug logs (owner, to, amount, asset_id length, toggle state).

### C. Rust Dispatch & Handler

- [ ] Replace the TODO at `rust-backend/crates/core/src/service/mod.rs:239–246` to call the new handler.
- [ ] Implement `execute_trc10_transfer_contract(...)` in `mod.rs`:
  - [ ] Validate feature flag `remote.trc10_enabled` (defensive check).
  - [ ] Extract inputs from `transaction` and `metadata`.
  - [ ] Validate `amount > 0`, `asset_id != None`, and `to.is_some()`.
  - [ ] Compute `bandwidth_used`.
  - [ ] AEXT tracking for owner if `aext_mode == "tracked"`; persist after AEXT; populate `aext_map`.
  - [ ] State changes:
    - [ ] Default path: single no‑op owner AccountChange (old==new).
    - [ ] Optional fee path (when `fees.non_vm_blackhole_credit_flat` present):
      - [ ] Debit owner; emit owner AccountChange; persist.
      - [ ] Credit blackhole (if mode="blackhole"); emit AccountChange; persist.
  - [ ] Build `Trc10Change::AssetTransferred` with {owner, to, asset_name=asset_id bytes, token_id=Some(..) only if ascii‑numeric, amount} and attach to result.
  - [ ] Keep `energy_used=0`, `logs=[]`, `error=None`.
  - [ ] Deterministic sort of state changes.

### D. Java Apply (RuntimeSpiImpl)

- [ ] Add handler to apply `asset_transferred`:
  - [ ] Determine V1 vs V2 via `DynamicPropertiesStore.getAllowSameTokenName()`.
  - [ ] Look up or create owner and recipient accounts in `AccountStore`.
  - [ ] Validate owner TRC‑10 balance; debit/credit asset map:
    - [ ] V1: `AccountCapsule.asset[name_bytes]`.
    - [ ] V2: `AccountCapsule.assetV2[token_id]`.
  - [ ] Persist back to stores; mark accounts dirty for resource sync.
  - [ ] Logs at INFO for deterministic outcomes (owner, to, amount, asset key).

### E. Fees & Parity Rules (Non‑VM)

- [ ] Disable forced TRX deduction for non‑VM by default when `fees.non_vm_blackhole_credit_flat` is None.
- [ ] When a flat fee is configured:
  - [ ] Deduct from `owner` TRX; `mode=burn` → no extra AccountChange; `mode=blackhole` → credit blackhole account (requires valid base58 address).
- [ ] Do not increment EVM nonce for non‑VM.
- [ ] Keep “Don’t treat TRC‑10 as TRX” — never touch TRX balances for the TRC‑10 transfer itself.

### F. Determinism & Ordering

- [ ] `energy_used = 0`, `logs = []`.
- [ ] Deterministic sort of `state_changes` (by address; owner before blackhole if both present).
- [ ] Stable serialization of AEXT per configured mode (via existing conversion pipeline).

### G. Tests

- Rust unit tests (core/service/tests):
  - [ ] Feature disabled → returns error and does not mutate state.
  - [ ] Happy path (no TRX fee): success, `bandwidth_used > 0`, one no‑op owner AccountChange, one `Trc10Change::AssetTransferred`.
  - [ ] With TRX flat fee (burn): owner AccountChange reflects TRX debit; no blackhole AccountChange.
  - [ ] With TRX flat fee (blackhole): owner and blackhole AccountChanges reflect deltas.
  - [ ] AEXT tracked mode: `aext_map` populated; protobuf contains expected AEXT values.

- Java unit/integration:
  - [ ] `RemoteExecutionSPI.convertExecuteTransactionResponse` parses `asset_transferred`.
  - [ ] `RuntimeSpiImpl.applyAssetTransferredChange` debits owner and credits recipient in V1 and V2 modes.
  - [ ] Recipient account creation on missing account behaves as actuator (create + credit).

### H. Observability

- [ ] Add trace/debug logs in Rust: inputs (owner, to, amount, asset_id len), bandwidth, AEXT path (FREE_NET vs ACCOUNT_NET when available), emitted trc10_changes count.
- [ ] Add INFO in Java apply with concise summary (owner, to, token key, amount, mode V1/V2).

### I. Rollout Plan

- [ ] Keep feature off by default in Java (`remote.exec.trc10.enabled=false`).
- [ ] Land proto/types/conversions + Java apply support first.
- [ ] Enable on a small replay window; compare CSV parity; verify no unintended extra state changes.
- [ ] Expand coverage and validate against embedded CSVs.

### J. Edge Cases & Follow‑Ups

- Asset identifier handling
  - [ ] Detect V2 `token_id` when `asset_id` bytes are ASCII digits; otherwise treat as V1 name bytes. Preserve raw bytes for V1.
  - [ ] If V2 id provided but missing in stores, Java apply should create recipient account but fail gracefully on missing asset supply (match actuator behavior).

- FREE_ASSET_NET / PUBLIC_NET semantics (follow‑up)
  - [ ] Implement issuer free asset limits, public pool counters, and path selection consistent with `BandwidthProcessor`.
  - [ ] Update dynamic props `PUBLIC_NET_USAGE` / `PUBLIC_NET_TIME` accordingly.

- Deterministic context
  - [ ] Ensure block_number/timestamp are sourced from Java and deterministic; avoid “now” fallbacks.

---

## Deliverables

- Proto + Rust types updated; conversions on both sides.
- Rust non‑VM handler for `TransferAssetContract` with AEXT tracking and optional flat TRX fee logic.
- Java apply logic for `asset_transferred` changes updating TRC‑10 balances (V1/V2).
- Unit tests in Rust and Java covering disabled gate, happy path, fee variants, V1/V2 maps, and AEXT tracked mode.

## Risks & Mitigations

- CSV parity regressions due to extra AccountChanges → Use single owner no‑op AccountChange unless TRX fee configured; keep deterministic ordering.
- Ambiguous asset identifier across ALLOW_SAME_TOKEN_NAME toggles → Detect V1/V2 by content; let Java side apply according to dynamic property.
- Resource path divergence (FREE_ASSET_NET vs ACCOUNT_NET) → Defer to follow‑up; document that initial bandwidth accounting uses owner path only.

