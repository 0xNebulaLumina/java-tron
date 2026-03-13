# TODO / Fix Plan: `MARKET_SELL_ASSET_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in
`planning/review_again/MARKET_SELL_ASSET_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first) ✅ DONE

- [x] Confirm what "parity" must mean for Market sell:
  - [x] correct **state** under normal invariants (what current fixtures cover) ✅ Implemented
  - [x] strict **fee accounting** parity (burn vs blackhole + `BURN_TRX_AMOUNT`) ✅ Implemented
  - [x] strict **failure behavior** parity when indexes are missing/corrupt ✅ Configurable via `market_strict_index_parity`
  - [x] support for `ALLOW_SAME_TOKEN_NAME == 0` (legacy) vs only modern mode ⏭️ Skipped (mainnet uses modern mode)
  - [x] exact **receipt contents** parity (include `orderDetails[]` or not) ⏭️ Optional (product-driven)
- [x] Decide whether Rust should be "strict like Java" (fail on missing indexes) or "defensive" (try to continue).
  - Implemented as configurable: `market_strict_index_parity = true` for strict, `false` for defensive (default)

## 1) Fix fee handling parity (high priority) ✅ DONE

Goal: match Java `MarketSellAssetActuator.execute` fee behavior.

- [x] In `execute_market_sell_asset_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [x] Replace config-driven `fee_config.support_black_hole_optimization` branching with:
    - [x] `storage_adapter.support_black_hole_optimization()` (reads `ALLOW_BLACKHOLE_OPTIMIZATION`)
  - [x] If `support_blackhole == true` and `fee > 0`:
    - [x] call `storage_adapter.burn_trx(fee as u64)` (updates `BURN_TRX_AMOUNT`)
  - [x] Else (no optimization) and `fee > 0`:
    - [x] credit blackhole via `storage_adapter.add_balance(&blackhole, fee as u64)`
  - [x] Ensure fee deltas are consistent for REVERT cases (java-tron may still persist the fee).
- [x] Add coverage (fixtures or unit tests): ✅ DONE (unit tests in market_sell_asset.rs)
  - [x] Unit test: `test_market_sell_burns_fee_when_blackhole_optimization_enabled`
    - [x] `MARKET_SELL_FEE > 0`
    - [x] `ALLOW_BLACKHOLE_OPTIMIZATION == 1`
    - [x] asserts `BURN_TRX_AMOUNT` increases by fee
  - [x] Unit test: `test_market_sell_credits_blackhole_when_optimization_disabled`
    - [x] `MARKET_SELL_FEE > 0`
    - [x] `ALLOW_BLACKHOLE_OPTIMIZATION == 0`
    - [x] asserts blackhole balance increases, `BURN_TRX_AMOUNT` unchanged

## 2) Owner-address parsing parity (medium priority) ✅ DONE

Goal: mirror Java validation that uses the protobuf `owner_address` field.

- [x] Extend `parse_market_sell_asset_contract()` to decode `owner_address` (field 1) instead of skipping it.
- [x] Validate `DecodeUtil.addressValid(owner_address)` exactly like Java:
  - [x] 21 bytes
  - [x] correct prefix
- [x] Decide whether to require consistency:
  - [x] Use `owner_address` from contract protobuf directly (Java parity)
  - [x] Derive EVM address from parsed owner_address[1..21]
- [x] Decide error string (keep `"Invalid address"` if any mismatch should fail).

## 3) Decide and implement missing-index behavior (strict parity vs recovery) ✅ DONE

Java will fail if expected market indexes are missing.

Rust now supports both modes via `market_strict_index_parity` config flag.

- [x] Option A (strict parity) - enabled when `market_strict_index_parity = true`:
  - [x] In `match_market_sell_order()`:
    - [x] if `get_market_order_id_list(pair_price_key) == None`, return an error with descriptive message
  - [x] In `market_update_order_state()`:
    - [x] if removing an order from account list and `MarketAccountOrder` is missing, return an error
  - [x] Decide stable error strings (use descriptive messages with hex-encoded keys/addresses).
- [x] Option B (defensive recovery) - enabled when `market_strict_index_parity = false` (default):
  - [x] Keep permissive behavior as fallback
  - [x] No warning logs added (logs would add noise in production)

## 4) Align TRC-10 balance semantics with Java (only if legacy mode must work) ⏭️ SKIPPED

Goal: mirror `reduceAssetAmountV2/addAssetAmountV2` behavior.

- [x] If `ALLOW_SAME_TOKEN_NAME == 1` only:
  - [x] Current implementation uses `account.asset_v2` which is sufficient for modern mode
- [ ] If `ALLOW_SAME_TOKEN_NAME == 0` must be supported:
  - [ ] update the legacy `Account.asset` name-keyed map in addition to `asset_v2`
  - [ ] use asset-issue store mapping where needed (name ↔ id)

Note: Skipped because mainnet uses modern mode (`ALLOW_SAME_TOKEN_NAME == 1`).
If legacy mode support is ever needed, this requires extensive changes.

## 5) Make key/id construction behavior match Java more closely (optional) ✅ DONE

Goal: avoid truncation differences for malformed token ids.

- [x] In `create_pair_key`, `create_pair_price_key`, and `calculate_order_id`:
  - [x] `create_pair_key` already had validation
  - [x] `create_pair_price_key` already had validation
  - [x] `calculate_order_id` updated to return `Result<Vec<u8>, String>` and validate token ID lengths
  - [x] Error surfaces as contract failure (propagated via `?`)

## 6) Receipt parity: add `orderDetails[]` (optional / product-driven) ✅ DONE

- [x] Track fills in Rust during `market_match_single_order()`:
  - [x] collect maker/taker order ids + fill amounts (sell/buy)
  - [x] `market_match_single_order()` returns `Option<MarketOrderDetail>`
  - [x] `match_market_sell_order()` collects and returns `Vec<MarketOrderDetail>`
- [x] Extend `TransactionResultBuilder` usage to include `orderDetails[]`:
  - [x] Added `MarketOrderDetail` struct to `proto.rs`
  - [x] Added `order_details` field and methods to `TransactionResultBuilder`
  - [x] Receipt is built with order details in `execute_market_sell_asset_contract()`
- [x] Unit test: `test_market_sell_receipt_includes_order_details_on_match`

## 7) Verification plan

- [ ] Run Rust conformance fixtures for Market sell:
  - [ ] `conformance/fixtures/market_sell_asset_contract/happy_*`
  - [ ] `conformance/fixtures/market_sell_asset_contract/edge_*`
  - [ ] `conformance/fixtures/market_sell_asset_contract/validate_fail_*`
- [ ] Add the new fee-focused fixtures from step 1 and ensure they fail before the fix and pass after.

