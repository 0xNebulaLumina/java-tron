# TODO / Fix Plan: `ASSET_ISSUE_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/ASSET_ISSUE_CONTRACT.planning.md`.

## 0) Decide "parity target" (do this first)

- [x] Confirm desired scope:
  - [x] **Actuator-only parity** (match `AssetIssueActuator.validate + execute`)
  - [ ] **End-to-end parity** (also match reporting/journaling expectations in remote mode)
- [x] Confirm network expectations:
  - [ ] mainnet only (`0x41`)
  - [ ] testnet only (`0xa0`)
  - [x] must enforce prefix strictly based on the DB/configured network
- [x] Confirm execution topology:
  - [x] "remote storage + remote execution" (shared dynamic properties) — Rust commits NON_VM writes with write_mode=PERSISTED, Java mirrors on PERSISTED (see RemoteExecutionSPI.java:992, mod.rs:1059/1226, RuntimeSpiImpl.java:93)
  - [ ] "remote execution only" (Java owns dynamic properties)

## 1) Address prefix strictness (match Java `DecodeUtil.addressValid`)

Goal: ownerAddress must be **21 bytes** and **prefix == configured prefix**.

- [x] Update `execute_asset_issue_contract()` in `rust-backend/crates/core/src/service/mod.rs`:
  - [x] Replace hardcoded `(0x41 || 0xa0)` owner prefix accept-list with validation against `storage_adapter.tron_address_prefix()`.
  - [x] Keep error message parity: `"Invalid ownerAddress"`.
- [x] Fix gRPC conversion prefixing:
  - [x] Added `add_tron_address_prefix_with(address, prefix)` variant that accepts configurable prefix.
  - [x] Added `validate_tron_address_prefix(address_bytes, expected_prefix)` for strict validation.
  - [ ] (Optional) Update conversion.rs to use DB prefix for all emitted addresses (not critical for actuator parity).
- [x] Add Rust tests:
  - [x] With a test DB prefixed `0x41`, a contract owner_address prefixed `0xa0` should fail with `"Invalid ownerAddress"`.
  - [ ] (Optional) With a test DB prefixed `0xa0`, emitted `Trc10AssetIssued.owner_address` should use `0xa0` prefix.

## 2) Stop validating on lossy strings (use raw bytes for validations + lookups)

Goal: mirror Java's byte-based validation (`TransactionUtil.validAssetName/validUrl/validAssetDescription`) and byte-keyed store lookups in legacy mode.

Options (pick one):

- [ ] **Option A (minimal)**: extend `parse_asset_issue_contract()` to return raw byte slices/Vecs for `name/abbr/url/description`, and keep strings only for logging.
- [x] **Option B (simpler overall)**: stop using the manual parser here; use the already-decoded `asset_proto` for all fields and derive:
  - [x] validation bytes from `asset_proto.name/abbr/url/description`
  - [x] legacy account `asset` map key from `String::from_utf8_lossy(asset_proto.name.as_slice())` (only at the final "write into map<string,…>" step)

Checklist:

- [x] Ensure the "trx" name ban in same-token-name mode uses Java-equivalent UTF-8 decoding semantics.
- [x] Ensure legacy "Token exists" lookup uses **exact name bytes** from the proto.
- [ ] Add tests for malformed UTF-8 name bytes:
  - [ ] expected validation failure matches Java ("Invalid assetName") and does not silently alter bytes used for lookups.

## 3) Make `Trc10Change::AssetIssued` self-contained (token_id emission)

Goal: reduce reliance on Java reading `TOKEN_ID_NUM` after execution.

- [x] Decide desired behavior:
  - [x] Always set `token_id = Some(token_id_str)` in Rust `Trc10AssetIssued`
  - [ ] Or gate it behind a config flag (e.g., "executor-only compatibility mode")
- [x] Update `execute_asset_issue_contract()` to populate `token_id`.
- [x] Add tests:
  - [x] `Trc10Change::AssetIssued.token_id` is present and matches the allocated id
  - [x] `TOKEN_ID_NUM` is persisted alongside token_id (guards future refactors; Java only increments TOKEN_ID_NUM when token_id is empty per RuntimeSpiImpl.java:700)
  - [ ] Java remote CSV extraction uses the provided token_id (no dynamicStore dependency)

## 4) Unify contract bytes source (`data` vs `contract_parameter.value`)

Goal: avoid accidental divergence if callers populate only one field.

- [x] In `execute_asset_issue_contract()`:
  - [x] Use `contract_bytes` consistently for both prost decode and minimal parsing.
- [ ] Add tests:
  - [ ] `transaction.data` empty + `metadata.contract_parameter` populated still executes correctly
  - [ ] both populated but different → define and enforce one source-of-truth (should likely reject)

## 5) Dynamic-property missing-key parity (optional but important)

Goal: decide whether Rust should match Java's "throw when missing" behavior or keep safe defaults.

- [ ] Identify which keys should be strict for this contract (likely):
  - [ ] `ASSET_ISSUE_FEE`
  - [ ] `TOKEN_ID_NUM`
  - [ ] `ALLOW_SAME_TOKEN_NAME`
  - [ ] `ONE_DAY_NET_LIMIT`
  - [ ] `MIN_FROZEN_SUPPLY_TIME`, `MAX_FROZEN_SUPPLY_TIME`, `MAX_FROZEN_SUPPLY_NUMBER`
- [ ] If choosing strict parity:
  - [ ] Change the corresponding getters in `rust-backend/crates/execution/src/storage_adapter/engine.rs` to error when absent (at least under conformance mode).
  - [ ] Add tests proving missing keys fail early with a clear error.

## 6) Verification steps

- [x] Rust:
  - [x] `cd rust-backend && cargo test` — All 14 asset_issue tests pass
  - [ ] Run any available conformance/fixture runner for AssetIssue cases (if present)
- [ ] Java (optional, if remote mode integration is under test):
  - [ ] `./gradlew :framework:test`
  - [ ] If dual-mode is relevant: `./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"`

---

## Implementation Summary (2026-01-30)

### Changes Made

1. **Address prefix strictness (Task 1)**:
   - Added `validate_tron_address_prefix(address_bytes, expected_prefix)` function in `address.rs`
   - Added `add_tron_address_prefix_with(address, prefix)` for configurable prefix
   - Updated `execute_asset_issue_contract()` to use `storage_adapter.tron_address_prefix()` for strict validation
   - Error message parity maintained: `"Invalid ownerAddress"`

2. **Raw bytes validation (Task 2)**:
   - Updated validation to use `asset_proto.name/abbr/url/description` (raw bytes) instead of lossy-decoded strings
   - `valid_asset_name()`, `valid_url()`, `valid_asset_description()` now receive raw bytes
   - Legacy "Token exists" lookup uses exact name bytes from proto
   - "trx" name ban uses lossy decode (matches Java's `new String(name).toLowerCase()`)

3. **Token ID emission (Task 3)**:
   - Changed `token_id: None` to `token_id: Some(token_id_str.clone())` in `Trc10Change::AssetIssued`
   - With shared Rust storage (write_mode=PERSISTED), Java mirrors the result; token_id in the change provides reporting/journaling parity without requiring Java to re-read TOKEN_ID_NUM from dynamicStore

4. **Unified contract bytes source (Task 4)**:
   - Changed `parse_asset_issue_contract()` to use `contract_bytes` (same as prost decode)
   - Both prost decode and manual parsing now use the same byte source

### Tests Added/Updated

- `test_asset_issue_validate_fail_wrong_address_prefix` — verifies 0xa0 prefix fails on 0x41 DB
- `test_asset_issue_token_id_populated_in_trc10_change` — verifies token_id is now `Some(...)`
- `test_asset_issue_token_id_num_persisted_alongside_token_id` — guards against future refactors that might emit token_id but forget to persist TOKEN_ID_NUM (Java only increments TOKEN_ID_NUM when token_id is empty per RuntimeSpiImpl.java:700)
- Updated `test_asset_issue_contract_trc10_change_emission` to check token_id is populated

All 15 asset issue contract tests pass.
