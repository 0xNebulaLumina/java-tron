# TODO / Fix Plan: `UNFREEZE_ASSET_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/UNFREEZE_ASSET_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [ ] Confirm scope:
  - [ ] **Actuator semantics parity** (match `UnfreezeAssetActuator.validate + execute`)
  - [ ] **Remote executor semantics** (accept assumptions from upstream mapping; less strict)
- [ ] Confirm whether error ordering/message parity matters (fixtures vs “consensus-only”)
- [ ] Confirm which execution mode(s) must work:
  - [ ] **WriteMode.COMPUTE_ONLY** (Java applies changes)
  - [ ] **WriteMode.PERSISTED** (Rust persists; Java mirrors via `touched_keys`)

## 1) Add Java-equivalent `Any` contract-type validation

Goal: match Java’s early failure behavior (`any.is(UnfreezeAssetContract.class)`).

- [ ] In `execute_unfreeze_asset_contract()`:
  - [ ] If `transaction.metadata.contract_parameter` is present:
    - [ ] Enforce `any_type_url_matches(type_url, "protocol.UnfreezeAssetContract")`
    - [ ] On mismatch: return the Java-equivalent string:
      - [ ] `"contract type error, expected type [UnfreezeAssetContract], real type[class com.google.protobuf.Any]"`
- [ ] Add Rust unit tests that cover:
  - [ ] wrong `type_url` → contract-type error (and ensure it happens before address/account errors)
  - [ ] missing `contract_parameter` (should still validate via owner address, or define expected behavior)

## 2) Align owner-address source with Java (or explicitly validate consistency)

Goal: remove the “owner address comes from different place” risk.

- [ ] Decide:
  - [ ] **Option A (strict parity)**: parse `UnfreezeAssetContract` bytes from `contract_parameter.value` and use `owner_address` from the proto as Java does.
  - [ ] **Option B (hybrid)**: keep using `from_raw` but parse proto and assert `owner_proto == from_raw` (define error message).
  - [ ] **Option C (status quo)**: document the assumption that `from_raw` is always derived from `owner_address`.
- [ ] Add regression tests for mismatch cases if Option A/B is chosen.

## 3) Match Java’s `AssetIssueStore` dependency and error ordering

Goal: avoid stricter-than-Java failures and align error precedence.

- [ ] Reorder validation to match Java:
  - [ ] address valid
  - [ ] account exists
  - [ ] frozen supply non-empty
  - [ ] issued asset name/id non-empty
  - [ ] expired entry exists (`allowedUnfreezeCount > 0`)
  - [ ] only then consult asset-issue store as needed
- [ ] Restrict asset-issue lookup to where Java actually requires it:
  - [ ] If `ALLOW_SAME_TOKEN_NAME == 0`: lookup is required to map name → tokenId (parity with `addAssetAmountV2`)
  - [ ] If `ALLOW_SAME_TOKEN_NAME == 1`: avoid mandatory lookup; use `assetIssuedID` bytes directly as tokenId string
- [ ] Add tests for missing asset-issue entry:
  - [ ] Decide expected behavior in both allowSameTokenName modes and assert it

## 4) Decide overflow parity for `unfreezeAsset` summation

Goal: either match Java’s unchecked `long` sum or document Rust as stricter.

- [ ] Decide policy:
  - [ ] **Strict safety**: keep `checked_add` and document divergence
  - [ ] **Exact parity**: use wrapping arithmetic for the sum (and rely on later checks) to emulate Java
- [ ] Add a dedicated test for overflow behavior (even if synthetic) so it’s intentional.

## 5) Make UnfreezeAsset state changes applyable in Java compute-only mode

Goal: prevent Java DB drift when remote execution is enabled with `WriteMode.COMPUTE_ONLY`.

- [ ] Decide how Java should learn about:
  - [ ] `Account.frozenSupply` list mutation
  - [ ] issuer TRC-10 balance credit
- [ ] Options:
  - [ ] **Option A (recommended)**: extend the TRC-10 change model:
    - [ ] Add a new proto + Rust enum variant (e.g., `Trc10AssetUnfrozen` or a generic `Trc10BalanceAdjusted`)
    - [ ] Teach Java `RuntimeSpiImpl.applyTrc10Changes(...)` to apply it to `AccountStore`
  - [ ] **Option B**: add a new “account proto patch” sidecar for system contracts
  - [ ] **Option C**: require `WriteMode.PERSISTED` + `touched_keys` for this contract and gate it otherwise
- [ ] Add an end-to-end integration test (Java+Rust) covering UnfreezeAsset in compute-only mode to ensure:
  - [ ] Java DB state matches embedded actuator result after applying remote result

## 6) Verification checklist

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] Run any existing TRC-10 conformance/fixture runner that includes UnfreezeAsset (if present)
- [ ] Java:
  - [ ] `./gradlew :framework:test --tests \"org.tron.core.actuator.UnfreezeAssetActuatorTest\"` (if such a test exists)
  - [ ] If validating remote exec: run a focused integration test suite for TRC-10 remote execution
