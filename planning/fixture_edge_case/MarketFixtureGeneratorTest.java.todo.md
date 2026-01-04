# MarketFixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/MarketFixtureGeneratorTest.java` fixture generation so conformance covers key validation branches and high-risk execution boundaries for:
  - `MarketSellAssetContract` (52)
  - `MarketCancelOrderContract` (53)

Non-Goals
- Do not change actuator logic; only add/adjust fixtures to reflect current java-tron behavior.
- Do not refactor fixture generator infrastructure unless required for determinism.
- Do not add legacy token-name mode coverage (`ALLOW_SAME_TOKEN_NAME=0`) unless explicitly needed.

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, and `expected/post_db/` (and `expected/result.pb` when present).
- Validation failures produce:
  - `metadata.json.expectedStatus == "VALIDATION_FAILED"`
  - `metadata.json.expectedErrorMessage` matches the thrown `ContractValidateException` message.
- Execution failures (reverts) produce:
  - `metadata.json.expectedStatus == "REVERT"`
  - `metadata.json.expectedErrorMessage` matches the thrown `ContractExeException` message.
- Happy/edge fixtures produce `SUCCESS` and mutate expected DBs consistently.

Checklist / TODO

Phase 0 — Confirm Baselines and Error Messages
- [x] Skim validation code paths and record exact messages (source of truth):
  - [x] `actuator/src/main/java/org/tron/core/actuator/MarketSellAssetActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/MarketCancelOrderActuator.java`
  - [x] `chainbase/src/main/java/org/tron/core/capsule/utils/MarketUtils.java` (matching + rounding rules)
- [ ] Cross-check "hard to craft" branches against unit tests for setup patterns:
  - [ ] `framework/src/test/java/org/tron/core/actuator/MarketSellAssetActuatorTest.java`
  - [ ] `framework/src/test/java/org/tron/core/actuator/MarketCancelOrderActuatorTest.java`
- [ ] Run the existing generator once to confirm current output is stable enough:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.MarketFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

Phase 1 — MarketSellAssetContract (52) Fixtures

Owner/address/account branches
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `ownerAddress = ByteString.EMPTY`
  - [x] Expect: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking address not inserted into `AccountStore`.
  - [x] Expect: `"Account does not exist!"`.

Token id format branches (non-TRX ids must be numeric bytes)
- [x] Add `validate_fail_sell_token_id_not_number`:
  - [x] `sellTokenId = "abc".getBytes()` (non-TRX)
  - [x] Expect: `"sellTokenId is not a valid number"`.
- [x] Add `validate_fail_buy_token_id_not_number`:
  - [x] `buyTokenId = "abc".getBytes()` (non-TRX)
  - [x] Expect: `"buyTokenId is not a valid number"`.

Token existence branches
- [x] Add `validate_fail_no_sell_token_id`:
  - [x] Use a numeric token id that is NOT seeded into `asset-issue-v2`.
  - [x] Expect: `"No sellTokenId !"`.
- [x] Add `validate_fail_no_buy_token_id`:
  - [x] Use a numeric buy token id that is NOT seeded.
  - [x] Expect: `"No buyTokenId !"`.

Quantity branches (buy-side missing today)
- [x] Add `validate_fail_zero_buy_quantity`:
  - [x] `buyTokenQuantity = 0`
  - [x] Expect: `"token quantity must greater than zero"`.
- [x] Add `validate_fail_negative_buy_quantity`:
  - [x] `buyTokenQuantity = -1`
  - [x] Expect: `"token quantity must greater than zero"`.
- [x] Add `validate_fail_buy_quantity_exceeds_limit`:
  - [x] Temporarily set `MARKET_QUANTITY_LIMIT` low (or set `buyTokenQuantity > limit`).
  - [x] Expect: `"token quantity must less than <limit>"`.

Order-count limit (`MAX_ACTIVE_ORDER_NUM`)
- [x] Add `validate_fail_max_active_order_num_exceeded`:
  - [x] Seed the owner with `MarketSellAssetActuator.getMAX_ACTIVE_ORDER_NUM()` active orders.
  - [x] Submit one more order.
  - [x] Expect: `"Maximum number of orders exceeded，100"`.
- [x] Add `edge_max_active_order_num_at_limit_succeeds` (optional but valuable):
  - [x] Ensure creating the 100th order succeeds when starting from 99 active orders.

Funding failures for token-sell (distinct from TRX "insufficient balance")
- [x] Add `validate_fail_sell_token_balance_not_enough`:
  - [x] Selling `TOKEN_A` where account has less than `sellTokenQuantity`.
  - [x] Expect: `"SellToken balance is not enough !"`.

Fee-related funding (requires temporarily setting `MARKET_SELL_FEE > 0`)
- [x] Add `validate_fail_trx_sell_fee_insufficient`:
  - [x] Set `MARKET_SELL_FEE = 1` for this fixture.
  - [x] Make balance == `sellTokenQuantity` (so fee is missing).
  - [x] Expect: `"No enough balance !"`.
- [x] Add `validate_fail_token_sell_fee_insufficient`:
  - [x] Sell a token with enough token balance, but TRX balance < fee.
  - [x] Expect: `"No enough balance !"`.

Execution/behavior branches (missing coverage today)
- [x] Add `edge_no_match_price_too_low`:
  - [x] Seed maker orders for the opposite pair.
  - [x] Submit a taker order whose price does not satisfy `MarketUtils.priceMatch(...)`.
  - [x] Expect: `SUCCESS`, `result.pb.orderDetailsCount == 0`, and taker order persisted into the book.
- [x] Add `edge_match_across_multiple_price_levels`:
  - [x] Seed maker orders at two+ price levels for the same pair.
  - [x] Make taker large enough to consume the best price level and continue.
  - [x] Assert store effects: deleted best `pairPriceKey`, decremented `priceNum`, remaining levels intact.
- [x] Add `edge_partial_fill_taker_less_than_maker`:
  - [x] Single maker order larger than taker's capacity at that maker price.
  - [x] Verify maker stays `ACTIVE` (remain > 0) and stays in the price list; taker becomes `INACTIVE`.
- [x] Add `edge_partial_fill_taker_greater_than_maker`:
  - [x] Taker larger than maker; maker becomes `INACTIVE` and removed; taker remains `ACTIVE` and is saved via `saveRemainOrder`.
- [x] Add `edge_rounding_quantity_too_small_returns_sell_token`:
  - [x] Craft maker price such that `multiplyAndDivide(...)` produces 0 for the taker.
  - [x] Expect: no fill, taker order becomes `INACTIVE` and sell token is returned (account balance/token restored).
- [x] Add `edge_full_pair_cleanup_last_price_level_consumed`:
  - [x] Seed only one price level and consume it fully.
  - [x] Assert `pairToPriceStore` deletes the pair key (not just `pairPriceToOrderStore`).
- [x] Add `edge_gcd_price_key_collision_same_ratio` (optional):
  - [x] Create maker orders with prices `1:2` and `2:4` and confirm they share the same `pairPriceKey` / price level.

Phase 2 — MarketCancelOrderContract (53) Fixtures

Validation branches
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `ownerAddress = ByteString.EMPTY`
  - [x] Expect: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking address not inserted into `AccountStore`.
  - [x] Expect: `"Account does not exist!"`.
- [x] Add `validate_fail_order_not_active_inactive_filled`:
  - [x] Create an order, fully fill it (so state becomes `INACTIVE`), then attempt cancel.
  - [x] Expect: `"Order is not active!"`.
- [x] Add `validate_fail_cancel_fee_insufficient_balance`:
  - [x] Temporarily set `MARKET_CANCEL_FEE = 1` and set account TRX balance to 0.
  - [x] Expect: `"No enough balance !"`.

Execution/behavior branches (order book maintenance)
- [x] Add `edge_cancel_removes_one_of_many_same_price`:
  - [x] Seed multiple orders at the same `pairPriceKey`; cancel one in the middle.
  - [x] Assert `pairPriceToOrderStore` still exists and remaining order ids are intact.
- [x] Add `edge_cancel_last_order_in_price_level_decrements_price_num`:
  - [x] Seed at least two price levels, cancel the only order at one level.
  - [x] Assert that level is deleted and `pairToPriceStore.priceNum` decrements but pair remains.
- [x] Add `edge_cancel_last_order_in_last_price_level_deletes_pair`:
  - [x] Seed a single price level, cancel its last order.
  - [x] Assert `pairToPriceStore` deletes the pair key.
- [x] Add `edge_cancel_partially_filled_order_refunds_only_remain`:
  - [x] Partially fill an order, then cancel it.
  - [x] Assert refunded amount equals `sellTokenQuantityRemain` (TRX and token variants).

Phase 3 — Determinism and Hygiene
- [ ] Consider making tx timestamps deterministic (avoid `System.currentTimeMillis()` drift in `request.pb`):
  - [ ] Reuse any shared deterministic helper used by other conformance generator tests (e.g., via `ConformanceFixtureTestSupport`).
- [ ] Ensure dynamic property mutations (`ALLOW_MARKET_TRANSACTION`, fees, limits) are restored after each fixture (use `@After` or local `try/finally`).
- [ ] Keep fixture `caseCategory` aligned with observed `expectedStatus` (avoid "validate_fail" metadata for a `SUCCESS` fixture).
