# TODO / Fix Plan: `ACCOUNT_UPDATE_CONTRACT` parity gaps

This checklist targets the parity risks identified in `planning/review_again/ACCOUNT_UPDATE_CONTRACT.planning.md`.

## 0) Confirm parity target (do this first)

- [ ] Confirm desired scope:
  - [ ] **Actuator-only parity** (match `UpdateAccountActuator` validation + execution)
  - [ ] **End-to-end parity** (also match receipt/resource/bandwidth semantics where observable)
- [ ] Confirm expected trust model for remote execution requests:
  - [ ] Java always shapes `from/data` correctly (then Rust can trust `transaction.from` + `transaction.data`)
  - [ ] Rust must be robust to inconsistent/malformed inputs (then unpack/validate `contract_parameter.value`)

## 1) Tighten owner address validation to match Java

Goal: match `DecodeUtil.addressValid(ownerAddress)` exactly.

- [ ] In `BackendService::execute_account_update_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [ ] Replace `0x41 || 0xa0` allowlist with `storage_adapter.address_prefix()`
  - [ ] Require `from_raw.len() == 21` (reject 20-byte owner addresses for this contract)
  - [ ] Preserve the Java error string: `Invalid ownerAddress`
- [ ] Add/adjust unit tests (Rust):
  - [ ] `from_raw = []` → `Invalid ownerAddress`
  - [ ] `from_raw = 20 bytes` → `Invalid ownerAddress` (Java rejects wrong length)
  - [ ] `from_raw = 21 bytes` with wrong prefix → `Invalid ownerAddress`
  - [ ] `from_raw = 21 bytes` with correct prefix → pass this validation step

Notes:

- If other system contracts are also meant to mirror `DecodeUtil.addressValid`, consider applying the same strictness consistently (but keep this change scoped unless you explicitly want broader parity tightening).

## 2) Add contract-parameter unpack parity (recommended)

Goal: mirror Java’s `any.unpack(AccountUpdateContract.class)` behavior and reduce coupling to `transaction.data`.

- [ ] Parse `AccountUpdateContract` from `transaction.metadata.contract_parameter.value` when present:
  - [ ] Extract `owner_address` and `account_name` from the decoded message
  - [ ] Validate:
    - [ ] decoded `owner_address` matches `from_raw` (byte-equal) when both exist
    - [ ] decoded `account_name` matches `transaction.data` (byte-equal) or switch source-of-truth to decoded field
  - [ ] If protobuf decode fails, return a validation error consistent with Java’s `InvalidProtocolBufferException` messaging (or, if strict message match is not required, at least fail deterministically before any writes)
- [ ] Update the handler to use a single canonical source for `name_bytes` (prefer decoded proto for less coupling).

## 3) Audit state-change parity expectation (quick verification)

Goal: ensure `ExecutionResult.state_changes` behavior matches embedded recording used by conformance/CSV.

- [ ] Verify via the existing fixture flow that embedded expects exactly:
  - [ ] one no-op owner `AccountChange` (old == new)
  - [ ] no “zero address” changes
- [ ] If mismatch observed:
  - [ ] adjust emission count/order/determinism for AccountUpdateContract only (keep other contracts unchanged)

## 4) Update stale Rust tests for AccountUpdateContract

Current `rust-backend/crates/core/src/service/tests/contracts.rs` tests for AccountUpdateContract appear inconsistent with Java:

- They assume empty name is invalid and a 32-byte max, which does not match `TransactionUtil.validAccountName`.
- Some assertions on `state_changes.len()` are self-contradictory.

Fix plan:

- [ ] Rewrite/replace AccountUpdateContract tests to reflect Java semantics:
  - [ ] allow empty name
  - [ ] allow up to 200 bytes; reject 201 bytes
  - [ ] enforce “only set once” only when `ALLOW_UPDATE_ACCOUNT_NAME == 0`
  - [ ] allow repeated updates when `ALLOW_UPDATE_ACCOUNT_NAME == 1`
  - [ ] duplicate-name constraint only when updates are disabled (`ALLOW_UPDATE_ACCOUNT_NAME == 0`)
  - [ ] confirm correct error strings for all validate-fail branches
- [ ] Ensure tests set dynamic properties explicitly where needed (don’t rely on implicit defaults unless that’s part of the behavior being tested).

## 5) Verification

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] Run any conformance runner cases involving `ACCOUNT_UPDATE_CONTRACT` (if the repo has a harness command/script)
- [ ] Java (only if integration behavior changes):
  - [ ] `./gradlew :framework:test --tests \"org.tron.core.conformance.CoreAccountFixtureGeneratorTest\"`
  - [ ] Execute a remote-vs-embedded CSV comparison run that includes AccountUpdateContract transactions

