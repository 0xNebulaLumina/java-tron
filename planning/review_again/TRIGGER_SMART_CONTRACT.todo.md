# TRIGGER_SMART_CONTRACT fix plan (todo/checklist)

## 0) Decide canonical wire encoding (must-do first)

- [ ] Confirm the intended encoding for `backend.TronTransaction.data` when `contract_type == TRIGGER_SMART_CONTRACT`:
  - [ ] **Option A (fixture-style)**: `data = TriggerSmartContract.toByteArray()` (full proto); Rust extracts calldata.
  - [ ] **Option B (RemoteExecutionSPI-style)**: `data = calldata`; full proto is carried in `contract_parameter`.
- [ ] Document the decision in `framework/src/main/proto/backend.proto` (field comments for `data` + `contract_parameter`).
- [ ] Add a short note to repo docs (or a `planning/` note) describing the canonical encoding and why.

## 1) Fix the Rust/Java mismatch (choose A or B; B is most backwards-compatible)

### Option A: change Java RemoteExecutionSPI to match fixtures / Rust today

- [ ] Update `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`:
  - [ ] For `case TriggerSmartContract`: set `data = triggerContract.toByteArray()` (not `triggerContract.getData()`).
  - [ ] Keep `toAddress = triggerContract.getContractAddress()` and `value = triggerContract.getCallValue()` unchanged.
  - [ ] Keep `.setContractParameter(contractParameter)` (optional, but useful for future parity).
- [ ] Add/extend a Java unit/integration test that asserts remote-built `ExecuteTransactionRequest` matches the fixture generator encoding for TriggerSmartContract.

### Option B: change Rust to accept the production RemoteExecutionSPI format (recommended)

- [ ] Add a Rust helper to obtain TriggerSmartContract bytes:
  - [ ] Prefer `tx.metadata.contract_parameter.value` when present and the type_url indicates TriggerSmartContract.
  - [ ] Fallback to decoding from `tx.data` for conformance fixtures / legacy senders.
- [ ] Update `rust-backend/crates/execution/src/lib.rs`:
  - [ ] `validate_trigger_smart_contract(...)` decodes via the helper (not directly from `tx.data`).
- [ ] Update `rust-backend/crates/execution/src/tron_evm.rs`:
  - [ ] TriggerSmartContract branch decodes via the helper, so it can still set `env.tx.data` and handle `call_value < 0` even when `tx.data` is calldata.
- [ ] Add Rust tests:
  - [ ] “Fixture-style” request: `tx.data = TriggerSmartContract.toByteArray()` executes decode path and extracts calldata.
  - [ ] “RemoteExecutionSPI-style” request: `tx.data = calldata`, `contract_parameter.value = TriggerSmartContract.toByteArray()` passes validation and results in calldata being used.

## 2) Parity follow-ups (separate commits; do after encoding is aligned)

- [ ] Hardfork gate parity:
  - [ ] Model `StorageUtils.getEnergyLimitHardFork()` equivalent in Rust (decide: dynamic property key vs spec_id vs config).
  - [ ] Enforce `call_value >= 0` and `call_token_value >= 0` only when the fork is active.
- [ ] Implement TRC-10 call_token_value/token_id transfer semantics for TriggerSmartContract:
  - [ ] Mirror Java’s “transfer token to contract before VM execution when enabled and tokenValue > 0”.
  - [ ] Emit corresponding TRC-10 state changes so Java-side can apply them consistently.
- [ ] Verify TRX callValue semantics:
  - [ ] Ensure callValue transfer happens only when `callValue > 0` (Java does not transfer on negative).
  - [ ] Re-check behavior for malformed negative callValue fixtures (ensure no unintended balance minting).

## 3) Validation / regression matrix

- [ ] Run the Java conformance generator(s) that cover TriggerSmartContract fixtures.
- [ ] Run Rust conformance runner against `trigger_smart_contract/*` fixtures.
- [ ] Add/verify coverage for these cases (fixtures or targeted tests):
  - [ ] happy path (storage write)
  - [ ] view/constant call selector behavior (if applicable to remote backend API)
  - [ ] nonexistent contract → `"No contract or not a smart contract"`
  - [ ] feeLimit negative / feeLimit above max
  - [ ] tokenValue > 0 with tokenId == 0 (validate fail)
  - [ ] tokenId too small (validate fail)

