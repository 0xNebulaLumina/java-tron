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
- [ ] Skim validation code paths and record exact messages (source of truth):
  - [ ] `actuator/src/main/java/org/tron/core/actuator/MarketSellAssetActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/MarketCancelOrderActuator.java`
  - [ ] `chainbase/src/main/java/org/tron/core/capsule/utils/MarketUtils.java` (matching + rounding rules)
- [ ] Cross-check “hard to craft” branches against unit tests for setup patterns:
  - [ ] `framework/src/test/java/org/tron/core/actuator/MarketSellAssetActuatorTest.java`
  - [ ] `framework/src/test/java/org/tron/core/actuator/MarketCancelOrderActuatorTest.java`
- [ ] Run the existing generator once to confirm current output is stable enough:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.MarketFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

Phase 1 — MarketSellAssetContract (52) Fixtures

Owner/address/account branches
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `ownerAddress = ByteString.EMPTY`
  - [ ] Expect: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Use a valid-looking address not inserted into `AccountStore`.
  - [ ] Expect: `"Account does not exist!"`.

Token id format branches (non-TRX ids must be numeric bytes)
- [ ] Add `validate_fail_sell_token_id_not_number`:
  - [ ] `sellTokenId = "abc".getBytes()` (non-TRX)
  - [ ] Expect: `"sellTokenId is not a valid number"`.
- [ ] Add `validate_fail_buy_token_id_not_number`:
  - [ ] `buyTokenId = "abc".getBytes()` (non-TRX)
  - [ ] Expect: `"buyTokenId is not a valid number"`.

Token existence branches
- [ ] Add `validate_fail_no_sell_token_id`:
  - [ ] Use a numeric token id that is NOT seeded into `asset-issue-v2`.
  - [ ] Expect: `"No sellTokenId !"`.
- [ ] Add `validate_fail_no_buy_token_id`:
  - [ ] Use a numeric buy token id that is NOT seeded.
  - [ ] Expect: `"No buyTokenId !"`.

Quantity branches (buy-side missing today)
- [ ] Add `validate_fail_zero_buy_quantity`:
  - [ ] `buyTokenQuantity = 0`
  - [ ] Expect: `"token quantity must greater than zero"`.
- [ ] Add `validate_fail_negative_buy_quantity`:
  - [ ] `buyTokenQuantity = -1`
  - [ ] Expect: `"token quantity must greater than zero"`.
- [ ] Add `validate_fail_buy_quantity_exceeds_limit`:
  - [ ] Temporarily set `MARKET_QUANTITY_LIMIT` low (or set `buyTokenQuantity > limit`).
  - [ ] Expect: `"token quantity must less than <limit>"`.

Order-count limit (`MAX_ACTIVE_ORDER_NUM`)
- [ ] Add `validate_fail_max_active_order_num_exceeded`:
  - [ ] Seed the owner with `MarketSellAssetActuator.getMAX_ACTIVE_ORDER_NUM()` active orders.
  - [ ] Submit one more order.
  - [ ] Expect: `"Maximum number of orders exceeded，100"`.
- [ ] Add `edge_max_active_order_num_at_limit_succeeds` (optional but valuable):
  - [ ] Ensure creating the 100th order succeeds when starting from 99 active orders.

Funding failures for token-sell (distinct from TRX “insufficient balance”)
- [ ] Add `validate_fail_sell_token_balance_not_enough`:
  - [ ] Selling `TOKEN_A` where account has less than `sellTokenQuantity`.
  - [ ] Expect: `"SellToken balance is not enough !"`.

Fee-related funding (requires temporarily setting `MARKET_SELL_FEE > 0`)
- [ ] Add `validate_fail_trx_sell_fee_insufficient`:
  - [ ] Set `MARKET_SELL_FEE = 1` for this fixture.
  - [ ] Make balance == `sellTokenQuantity` (so fee is missing).
  - [ ] Expect: `"No enough balance !"`.
- [ ] Add `validate_fail_token_sell_fee_insufficient`:
  - [ ] Sell a token with enough token balance, but TRX balance < fee.
  - [ ] Expect: `"No enough balance !"`.

Execution/behavior branches (missing coverage today)
- [ ] Add `edge_no_match_price_too_low`:
  - [ ] Seed maker orders for the opposite pair.
  - [ ] Submit a taker order whose price does not satisfy `MarketUtils.priceMatch(...)`.
  - [ ] Expect: `SUCCESS`, `result.pb.orderDetailsCount == 0`, and taker order persisted into the book.
- [ ] Add `edge_match_across_multiple_price_levels`:
  - [ ] Seed maker orders at two+ price levels for the same pair.
  - [ ] Make taker large enough to consume the best price level and continue.
  - [ ] Assert store effects: deleted best `pairPriceKey`, decremented `priceNum`, remaining levels intact.
- [ ] Add `edge_partial_fill_taker_less_than_maker`:
  - [ ] Single maker order larger than taker’s capacity at that maker price.
  - [ ] Verify maker stays `ACTIVE` (remain > 0) and stays in the price list; taker becomes `INACTIVE`.
- [ ] Add `edge_partial_fill_taker_greater_than_maker`:
  - [ ] Taker larger than maker; maker becomes `INACTIVE` and removed; taker remains `ACTIVE` and is saved via `saveRemainOrder`.
- [ ] Add `edge_rounding_quantity_too_small_returns_sell_token`:
  - [ ] Craft maker price such that `multiplyAndDivide(...)` produces 0 for the taker.
  - [ ] Expect: no fill, taker order becomes `INACTIVE` and sell token is returned (account balance/token restored).
- [ ] Add `edge_full_pair_cleanup_last_price_level_consumed`:
  - [ ] Seed only one price level and consume it fully.
  - [ ] Assert `pairToPriceStore` deletes the pair key (not just `pairPriceToOrderStore`).
- [ ] Add `edge_gcd_price_key_collision_same_ratio` (optional):
  - [ ] Create maker orders with prices `1:2` and `2:4` and confirm they share the same `pairPriceKey` / price level.

Phase 2 — MarketCancelOrderContract (53) Fixtures

Validation branches
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `ownerAddress = ByteString.EMPTY`
  - [ ] Expect: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Use a valid-looking address not inserted into `AccountStore`.
  - [ ] Expect: `"Account does not exist!"`.
- [ ] Add `validate_fail_order_not_active_inactive_filled`:
  - [ ] Create an order, fully fill it (so state becomes `INACTIVE`), then attempt cancel.
  - [ ] Expect: `"Order is not active!"`.
- [ ] Add `validate_fail_cancel_fee_insufficient_balance`:
  - [ ] Temporarily set `MARKET_CANCEL_FEE = 1` and set account TRX balance to 0.
  - [ ] Expect: `"No enough balance !"`.

Execution/behavior branches (order book maintenance)
- [ ] Add `edge_cancel_removes_one_of_many_same_price`:
  - [ ] Seed multiple orders at the same `pairPriceKey`; cancel one in the middle.
  - [ ] Assert `pairPriceToOrderStore` still exists and remaining order ids are intact.
- [ ] Add `edge_cancel_last_order_in_price_level_decrements_price_num`:
  - [ ] Seed at least two price levels, cancel the only order at one level.
  - [ ] Assert that level is deleted and `pairToPriceStore.priceNum` decrements but pair remains.
- [ ] Add `edge_cancel_last_order_in_last_price_level_deletes_pair`:
  - [ ] Seed a single price level, cancel its last order.
  - [ ] Assert `pairToPriceStore` deletes the pair key.
- [ ] Add `edge_cancel_partially_filled_order_refunds_only_remain`:
  - [ ] Partially fill an order, then cancel it.
  - [ ] Assert refunded amount equals `sellTokenQuantityRemain` (TRX and token variants).

Phase 3 — Determinism and Hygiene
- [ ] Consider making tx timestamps deterministic (avoid `System.currentTimeMillis()` drift in `request.pb`):
  - [ ] Reuse any shared deterministic helper used by other conformance generator tests (e.g., via `ConformanceFixtureTestSupport`).
- [ ] Ensure dynamic property mutations (`ALLOW_MARKET_TRANSACTION`, fees, limits) are restored after each fixture (use `@After` or local `try/finally`).
- [ ] Keep fixture `caseCategory` aligned with observed `expectedStatus` (avoid “validate_fail” metadata for a `SUCCESS` fixture).
