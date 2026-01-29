# TODO / Fix Plan: `ACCOUNT_UPDATE_CONTRACT` parity gaps

This checklist targets the parity risks identified in `planning/review_again/ACCOUNT_UPDATE_CONTRACT.planning.md`.

## 0) Confirm parity target (do this first)

- [x] Confirm desired scope:
  - [x] **Actuator-only parity** (match `UpdateAccountActuator` validation + execution) ✓ Implemented
  - [ ] **End-to-end parity** (also match receipt/resource/bandwidth semantics where observable)
- [x] Confirm expected trust model for remote execution requests:
  - [x] Java always shapes `from/data` correctly (then Rust can trust `transaction.from` + `transaction.data`) ✓ Current approach

## 1) Tighten owner address validation to match Java

Goal: match `DecodeUtil.addressValid(ownerAddress)` exactly.

- [x] In `BackendService::execute_account_update_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [x] Replace `0x41 || 0xa0` allowlist with `storage_adapter.address_prefix()` ✓ Done
  - [x] Require `from_raw.len() == 21` (reject 20-byte owner addresses for this contract) ✓ Done
  - [x] Preserve the Java error string: `Invalid ownerAddress` ✓ Done
- [x] Add/adjust unit tests (Rust):
  - [x] `from_raw = None` → `Invalid ownerAddress` ✓ `test_account_update_rejects_missing_from_raw`
  - [x] `from_raw = 20 bytes` → `Invalid ownerAddress` ✓ `test_account_update_rejects_20_byte_address`
  - [x] `from_raw = 21 bytes` with wrong prefix → `Invalid ownerAddress` ✓ `test_account_update_rejects_wrong_prefix`
  - [x] `from_raw = 21 bytes` with correct prefix → pass this validation step ✓ `test_account_update_happy_path_with_valid_from_raw`

Notes:

- If other system contracts are also meant to mirror `DecodeUtil.addressValid`, consider applying the same strictness consistently (but keep this change scoped unless you explicitly want broader parity tightening).

## 2) Add contract-parameter unpack parity (recommended)

Goal: mirror Java's `any.unpack(AccountUpdateContract.class)` behavior and reduce coupling to `transaction.data`.

- [ ] Parse `AccountUpdateContract` from `transaction.metadata.contract_parameter.value` when present:
  - [ ] Extract `owner_address` and `account_name` from the decoded message
  - [ ] Validate:
    - [ ] decoded `owner_address` matches `from_raw` (byte-equal) when both exist
    - [ ] decoded `account_name` matches `transaction.data` (byte-equal) or switch source-of-truth to decoded field
  - [ ] If protobuf decode fails, return a validation error consistent with Java's `InvalidProtocolBufferException` messaging (or, if strict message match is not required, at least fail deterministically before any writes)
- [ ] Update the handler to use a single canonical source for `name_bytes` (prefer decoded proto for less coupling).

**Status**: Not implemented yet. Current approach trusts Java-shaped `from/data` fields.

## 3) Audit state-change parity expectation (quick verification)

Goal: ensure `ExecutionResult.state_changes` behavior matches embedded recording used by conformance/CSV.

- [ ] Verify via the existing fixture flow that embedded expects exactly:
  - [ ] one no-op owner `AccountChange` (old == new)
  - [ ] no "zero address" changes
- [ ] If mismatch observed:
  - [ ] adjust emission count/order/determinism for AccountUpdateContract only (keep other contracts unchanged)

**Status**: Not verified yet.

## 4) Update stale Rust tests for AccountUpdateContract

Current `rust-backend/crates/core/src/service/tests/contracts/account_update.rs` tests have been completely rewritten.

Fix plan:

- [x] Rewrite/replace AccountUpdateContract tests to reflect Java semantics:
  - [x] allow empty name ✓ `test_account_update_allows_empty_name`
  - [x] allow up to 200 bytes; reject 201 bytes ✓ `test_account_update_allows_200_byte_name`, `test_account_update_rejects_201_byte_name`
  - [x] enforce "only set once" only when `ALLOW_UPDATE_ACCOUNT_NAME == 0` ✓ `test_account_update_only_set_once_when_updates_disabled`
  - [x] allow repeated updates when `ALLOW_UPDATE_ACCOUNT_NAME == 1` ✓ `test_account_update_allows_repeated_updates_when_enabled`
  - [x] duplicate-name constraint only when updates are disabled (`ALLOW_UPDATE_ACCOUNT_NAME == 0`) ✓ `test_account_update_duplicate_name_check_when_updates_disabled`, `test_account_update_duplicate_name_allowed_when_updates_enabled`
  - [x] confirm correct error strings for all validate-fail branches ✓ All tests verify exact Java error strings
- [x] Ensure tests set dynamic properties explicitly where needed ✓ Tests explicitly seed `ALLOW_UPDATE_ACCOUNT_NAME`

## 5) Verification

- [x] Rust:
  - [x] `cd rust-backend && cargo test` (AccountUpdateContract tests) ✓ All 12 tests pass
  - [ ] Run any conformance runner cases involving `ACCOUNT_UPDATE_CONTRACT` (if the repo has a harness command/script)
- [ ] Java (only if integration behavior changes):
  - [ ] `./gradlew :framework:test --tests \"org.tron.core.conformance.CoreAccountFixtureGeneratorTest\"`
  - [ ] Execute a remote-vs-embedded CSV comparison run that includes AccountUpdateContract transactions

---

## Summary of Changes Made

1. **Owner address validation tightened** (`mod.rs:1992-2007`):
   - Now requires `from_raw` to be present (no longer optional for this contract)
   - Requires exactly 21 bytes
   - Requires prefix byte to match `storage_adapter.address_prefix()` (no longer accepts both `0x41` and `0xa0`)
   - Matches Java's `DecodeUtil.addressValid()` exactly

2. **Tests completely rewritten** (`account_update.rs`):
   - 12 comprehensive tests covering all validation branches
   - Tests explicitly seed `ALLOW_UPDATE_ACCOUNT_NAME` dynamic property
   - Tests use `make_from_raw()` helper for proper 21-byte TRON addresses
   - All error string assertions match Java exactly
