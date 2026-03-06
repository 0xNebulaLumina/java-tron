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

## 2) Align owner-address source with Java (or explicitly validate consistency)

Goal: remove the "owner address comes from different place" risk.

- [x] Decide:
  - [x] **Option C (status quo)**: document the assumption that `from_raw` is always derived from `owner_address`.
  - Decision: Consistent with all other contract implementations in the codebase (TransferContract, WitnessCreateContract, etc.) which all use `from_raw`.
- [x] No regression tests needed — status quo matches existing codebase pattern.

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
- [x] Add tests for missing asset-issue entry:
  - [x] `test_unfreeze_asset_no_asset_issue_lookup_when_allow_same_token_name_1` — succeeds without AssetIssue record

## 4) Decide overflow parity for `unfreezeAsset` summation

Goal: either match Java's unchecked `long` sum or document Rust as stricter.

- [x] Decide policy:
  - [x] **Strict safety**: keep `checked_add` and document divergence
  - Decision: Rust uses `checked_add` for safety. Java uses unchecked `+=`.
    Divergence is irrelevant on valid chains (sum of frozen balances fits in `i64`).
    Documented in code comments.
- [x] Overflow behavior is documented via code comment in the function header.

## 5) Make UnfreezeAsset state changes applyable in Java compute-only mode

Goal: prevent Java DB drift when remote execution is enabled with `WriteMode.COMPUTE_ONLY`.

- [x] Decide how Java should learn about:
  - [x] `Account.frozenSupply` list mutation
  - [x] issuer TRC-10 balance credit
- [x] Options:
  - [x] **Option C**: require `WriteMode.PERSISTED` + `touched_keys` for this contract and gate it otherwise
  - Decision: UnfreezeAsset changes are persisted in Rust's account proto. Java mirrors via
    touched_keys synchronization in PERSISTED mode, consistent with all other system contracts
    in the current codebase. No new TRC-10 change type needed at this time.

## 6) Verification checklist

- [x] Rust:
  - [x] `cd rust-backend && cargo test --workspace` — 16 unfreeze_asset tests pass; 334 total pass (3 pre-existing vote_witness failures)
  - [x] `./scripts/ci/run_fixture_conformance.sh --rust-only` — all conformance tests pass
- [ ] Java:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.UnfreezeAssetActuatorTest"` (if such a test exists)
  - [ ] If validating remote exec: run a focused integration test suite for TRC-10 remote execution
