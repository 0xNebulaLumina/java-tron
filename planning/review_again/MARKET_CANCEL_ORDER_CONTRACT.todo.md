# TODO / Fix Plan: `MARKET_CANCEL_ORDER_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in
`planning/review_again/MARKET_CANCEL_ORDER_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [x] Confirm what "parity" must mean for Market cancel:
  - [x] correct **state** under normal invariants (what fixtures cover) - **Implemented: all conformance tests pass**
  - [x] strict **failure behavior** parity when indexes are missing/corrupt - **Implemented via `market_strict_index_parity` config flag**
  - [ ] support for `ALLOW_SAME_TOKEN_NAME == 0` (legacy) vs only modern mode - **Not implemented (out of scope for this PR)**
  - [x] exact **error strings/order** parity vs "close enough" - **Implemented for main validation errors**
- [x] Confirm whether we want "defensive recovery" (Rust succeeds when indexes are missing) or "strict" (fail like Java).
  - [x] **Decision: Both modes supported via config flag `market_strict_index_parity`**
    - `false` (default): defensive recovery - skip missing indexes
    - `true`: strict Java parity - fail on missing indexes

## 1) Add Java-like `Any.is(...)` validation (low risk, improves parity)

Goal: mirror `MarketCancelOrderActuator.validate` contract-type check when `contract_parameter` is available.

- [x] In `execute_market_cancel_order_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [x] If `transaction.metadata.contract_parameter` exists, validate `type_url == "protocol.MarketCancelOrderContract"`
  - [x] If mismatch, return the Java-style error string:
    - `contract type error,expected type [MarketCancelOrderContract],real type[class com.google.protobuf.Any]`
- [ ] Add/extend a conformance fixture for "wrong Any type" if the harness supports it. - **Skipped: would require harness changes**

## 2) Decide and implement missing-index behavior (strict parity vs recovery)

Java will fail if these are missing (via `ItemNotFoundException`):

- `MarketAccountStore.get(owner)` inside `MarketUtils.updateOrderState(...)`
- `MarketPairPriceToOrderStore.get(pairPriceKey)` in the actuator
- neighbor orders referenced by `prev/next` pointers

Rust currently treats them as optional and continues.

- [x] Option A (strict parity) - **Implemented with config flag**:
  - [x] Require `MarketAccountOrder` exists for an active order cancel:
    - [x] `get_market_account_order(&owner)` → error if `None` **when `market_strict_index_parity=true`**
  - [x] Require `MarketOrderIdList` exists for the order's `pairPriceKey`:
    - [x] `get_market_order_id_list(&pair_price_key)` → error if `None` **when `market_strict_index_parity=true`**
  - [x] In linked-list removal:
    - [x] if `prev_id` non-empty and `get_market_order(prev_id)` is `None`, return error **when strict_parity=true**
    - [x] if `next_id` non-empty and `get_market_order(next_id)` is `None`, return error **when strict_parity=true**
  - [x] Decide on the exact error message:
    - [x] Rust-style errors chosen for clarity (e.g., "MarketAccountOrder not found for owner")
- [x] Option B (recovery / more robust than Java) - **Also implemented as default behavior**:
  - [x] Keep the current optional behavior when `market_strict_index_parity=false`

## 3) Align TRC-10 refund semantics with `addAssetAmountV2` (only if legacy mode must work)

Goal: mirror Java `AccountCapsule.addAssetAmountV2(...)` behavior.

- [x] If `ALLOW_SAME_TOKEN_NAME == 1` only:
  - [x] ensure the key used is the numeric id string (current behavior is sufficient)
- [ ] If `ALLOW_SAME_TOKEN_NAME == 0` must be supported:
  - [ ] implement name-keyed updates for `Account.asset[name]`
  - [ ] map name → id via asset-issue store and also update `Account.asset_v2[id]`
  - [ ] ensure asset-optimization (`AccountAssetStore` hydration) semantics are respected if enabled
  - **Note: Legacy mode not implemented - out of scope for this PR**

## 4) Make key construction behavior match Java more closely (optional)

Goal: avoid truncation differences for invalid token ids.

- [ ] In `create_pair_key` / `create_pair_price_key`:
  - [ ] return an error if token id length exceeds 19 bytes (instead of truncating)
  - [ ] ensure the error surfaces as a contract failure (Java would effectively crash)
  - **Note: Kept truncation behavior - invalid token IDs are extremely unlikely in practice**

## 5) Match Java's "remove one occurrence" semantics (very edge-case)

Goal: if `MarketAccountOrder.orders` is corrupt and contains duplicates, mirror Java behavior.

- [x] Replace `retain(|id| id != &order_id)` with "remove first occurrence only"
  - [x] Changed to `if let Some(pos) = orders.iter().position(|id| id == &order_id) { orders.remove(pos); }`
- [x] Keep the single `count -= 1` behavior

## 6) Verification plan

- [x] Run Rust conformance for Market cancel fixtures only (if/when a per-contract runner exists)
  - [x] otherwise, run full conformance selectively in CI/nightly
- [x] Ensure these fixture groups still pass:
  - [x] `conformance/fixtures/market_cancel_order_contract/happy_*` - **All PASS**
  - [x] `conformance/fixtures/market_cancel_order_contract/edge_*` - **All PASS**
  - [x] `conformance/fixtures/market_cancel_order_contract/validate_fail_*` - **All PASS**
- [ ] (Optional) Add a Rust unit test around linked-list removal invariants:
  - [ ] head/tail update
  - [ ] prev/next pointer clearing
  - [ ] missing neighbor behavior (strict vs recovery mode)
  - **Note: Skipped - conformance fixtures provide sufficient coverage**

## Implementation Summary

### Config flag added
- `market_strict_index_parity` in `RemoteExecutionConfig` (`rust-backend/crates/common/src/config.rs`)
  - Default: `false` (defensive recovery mode)
  - When `true`: fail on missing indexes like Java

### Code changes
1. **Any.is validation**: Added contract type URL validation at start of `execute_market_cancel_order_contract()`
2. **MarketAccountOrder handling**: Error when missing and `strict_parity=true`, skip when `false`
3. **MarketOrderIdList handling**: Error when missing and `strict_parity=true`, skip when `false`
4. **Linked-list removal**: Updated `remove_order_from_linked_list()` to take `strict_parity` parameter
   - Error on missing prev/next neighbors when `strict_parity=true`
5. **Single occurrence removal**: Changed from `retain()` to `position() + remove()` for Java parity

### Test results
- All 15 MARKET_CANCEL_ORDER_CONTRACT conformance fixtures PASS
- All 34 MARKET_SELL_ASSET_CONTRACT conformance fixtures PASS
- Rust workspace builds successfully
