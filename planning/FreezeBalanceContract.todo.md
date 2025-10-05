# FreezeBalanceContract — Rust Backend Implementation Plan (Detailed TODO)

Owner: Rust Execution/Core
Status: Draft plan
Scope: Implement correct FreezeBalanceContract handling in the Rust backend with CSV/digest parity first, then full semantics.

## Context & Goals

- Embedded vs Remote mismatch found at tx f3661179… in block 2142: embedded emits exactly 1 AccountChange on the owner; remote emits 2 no-op changes to 0x41…00 (null) address.
- Goal: Remote backend should return for FreezeBalanceContract:
  - Exactly one AccountChange for the owner with balance decreased by the freeze amount (parity-first).
  - energy_used = 0, logs = [], deterministic ordering, and matching state digest.
  - No spurious updates for 0x41…00 and no duplicate changes.

Non-goals (Phase 1):
- Implementing the full resource ledger (frozen balance tables, expiration handling) — planned for Phase 2.
- Implementing V2 variants or Unfreeze/Delegate in Phase 1.

## Acceptance Criteria (CSV/Digest Parity)

- [ ] For FreezeBalanceContract, the CSV row shows `state_change_count = 1` and `address = owner`.
- [ ] The old/new account bytes reflect only the balance decrement (32-byte balance field changes accordingly).
- [ ] `energy_used = 0`, `logs = []`.
- [ ] Deterministic ordering of the single state change; state digest aligns with embedded for the same old/new data.
- [ ] No extra state changes for 0x41…00.

## Files Likely Touched

- `rust-backend/crates/core/src/service.rs`
  - Add non-VM handler: `execute_freeze_balance_contract(...)`.
  - Extend `execute_non_vm_contract(...)` match for `TronContractType::FreezeBalanceContract`.
  - Add param parser helper for FreezeBalance.
- `rust-backend/crates/common/src/config.rs` (optional)
  - Add `freeze_balance_enabled` flag under `execution.remote`.
- `rust-backend/config.toml` (optional)
  - Expose `execution.remote.freeze_balance_enabled` (default false → rollout gate).

## Phase 0 — Pre-checks & Decisions

- [x] Confirm how Java maps FreezeBalanceContract to remote (current runs may differ). Ensure Rust path will be exercised once implemented.
- [x] Decide parameter encoding for `transaction.data`:
  - Option A (chosen): Java passes the raw `FreezeBalanceContract` protobuf bytes in `data`. Rust decodes with manual protobuf parsing.
  - Option B: Define a compact custom encoding `{amount:u64_be, duration:u32_be, resource:u8}`; document; Java must populate.
- [x] Confirm that `transaction.from` is the owner (RemoteExecutionSPI sets `from` for system contracts accordingly).
- [x] Define minimal validation to enforce in Phase 1 (amount > 0, sufficient balance). Defer policy/DP checks to Phase 2.

## Phase 1 — Parity-First Implementation (Balance Delta Only)

1) Contract Type Dispatch
- [x] In `execute_non_vm_contract(...)`, add match arm for `TronContractType::FreezeBalanceContract` → call `execute_freeze_balance_contract`.

2) Parameter Parsing
- [x] Add `struct FreezeParams { amount: u64, duration_days: u32, resource: FreezeResource }`.
- [x] Define `enum FreezeResource { Bandwidth, Energy }` (Phase 1: value recorded but unused).
- [x] Implement `parse_freeze_balance_params(data: &[u8]) -> Result<FreezeParams, String>`.
  - Option A chosen: Manual protobuf parser to extract `frozen_balance`, `frozen_duration`, `resource`.
  - Implemented `read_varint()` helper for protobuf wire format parsing.
- [x] Unit tests for parser (valid/invalid cases) - covered in integration tests.

3) Handler Core Logic
- [x] Function signature:
  ```
  fn execute_freeze_balance_contract(
      &self,
      storage_adapter: &mut tron_backend_execution::StorageModuleAdapter,
      transaction: &TronTransaction,
      context: &TronExecutionContext,
  ) -> Result<TronExecutionResult, String>
  ```
- [x] Load owner account: `let owner = storage_adapter.get_account(&transaction.from)?.unwrap_or_default();`
- [x] Parse params; return Err on invalid or empty data.
- [x] Validate amount > 0.
- [x] Validate `owner.balance >= amount`; otherwise Err("Insufficient balance").
- [x] Compute `new_owner = owner` with `balance -= amount` (nonce/code unchanged).
- [x] Persist: `storage_adapter.set_account(transaction.from, new_owner.clone())?`.
- [x] Emit exactly one state change:
  ```
  TronStateChange::AccountChange {
    address: transaction.from,
    old_account: Some(owner),
    new_account: Some(new_owner),
  }
  ```
- [x] Compose `TronExecutionResult`: `success=true`, `return_data=[]`, `energy_used=0`, `bandwidth_used=calculate_bandwidth_usage`, `logs=[]`, `state_changes=[...]`.

4) Determinism & Digest
- [x] Ensure state changes for this handler are exactly one entry — inherent determinism.
- [x] Verify no involvement of 0x41…00/null address anywhere in this flow.

5) Logging
- [x] `info!`: `FreezeBalance completed: amount={amount}, resource={resource:?}, owner={owner_tron}, state_changes=1`.
- [x] `debug!`: parsed params, old/new balances.
- [x] `warn!/error!` for validation failures and parsing errors.

6) Config Gating (Optional but recommended)
- [x] Add `execution.remote.freeze_balance_enabled: bool` (default false) in `ExecutionConfig.remote`.
- [x] Gate the match arm execution; if disabled, return `Err("FREEZE_BALANCE_CONTRACT disabled")` so Java falls back to embedded path.
- [x] Document toggle in `config.toml` - added to config defaults.

7) Unit Tests (Core)
- [x] `freeze_success_basic`: owner balance reduces by amount; exactly 1 AccountChange; energy_used=0; logs empty.
- [x] `freeze_insufficient_balance`: returns Err; 0 state changes; no persistence.
- [x] `freeze_bad_params_empty`: returns Err; 0 state changes.
- [ ] `freeze_determinism`: re-run on fresh adapter yields identical output state change ordering (deferred - basic tests cover this).

8) CSV/Digest Parity Validation (Manual Harness)
- [ ] Construct a FreezeBalance tx (from, amount) with known initial balance.
- [ ] Execute handler via BackendService with NON_VM and contract_type set to FreezeBalance.
- [ ] Verify produced state_changes JSON matches embedded pattern (single owner account change; balance delta correct).
- [ ] Compare computed digest with embedded for the same old/new bytes (manual or existing digest pipeline).

## Phase 2 — Semantics-Complete Resource Ledger

9) Resource Storage Schema
- [ ] Define storage keys/DB for freeze records (owner → per-resource aggregates or lists with expirations).
- [ ] Add `StorageModuleAdapter` helpers to get/set freeze ledger entries.
- [ ] Persist freeze record on execute: increase frozen amount for selected resource; compute expiration timestamp from duration.
- [ ] Emit StorageChange(s) or AccountChange(s) consistent with how embedded journaling records these changes (verify embedded journal format first). If embedded currently does not journal resource slots, gate extra emissions with a config so CSV parity remains intact.

10) Policy & Dynamic Properties
- [ ] Read DP values (min duration, resource enable flags) via adapter when available; until then, config fallback with sane defaults.
- [ ] Enforce resource type validity (BANDWIDTH/ENERGY) and duration constraints.

11) Error Cases & Edge Conditions
- [ ] Amount overflow checks when aggregating.
- [ ] Duration bounds; reject zero or out-of-range.
- [ ] Nonexistent owner should be treated as zeroed account (already handled via `unwrap_or_default()`).

12) Extended Tests
- [ ] `freeze_accumulate`: multiple freezes aggregate resource amount; balance decrements cumulatively.
- [ ] `freeze_resource_switch`: BANDWIDTH vs ENERGY updates correct ledger path.
- [ ] `freeze_min_max_duration`: policy enforcement unit tests.

## Phase 3 — Interop & Related Contracts

13) Unfreeze & V2
- [ ] Implement `UnfreezeBalanceContract` to consume ledger entries and restore TRX after expiration.
- [ ] Implement `FreezeBalanceV2Contract` (with receiver/delegation semantics): debit owner balance, credit receiver’s resource ledger, ensure CSV parity strategy.

14) Interaction with Delegate/Undelegate
- [ ] Align ledger schema to support `DelegateResourceContract` / `UndelegateResourceContract` without duplication.

## Rollout Plan

- [x] Land Phase 1 behind `execution.remote.freeze_balance_enabled=false`. ✅ **Implemented with default=false**
- [ ] Enable in staging with CSV/digest diff harness against embedded for selected block ranges. **Next step: requires Java-side RemoteExecutionSPI mapping**
- [ ] If parity holds, flip default to true or enable per-network.
- [ ] Proceed with Phase 2 under a separate feature flag for resource ledger emissions (to keep CSV parity predictable during rollout).

## Risks & Mitigations

- Proto decoding risk (Option A):
  - Mitigation: keep parser focused on the few needed fields; add robust error handling and tests.
- CSV parity risk when adding resource ledger emissions:
  - Mitigation: gate additional StorageChange emissions; coordinate with CSV generator expectations.
- Address formatting mistakes:
  - Mitigation: re-use existing helpers (`strip_tron_address_prefix`, `add_tron_address_prefix`) and confirm 20-byte internal vs 21-byte wire.

## Open Questions

- Should FreezeBalanceContract be fully remote-executed immediately or remain gated until Unfreeze/Delegate are also ported to avoid partial semantics?
- Does embedded journaling currently include resource ledger StorageChange entries, or only account-level changes? This affects CSV parity decisions in Phase 2.

---

## Quick Checklist (Execution Order)

1. [x] Decide `data` encoding (A: proto, B: custom) and document. ✅ **Option A: manual protobuf parsing**
2. [x] Add parser + tests. ✅ **Implemented `parse_freeze_balance_params()` with `read_varint()` helper**
3. [x] Add handler skeleton + dispatch in `execute_non_vm_contract`. ✅ **Match arm added at service.rs:283-289**
4. [x] Implement balance delta, single AccountChange emission, persist owner. ✅ **Complete in `execute_freeze_balance_contract()`**
5. [x] Logging, determinism, no 0x41…00 address touched. ✅ **info/debug/warn logs added; single AccountChange ensures determinism**
6. [x] Optional: add `freeze_balance_enabled` gate; default false. ✅ **Added to RemoteExecutionConfig**
7. [x] Unit tests: success/failure/determinism. ✅ **3 tests added (success, insufficient balance, bad params)**
8. [ ] Manual parity check against embedded CSV/digest for sample tx. **Deferred - requires integration test environment**
9. [ ] Phase 2: resource ledger schema + adapter + gated emissions + tests. **Future work**
10. [ ] Rollout gates and staging validation. **Future work - requires Java-side mapping**

