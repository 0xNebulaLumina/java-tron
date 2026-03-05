# TRIGGER_SMART_CONTRACT fix plan (todo/checklist)

## 0) Decide canonical wire encoding (must-do first)

- [x] Confirm the intended encoding for `backend.TronTransaction.data` when `contract_type == TRIGGER_SMART_CONTRACT`:
  - [ ] **Option A (fixture-style)**: `data = TriggerSmartContract.toByteArray()` (full proto); Rust extracts calldata.
  - [x] **Option B (RemoteExecutionSPI-style)**: `data = calldata`; full proto is carried in `contract_parameter`.
- [x] Document the decision in `framework/src/main/proto/backend.proto` (field comments for `data` + `contract_parameter`).
- [x] Add a short note to repo docs (or a `planning/` note) describing the canonical encoding and why.

## 1) Fix the Rust/Java mismatch (choose A or B; B is most backwards-compatible)

### Option A: change Java RemoteExecutionSPI to match fixtures / Rust today

- [ ] _(Not chosen — Option B implemented instead)_

### Option B: change Rust to accept the production RemoteExecutionSPI format (recommended)

- [x] Add a Rust helper to obtain TriggerSmartContract bytes:
  - [x] Prefer `tx.metadata.contract_parameter.value` when present and the type_url indicates TriggerSmartContract.
  - [x] Fallback to decoding from `tx.data` for conformance fixtures / legacy senders.
- [x] Update `rust-backend/crates/execution/src/lib.rs`:
  - [x] `validate_trigger_smart_contract(...)` decodes via the helper (not directly from `tx.data`).
- [x] Update `rust-backend/crates/execution/src/tron_evm.rs`:
  - [x] TriggerSmartContract branch decodes via the helper, so it can still set `env.tx.data` and handle `call_value < 0` even when `tx.data` is calldata.
- [x] Add Rust tests:
  - [x] "Fixture-style" request: `tx.data = TriggerSmartContract.toByteArray()` executes decode path and extracts calldata.
  - [x] "RemoteExecutionSPI-style" request: `tx.data = calldata`, `contract_parameter.value = TriggerSmartContract.toByteArray()` passes validation and results in calldata being used.

## 2) Parity follow-ups (separate commits; do after encoding is aligned)

- [x] Hardfork gate parity:
  - [x] Model `StorageUtils.getEnergyLimitHardFork()` equivalent in Rust (uses `LATEST_BLOCK_HEADER_NUMBER >= 4727890` dynamic property).
  - [x] Enforce `call_value >= 0` and `call_token_value >= 0` only when the fork is active.
- [ ] Implement TRC-10 call_token_value/token_id transfer semantics for TriggerSmartContract:
  - [ ] Mirror Java's "transfer token to contract before VM execution when enabled and tokenValue > 0".
  - [ ] Emit corresponding TRC-10 state changes so Java-side can apply them consistently.
  - _Note: Deferred — no conformance fixtures currently test TRC-10 pre-execution transfers for TriggerSmartContract. TRX callValue is handled by REVM's value field._
- [x] Verify TRX callValue semantics:
  - [x] Ensure callValue transfer happens only when `callValue > 0` (Java does not transfer on negative).
  - [x] Re-check behavior for malformed negative callValue fixtures (ensure no unintended balance minting).
  - _Verified: negative callValue is handled by `disable_balance_check = true` in setup_environment, resulting in REVERT. Conformance fixture `validate_fail_call_value_negative` passes._

## 3) Validation / regression matrix

- [ ] Run the Java conformance generator(s) that cover TriggerSmartContract fixtures.
- [x] Run Rust conformance runner against `trigger_smart_contract/*` fixtures.
- [x] Add/verify coverage for these cases (fixtures or targeted tests):
  - [x] happy path (storage write) — fixture `happy_path` passes
  - [x] view/constant call selector behavior — fixture `view_function` passes
  - [x] nonexistent contract → `"No contract or not a smart contract"` — fixture `edge_nonexistent_contract` passes
  - [x] feeLimit negative / feeLimit above max — fixtures `validate_fail_fee_limit_negative` and `validate_fail_fee_limit_above_max` pass
  - [x] tokenValue > 0 with tokenId == 0 (validate fail) — fixture `validate_fail_token_value_positive_token_id_zero` passes
  - [x] tokenId too small (validate fail) — fixture `validate_fail_token_id_too_small` passes
- [x] Added 5 unit tests for `decode_trigger_smart_contract` helper covering both encoding styles, precedence, fallback, and error cases.
