# TODO / Fix Plan: `UNFREEZE_ASSET_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/UNFREEZE_ASSET_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [x] Confirm scope:
  - [x] **Actuator semantics parity** (match `UnfreezeAssetActuator.validate + execute`)
  - [x] **Remote executor semantics** (accept assumptions from upstream mapping; less strict)
- [x] Confirm whether error ordering/message parity matters (fixtures vs "consensus-only")
  - Decision: Yes, validation order matches Java exactly.
- [x] Confirm which execution mode(s) must work:
  - [x] **WriteMode.COMPUTE_ONLY** (Java applies changes)
  - [x] **WriteMode.PERSISTED** (Rust persists; Java mirrors via `touched_keys`)

## 1) Add Java-equivalent `Any` contract-type validation

Goal: match Java's early failure behavior (`any.is(UnfreezeAssetContract.class)`).

- [x] In `execute_unfreeze_asset_contract()`:
  - [x] If `transaction.metadata.contract_parameter` is present:
    - [x] Enforce `any_type_url_matches(type_url, "protocol.UnfreezeAssetContract")`
    - [x] On mismatch: return the Java-equivalent string:
      - [x] `"contract type error, expected type [UnfreezeAssetContract], real type[class com.google.protobuf.Any]"`
- [x] Add Rust unit tests that cover:
  - [x] wrong `type_url` → contract-type error (and ensure it happens before address/account errors)
  - [x] correct `type_url` with googleapis prefix passes type check

## 2) Parse owner_address from contract bytes (Java parity with `any.unpack()`)

Goal: match Java's behavior of extracting `ownerAddress` from the unpacked protobuf.

- [x] Parse `UnfreezeAssetContract` bytes from `contract_parameter.value` (falling back to `transaction.data`)
- [x] Extract `owner_address` (field 1) via `parse_unfreeze_asset_owner_address()`
- [x] Use parsed owner_address for validation; fall back to `from_raw` only if proto field is empty
- [x] Add unit tests:
  - [x] `test_parse_unfreeze_asset_owner_address` — verifies proto parsing
  - [x] `test_parse_unfreeze_asset_owner_address_empty` — empty data returns empty vec
  - [x] `test_unfreeze_asset_owner_from_proto_preferred_over_from_raw` — proto address takes precedence
  - [x] `test_unfreeze_asset_invalid_address_wrong_length_in_proto` — proto address validated
  - [x] `test_unfreeze_asset_invalid_address_wrong_prefix_in_proto` — proto prefix validated

## 3) Match Java's `AssetIssueStore` dependency and error ordering

Goal: avoid stricter-than-Java failures and align error precedence.

- [x] Reorder validation to match Java:
  - [x] address valid
  - [x] account exists
  - [x] frozen supply non-empty
  - [x] issued asset name/id non-empty
  - [x] expired entry exists (`allowedUnfreezeCount > 0`)
  - [x] only then consult asset-issue store as needed
- [x] Restrict asset-issue lookup to where Java actually requires it:
  - [x] If `ALLOW_SAME_TOKEN_NAME == 0`: lookup is required to map name → tokenId (parity with `addAssetAmountV2`)
  - [x] If `ALLOW_SAME_TOKEN_NAME == 1`: avoid mandatory lookup; use `assetIssuedID` bytes directly as tokenId string
- [x] In legacy mode, use `asset_issue.id` directly (no fallback to asset_key) — matches Java's `assetIssueCapsule.getId()`
- [x] Add tests for missing asset-issue entry:
  - [x] `test_unfreeze_asset_no_asset_issue_lookup_when_allow_same_token_name_1` — succeeds without AssetIssue record

## 4) Match overflow parity for `unfreezeAsset` summation

Goal: match Java's exact overflow semantics.

- [x] Use `wrapping_add` for frozen-balance summation (matches Java's unchecked `+=`)
- [x] Overflow is caught later in `add_asset_amount_v2` via `checked_add` (matches Java's `addExact` → `ArithmeticException`)
- [x] Add test: `test_unfreeze_asset_wrapping_add_for_summation` — verifies wrapping behavior

## 5) Implement `importAsset` behavior for ALLOW_ASSET_OPTIMIZATION

Goal: match Java's `AccountCapsule.importAsset(key)` called inside `addAssetAmountV2`.

- [x] Call `import_asset_if_optimized()` before `add_asset_amount_v2` to load asset balances from `AccountAssetStore` when `ALLOW_ASSET_OPTIMIZATION == 1 && ALLOW_SAME_TOKEN_NAME == 1`
- [x] Follows the same pattern as ExchangeCreate, ExchangeInject, and other contracts that already call this

## 6) Make UnfreezeAsset state changes applyable in Java compute-only mode

Goal: prevent Java DB drift when remote execution is enabled with `WriteMode.COMPUTE_ONLY`.

- [x] Decision: `WriteMode.PERSISTED` + `touched_keys` for this contract, consistent with all other system contracts. No new TRC-10 change type needed at this time.

## 7) Verification checklist

- [x] Rust:
  - [x] `cd rust-backend && cargo test --workspace` — 20 unfreeze_asset tests pass; 338 total pass (3 pre-existing vote_witness failures)
  - [x] `./scripts/ci/run_fixture_conformance.sh --rust-only` — all conformance tests pass
- [ ] Java:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.UnfreezeAssetActuatorTest"` (if such a test exists)
  - [ ] If validating remote exec: run a focused integration test suite for TRC-10 remote execution
