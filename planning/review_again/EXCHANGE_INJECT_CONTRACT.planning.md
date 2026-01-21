# Review: `EXCHANGE_INJECT_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**
  - `execute_exchange_inject_contract()` + `parse_exchange_inject_contract()` in `rust-backend/crates/core/src/service/mod.rs`
  - math helpers in `rust-backend/crates/core/src/service/contracts/exchange.rs`
  - store helpers in `rust-backend/crates/execution/src/storage_adapter/engine.rs`:
    - exchange store access: `get_exchange{,_from_store}()`, `put_exchange{,_to_store}()`
    - TRC-10 balance helpers: `get_asset_balance_v2()` + account-proto `asset/asset_v2` maps
    - token-id resolution: `get_asset_issue()`
- **Java reference**
  - `ExchangeInjectActuator.validate/execute` in `actuator/src/main/java/org/tron/core/actuator/ExchangeInjectActuator.java`
  - exchange store routing + dual-write behavior: `Commons.getExchangeStoreFinal()` + `Commons.putExchangeCapsule()` in `chainbase/src/main/java/org/tron/common/utils/Commons.java`
  - TRC-10 balance helpers: `AccountCapsule.assetBalanceEnoughV2()` + `AccountCapsule.reduceAssetAmountV2()` in `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java`
  - behavior across `ALLOW_SAME_TOKEN_NAME` modes: `framework/src/test/java/org/tron/core/actuator/ExchangeInjectActuatorTest.java`

Goal: determine whether Rust execution matches java-tron’s actuator semantics (validation, state updates, and receipt fields).

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`ExchangeInjectActuator.validate`)

Key checks (in order):

1. Contract is present + correct Any type (`ExchangeInjectContract`)
2. Owner address passes `DecodeUtil.addressValid()` and the account exists
3. Exchange exists, fetched via:
   - `Commons.getExchangeStoreFinal(dynamicStore, exchangeStore, exchangeV2Store)`
   - `ALLOW_SAME_TOKEN_NAME == 0` → `ExchangeStore` (“v1”, token **names**)
   - `ALLOW_SAME_TOKEN_NAME == 1` → `ExchangeV2Store` (“v2”, token **ids**)
4. Owner is the exchange creator
5. When `ALLOW_SAME_TOKEN_NAME == 1` and token != TRX (`"_"`), token id must be numeric (`TransactionUtil.isNumber`)
6. `token_id` must be one side of the exchange
7. Exchange must not be closed (`firstBalance != 0 && secondBalance != 0`)
8. `quant > 0`
9. Compute the proportional other-side amount using **BigInteger math**:
   - `another = (otherBalance * quant) / tokenBalance`
   - must be `> 0`
   - resulting post-inject balances must be `<= EXCHANGE_BALANCE_LIMIT`
10. Funding checks:
    - TRX side: `account.balance >= (needed + fee)` (fee is 0 here)
    - TRC-10 side: `account.assetBalanceEnoughV2(tokenKey, amount, dynamicStore)`

Important nuance: `assetBalanceEnoughV2()` calls `importAsset(key)` internally; when asset optimization is enabled, balances may be fetched from the per-account asset store rather than the `Account` proto’s maps.

### 2) Execution (`ExchangeInjectActuator.execute`)

- Computes `anotherTokenQuant` using **overflow-checking long math**:
  - `another = floorDiv(multiplyExact(otherBalance, quant), tokenBalance)`
  - this can throw (`ArithmeticException: long overflow`) even when validation passed (because validation used BigInteger)
- Updates:
  - deduct the injected token from the creator’s holdings (TRX balance or TRC-10 balance)
  - deduct the computed other token amount
  - update the exchange balances accordingly
  - store exchange via `Commons.putExchangeCapsule()`:
    - if `ALLOW_SAME_TOKEN_NAME == 0`: write v1 exchange (names) **and** write a v2 copy (ids)
    - if `ALLOW_SAME_TOKEN_NAME == 1`: write v2 only (v1 is not updated)
- Receipt: `ret.setExchangeInjectAnotherAmount(anotherTokenQuant)` + `ret.setStatus(fee=0, SUCESS)`

The behavior matrix is explicitly tested in `ExchangeInjectActuatorTest`:

- `ALLOW_SAME_TOKEN_NAME == 0`: injection updates **both** v1 and v2 exchange stores.
- `ALLOW_SAME_TOKEN_NAME == 1`: injection updates **v2 only**; legacy v1 stays unchanged.

---

## Rust implementation behavior (what it currently does)

### Parsing

`parse_exchange_inject_contract()` manually decodes protobuf fields:

- `exchange_id` (field 2, int64)
- `token_id` (field 3, bytes)
- `quant` (field 4, int64)

(`owner_address` is skipped and the sender is taken from `transaction.from`.)

### Validation + execution

`execute_exchange_inject_contract()`:

- loads the owner account and `ALLOW_SAME_TOKEN_NAME`
- fetches the exchange via `storage_adapter.get_exchange(exchange_id)` (always **v2**)
- validates:
  - creator check
  - numeric token-id format when `ALLOW_SAME_TOKEN_NAME == 1`
  - token membership in the exchange
  - non-closed exchange and positive quant
  - calculates the other-side amount for validation using `calculate_inject_another_amount()` (i128 multiply then `/`)
  - checks `EXCHANGE_BALANCE_LIMIT`
  - checks TRX balance or TRC-10 token balances (currently via `storage_adapter.get_asset_balance_v2(address, token_bytes)`)
- executes:
  - recalculates other-side amount using `calculate_inject_another_amount_multiply_exact()` (checked_mul + `div_euclid`)
  - applies deductions via `reduce_asset_amount_v2()` for TRC-10 assets
  - updates the exchange via `storage_adapter.put_exchange()` (writes **v2 only**)
  - builds a receipt with `exchange_inject_another_amount`

---

## Does it match java-tron?

### What matches (good parity)

1) **Overflow behavior matches Java’s validate/execute split**

- Java validate uses BigInteger and can succeed even when execute overflows via `multiplyExact`.
- Rust mirrors this with:
  - validation: `calculate_inject_another_amount()` using i128 (BigInteger-like)
  - execution: `calculate_inject_another_amount_multiply_exact()` using `checked_mul` (multiplyExact-like)

This is consistent with the current conformance fixtures that expect `Unexpected error: long overflow` for the “happy path” cases that intentionally trigger overflow in execute.

2) **`ALLOW_SAME_TOKEN_NAME == 1` (modern mode) logic is broadly aligned**

- exchange is read/written from the v2 store
- token-id numeric checks match `TransactionUtil.isNumber`
- balance-limit and error strings match Java’s `ExchangeInjectActuator.validate`
- receipt field `exchange_inject_another_amount` is set

3) **Post-`ALLOW_SAME_TOKEN_NAME` behavior (v1 not updated) matches**

Java’s `ExchangeInjectActuatorTest.OldNotUpdateSuccessExchangeInject` shows that once `ALLOW_SAME_TOKEN_NAME` flips to 1, v1 exchanges are not updated anymore. Rust updating v2 only is consistent with that mode.

### Where it diverges (real parity breaks)

1) **`ALLOW_SAME_TOKEN_NAME == 0` (legacy mode) is not implemented correctly**

Java behavior when allowSameTokenName==0:

- read exchange from `ExchangeStore` (v1, token **names**)
- write both v1 and v2 stores on update (`Commons.putExchangeCapsule`)

Rust behavior today:

- always reads the exchange from v2 (`get_exchange`)
- always writes v2 only (`put_exchange`)

Impact:

- For legacy-mode injects where the contract uses token **names** (e.g. `"abc"`), Rust will fail token membership checks because v2 exchanges contain token **ids** (e.g. `"1"`).
- Even for legacy-mode injects where the injected side is TRX (so membership might pass), the “other token id” comes from v2 (numeric id) and TRC-10 balance validation will likely read the wrong key/map.
- Even if execution succeeds, v1 is left stale, but Java expects v1+v2 to be updated in allowSameTokenName==0 mode.

This breaks the behavior validated by `ExchangeInjectActuatorTest.SameTokenNameCloseSuccessExchangeInject` and `SameTokenNameCloseSuccessExchangeInject2`.

2) **TRC-10 balance validation does not match Java’s `assetBalanceEnoughV2()` routing**

Rust uses `storage_adapter.get_asset_balance_v2(address, token_bytes)` which always reads `Account.asset_v2[token_bytes_as_string]`.

Java `assetBalanceEnoughV2()`:

- allowSameTokenName==0 → reads `Account.asset[name]`
- allowSameTokenName==1 → reads `Account.assetV2[token_id]`
- may import balances from the optimized account-asset store

So in legacy mode (and/or under asset optimization), Rust will treat valid balances as zero and reject transactions.

3) **Minor: missing “account[…] not exists” parity for missing owner**

Java validate errors with `account[<hex>] not exists`. Rust currently errors with `"Owner account not found"` for this contract.

This likely doesn’t show up in existing fixtures, but it is a real error-string mismatch if/when that path is exercised.

---

## Bottom line

- For **`ALLOW_SAME_TOKEN_NAME == 1`**, Rust’s `EXCHANGE_INJECT_CONTRACT` implementation is close to Java and intentionally matches the validate-vs-execute overflow behavior.
- It does **not** fully match java-tron across the full fork/property matrix:
  - legacy mode (`ALLOW_SAME_TOKEN_NAME == 0`) exchange store selection + dual-write is missing
  - TRC-10 balance lookup does not follow Java’s `assetBalanceEnoughV2()` semantics (legacy routing and asset optimization)

