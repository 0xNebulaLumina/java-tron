# Review: `MARKET_SELL_ASSET_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**
  - `execute_market_sell_asset_contract()` + `parse_market_sell_asset_contract()` in `rust-backend/crates/core/src/service/mod.rs`
  - matching + book helpers in `rust-backend/crates/core/src/service/mod.rs`:
    - `match_market_sell_order()`, `market_match_single_order()`, `save_remain_market_order()`
    - `market_get_price_keys_list()`, `market_has_match()`, `market_update_order_state()`
    - key/id builders: `create_pair_key()`, `create_pair_price_key()`, `calculate_order_id()`
- **Java reference**
  - `MarketSellAssetActuator.validate/execute` in `actuator/src/main/java/org/tron/core/actuator/MarketSellAssetActuator.java`
  - price/key/math utilities in `chainbase/src/main/java/org/tron/core/capsule/utils/MarketUtils.java`
  - price-key iteration: `MarketPairPriceToOrderStore.getPriceKeysList` in
    `chainbase/src/main/java/org/tron/core/store/MarketPairPriceToOrderStore.java`
  - linked list ops: `MarketOrderIdListCapsule.addOrder/removeOrder` in
    `chainbase/src/main/java/org/tron/core/capsule/MarketOrderIdListCapsule.java`
  - account-order list updates: `MarketAccountOrderCapsule.removeOrder` in
    `chainbase/src/main/java/org/tron/core/capsule/MarketAccountOrderCapsule.java`

Goal: determine whether Rust execution matches java-tron’s actuator semantics (validation, state updates, and failure behavior).

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`MarketSellAssetActuator.validate`)

Key checks (order-preserving, simplified):

1. Contract Any type is `MarketSellAssetContract`
2. `ALLOW_MARKET_TRANSACTION == 1` else `"Not support Market Transaction, need to be opened by the committee"`
3. `DecodeUtil.addressValid(owner)` else `"Invalid address"`
4. Owner account exists else `"Account does not exist!"`
5. `sellTokenId` / `buyTokenId` are `"_"` or numeric (no leading zeros) else:
   - `"sellTokenId is not a valid number"`
   - `"buyTokenId is not a valid number"`
6. `sellTokenId != buyTokenId` else `"cannot exchange same tokens"`
7. `sellTokenQuantity > 0` and `buyTokenQuantity > 0` else `"token quantity must greater than zero"`
8. Both quantities ≤ `MARKET_QUANTITY_LIMIT` else `"token quantity must less than <limit>"`
9. Active order count `< MAX_ACTIVE_ORDER_NUM (100)` else `"Maximum number of orders exceeded，100"`
10. Fee + balance checks:
    - if selling TRX (`sellTokenId == "_"`): `balance ≥ sellQty + fee` else `"No enough balance !"`
    - if selling token: `balance ≥ fee` and `assetBalanceEnoughV2(sellId, sellQty)` else:
      - `"No enough balance !"` (fee)
      - `"SellToken balance is not enough !"` (token)
11. Token existence checks for non-TRX:
    - missing sell token: `"No sellTokenId !"`
    - missing buy token: `"No buyTokenId !"`

### 2) Execution (`MarketSellAssetActuator.execute`)

1. Charge fee (always):
   - deduct from owner TRX balance
   - if `supportBlackHoleOptimization`: `dynamicStore.burnTrx(fee)` (updates `BURN_TRX_AMOUNT`)
   - else credit blackhole account balance
2. Escrow sell-side funds:
   - selling TRX: `balance -= sellQty`
   - selling token: `reduceAssetAmountV2(sellId, sellQty, ...)`
3. Create + persist a new `MarketOrder`:
   - `orderId = MarketUtils.calculateOrderId(owner, sellId, buyId, accountOrder.totalCount)`
   - `createTime = latest_block_header_timestamp`
   - append orderId to `MarketAccountOrder.orders`, increment `count` and `totalCount`
   - persist to `market_account` + `market_order`
4. Match taker order against maker book (`matchOrder(...)`):
   - maker pair is reversed: `(makerSell, makerBuy) = (buyId, sellId)`
   - iterate maker price levels (lowest price first) via `pairPriceToOrderStore.getPriceKeysList(headKey, MAX_MATCH_NUM+1, priceNum, true)`
   - per price level, traverse the linked list of maker orders
   - match math/rounding is via `MarketUtils.multiplyAndDivide(...)`
   - enforce `MAX_MATCH_NUM (20)`; if exceeded: throw `"Too many matches. MAX_MATCH_NUM = 20"`
   - on full consumption of a maker order: remove it from the price-level list (`MarketOrderIdListCapsule.removeOrder(...)`)
   - when a price level becomes empty: delete the price key and decrement pair price count; delete the pair if it hits 0
5. If taker has remaining sell-side quantity: save into order book (`saveRemainOrder(...)`):
   - key is `MarketUtils.createPairPriceKey(sellId,buyId,sellQty,buyQty)` (GCD-normalized)
   - if new price key: increment pair price count and ensure head key exists
   - append order to the price-level linked list tail
6. Persist final taker order + updated owner account
7. Receipt:
   - `ret.setOrderId(orderId)`
   - includes `orderDetails[]` entries for each fill (`ret.addOrderDetails(...)`)

---

## Rust implementation behavior (what it currently does)

### Parsing (`parse_market_sell_asset_contract`)

Decodes protobuf fields:

- `sell_token_id` (field 2)
- `sell_token_quantity` (field 3)
- `buy_token_id` (field 4)
- `buy_token_quantity` (field 5)

It **skips** `owner_address` (field 1) and uses `transaction.from` / `transaction.metadata.from_raw` for ownership.

### Validation + execution (`execute_market_sell_asset_contract`)

Matches Java’s validation gates and error strings for the “normal” fixture set:

- market enabled, owner address format, account exists
- token id numeric checks (`"_"` or digits, no leading zeros)
- same token rejection
- quantities > 0 and ≤ `MARKET_QUANTITY_LIMIT`
- active order count `< 100`
- fee + balance checks and token existence checks

Execution order is Java-like:

1. deduct fee from owner balance
2. escrow sell-side funds
3. create + persist order + per-account order tracking
4. match against maker book (`match_market_sell_order` + `market_match_single_order`)
5. save remain order into the order book (`save_remain_market_order`) if remain > 0
6. persist final taker order + owner account

Receipt:

- returns an order id via `TransactionResultBuilder::with_order_id(...)`
- explicitly **omits** `orderDetails[]` (“fixtures currently assert DB state only”)

---

## Does it match java-tron?

### What matches (standard invariants / conformance fixtures)

- **Core validation semantics**: same guardrails and (mostly) the same error strings.
- **OrderId + key derivation**:
  - 19-byte token id padding
  - price-key GCD normalization
  - Keccak256/SHA3 order id generation
- **Price matching + fill math**: `priceMatch` / `multiplyAndDivide` logic aligns with `MarketUtils`.
- **MAX_MATCH_NUM behavior**: Rust returns the Java error string when exceeding 20 matches.
- **Order book updates**:
  - linked-list head/tail + neighbor pointer updates
  - price-level deletion + pair price-count updates
  - full-pair cleanup when last price level is consumed

The existing fixture set (`conformance/fixtures/market_sell_asset_contract/*`) covers matching/cleanup edge cases well.

### Parity gaps / risks (where behavior can diverge from Java)

1) **Fee handling does not match Java when `MARKET_SELL_FEE > 0`**

Java behavior depends on chain state:

- if `ALLOW_BLACKHOLE_OPTIMIZATION == 1`: `DynamicPropertiesStore.burnTrx(fee)` (updates `BURN_TRX_AMOUNT`)
- else: credit blackhole account balance

Rust currently:

- decides using `execution_config.fees.support_black_hole_optimization` (static config), not
  `storage_adapter.support_black_hole_optimization()` (dynamic property)
- credits blackhole only when the config flag is false
- does **not** call `storage_adapter.burn_trx(...)` when the flag is true (so `BURN_TRX_AMOUNT` is not updated)

This is a real parity divergence; it’s just not exercised by the current MarketSellAsset fixtures because `MARKET_SELL_FEE`
is effectively 0 in those fixtures (dynamic-properties pre/post are identical).

2) **Owner address source differs**

Java validates/uses `contract.owner_address`. Rust validates `transaction.metadata.from_raw` and ignores the protobuf
`owner_address` field (it’s skipped in parsing). This is usually equivalent under normal request construction, but it is
not strict “same inputs, same validation” parity.

3) **Missing-index handling is more permissive than Java**

Java will throw/fail if a maker price key exists but `MarketPairPriceToOrderStore.get(pairPriceKey)` is missing.

Rust currently treats a missing `MarketOrderIdList` for a price key as “no match possible” and returns success:

- `get_market_order_id_list(pair_price_key) == None` → `Ok(())`

This is a strict parity difference under corrupted/inconsistent market indexes.

4) **TRC-10 legacy-map parity is incomplete**

Java’s `addAssetAmountV2/reduceAssetAmountV2` have legacy behavior when `ALLOW_SAME_TOKEN_NAME == 0` (name-keyed + id-keyed
maps). Rust mutates only `account.asset_v2` keyed by the token-id string.

5) **Token id length > 19 behavior differs (malformed inputs)**

Java key/id builders assume `tokenId.length <= 19` and can throw at runtime otherwise; Rust truncates to 19 bytes when
building keys/ids, which would silently diverge in malformed states.

6) **Receipt completeness**

Java records per-fill `orderDetails[]` in the tx result; Rust currently doesn’t emit them (and fixtures don’t assert them).
If RPC/receipt parity matters for your use case, this is a gap.

---

## Bottom line

- **Yes, broadly (for order matching and book state)**: the Rust implementation is structurally aligned with Java’s
  matching algorithm, key derivation, and index maintenance under normal invariants.
- **No, strictly (for fee + some failure modes)**: fee burning vs blackhole crediting is currently config-driven and does
  not update `BURN_TRX_AMOUNT`, so it will diverge from Java when `MARKET_SELL_FEE > 0` and/or when
  `ALLOW_BLACKHOLE_OPTIMIZATION` differs from the Rust config. Additionally, Rust is more permissive in some “missing index”
  scenarios.

