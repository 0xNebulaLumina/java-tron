# TODO / Fix Plan: `UPDATE_ASSET_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/UPDATE_ASSET_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [ ] Confirm scope:
  - [ ] **Actuator semantics parity** (match `UpdateAssetActuator.validate + execute`)
  - [ ] **Remote executor semantics** (accept assumptions from upstream mapping; less strict)
- [ ] Confirm whether **error ordering/message parity** matters (fixtures vs “consensus-only”)
- [ ] Confirm which execution mode(s) must work:
  - [ ] **WriteMode.COMPUTE_ONLY** (Java applies changes)
  - [ ] **WriteMode.PERSISTED** (Rust persists; Java mirrors via `touched_keys`)

## 1) Fix `ONE_DAY_NET_LIMIT` default parity (affects limit validation)

Goal: align Rust dynamic-property fallback with Java’s `DynamicPropertiesStore` defaults.

- [ ] In `rust-backend/crates/execution/src/storage_adapter/engine.rs`:
  - [ ] Change `get_one_day_net_limit()` fallback default from `8_640_000_000` to `57_600_000_000`
  - [ ] Add/adjust unit tests (or fixture-based tests) that cover “key missing” behavior

## 2) Align validation ordering (error precedence parity)

Goal: match Java’s validate() ordering:

1) address valid → 2) account exists → 3) assetIssuedName/ID + store existence → 4) url/description → 5) limits

- [ ] In `execute_update_asset_contract()`:
  - [ ] After selecting `asset_key`, perform the asset store existence lookup before URL/description/limit checks.
  - [ ] Ensure error strings stay identical to Java:
    - [ ] `"Asset is not existed in AssetIssueStore"`
    - [ ] `"Asset is not existed in AssetIssueV2Store"`
- [ ] Add a regression test that constructs a combined-bad case (e.g. missing asset + invalid URL) and asserts Java-order error choice.

## 3) Align owner-address source with Java (or explicitly validate consistency)

Goal: remove the “contract owner field ignored” mismatch.

- [ ] Decide:
  - [ ] **Option A (strict parity)**: parse `UpdateAssetContract` bytes, read `owner_address`, and use it for validation + account lookup (as Java does).
  - [ ] **Option B (hybrid)**: keep using `from_raw`/`transaction.from` but parse proto and assert `owner_address == from_raw` (define error handling).
  - [ ] **Option C (status quo)**: document assumption that mapping guarantees equality.
- [ ] If Option A/B:
  - [ ] Add tests where `from_raw` and embedded `owner_address` differ to ensure deterministic failure semantics.

## 4) Match Java address-validity semantics

Goal: make Rust’s “Invalid ownerAddress” conditions match `DecodeUtil.addressValid`.

- [ ] Require 21-byte owner addresses for this contract (unless you explicitly decide to support 20-byte EVM forms).
- [ ] Validate the prefix against the detected DB prefix (`storage_adapter.address_prefix()`), not hard-coded `{0x41, 0xa0}`.
- [ ] Keep error string: `"Invalid ownerAddress"`

## 5) Preserve per-store fields when updating both stores (legacy mode)

Goal: match Java’s “update four fields in-place on each store entry” behavior.

- [ ] In `allowSameTokenName == 0` path:
  - [ ] Load legacy entry from `AssetIssueStore` (name key) and update only:
    - [ ] `free_asset_net_limit`
    - [ ] `public_free_asset_net_limit`
    - [ ] `url`
    - [ ] `description`
  - [ ] Load V2 entry from `AssetIssueV2Store` (id key) and update only the same four fields
  - [ ] Write each updated object back to its own store
- [ ] Add a regression test where legacy and v2 entries differ in some unrelated field (e.g. `public_free_asset_net_usage`) and assert that an update preserves those per-store values (Java behavior).

## 6) Decide behavior for inconsistent issuer state in legacy mode

Goal: decide what to do when `ALLOW_SAME_TOKEN_NAME == 0` but `assetIssuedID` is missing.

- [ ] Options:
  - [ ] **Strict parity**: treat it as an invariant and return a deterministic error (even if Java would NPE).
  - [ ] **Robust mode**: update only the legacy store and skip V2 (document divergence).
  - [ ] **Hybrid**: if missing, attempt to recover the ID from the legacy asset entry’s `id` field and update V2 by that key.
- [ ] Add tests for the chosen behavior.

## 7) Verification checklist

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] Run any TRC-10 conformance runner that includes UpdateAsset fixtures (if present in this repo)
- [ ] Java:
  - [ ] `./gradlew :framework:test --tests \"org.tron.core.actuator.UpdateAssetActuatorTest\"`
  - [ ] If validating remote exec: run the TRC-10 remote execution integration suite (if present)

