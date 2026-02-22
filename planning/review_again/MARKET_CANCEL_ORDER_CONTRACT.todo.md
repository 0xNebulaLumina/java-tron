# TODO / Fix Plan: `MARKET_CANCEL_ORDER_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in
`planning/review_again/MARKET_CANCEL_ORDER_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [x] Confirm what "parity" must mean for Market cancel:
  - [x] correct **state** under normal invariants (what fixtures cover)
  - [x] strict **failure behavior** parity when indexes are missing/corrupt → **Decision: keep defensive recovery mode (intentional deviation)**
  - [x] support for `ALLOW_SAME_TOKEN_NAME == 0` (legacy) vs only modern mode → **Decision: modern mode only (legacy not required)**
  - [x] exact **error strings/order** parity vs "close enough" → **Decision: exact error strings required**
- [x] Confirm whether we want "defensive recovery" (Rust succeeds when indexes are missing) or "strict" (fail like Java).
  - [x] **Decision: defensive recovery is desired - documented as intentional deviation.** Rust is more robust when market auxiliary indexes are missing/corrupt. This is acceptable because:
    1. Normal invariants are maintained by the system
    2. All conformance fixtures pass
    3. The deviation only affects corrupted state scenarios that "should never happen"

## 1) Add Java-like `Any.is(...)` validation (low risk, improves parity)

Goal: mirror `MarketCancelOrderActuator.validate` contract-type check when `contract_parameter` is available.

- [x] In `execute_market_cancel_order_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [x] If `transaction.metadata.contract_parameter` exists, validate `type_url == "protocol.MarketCancelOrderContract"`
  - [x] If mismatch, return the Java-style error string:
    - `contract type error,expected type [MarketCancelOrderContract],real type[class com.google.protobuf.Any]`
- [ ] Add/extend a conformance fixture for "wrong Any type" if the harness supports it. → **Skipped: no existing fixture pattern for this; existing fixtures validate the behavior implicitly**

## 2) Decide and implement missing-index behavior (strict parity vs recovery)

Java will fail if these are missing (via `ItemNotFoundException`):

- `MarketAccountStore.get(owner)` inside `MarketUtils.updateOrderState(...)`
- `MarketPairPriceToOrderStore.get(pairPriceKey)` in the actuator
- neighbor orders referenced by `prev/next` pointers

Rust currently treats them as optional and continues.

- [ ] Option A (strict parity): **Not selected**
  - [ ] Require `MarketAccountOrder` exists for an active order cancel:
    - [ ] `get_market_account_order(&owner)` → error if `None`
  - [ ] Require `MarketOrderIdList` exists for the order's `pairPriceKey`:
    - [ ] `get_market_order_id_list(&pair_price_key)` → error if `None`
  - [ ] In linked-list removal:
    - [ ] if `prev_id` non-empty and `get_market_order(prev_id)` is `None`, return error
    - [ ] if `next_id` non-empty and `get_market_order(next_id)` is `None`, return error
  - [ ] Decide on the exact error message:
    - [ ] match Java's `ItemNotFoundException` messages (preferred if fixtures/assertions depend on it)
    - [ ] otherwise, choose stable Rust errors and update expectations
- [x] Option B (recovery / more robust than Java): **Selected**
  - [x] Keep the current optional behavior
  - [x] **Documented as intentional deviation** in section 0 above
  - [ ] emit a warning log when `market_account` or `market_pair_price_to_order` entry is missing for an ACTIVE order → **Skipped: not required for correctness**
  - [ ] consider writing a "repair" routine (out of scope for contract execution) to rebuild indexes offline → **Out of scope**

## 3) Align TRC-10 refund semantics with `addAssetAmountV2` (only if legacy mode must work)

Goal: mirror Java `AccountCapsule.addAssetAmountV2(...)` behavior.

- [x] If `ALLOW_SAME_TOKEN_NAME == 1` only:
  - [x] ensure the key used is the numeric id string (current behavior is sufficient)
- [ ] If `ALLOW_SAME_TOKEN_NAME == 0` must be supported: → **Not required (modern mode only)**
  - [ ] implement name-keyed updates for `Account.asset[name]`
  - [ ] map name → id via asset-issue store and also update `Account.asset_v2[id]`
  - [ ] ensure asset-optimization (`AccountAssetStore` hydration) semantics are respected if enabled

## 4) Make key construction behavior match Java more closely (optional)

Goal: avoid truncation differences for invalid token ids.

- [ ] In `create_pair_key` / `create_pair_price_key`: → **Skipped: edge case for malformed data only**
  - [ ] return an error if token id length exceeds 19 bytes (instead of truncating)
  - [ ] ensure the error surfaces as a contract failure (Java would effectively crash)

**Note:** This is an intentional deviation. Rust truncates token IDs >19 bytes rather than crashing like Java. This only matters for malformed state that can't occur under normal operation.

## 5) Match Java's "remove one occurrence" semantics (very edge-case)

Goal: if `MarketAccountOrder.orders` is corrupt and contains duplicates, mirror Java behavior.

- [x] Replace `retain(|id| id != &order_id)` with "remove first occurrence only"
- [x] Keep the single `count -= 1` behavior

## 6) Verification plan

- [x] Run Rust conformance for Market cancel fixtures only (if/when a per-contract runner exists)
  - [x] Full conformance run passed (all 15 MARKET_CANCEL_ORDER_CONTRACT fixtures pass)
- [x] Ensure these fixture groups still pass:
  - [x] `conformance/fixtures/market_cancel_order_contract/happy_*` (2/2 pass)
  - [x] `conformance/fixtures/market_cancel_order_contract/edge_*` (4/4 pass)
  - [x] `conformance/fixtures/market_cancel_order_contract/validate_fail_*` (9/9 pass)
- [ ] (Optional) Add a Rust unit test around linked-list removal invariants: → **Skipped: conformance fixtures cover these scenarios**
  - [ ] head/tail update
  - [ ] prev/next pointer clearing
  - [ ] missing neighbor behavior (strict vs recovery mode)

---

## Summary of Changes Made

1. **Added `Any.is(...)` validation** (section 1): Added contract type validation at the start of `execute_market_cancel_order_contract()` to match Java's `MarketCancelOrderActuator.validate()` behavior.

2. **Fixed duplicate order-id removal semantics** (section 5): Changed from `retain(|id| id != &order_id)` (removes all occurrences) to `position().remove()` (removes first occurrence only) to match Java's `List.remove(orderId)` behavior.

3. **Documented intentional deviations** (sections 0, 2, 4):
   - Defensive recovery mode when market indexes are missing (more robust than Java)
   - Token ID truncation vs crash for >19 byte IDs
   - Modern mode only (no legacy `ALLOW_SAME_TOKEN_NAME == 0` support)
