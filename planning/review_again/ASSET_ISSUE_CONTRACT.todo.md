# TODO / Fix Plan: `ASSET_ISSUE_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/ASSET_ISSUE_CONTRACT.planning.md`.

## 0) Decide “parity target” (do this first)

- [ ] Confirm desired scope:
  - [ ] **Actuator-only parity** (match `AssetIssueActuator.validate + execute`)
  - [ ] **End-to-end parity** (also match reporting/journaling expectations in remote mode)
- [ ] Confirm network expectations:
  - [ ] mainnet only (`0x41`)
  - [ ] testnet only (`0xa0`)
  - [ ] must enforce prefix strictly based on the DB/configured network
- [ ] Confirm execution topology:
  - [ ] “remote storage + remote execution” (shared dynamic properties)
  - [ ] “remote execution only” (Java owns dynamic properties) — affects `token_id` emission requirements

## 1) Address prefix strictness (match Java `DecodeUtil.addressValid`)

Goal: ownerAddress must be **21 bytes** and **prefix == configured prefix**.

- [ ] Update `execute_asset_issue_contract()` in `rust-backend/crates/core/src/service/mod.rs`:
  - [ ] Replace hardcoded `(0x41 || 0xa0)` owner prefix accept-list with validation against `storage_adapter.address_prefix()`.
  - [ ] Keep error message parity: `"Invalid ownerAddress"`.
- [ ] Fix gRPC conversion prefixing:
  - [ ] Replace `grpc::address::add_tron_address_prefix()` (currently hardcoded `0x41`) with a variant that uses the DB prefix.
  - [ ] Ensure emitted TRC-10 change addresses and receipt-related addresses preserve testnet prefixes.
- [ ] Add Rust tests:
  - [ ] With a test DB prefixed `0x41`, a contract owner_address prefixed `0xa0` should fail with `"Invalid ownerAddress"`.
  - [ ] With a test DB prefixed `0xa0`, emitted `Trc10AssetIssued.owner_address` should use `0xa0` prefix (not `0x41`).

## 2) Stop validating on lossy strings (use raw bytes for validations + lookups)

Goal: mirror Java’s byte-based validation (`TransactionUtil.validAssetName/validUrl/validAssetDescription`) and byte-keyed store lookups in legacy mode.

Options (pick one):

- [ ] **Option A (minimal)**: extend `parse_asset_issue_contract()` to return raw byte slices/Vecs for `name/abbr/url/description`, and keep strings only for logging.
- [ ] **Option B (simpler overall)**: stop using the manual parser here; use the already-decoded `asset_proto` for all fields and derive:
  - [ ] validation bytes from `asset_proto.name/abbr/url/description`
  - [ ] legacy account `asset` map key from `String::from_utf8_lossy(asset_proto.name.as_slice())` (only at the final “write into map<string,…>” step)

Checklist:

- [ ] Ensure the “trx” name ban in same-token-name mode uses Java-equivalent UTF-8 decoding semantics.
- [ ] Ensure legacy “Token exists” lookup uses **exact name bytes** from the proto.
- [ ] Add tests for malformed UTF-8 name bytes:
  - [ ] expected validation failure matches Java (“Invalid assetName”) and does not silently alter bytes used for lookups.

## 3) Make `Trc10Change::AssetIssued` self-contained (token_id emission)

Goal: reduce reliance on Java reading `TOKEN_ID_NUM` after execution.

- [ ] Decide desired behavior:
  - [ ] Always set `token_id = Some(token_id_str)` in Rust `Trc10AssetIssued`
  - [ ] Or gate it behind a config flag (e.g., “executor-only compatibility mode”)
- [ ] Update `execute_asset_issue_contract()` to populate `token_id`.
- [ ] Add tests:
  - [ ] `Trc10Change::AssetIssued.token_id` is present and matches the allocated id
  - [ ] Java remote CSV extraction uses the provided token_id (no dynamicStore dependency)

## 4) Unify contract bytes source (`data` vs `contract_parameter.value`)

Goal: avoid accidental divergence if callers populate only one field.

- [ ] In `execute_asset_issue_contract()`:
  - [ ] Use `contract_bytes` consistently for both prost decode and minimal parsing (or eliminate the manual parser entirely as per 2.B).
- [ ] Add tests:
  - [ ] `transaction.data` empty + `metadata.contract_parameter` populated still executes correctly
  - [ ] both populated but different → define and enforce one source-of-truth (should likely reject)

## 5) Dynamic-property missing-key parity (optional but important)

Goal: decide whether Rust should match Java’s “throw when missing” behavior or keep safe defaults.

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

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] Run any available conformance/fixture runner for AssetIssue cases (if present)
- [ ] Java (optional, if remote mode integration is under test):
  - [ ] `./gradlew :framework:test`
  - [ ] If dual-mode is relevant: `./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"`

