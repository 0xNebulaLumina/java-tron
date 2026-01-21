# TODO / Fix Plan: `MARKET_CANCEL_ORDER_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in
`planning/review_again/MARKET_CANCEL_ORDER_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [ ] Confirm what “parity” must mean for Market cancel:
  - [ ] correct **state** under normal invariants (what fixtures cover)
  - [ ] strict **failure behavior** parity when indexes are missing/corrupt
  - [ ] support for `ALLOW_SAME_TOKEN_NAME == 0` (legacy) vs only modern mode
  - [ ] exact **error strings/order** parity vs “close enough”
- [ ] Confirm whether we want “defensive recovery” (Rust succeeds when indexes are missing) or “strict” (fail like Java).
  - [ ] If defensive recovery is desired, document it as an intentional deviation and stop here.

## 1) Add Java-like `Any.is(...)` validation (low risk, improves parity)

Goal: mirror `MarketCancelOrderActuator.validate` contract-type check when `contract_parameter` is available.

- [ ] In `execute_market_cancel_order_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [ ] If `transaction.metadata.contract_parameter` exists, validate `type_url == "protocol.MarketCancelOrderContract"`
  - [ ] If mismatch, return the Java-style error string:
    - `contract type error,expected type [MarketCancelOrderContract],real type[class com.google.protobuf.Any]`
- [ ] Add/extend a conformance fixture for “wrong Any type” if the harness supports it.

## 2) Decide and implement missing-index behavior (strict parity vs recovery)

Java will fail if these are missing (via `ItemNotFoundException`):

- `MarketAccountStore.get(owner)` inside `MarketUtils.updateOrderState(...)`
- `MarketPairPriceToOrderStore.get(pairPriceKey)` in the actuator
- neighbor orders referenced by `prev/next` pointers

Rust currently treats them as optional and continues.

- [ ] Option A (strict parity):
  - [ ] Require `MarketAccountOrder` exists for an active order cancel:
    - [ ] `get_market_account_order(&owner)` → error if `None`
  - [ ] Require `MarketOrderIdList` exists for the order’s `pairPriceKey`:
    - [ ] `get_market_order_id_list(&pair_price_key)` → error if `None`
  - [ ] In linked-list removal:
    - [ ] if `prev_id` non-empty and `get_market_order(prev_id)` is `None`, return error
    - [ ] if `next_id` non-empty and `get_market_order(next_id)` is `None`, return error
  - [ ] Decide on the exact error message:
    - [ ] match Java’s `ItemNotFoundException` messages (preferred if fixtures/assertions depend on it)
    - [ ] otherwise, choose stable Rust errors and update expectations
- [ ] Option B (recovery / more robust than Java):
  - [ ] Keep the current optional behavior, but:
    - [ ] emit a warning log when `market_account` or `market_pair_price_to_order` entry is missing for an ACTIVE order
    - [ ] consider writing a “repair” routine (out of scope for contract execution) to rebuild indexes offline

## 3) Align TRC-10 refund semantics with `addAssetAmountV2` (only if legacy mode must work)

Goal: mirror Java `AccountCapsule.addAssetAmountV2(...)` behavior.

- [ ] If `ALLOW_SAME_TOKEN_NAME == 1` only:
  - [ ] ensure the key used is the numeric id string (current behavior is likely sufficient)
- [ ] If `ALLOW_SAME_TOKEN_NAME == 0` must be supported:
  - [ ] implement name-keyed updates for `Account.asset[name]`
  - [ ] map name → id via asset-issue store and also update `Account.asset_v2[id]`
  - [ ] ensure asset-optimization (`AccountAssetStore` hydration) semantics are respected if enabled

## 4) Make key construction behavior match Java more closely (optional)

Goal: avoid truncation differences for invalid token ids.

- [ ] In `create_pair_key` / `create_pair_price_key`:
  - [ ] return an error if token id length exceeds 19 bytes (instead of truncating)
  - [ ] ensure the error surfaces as a contract failure (Java would effectively crash)

## 5) Match Java’s “remove one occurrence” semantics (very edge-case)

Goal: if `MarketAccountOrder.orders` is corrupt and contains duplicates, mirror Java behavior.

- [ ] Replace `retain(|id| id != &order_id)` with “remove first occurrence only”
- [ ] Keep the single `count -= 1` behavior

## 6) Verification plan

- [ ] Run Rust conformance for Market cancel fixtures only (if/when a per-contract runner exists)
  - [ ] otherwise, run full conformance selectively in CI/nightly
- [ ] Ensure these fixture groups still pass:
  - [ ] `conformance/fixtures/market_cancel_order_contract/happy_*`
  - [ ] `conformance/fixtures/market_cancel_order_contract/edge_*`
  - [ ] `conformance/fixtures/market_cancel_order_contract/validate_fail_*`
- [ ] (Optional) Add a Rust unit test around linked-list removal invariants:
  - [ ] head/tail update
  - [ ] prev/next pointer clearing
  - [ ] missing neighbor behavior (strict vs recovery mode)

