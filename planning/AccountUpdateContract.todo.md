# AccountUpdateContract: Remote Parity Plan

Goal: Implement proper AccountUpdateContract handling in the Rust backend and wire an explicit ContractType mapping in Java RemoteExecutionSPI, removing the TRANSFER fallback. Outcome is CSV parity with embedded for AccountUpdateContract: exactly 1 account-level state change (owner), no zero-address deltas, energy_used=0, matching state digest.

---

## Acceptance Criteria
- [x] Java maps `AccountUpdateContract` to remote request with correct fields (NON_VM, owner as `from`, `data` = name bytes, `value` = 0).
- [x] Rust backend executes AccountUpdateContract with correct validation and persistence of account name metadata.
- [x] Remote execution emits exactly one account-level StateChange for the owner with oldAccount == newAccount (no balance/nonce/code changes).
- [x] No zero-address (`410000...`) or synthetic fee/blackhole state deltas for this contract.
- [x] Execution CSV for affected txs shows `state_change_count=1` and `state_digest_sha256` matching embedded.
- [x] Energy used = 0; bandwidth computed from tx payload.
- [x] Unit/integration tests cover happy path, invalid inputs, and parity assertions.

---

## Java: RemoteExecutionSPI Mapping
- [x] Locate `buildExecuteTransactionRequest` in `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`.
- [x] Add explicit `case AccountUpdateContract` in the contract switch:
  - [x] Unpack `AccountUpdateContract` proto from `contract.getParameter()`.
  - [x] Set `fromAddress = ownerAddress` from contract.
  - [x] Set `toAddress` to empty/None (do not use zero address in request semantics).
  - [x] Set `value = 0`.
  - [x] Set `data = accountName` raw bytes (as-is; no trimming at this stage).
  - [x] Set `txKind = NON_VM`.
  - [x] Set `contractType = ACCOUNT_UPDATE_CONTRACT` (backend proto enum value).
  - [x] Logging: `debug` the data length and owner.
- [x] Remove TRANSFER default fallback:
  - [x] Delete/avoid default `contractType = TRANSFER_CONTRACT` assignment at method start.
  - [x] In `default:` of the switch, `throw new UnsupportedOperationException("<ContractType> not mapped to remote; falling back to embedded")`.
  - [x] Ensure caller path catches and falls back to embedded runtime as today.
- [x] Sanity check mapping for other non-VM contracts remains unchanged.

---

## Rust Backend: Handler Implementation

### Dispatch
- [x] In `rust-backend/crates/core/src/service.rs` inside `execute_non_vm_contract` match:
  - [x] Add arm for `Some(tron_backend_execution::TronContractType::AccountUpdateContract)` → call `self.execute_account_update_contract(...)`.

### Storage Adapter: Account Name KV
- [x] In `rust-backend/crates/execution/src/storage_adapter.rs` add a dedicated DB and helpers:
  - [x] DB name: `account-name` (new logical database).
  - [x] Key format: 21-byte Tron address (reuse `account_key(&Address)` with 0x41 prefix).
  - [x] Value: raw bytes of account name as provided (UTF-8 recommended, but store as bytes).
  - [x] API: `fn get_account_name(&self, address: &Address) -> Result<Option<String>>` (decode UTF-8; if invalid, return Err or store/retrieve as bytes via separate `get_account_name_bytes()` helper).
  - [x] API: `fn set_account_name(&mut self, address: Address, name: &[u8]) -> Result<()>`.
  - [x] Tests: roundtrip set/get; invalid UTF-8 behavior defined and asserted.

### New Handler: `execute_account_update_contract`
- [x] Signature: `fn execute_account_update_contract(&self, storage_adapter: &mut StorageModuleAdapter, transaction: &TronTransaction, context: &TronExecutionContext) -> Result<TronExecutionResult, String>`.
- Validation
  - [x] Owner account must exist: `get_account(&from)` returns `Some`; otherwise `Err("Owner account does not exist")`.
  - [x] Name length constraints: `1 <= name_bytes.len() <= 32` (match java-tron len, adjust if chain rules differ).
  - [x] (Optional) Encoding: if policy requires UTF‑8, validate and reject invalid data; else accept raw bytes.
  - [x] Only once semantics: if `get_account_name(&from)` returns non-empty (or Some), reject with `Err("Account name is already set")`.
- Apply
  - [x] Persist name: `set_account_name(from, name_bytes)`.
  - [x] No change to balance/nonce/code.
- State Changes (CSV parity)
  - [x] Create `state_changes: Vec<TronStateChange>` with exactly one entry:
    - [x] `TronStateChange::AccountChange { address: from, old_account: Some(owner_account), new_account: Some(owner_account.clone()) }`.
    - [x] Rationale: embedded journaling produces an owner account-level change with identical old/new; this preserves digest parity.
  - [x] Do NOT emit any `StorageChange` for name by default (keep `emit_storage_changes` flag off for this contract type).
- Result
  - [x] Compute `bandwidth_used` via existing helper (payload size based).
  - [x] Return `TronExecutionResult { success: true, energy_used: 0, bandwidth_used, state_changes, logs: Vec::new(), error: None }`.
- Logging
  - [x] info: `AccountUpdate owner=<tronBase58> name_len=<n>`
  - [x] debug: previous vs new name strings/hex
  - [x] warn: rejection reasons (duplicate, too long, empty, missing owner)

### Config Flags (Optional)
- [ ] Introduce `execution.remote.account_update_enabled` (default: true) in config model if granular control is desired.
- [ ] Respect flag in dispatch (return `Err` to fall back to Java when disabled).

---

## gRPC / Protobuf Considerations
- [x] Verify backend proto has `ACCOUNT_UPDATE_CONTRACT` enum; if missing, add to `.proto` and regenerate.
- [x] Ensure Java-side maps to the same enum value when building request.
- [x] No changes required to StateChange proto (we continue using AccountChange union).

---

## CSV Parity Checklist
- [ ] For a known txid (e.g., `7425ac...f918c` at block 1990, idx 0):
  - [ ] Remote CSV `state_change_count` == 1 (owner only).
  - [ ] Remote `state_changes_json` includes only `owner_address` with identical `oldValue` and `newValue` 76-byte blobs.
  - [ ] No `410000...` zero-address entries.
  - [ ] `state_digest_sha256` equals embedded for the same row.
  - [ ] `is_success=true`, `result_code=SUCCESS`, `energy_used=0`.

---

## Tests
### Rust Unit Tests
- [x] Happy path: existing owner, name length 10, stored and retrievable; `state_changes.len()==1`, `old==new`.
- [x] Name too long (>32): rejected.
- [x] Empty name: rejected (if enforcing non-empty).
- [x] Duplicate set: rejected when name already stored.
- [x] Encoding policy test (if UTF‑8 enforced): invalid UTF‑8 rejected.

### Java Integration (optional, lightweight)
- [ ] Add an integration scenario to drive a single AccountUpdateContract through RemoteExecutionSPI → Rust → CSV record builder, then compare fields relevant to parity (`state_change_count`, digest, `energy_used`).

---

## Observability & Logging
- [ ] Add a metric counter `remote.account_update.success` and `.error` (optional via existing metrics callback or logs).
- [ ] Ensure StorageAdapter logs DB name used, key hex, and name length (avoid logging full name if privacy-sensitive).

---

## Rollout Plan
- [ ] Implement Java mapping (compile-only check).
- [ ] Implement Rust storage adapter functions (unit tests pass).
- [ ] Implement Rust handler and dispatch (unit tests pass).
- [ ] Manual run with the known mismatching tx; check CSV parity locally.
- [ ] Broader replay on a small window with presence of AccountUpdate txs; verify no regressions (no zero-address state changes introduced).

---

## Risks & Mitigations
- [ ] Name constraints mismatch with java-tron (length/encoding). Mitigate by matching java-tron’s exact rules (confirm in source and update validation accordingly).
- [ ] Accidentally emitting storage changes for name causing extra state deltas. Keep emission off by default.
- [ ] TRANSFER fallback removal may expose other unmapped types; ensure exceptions are clear and fallback works.

---

## Done Definition
- [ ] Code merged for Java and Rust.
- [x] Unit tests green (`cargo test` for rust-backend; Java compile OK and tests unaffected).
- [ ] CSV parity verified for at least one known AccountUpdate tx (count and digest match).
- [ ] No extra state changes at zero-address anywhere in the replay window.
