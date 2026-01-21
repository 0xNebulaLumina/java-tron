# TODO / Fix Plan: `MARKET_SELL_ASSET_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in
`planning/review_again/MARKET_SELL_ASSET_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [ ] Confirm what “parity” must mean for Market sell:
  - [ ] correct **state** under normal invariants (what current fixtures cover)
  - [ ] strict **fee accounting** parity (burn vs blackhole + `BURN_TRX_AMOUNT`)
  - [ ] strict **failure behavior** parity when indexes are missing/corrupt
  - [ ] support for `ALLOW_SAME_TOKEN_NAME == 0` (legacy) vs only modern mode
  - [ ] exact **receipt contents** parity (include `orderDetails[]` or not)
- [ ] Decide whether Rust should be “strict like Java” (fail on missing indexes) or “defensive” (try to continue).

## 1) Fix fee handling parity (high priority)

Goal: match Java `MarketSellAssetActuator.execute` fee behavior.

- [ ] In `execute_market_sell_asset_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [ ] Replace config-driven `fee_config.support_black_hole_optimization` branching with:
    - [ ] `storage_adapter.support_black_hole_optimization()` (reads `ALLOW_BLACKHOLE_OPTIMIZATION`)
  - [ ] If `support_blackhole == true` and `fee > 0`:
    - [ ] call `storage_adapter.burn_trx(fee as u64)` (updates `BURN_TRX_AMOUNT`)
  - [ ] Else (no optimization) and `fee > 0`:
    - [ ] credit blackhole via `storage_adapter.add_balance(&blackhole, fee as u64)`
  - [ ] Ensure fee deltas are consistent for REVERT cases (java-tron may still persist the fee).
- [ ] Add coverage (fixtures or unit tests):
  - [ ] New conformance fixture where:
    - [ ] `MARKET_SELL_FEE > 0`
    - [ ] `ALLOW_BLACKHOLE_OPTIMIZATION == 1`
    - [ ] expected post-state increments `BURN_TRX_AMOUNT` (dynamic-properties.kv differs)
  - [ ] New conformance fixture where:
    - [ ] `MARKET_SELL_FEE > 0`
    - [ ] `ALLOW_BLACKHOLE_OPTIMIZATION == 0`
    - [ ] expected post-state credits blackhole balance

## 2) Owner-address parsing parity (medium priority)

Goal: mirror Java validation that uses the protobuf `owner_address` field.

- [ ] Extend `parse_market_sell_asset_contract()` to decode `owner_address` (field 1) instead of skipping it.
- [ ] Validate `DecodeUtil.addressValid(owner_address)` exactly like Java:
  - [ ] 21 bytes
  - [ ] correct prefix
- [ ] Decide whether to require consistency:
  - [ ] `owner_address` must match `transaction.metadata.from_raw`
  - [ ] and/or must match `transaction.from` when converted to 21-byte TRON address
- [ ] Decide error string (keep `"Invalid address"` if any mismatch should fail).

## 3) Decide and implement missing-index behavior (strict parity vs recovery)

Java will fail if expected market indexes are missing.

Rust currently succeeds in some cases (e.g. missing `MarketOrderIdList` for a price key).

- [ ] Option A (strict parity):
  - [ ] In `match_market_sell_order()`:
    - [ ] if `get_market_order_id_list(pair_price_key) == None`, return an error (not `Ok(())`)
  - [ ] In `market_update_order_state()`:
    - [ ] if removing an order from account list and `MarketAccountOrder` is missing, return an error
  - [ ] Decide stable error strings (prefer Java parity if tests/fixtures depend on it).
- [ ] Option B (defensive recovery):
  - [ ] Keep permissive behavior but document it as intentional deviation.
  - [ ] Add warning logs when invariants are violated (missing list, missing account-order store, etc.).

## 4) Align TRC-10 balance semantics with Java (only if legacy mode must work)

Goal: mirror `reduceAssetAmountV2/addAssetAmountV2` behavior.

- [ ] If `ALLOW_SAME_TOKEN_NAME == 1` only:
  - [ ] ensure the numeric id string map (`account.asset_v2`) is sufficient
- [ ] If `ALLOW_SAME_TOKEN_NAME == 0` must be supported:
  - [ ] update the legacy `Account.asset` name-keyed map in addition to `asset_v2`
  - [ ] use asset-issue store mapping where needed (name ↔ id)

## 5) Make key/id construction behavior match Java more closely (optional)

Goal: avoid truncation differences for malformed token ids.

- [ ] In `create_pair_key`, `create_pair_price_key`, and `calculate_order_id`:
  - [ ] return an error if `token_id.len() > 19` (instead of truncating)
  - [ ] ensure the error surfaces as contract failure (Java would crash on array copy)

## 6) Receipt parity: add `orderDetails[]` (optional / product-driven)

If full receipt parity matters:

- [ ] Track fills in Rust during `market_match_single_order()`:
  - [ ] collect maker/taker order ids + fill amounts (sell/buy)
- [ ] Extend `TransactionResultBuilder` usage to include `orderDetails[]`
- [ ] Add/extend a conformance fixture that asserts `expected/result.pb` includes order details

## 7) Verification plan

- [ ] Run Rust conformance fixtures for Market sell:
  - [ ] `conformance/fixtures/market_sell_asset_contract/happy_*`
  - [ ] `conformance/fixtures/market_sell_asset_contract/edge_*`
  - [ ] `conformance/fixtures/market_sell_asset_contract/validate_fail_*`
- [ ] Add the new fee-focused fixtures from step 1 and ensure they fail before the fix and pass after.

