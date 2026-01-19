# Review: `EXCHANGE_CREATE_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**:
  - `execute_exchange_create_contract()` + `parse_exchange_create_contract()` in `rust-backend/crates/core/src/service/mod.rs`
  - dynamic-property + helper behavior in `rust-backend/crates/execution/src/storage_adapter/engine.rs` (notably `get_exchange_create_fee()`, `get_exchange_balance_limit()`, `support_black_hole_optimization()`, `burn_trx()`, and TRC-10 balance helpers)
- **Java reference**:
  - `ExchangeCreateActuator.validate/execute` in `actuator/src/main/java/org/tron/core/actuator/ExchangeCreateActuator.java`
  - TRC-10 balance helpers: `AccountCapsule.assetBalanceEnoughV2()` + `AccountCapsule.reduceAssetAmountV2()` in `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java`
  - asset optimization import path: `AssetUtil.importAsset()` in `chainbase/src/main/java/org/tron/core/capsule/utils/AssetUtil.java`
  - fee sink semantics: `DynamicPropertiesStore.supportBlackHoleOptimization()` + `burnTrx()` in `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java`

Goal: determine whether Rust execution matches java-tron’s actuator semantics (validation, state updates, and receipt fields).

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`ExchangeCreateActuator.validate`)

Source: `actuator/src/main/java/org/tron/core/actuator/ExchangeCreateActuator.java`

Checks, in order:

1. Contract is present + correct Any type (`ExchangeCreateContract`)
2. `DecodeUtil.addressValid(owner)`:
   - non-empty
   - exactly 21 bytes
   - prefix byte matches network
3. Owner account exists in `AccountStore`
4. Owner has enough TRX for the **exchange create fee**:
   - `account.balance >= DynamicPropertiesStore.getExchangeCreateFee()`
5. If `ALLOW_SAME_TOKEN_NAME == 1`:
   - for each token id (if not TRX symbol `_`), `TransactionUtil.isNumber(token_id)` must be true
6. Token ids must differ: `first_token_id != second_token_id`
7. Both token balances must be positive (`> 0`)
8. Both balances must be <= `EXCHANGE_BALANCE_LIMIT`
9. Funding checks:
   - if TRX is one side: `account.balance >= (trx_deposit + fee)` else fail with `"balance is not enough"`
   - if TRC-10 is one side: `account.assetBalanceEnoughV2(token_id, amount, dynamicStore)` else fail with `"first token balance is not enough"` / `"second token balance is not enough"`

Important nuance: `assetBalanceEnoughV2()` calls `importAsset(key)` internally, which may pull balances from `AccountAssetStore` when `ALLOW_ASSET_OPTIMIZATION == 1`.

### 2) Execution (`ExchangeCreateActuator.execute`)

Source: `actuator/src/main/java/org/tron/core/actuator/ExchangeCreateActuator.java`

Steps:

1. `fee = DynamicPropertiesStore.getExchangeCreateFee()`
2. Deduct fee from owner balance
3. Deduct deposits:
   - TRX side: subtract from owner balance
   - TRC-10 side: `reduceAssetAmountV2(token_key, amount, dynamicStore, assetIssueStore)`
4. Allocate new `exchange_id = latestExchangeNum + 1` and `create_time = latestBlockHeaderTimestamp`
5. Store exchange:
   - if `ALLOW_SAME_TOKEN_NAME == 0`:
     - write “v1” entry (token **names**) to `ExchangeStore`
     - resolve token names to numeric ids (via `AssetIssueStore.get(name).getId()`), then write “v2” entry
   - always write “v2” entry to `ExchangeV2Store`
6. Persist owner account and update `LATEST_EXCHANGE_NUM`
7. Fee destination:
   - if `supportBlackHoleOptimization() == true`: increment `BURN_TRX_AMOUNT` (`burnTrx(fee)`)
   - else: credit blackhole account balance
8. Receipt: `ret.setExchangeId(exchange_id)` + `ret.setStatus(fee, SUCESS)`

---

## Rust implementation behavior (what it currently does)

Source: `rust-backend/crates/core/src/service/mod.rs`

### Parsing

`parse_exchange_create_contract()` manually decodes protobuf fields:

- `owner_address` (field 1, bytes)
- `first_token_id` (field 2, bytes)
- `first_token_balance` (field 3, int64)
- `second_token_id` (field 4, bytes)
- `second_token_balance` (field 5, int64)

### Validation + execution

`execute_exchange_create_contract()`:

- Validates owner address length/prefix (expects 21 bytes and prefix == `storage_adapter.address_prefix()`)
- Loads owner account from `AccountStore`
- Loads dynamic properties:
  - `ALLOW_SAME_TOKEN_NAME`
  - `EXCHANGE_CREATE_FEE`
  - `EXCHANGE_BALANCE_LIMIT`
- Performs Java-parity checks for:
  - fee coverage
  - numeric token-id format (when allowSameTokenName==1)
  - tokens not equal
  - balances > 0
  - balances <= limit
  - TRX-side funding checks
  - TRC-10-side funding checks (currently via `storage_adapter.get_asset_balance_v2(...)`)
- Applies state updates:
  - deduct fee + deposits from a cloned account proto
  - writes exchange record(s) and updates `LATEST_EXCHANGE_NUM`
  - persists owner account
  - fee destination: credits blackhole when blackhole optimization is disabled
- Builds receipt bytes via `TransactionResultBuilder`:
  - currently sets `exchange_id` only

---

## Does it match java-tron?

### What matches (good parity vs `ExchangeCreateActuator`)

- Validation error strings for the fixture-driven cases appear to match Java’s `ExchangeCreateActuator.validate` messages:
  - `"Invalid address"`
  - `"No enough balance for exchange create fee!"`
  - `"first token id is not a valid number"` / `"second token id is not a valid number"`
  - `"cannot exchange same tokens"`
  - `"token balance must greater than zero"`
  - `"token balance must less than <limit>"`
  - `"balance is not enough"`
  - `"first token balance is not enough"` / `"second token balance is not enough"`
- Exchange-id allocation logic matches: `latestExchangeNum + 1`
- Exchange storage logic matches the high-level Java behavior:
  - writes v1 exchange when `ALLOW_SAME_TOKEN_NAME == 0`
  - always writes v2 exchange
- Owner account fee/deposit deductions match Java’s intent (given `first_token_id != second_token_id` ensures only one side can be TRX)

### Where it diverges (real mismatches / likely parity breaks)

1) **Receipt fee is missing (conformance breaker)**

Java sets receipt status and fee via `ret.setStatus(fee, SUCESS)`.

Conformance fixtures for happy-path exchange-create expect `Protocol.Transaction.Result` to include:

- field `1` (`fee`) = exchange-create fee
- field `21` (`exchange_id`) = newly created exchange id

Example: `conformance/fixtures/exchange_create_contract/*/expected/result.pb` decodes as:

- `1: 1024000000`
- `21: 1`

Rust currently emits only `exchange_id` in `tron_transaction_result`, so remote execution would produce a receipt missing the fee field.

2) **Blackhole optimization “burn” counter update is missing**

Java behavior when `ALLOW_BLACKHOLE_OPTIMIZATION == 1`:

- fee is burned by incrementing `BURN_TRX_AMOUNT` (`DynamicPropertiesStore.burnTrx(fee)`)

Rust behavior:

- credits blackhole account only when optimization is disabled
- when optimization is enabled, it does *not* call `burn_trx()`, so `BURN_TRX_AMOUNT` will not increment

This diverges from Java state even though balances might “look right” (because both paths subtract from the owner).

3) **TRC-10 balance validation does not match Java’s `assetBalanceEnoughV2()`**

Java `assetBalanceEnoughV2()`:

- chooses which map to read based on `ALLOW_SAME_TOKEN_NAME`
  - allow==0 → `Account.asset` (name key)
  - allow==1 → `Account.assetV2`
- additionally calls `importAsset(key)` which may load balances from `AccountAssetStore` when `ALLOW_ASSET_OPTIMIZATION == 1`

Rust validation currently calls `storage_adapter.get_asset_balance_v2(address, token_id_bytes)` which:

- reads only `Account.asset_v2`
- uses `token_id_bytes` directly as the map key

Impacts:

- `ALLOW_SAME_TOKEN_NAME == 0`: contracts use token *names* as keys → Rust will likely read the wrong map/key and reject valid transactions.
- `ALLOW_ASSET_OPTIMIZATION == 1`: Java may source balances from `AccountAssetStore` → Rust will treat balances as zero unless it implements the same import path.

4) **Rust default for `EXCHANGE_CREATE_FEE` is wrong when the key is absent**

Java initializes missing `EXCHANGE_CREATE_FEE` to `1024000000L`.

Rust `get_exchange_create_fee()` currently falls back to `1024_000_000_000` when the key is missing/invalid, which is off by 1000×.

Even if mainnet fixtures always include the key, this is still a parity footgun for minimal DBs and early-chain replays.

5) **Minor: error-handling default for `support_black_hole_optimization` differs**

In `execute_exchange_create_contract()`, Rust uses `support_black_hole_optimization().unwrap_or(true)` (defaults to “burn” on read error).

Java throws if the key is missing, and Rust’s own storage adapter default is “credit blackhole” when absent. Defaulting to “burn” on error is inconsistent with both.

---

## Bottom line

- The core actuator-style validation + state-transition logic is close for the common path (`ALLOW_SAME_TOKEN_NAME == 1`, `ALLOW_BLACKHOLE_OPTIMIZATION == 0`).
- It does *not* fully match java-tron today:
  - happy-path receipts are missing the fee field (conformance mismatch)
  - burn-counter updates are missing when blackhole optimization is enabled
  - TRC-10 balance reads diverge in legacy mode and under asset optimization
  - the `EXCHANGE_CREATE_FEE` fallback default is incorrect

