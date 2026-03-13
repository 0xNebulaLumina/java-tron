# TODO / Fix Plan: `UPDATE_ASSET_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/UPDATE_ASSET_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [x] Confirm scope:
  - [x] **Actuator semantics parity** (match `UpdateAssetActuator.validate + execute`)
  - [x] **Remote executor semantics** (accept assumptions from upstream mapping; less strict)
- [x] Confirm whether **error ordering/message parity** matters (fixtures vs "consensus-only")
- [x] Confirm which execution mode(s) must work:
  - [x] **WriteMode.COMPUTE_ONLY** (Java applies changes)
  - [x] **WriteMode.PERSISTED** (Rust persists; Java mirrors via `touched_keys`)

## 1) Fix `ONE_DAY_NET_LIMIT` default parity (affects limit validation)

Goal: align Rust dynamic-property fallback with Java's `DynamicPropertiesStore` defaults.

- [x] In `rust-backend/crates/execution/src/storage_adapter/engine.rs`:
  - [x] Change `get_one_day_net_limit()` fallback default from `8_640_000_000` to `57_600_000_000`
  - [x] Add/adjust unit tests (or fixture-based tests) that cover "key missing" behavior
    - Updated `test_strict_get_one_day_net_limit_missing` to assert new default
    - Added `test_update_asset_one_day_net_limit_default_matches_java` test

## 2) Align validation ordering (error precedence parity)

Goal: match Java's validate() ordering:

1) address valid → 2) account exists → 3) assetIssuedName/ID + store existence → 4) url/description → 5) limits

- [x] In `execute_update_asset_contract()`:
  - [x] After selecting `asset_key`, perform the asset store existence lookup before URL/description/limit checks.
  - [x] Ensure error strings stay identical to Java:
    - [x] `"Asset is not existed in AssetIssueStore"`
    - [x] `"Asset is not existed in AssetIssueV2Store"`
- [x] Add a regression test that constructs a combined-bad case (e.g. missing asset + invalid URL) and asserts Java-order error choice.
  - Added `test_update_asset_error_precedence_missing_asset_and_invalid_url`

## 3) Align owner-address source with Java (or explicitly validate consistency)

Goal: remove the "contract owner field ignored" mismatch.

- [x] Decide:
  - [x] **Option A (strict parity)**: parse `UpdateAssetContract` bytes, read `owner_address`, and use it for validation + account lookup (as Java does).
  - [ ] ~~Option B (hybrid)~~
  - [ ] ~~Option C (status quo)~~
- [x] If Option A/B:
  - [x] Add tests where `from_raw` and embedded `owner_address` differ to ensure deterministic failure semantics.
    - `test_update_asset_invalid_owner_address_20_bytes` tests 20-byte address (Java rejects)
    - `test_update_asset_invalid_owner_address_wrong_prefix` tests wrong prefix

## 4) Match Java address-validity semantics

Goal: make Rust's "Invalid ownerAddress" conditions match `DecodeUtil.addressValid`.

- [x] Require 21-byte owner addresses for this contract (unless you explicitly decide to support 20-byte EVM forms).
- [x] Validate the prefix against `0x41` or `0xa0` (mainnet/testnet), matching Java behavior.
- [x] Keep error string: `"Invalid ownerAddress"`

## 5) Preserve per-store fields when updating both stores (legacy mode)

Goal: match Java's "update four fields in-place on each store entry" behavior.

- [x] In `allowSameTokenName == 0` path:
  - [x] Load legacy entry from `AssetIssueStore` (name key) and update only:
    - [x] `free_asset_net_limit`
    - [x] `public_free_asset_net_limit`
    - [x] `url`
    - [x] `description`
  - [x] Load V2 entry from `AssetIssueV2Store` (id key) and update only the same four fields
  - [x] Write each updated object back to its own store
- [x] Add a regression test where legacy and v2 entries differ in some unrelated field (e.g. `public_free_asset_net_usage`) and assert that an update preserves those per-store values (Java behavior).
  - Added `test_update_asset_happy_path_legacy_mode_preserves_per_store_fields`

## 6) Decide behavior for inconsistent issuer state in legacy mode

Goal: decide what to do when `ALLOW_SAME_TOKEN_NAME == 0` but `assetIssuedID` is missing.

- [x] Options:
  - [x] **Strict parity**: Java always loads V2 entry by `account.assetIssuedID` in execute — if it's empty, Java would fail. Rust now mirrors this: we always load V2 entry, and if `asset_issued_id` is empty, we get an error.
- [x] Added test coverage for no-asset-issued cases in both V2 and legacy modes.

## 7) Verification checklist

- [x] Rust:
  - [x] `cd rust-backend && cargo test --workspace` — 369 passed, 3 failed (pre-existing vote_witness failures)
  - [x] All 21 update_asset tests pass
  - [x] Run TRC-10 conformance runner — all fixtures pass
  - [x] `./scripts/ci/run_fixture_conformance.sh --rust-only` — all passed
