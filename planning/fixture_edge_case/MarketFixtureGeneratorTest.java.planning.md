Review Target

- `framework/src/test/java/org/tron/core/conformance/MarketFixtureGeneratorTest.java`

Scope

- Fixture generation for Market contracts:
  - `MarketSellAssetContract` (type 52)
  - `MarketCancelOrderContract` (type 53)
- Baseline assumptions baked into this test class:
  - `ALLOW_SAME_TOKEN_NAME = 1` and TRC-10 assets are seeded into `asset-issue-v2` via `ConformanceFixtureTestSupport.putAssetIssueV2(...)`
  - `ALLOW_MARKET_TRANSACTION = 1`
  - Fees are set to `0` (`MARKET_SELL_FEE`, `MARKET_CANCEL_FEE`)
  - `MARKET_QUANTITY_LIMIT = 1_000_000_000_000_000`

Current Coverage (as written)

MarketSellAssetContract (52)

- Happy: create orders in all three directions:
  - `TRX -> TOKEN_A`
  - `TOKEN_A -> TRX`
  - `TOKEN_A -> TOKEN_B`
- Edge: matching behaviors and order-book maintenance:
  - Match a taker against a single maker order.
  - Inner match loop across multiple maker orders at the same price.
  - `MAX_MATCH_NUM` boundary: exactly at limit succeeds; `MAX_MATCH_NUM + 1` reverts.
  - Price queue cleanup: consume best price level and remove that empty price level while keeping a worse price level.
- Validate-fail:
  - Market disabled (`ALLOW_MARKET_TRANSACTION = 0`).
  - Same tokens (sell == buy).
  - Insufficient TRX balance (sell TRX > balance).
  - Sell quantity exceeds `MARKET_QUANTITY_LIMIT`.
  - Zero sell quantity.

MarketCancelOrderContract (53)

- Happy:
  - Cancel a TRX->TOKEN order.
  - Cancel a token->token order.
- Validate-fail:
  - Non-owner tries to cancel.
  - Cancel non-existent order id.
  - Cancel already-canceled (non-active) order.
  - Market disabled (`ALLOW_MARKET_TRANSACTION = 0`).

Missing Edge Cases (high value for conformance)

Validation paths (source of truth)
- `actuator/src/main/java/org/tron/core/actuator/MarketSellAssetActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/MarketCancelOrderActuator.java`

MarketSellAssetContract (52) — missing validation branches

- Invalid `ownerAddress` (`DecodeUtil.addressValid`): empty / wrong length / bad prefix bytes.
- Owner account does not exist: `"Account does not exist!"`.
- Token id encoding checks (non-TRX ids must be numeric bytes):
  - sell id not number → `"sellTokenId is not a valid number"`.
  - buy id not number → `"buyTokenId is not a valid number"`.
- Buy quantity validation:
  - `buyTokenQuantity <= 0` (0 and negative) → `"token quantity must greater than zero"`.
  - `buyTokenQuantity > MARKET_QUANTITY_LIMIT` → `"token quantity must less than <limit>"`.
- Token existence checks:
  - Selling a non-existent token id (non-TRX) → `"No sellTokenId !"`.
  - Buying a non-existent token id (non-TRX) → `"No buyTokenId !"`.
- Selling token balance insufficient (non-TRX sell): `"SellToken balance is not enough !"`.
- `MAX_ACTIVE_ORDER_NUM` saturation:
  - Account already has `>= 100` active orders → `"Maximum number of orders exceeded，100"`.
  - Boundary at `== 99` then place 100th (should succeed) is also untested.
- Fee-related funding (currently masked because fees are forced to 0 here):
  - TRX sell: balance must cover `sellTokenQuantity + marketSellFee`.
  - Token sell: balance must cover `marketSellFee` even if token balance is sufficient.

MarketSellAssetContract (52) — missing execution/behavior branches

- “No match” with an existing maker book:
  - Maker orders exist, but taker price does not satisfy `MarketUtils.priceMatch(...)` → taker should not match and the new order should be added to the order book.
- Cross-price matching:
  - Taker consumes best price level then continues into the next best price level (tests currently only match within one price level).
- Partial fill coverage:
  - `taker < maker` branch (maker remains `ACTIVE` with reduced remain quantity).
  - `taker > maker` branch (taker remains `ACTIVE` and is persisted via `saveRemainOrder`).
- Rounding / “quantity too small” path:
  - `MarketUtils.multiplyAndDivide(...)` returns `0` (taker sell quantity too small vs maker price), causing `setSellTokenQuantityReturn()` + `returnSellTokenRemain()` and order becomes `INACTIVE`.
  - This is a high-risk cross-implementation edge because tiny integer math differences can change whether a trade happens at all.
- Full book cleanup:
  - When the last price level for a pair is consumed, `pairToPriceStore.delete(makerPair)` should occur (currently only tests removing one empty price level while leaving another).
- GCD-normalized price key behavior:
  - Orders with the same ratio (e.g., 1:2 and 2:4) share the same `pairPriceKey` (via `MarketUtils.createPairPriceKey`); no fixture currently locks this down.

MarketCancelOrderContract (53) — missing validation branches

- Invalid `ownerAddress` (`DecodeUtil.addressValid`) and “account does not exist”.
- Insufficient balance for cancel fee when `MARKET_CANCEL_FEE > 0`: `"No enough balance !"`.
- Not-active orders in other states:
  - `State.INACTIVE` (filled) vs `State.CANCELED` (currently only simulates canceled) → both fail with `"Order is not active!"`, but a fixture can assert “filled orders cannot be canceled”.

MarketCancelOrderContract (53) — missing execution/behavior branches

- Cancel when multiple orders exist at the same price:
  - After cancel, `pairPriceToOrderStore` remains and only that order id is removed.
- Cancel last order at a price level while other price levels exist:
  - `pairPriceToOrderStore.delete(pairPriceKey)` triggers, and `pairToPriceStore.setPriceNum(pair, remainCount)` decrements but does not delete pair.
- Cancel last order in the last remaining price level:
  - `pairToPriceStore.delete(pair)` triggers.
- Cancel a partially-filled order:
  - Only `sellTokenQuantityRemain` should be refunded (not the original `sellTokenQuantity`), for both TRX-sell and token-sell orders.

Notes / Potential fixture-generation pitfalls

- `createTransaction()` uses `System.currentTimeMillis()` for tx timestamp/expiration, so rerunning the generator produces different `request.pb` content even when DB state is identical. Other fixture generators often centralize deterministic tx creation in `ConformanceFixtureTestSupport`.
- `createMarketOrder(...)` directly edits stores (skipping `validate()/execute()`), which is fine for simple “seed maker book” setup, but is risky for scenarios that require precise `sellTokenQuantityRemain` evolution (partial fills / rounding). For those, driving state transitions through the actuators can be more robust.
