# TODO / Fix Plan: `PARTICIPATE_ASSET_ISSUE_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/PARTICIPATE_ASSET_ISSUE_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [ ] Confirm scope:
  - [ ] **Actuator semantics parity** (match `ParticipateAssetIssueActuator.validate + execute`)
  - [ ] **“Remote executor” semantics** (allow EVM-style 20-byte addresses and rely on upstream validation)
- [ ] Confirm network prefix expectations:
  - [ ] mainnet only (`0x41`)
  - [ ] testnet only (`0xa0`)
  - [ ] prefix must be enforced based on DB/config, like Java’s `DecodeUtil.addressValid`
- [ ] Confirm whether edge-case error message parity matters (e.g., `asset_name == []` → `"null"` vs `""`)

## 1) Validate `owner_address` like Java (currently skipped)

Goal: match Java’s early failure behavior for malformed `owner_address`.

- [ ] Extend `parse_participate_asset_issue_contract()` to return `owner_address` bytes (field 1) instead of skipping.
- [ ] In `execute_participate_asset_issue_contract()`:
  - [ ] Validate `owner_address` with Java-equivalent rules (length == 21 and prefix == configured prefix).
  - [ ] Keep error string parity: `"Invalid ownerAddress"`.
  - [ ] Decide whether to enforce `owner_address` ↔ `transaction.from` consistency:
    - [ ] **Option A (strict)**: reject mismatch with a clear error (define expected message).
    - [ ] **Option B (Java-like)**: only validate format; leave signature/ownership mismatch to upstream.
- [ ] Add conformance-style tests in Rust for:
  - [ ] malformed `owner_address` (too short) → `"Invalid ownerAddress"`
  - [ ] wrong prefix (e.g., `0xa0` vs DB `0x41`) → `"Invalid ownerAddress"`

## 2) Validate `to_address` like Java (currently length-only)

Goal: match `DecodeUtil.addressValid(toAddress)` behavior.

- [ ] Replace the `len == 20 || len == 21` acceptance with strict validation:
  - [ ] Require 21 bytes with correct prefix (or enforce configured prefix).
  - [ ] Map to internal 20-byte address only after validation.
  - [ ] Keep error string parity: `"Invalid toAddress"`.
- [ ] Add tests in Rust for malformed `to_address` lengths and wrong-prefix addresses.

## 3) TRC-10 asset optimization support (`account-asset` DB)

Goal: ensure TRC-10 balance reads/writes match java-tron when `ALLOW_ASSET_OPTIMIZATION` is enabled.

- [ ] Decide if remote execution must support this feature:
  - [ ] If **no**, document the limitation and gate TRC-10 remote execution when the flag is enabled.
  - [ ] If **yes**, implement read/write support:
    - [ ] Add `account-asset` DB access in the Rust storage adapter (see Java: `AccountAssetStore`).
    - [ ] When reading balances (`get_asset_balance_v2`), if the account indicates optimized storage:
      - [ ] fall back to `account-asset` for missing token IDs (V2 mode)
    - [ ] When mutating balances, update both:
      - [ ] the account proto map (if Java would keep it updated)
      - [ ] and the `account-asset` DB (for optimized accounts)
- [ ] Add tests:
  - [ ] issuer balance present only in `account-asset` → Rust still validates and executes correctly
  - [ ] post-state matches Java for optimized accounts

## 4) Edge-case error message parity (`asset_name == []`)

Goal: decide whether to match Java’s `"No asset named null"` behavior.

- [ ] If strict parity is required:
  - [ ] Adjust the error message path to emulate `ByteArray.toStr([]) == null`
  - [ ] Add a regression test for empty `asset_name`
- [ ] If not required:
  - [ ] Document the difference as “non-consensus / message-only”

## 5) `token_id` empty handling

Goal: decide whether Rust should reject empty `asset_issue.id` or mirror Java’s implicit assumption.

- [ ] Confirm invariants on real DBs (does `AssetIssueContract.id` ever appear empty?).
- [ ] If strict Java parity is required:
  - [ ] Remove/relax `"token_id cannot be empty"` and align behavior with Java’s map-update logic.
- [ ] If safety is preferred:
  - [ ] Keep the check but document it as a stricter-than-Java invariant enforcement.

## 6) Verification steps

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] Run any existing conformance runner/fixture suite that exercises TRC-10 ParticipateAssetIssue (if present)
- [ ] Java (optional, if validating remote mode end-to-end):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.ParticipateAssetIssueActuatorTest"`

