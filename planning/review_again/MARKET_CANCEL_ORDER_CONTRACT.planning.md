# Review: `MARKET_CANCEL_ORDER_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**
  - `execute_market_cancel_order_contract()` + `parse_market_cancel_order_contract()` in `rust-backend/crates/core/src/service/mod.rs`
  - linked-list removal helper: `remove_order_from_linked_list()` in `rust-backend/crates/core/src/service/mod.rs`
  - key builders: `create_pair_price_key()` / `create_pair_key()` in `rust-backend/crates/core/src/service/mod.rs`
  - storage adapter methods used (not exhaustive):
    - dynamic: `allow_market_transaction()`, `get_market_cancel_fee()`, `support_black_hole_optimization()`, `burn_trx()`
    - accounts: `get_account_proto()`, `set_account_proto()`, `get_blackhole_address_evm()`, `add_balance()`
    - market: `get_market_order()`, `put_market_order()`, `get_market_account_order()`, `put_market_account_order()`,
      `get_market_order_id_list()`, `put_market_order_id_list()`, `delete_market_order_id_list()`,
      `get_market_pair_price_count()`, `set_market_pair_price_count()`, `delete_market_pair()`
- **Java reference**
  - `MarketCancelOrderActuator.validate/execute` in `actuator/src/main/java/org/tron/core/actuator/MarketCancelOrderActuator.java`
  - `MarketUtils.returnSellTokenRemain()` + `MarketUtils.updateOrderState()` + key builders in
    `chainbase/src/main/java/org/tron/core/capsule/utils/MarketUtils.java`
  - linked-list removal: `MarketOrderIdListCapsule.removeOrder()` in
    `chainbase/src/main/java/org/tron/core/capsule/MarketOrderIdListCapsule.java`
  - account-order list update: `MarketAccountOrderCapsule.removeOrder()` in
    `chainbase/src/main/java/org/tron/core/capsule/MarketAccountOrderCapsule.java`

Goal: determine whether Rust execution matches java-tron’s actuator semantics (validation, state updates, and failure behavior).

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`MarketCancelOrderActuator.validate`)

Key checks (simplified, but order-preserving):

1. Contract Any type is `MarketCancelOrderContract`
2. `ALLOW_MARKET_TRANSACTION == 1` else `"Not support Market Transaction, need to be opened by the committee"`
3. `DecodeUtil.addressValid(owner)` (21 bytes + correct prefix) else `"Invalid address"`
4. Owner account exists else `"Account does not exist!"`
5. Order exists else `"orderId not exists"`
6. Order is active (`state == ACTIVE`) else `"Order is not active!"`
7. Order owner matches caller else `"Order does not belong to the account!"`
8. TRX balance ≥ cancel fee else `"No enough balance !"`

### 2) Execution (`MarketCancelOrderActuator.execute`)

1. Loads owner account and `MarketOrder` by `order_id`
2. Charges cancel fee:
   - deduct from owner balance
   - burn (`burnTrx`) when `supportBlackHoleOptimization` else credit `blackhole`
3. Returns remaining sell-side funds to owner:
   - `MarketUtils.returnSellTokenRemain(order, account, dynamicStore, assetIssueStore)`
   - sets `sell_token_quantity_remain = 0`
4. Marks order canceled + removes it from the owner’s active order list:
   - `MarketUtils.updateOrderState(order, State.CANCELED, marketAccountStore)`
5. Persists updated owner account + order
6. Removes order from the orderbook list for its (pair, price):
   - `pairPriceKey = MarketUtils.createPairPriceKey(sellTokenId, buyTokenId, sellQty, buyQty)`
   - `MarketOrderIdListCapsule.removeOrder(...)` updates neighbor pointers, head/tail, and clears the canceled order’s
     `prev/next`
7. If the price-level list is empty:
   - delete the price key
   - decrement `MarketPairToPriceStore` price count; delete the pair key if it hits 0

---

## Rust implementation behavior (what it currently does)

### Parsing

`parse_market_cancel_order_contract()` decodes protobuf fields:

- `owner_address` (field 1)
- `order_id` (field 2)

### Validation + execution (`execute_market_cancel_order_contract`)

- Validates `ALLOW_MARKET_TRANSACTION == 1`
- Validates `owner_address` is 21 bytes with the correct prefix
- Loads owner account and the `MarketOrder` by `order_id`
- Validates:
  - order is active (`state == 0`)
  - order owner matches `owner_address`
  - account.balance ≥ cancel fee
- Charges fee (burn or credit to blackhole)
- Refunds `sell_token_quantity_remain` back to the owner:
  - `"_"` (or empty) → TRX balance
  - otherwise → `account.asset_v2[<token_id_string>]`
- Sets order to `CANCELED` and zeros `sell_token_quantity_remain`
- Updates `MarketAccountOrder` (if present): removes `order_id` and decrements `count`
- Removes from the price-level linked list (if the list exists):
  - updates prev/next pointers for neighbors (if present)
  - updates list head/tail
  - clears canceled order’s `prev/next`
  - deletes the list if empty; otherwise persists updated list
  - if the list becomes empty, decrements pair price count and deletes the pair when count <= 1
- Persists updated order + owner account

---

## Does it match java-tron?

### What matches (normal, consistent DB state)

On the “happy path” with consistent market DBs (order exists, list exists, market_account entry exists), Rust matches Java’s
intended state transition:

- same validation gates + error strings for the fixture-covered failure modes
- same fee burn vs blackhole behavior
- same “refund only `sell_token_quantity_remain`” behavior for partial fills
- same order state transition to `CANCELED`, account-order list removal, and orderbook cleanup (head/tail + price count)

The existing conformance fixture set (`conformance/fixtures/market_cancel_order_contract/*`) targets these behaviors.

### Parity gaps / risks (where behavior can diverge from Java)

1) **Missing `Any.is(MarketCancelOrderContract)` parity check**

Java rejects mismatched Any types in `validate()`. Rust does not currently check
`transaction.metadata.contract_parameter.type_url` for `protocol.MarketCancelOrderContract` (unlike `MarketSellAsset`).

2) **Missing-store handling is more permissive than Java**

Java uses `store.get(...)` calls that throw `ItemNotFoundException` (and the tx fails) when these are missing:

- `MarketAccountStore.get(owner)` inside `MarketUtils.updateOrderState(...)`
- `MarketPairPriceToOrderStore.get(pairPriceKey)` in the cancel actuator

Rust currently treats both as optional and proceeds successfully if absent:

- missing `MarketAccountOrder` → skip removing order id / decrementing count
- missing `MarketOrderIdList` → skip orderbook removal entirely

This is a strict parity difference, even if “should never happen” under normal invariants.

3) **Linked-list neighbor missing behavior differs**

Java will throw if `prev`/`next` pointers reference missing orders (via `orderStore.get(...)` in `getPrevCapsule/getNextCapsule`).
Rust currently ignores missing neighbors (only updates them if present), allowing silent divergence in corrupted states.

4) **TRC-10 crediting semantics are only equivalent in the modern (id-keyed) mode**

Java refunds tokens via `AccountCapsule.addAssetAmountV2(tokenIdBytes, ...)`, which keys differently when:

- `ALLOW_SAME_TOKEN_NAME == 0` (legacy): updates both `Account.asset[name]` and `Account.assetV2[id]` using the asset-issue
  store to map name → id
- `ALLOW_SAME_TOKEN_NAME == 1` (modern): updates `Account.assetV2[id]` only

Rust refunds by directly mutating `account.asset_v2[String::from_utf8_lossy(token_id_bytes)]` and never touches the legacy
`asset` map. This matches Java in the common modern mode but is not full legacy parity.

5) **Key construction is more forgiving than Java**

Java’s `MarketUtils.createPairKey/createPairPriceKey` will throw if `sellTokenId.length > 19` (array copy overflow).
Rust truncates token ids to 19 bytes when building keys, which avoids the crash but produces different keys in malformed states.

6) **Duplicate order-id removal semantics differ**

Java removes a single matching order id (`List.remove`), Rust removes all matches (`retain`).
This only matters if `MarketAccountOrder.orders` is corrupted with duplicates.

---

## Bottom line

- **Yes (for the standard invariants / conformance fixtures)**: the Rust implementation matches the Java-side behavior for
  the normal cases the system relies on (active order + consistent orderbook + id-keyed assets).
- **No (for strict “same failure mode” / legacy parity)**: Rust is intentionally more permissive when market auxiliary
  indexes are missing/corrupt, and it doesn’t fully mirror Java’s legacy TRC-10 asset-map semantics or `Any.is` checks.

